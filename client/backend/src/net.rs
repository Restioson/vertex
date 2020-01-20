use super::Error as VertexError;
use super::Result as VertexResult;

use vertex_common::{OkResponse, ErrResponse, RequestId, ServerAction, ServerMessage, ClientRequest, ClientMessage};

use futures::stream::{Stream, StreamExt, SplitStream, SplitSink};
use futures::future::FutureExt;

use futures::sink::SinkExt;
use futures::channel::oneshot;

use std::collections::HashMap;

use url::Url;

use std::sync::atomic::{AtomicU32, Ordering};
use std::rc::Rc;
use std::cell::RefCell;
use std::ops::Deref;

struct RequestIdGenerator {
    next_request_id: AtomicU32,
}

impl RequestIdGenerator {
    pub fn new() -> RequestIdGenerator {
        RequestIdGenerator { next_request_id: AtomicU32::new(0) }
    }

    pub fn next(&self) -> RequestId {
        RequestId::new(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }
}

type WsStream = tokio_tls::TlsStream<tokio::net::TcpStream>;
type WsClient = tokio_tungstenite::WebSocketStream<WsStream>;
type RequestResult = Result<OkResponse, ErrResponse>;

/// Module adapted from tokio-tungstenite
/// tokio-tungstenite does not support connection with a custom TlsConnector, so we need to reimplement it
mod ws {
    use tokio::net::TcpStream;

    use tungstenite::{Result, Error};
    use tungstenite::handshake::client::{Request, Response};

    pub(super) async fn connect_async<R>(request: R) -> Result<(super::WsClient, Response)>
        where R: Into<Request<'static>> + Unpin,
    {
        let request: Request = request.into();

        let domain = request.url.host_str()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Url("no host name in the url".into()))?;

        let port = request.url.port_or_known_default().expect("port unknown");

        let socket = TcpStream::connect(format!("{}:{}", domain, port)).await
            .map_err(Error::Io)?;

        let connector = tokio_tls::TlsConnector::from(
            native_tls::TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(Error::Tls)?
        );

        let stream = connector.connect(&domain, socket).await
            .map_err(Error::Tls)?;

        tokio_tungstenite::client_async(request, stream).await
    }
}

pub async fn connect(url: Url) -> VertexResult<Net> {
    let (client, _) = ws::connect_async(url).await?;

    let (sink, stream) = client.split();

    let request_tracker = RequestTracker::new();
    let request_tracker = Rc::new(request_tracker);

    Ok(Net(
        Sender {
            request_tracker: request_tracker.clone(),
            request_id_generator: RequestIdGenerator::new(),
            sink: RefCell::new(sink),
        },
        Receiver {
            request_tracker: request_tracker.clone(),
            stream,
        },
    ))
}

pub struct Request(oneshot::Receiver<RequestResult>);

impl Request {
    pub async fn response(self) -> RequestResult {
        self.0.map(|result| result.expect("channel closed")).await
    }
}

pub struct Net(Sender, Receiver);

impl Net {
    pub fn split(self) -> (Sender, Receiver) {
        (self.0, self.1)
    }

    pub fn sender(&self) -> &Sender { &self.0 }

    pub fn receiver(&self) -> &Receiver { &self.1 }
}

pub struct Sender {
    request_tracker: Rc<RequestTracker>,
    request_id_generator: RequestIdGenerator,
    sink: RefCell<SplitSink<WsClient, tungstenite::Message>>,
}

impl Sender {
    pub async fn request(&self, request: ClientRequest) -> VertexResult<Request> {
        let id = self.request_id_generator.next();

        let receiver = self.request_tracker.enqueue(id)
            .expect("unable to enqueue message");

        let message = ClientMessage { id, request };
        self.send(message).await?;

        Ok(Request(receiver))
    }

    pub async fn send(&self, message: ClientMessage) -> VertexResult<()> {
        self.send_raw(tungstenite::Message::Binary(message.into())).await?;
        Ok(())
    }

    pub async fn dispatch_heartbeat(&self) -> VertexResult<()> {
        self.send_raw(tungstenite::Message::Ping(Vec::new())).await?;
        Ok(())
    }

    #[inline]
    async fn send_raw(&self, message: tungstenite::Message) -> VertexResult<()> {
        self.sink.borrow_mut().send(message).await?;
        Ok(())
    }
}

pub struct Receiver {
    request_tracker: Rc<RequestTracker>,
    stream: SplitStream<WsClient>,
}

impl Receiver {
    pub fn stream(self) -> impl Stream<Item=VertexResult<ServerAction>> {
        let request_tracker = Rc::downgrade(&self.request_tracker);

        self.stream.filter_map(move |result| futures::future::ready(
            match result {
                Ok(tungstenite::Message::Binary(bytes)) => {
                    match serde_cbor::from_slice::<ServerMessage>(&bytes) {
                        Ok(ServerMessage::Action(action)) => Some(Ok(action)),
                        Ok(ServerMessage::Response { result, id }) => {
                            if let Some(request_tracker) = request_tracker.upgrade() {
                                request_tracker.complete(id, result);
                            }
                            None
                        }
                        Ok(ServerMessage::MalformedMessage) => Some(Err(VertexError::MalformedRequest)),
                        Err(_) => Some(Err(VertexError::MalformedResponse)),
                    }
                }
                Ok(tungstenite::Message::Close(_)) => Some(Err(VertexError::ServerClosed)),
                Err(e) => Some(Err(VertexError::WebSocketError(e))),
                _ => None,
            }
        ))
    }
}

struct RequestTracker {
    pending_requests: RefCell<HashMap<RequestId, EnqueuedRequest>>,
}

impl RequestTracker {
    fn new() -> RequestTracker {
        RequestTracker { pending_requests: RefCell::new(HashMap::new()) }
    }

    fn enqueue(&self, id: RequestId) -> Option<oneshot::Receiver<RequestResult>> {
        let mut pending_requests = self.pending_requests.borrow_mut();

        if pending_requests.contains_key(&id) {
            return None;
        }

        let (send, recv) = oneshot::channel();
        pending_requests.insert(id, EnqueuedRequest(send));

        return Some(recv);
    }

    fn complete(&self, id: RequestId, result: RequestResult) {
        self.pending_requests.borrow_mut().remove(&id).map(|request| request.handle(result));
    }
}

struct EnqueuedRequest(oneshot::Sender<RequestResult>);

impl EnqueuedRequest {
    fn handle(self, result: RequestResult) {
        self.0.send(result).expect("channel closed")
    }
}
