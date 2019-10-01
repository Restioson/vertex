use websocket::client::Url;
use websocket::{ClientBuilder, OwnedMessage, WebSocketResult, WebSocketError};
use native_tls::TlsConnector;

use super::Error as VertexError;
use super::Result as VertexResult;

use vertex_common::{ServerboundRequest, ClientboundMessage, OkResponse, ServerError, RequestId, ClientboundAction};

use std::time::Instant;
use futures::{Future, Async, Poll};
use futures::stream::Stream;
use futures::sync::{mpsc, oneshot};
use futures::sink::Sink;
use tokio_tcp::TcpStream;
use tokio_tls::TlsStream;
use std::collections::{LinkedList, HashMap};
use futures::future::IntoFuture;

pub fn connect(url: Url) -> impl Future<Item=Ready, Error=WebSocketError> {
    let (send_in, recv_in) = mpsc::unbounded();
    let (send_out, recv_out) = mpsc::unbounded();

    ClientBuilder::from_url(&url).async_connect_secure(
        Some(TlsConnector::builder()
            .danger_accept_invalid_certs(true) // TODO needed for self signed certs
            .build().expect("Error setting TLS settings"))
    ).map(move |(client, _)| {
        Ready {
            client,
            send_out,
            recv_in,
            send_in,
            recv_out,
        }
    })
}

pub struct Ready {
    client: websocket::r#async::Client<TlsStream<TcpStream>>,
    send_in: mpsc::UnboundedSender<OwnedMessage>,
    recv_in: mpsc::UnboundedReceiver<OwnedMessage>,
    send_out: mpsc::UnboundedSender<OwnedMessage>,
    recv_out: mpsc::UnboundedReceiver<OwnedMessage>,
}

impl Ready {
    pub fn start(self) -> (Active, impl Future<Item=(), Error=VertexError>) {
        let net = Active {
            recv: self.recv_in,
            send: self.send_out,
            next_request_id: 0,
            pending_requests: HashMap::new(),
            last_message: Instant::now(),
        };

        let (sink, stream) = self.client.split();
        let recv = stream
            .map_err(|err| VertexError::WebSocketError(err))
            .forward(self.send_in.sink_map_err(|_| VertexError::ServerClosed));

        let send = self.recv_out
            .map_err(|_| VertexError::ChannelClosed)
            .forward(sink);

        let future = recv.join(send).map(|_| ());

        (net, future)
    }
}

// TODO: really this could be split into a stream & sink
pub struct Active {
    recv: mpsc::UnboundedReceiver<OwnedMessage>,
    send: mpsc::UnboundedSender<OwnedMessage>,
    next_request_id: u64,
    pending_requests: HashMap<RequestId, PendingRequest>,
    last_message: Instant,
}

impl Active {
    pub fn request(&mut self, request: ServerboundRequest) -> impl Future<Item=OkResponse, Error=ServerError> {
        let id = RequestId(self.next_request_id);
        self.next_request_id += 1;

        let (send, recv) = oneshot::channel();
        self.pending_requests.insert(id, PendingRequest(send));

        recv.then(|result| result.expect("response channel closed"))
    }

    #[inline]
    pub fn send(&self, message: OwnedMessage) {
        self.send.unbounded_send(message)
            .expect("send channel closed")
    }

    #[inline]
    pub fn dispatch_heartbeat(&self) {
        self.send(OwnedMessage::Ping(Vec::new()))
    }

    #[inline]
    pub fn last_message(&self) -> Instant {
        self.last_message
    }

    #[inline]
    pub fn stream(&mut self) -> ActionStream { ActionStream(self) }
}

struct ActionStream<'a>(&'a mut Active);

impl<'a> Stream for ActionStream<'a> {
    type Item = ClientboundAction;
    type Error = VertexError;

    fn poll(&mut self) -> Poll<Option<ClientboundAction>, VertexError> {
        // TODO: Clean this
        while let Async::Ready(ready) = self.0.recv.poll().map_err(|_| VertexError::ChannelClosed)? {
            match ready {
                Some(message) => {
                    match message {
                        OwnedMessage::Binary(bytes) => {
                            match serde_cbor::from_slice::<ClientboundMessage>(&bytes) {
                                Ok(ClientboundMessage::Action(action)) => return Ok(Async::Ready(Some(action))),
                                Ok(ClientboundMessage::Response { id, result }) => {
                                    if let Some(pending_request) = self.0.pending_requests.remove(&id) {
                                        pending_request.handle(result)
                                    }
                                }
                                Err(_) => return Err(VertexError::MalformedResponse),
                            }
                        }
                        OwnedMessage::Close(_) => return Err(VertexError::ServerClosed),
                        _ => (),
                    }
                }
                None => return Ok(Async::Ready(None)),
            }
        }
        Ok(Async::NotReady)
    }
}

struct PendingRequest(oneshot::Sender<Result<OkResponse, ServerError>>);

impl PendingRequest {
    fn handle(self, response: Result<OkResponse, ServerError>) {
        self.0.send(response).expect("channel closed")
    }
}
