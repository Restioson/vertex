use super::Error as VertexError;
use super::Result as VertexResult;

use vertex_common::{ServerboundRequest, ClientboundMessage, OkResponse, ErrResponse, RequestId, ClientboundAction, ServerboundMessage};

use futures::stream::{Stream, StreamExt, SplitStream, SplitSink};
use futures::future::FutureExt;

use futures::sink::SinkExt;
use futures::channel::oneshot;
use futures::task::{Context, Poll};

use std::collections::HashMap;

use std::future::Future;
use std::pin::Pin;

use url::Url;

use std::sync::atomic::{AtomicU64, Ordering};
use std::rc::Rc;
use std::cell::RefCell;

struct RequestIdGenerator {
    next_request_id: AtomicU64,
}

impl RequestIdGenerator {
    pub fn new() -> RequestIdGenerator {
        RequestIdGenerator { next_request_id: AtomicU64::new(0) }
    }

    pub fn next(&self) -> RequestId {
        RequestId(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }
}

type WsClient = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;
type RequestResult = Result<OkResponse, ErrResponse>;

pub async fn connect(url: Url) -> VertexResult<(Sender, Receiver)> {
    let (client, _) = tokio_tungstenite::connect_async(url).await?;
    let (sink, stream) = client.split();

    let request_tracker = RequestTracker::new();
    let request_tracker = Rc::new(request_tracker);

    Ok((
        Sender {
            request_tracker: request_tracker.clone(),
            request_id_generator: RequestIdGenerator::new(),
            sink: RefCell::new(sink),
        },
        Receiver {
            request_tracker: request_tracker.clone(),
            stream,
        }
    ))
}

pub struct Request(oneshot::Receiver<RequestResult>);

impl Request {
    pub async fn response(self) -> RequestResult {
        self.0.map(|result| result.expect("channel closed")).await
    }
}

pub struct Sender {
    request_tracker: Rc<RequestTracker>,
    request_id_generator: RequestIdGenerator,
    sink: RefCell<SplitSink<WsClient, tungstenite::Message>>,
}

impl Sender {
    pub async fn request(&self, request: ServerboundRequest) -> VertexResult<Request> {
        let id = self.request_id_generator.next();

        let receiver = self.request_tracker.enqueue(id)
            .expect("unable to enqueue message");

        let message = ServerboundMessage { id, request };
        self.send(message).await?;

        Ok(Request(receiver))
    }

    pub async fn send(&self, message: ServerboundMessage) -> VertexResult<()> {
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
    pub fn stream(self) -> impl Stream<Item=VertexResult<ClientboundAction>> {
        let request_tracker = Rc::downgrade(&self.request_tracker);

        self.stream.filter_map(move |result| futures::future::ready(
            match result {
                Ok(tungstenite::Message::Binary(bytes)) => {
                    match serde_cbor::from_slice::<ClientboundMessage>(&bytes) {
                        Ok(ClientboundMessage::Action(action)) => Some(Ok(action)),
                        Ok(ClientboundMessage::Response { id, result }) => {
                            if let Some(request_tracker) = request_tracker.upgrade() {
                                request_tracker.complete(id, result);
                            }
                            None
                        }
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
