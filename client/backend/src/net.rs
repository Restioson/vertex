use std::time::{Instant, Duration};
use websocket::{ClientBuilder, OwnedMessage, WebSocketError, WebSocketResult};
use websocket::client::Url;

use futures::{future, Future, Async};

use super::Vertex;
use super::Error as VertexError;

use vertex_common::{ServerMessage, ClientRequest, RequestId, ClientMessage, RequestResponse, ServerError};

use futures::stream::{Stream, SplitStream, SplitSink};
use futures::sink::SinkFromErr;
use futures::sync::{mpsc, oneshot};
use actix::{Actor, Context, Arbiter, StreamHandler, Addr, Handler, Message, AsyncContext, ResponseFuture, ActorContext};
use tokio::codec::FramedRead;
use futures::future::{IntoFuture, FutureResult};
use uuid::Uuid;
use websocket::sender::Writer;
use websocket::receiver::Reader;
use websocket::stream::sync::TcpStream;
use std::io;
use std::collections::HashMap;

#[derive(Message)]
pub struct AddClient(Addr<Vertex>);

pub struct MakeRequest(pub ClientRequest);

#[derive(Message)]
pub struct DispatchHeartbeat;

impl Message for MakeRequest {
    type Result = Result<RequestResponse, VertexError>;
}

pub struct Net {
    client: Option<Addr<Vertex>>,
    writer: Writer<TcpStream>,
    last_heartbeat: Instant,
    pending_requests: HashMap<RequestId, PendingRequest>,
}

impl Net {
    pub fn connect(url: Url) -> WebSocketResult<Addr<Net>> {
        let client = ClientBuilder::from_url(&url)
            .connect_insecure()?;

        // TODO: reconsider timeouts i guess
        // noblocking mode doesn't work so we have to just set a really low timeout
        client.stream_ref().set_read_timeout(Some(Duration::from_micros(1)))?;
        client.stream_ref().set_write_timeout(Some(Duration::from_millis(1000)))?;

        let (reader, writer) = client.split()?;

        Ok(Actor::create(move |ctx| {
            ctx.add_stream(NetStream(reader));

            Net {
                client: None,
                writer,
                last_heartbeat: Instant::now(),
                pending_requests: HashMap::new(),
            }
        }))
    }
}

impl Actor for Net {
    type Context = Context<Net>;
}

impl StreamHandler<OwnedMessage, WebSocketError> for Net {
    fn handle(&mut self, message: OwnedMessage, ctx: &mut Self::Context) {
        match message {
            OwnedMessage::Binary(bytes) => {
                match serde_cbor::from_slice::<ServerMessage>(&bytes) {
                    Ok(ServerMessage::Response { response, request_id }) => {
                        if let Some(pending) = self.pending_requests.remove(&request_id) {
                            pending.complete(response);
                        } else {
                            // TODO: also send to client normally in this case
                        }
                    }
                    Ok(message) => {
                        if let Some(client) = &self.client {
                            client.do_send(message);
                        }
                    }
                    Err(e) => eprintln!("got malformed message from server {}", e),
                }
            }
            OwnedMessage::Pong(_) => self.last_heartbeat = Instant::now(),
            OwnedMessage::Close(_) => ctx.stop(), // TODO: consider how to handle this
            _ => eprintln!("received unexpected message type"),
        }
    }
}

impl Handler<AddClient> for Net {
    type Result = ();

    fn handle(&mut self, add: AddClient, ctx: &mut Context<Net>) {
        self.client = Some(add.0);
    }
}

impl Handler<MakeRequest> for Net {
    type Result = ResponseFuture<RequestResponse, VertexError>;

    fn handle(&mut self, msg: MakeRequest, ctx: &mut Self::Context) -> Self::Result {
        let msg = ClientMessage::new(msg.0);

        let (sender, receiver) = oneshot::channel();
        self.pending_requests.insert(msg.id, PendingRequest(sender));

        // TODO: is this fine to be blocking op?
        match self.writer.send_message(&OwnedMessage::Binary(msg.into())) {
            Ok(_) => Box::new(receiver.then(|result| {
                match result {
                    Ok(response) => match response {
                        Ok(response) => Ok(response),
                        Err(err) => Err(VertexError::ServerError(err)),
                    },
                    Err(_) => Err(VertexError::ResponseCancelled),
                }
            })),
            Err(err) => Box::new(future::err(VertexError::WebSocketError(err))),
        }
    }
}

impl Handler<DispatchHeartbeat> for Net {
    type Result = ();

    fn handle(&mut self, msg: DispatchHeartbeat, ctx: &mut Self::Context) {
        // TODO: is this fine to be blocking op?
        match self.writer.send_message(&OwnedMessage::Ping(Vec::new())) {
            // TODO: actually do something with the error
            Err(e) => eprintln!("error while ping {}", e),
            _ => (),
        }
    }
}

struct NetStream(Reader<TcpStream>);

impl Stream for NetStream {
    type Item = OwnedMessage;
    type Error = WebSocketError;

    fn poll(&mut self) -> WebSocketResult<Async<Option<OwnedMessage>>> {
        match self.0.recv_message() {
            Ok(msg) => Ok(Async::Ready(Some(msg))),
            Err(WebSocketError::NoDataAvailable) => Ok(Async::NotReady),
            Err(WebSocketError::IoError(e)) => {
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
                    Ok(Async::NotReady)
                } else {
                    Err(WebSocketError::IoError(e))
                }
            }
            Err(e) => Err(e),
        }
    }
}

struct PendingRequest(oneshot::Sender<Result<RequestResponse, ServerError>>);

impl PendingRequest {
    fn complete(self, response: Result<RequestResponse, ServerError>) {
        self.0.send(response).unwrap()
    }
}
