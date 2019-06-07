use std::io::Cursor;
use actix::prelude::*;
use actix_web::web::{Data, Payload, HttpRequest, HttpResponse, Bytes};
use actix_web_actors::ws::{self, WebsocketContext};
use websocket::message::OwnedMessage;
use websocket::result::WebSocketError;
use websocket::sender::Writer;
use websocket::receiver::Reader;
use websocket::stream::sync::TcpStream;
use futures::Async;
use url::Url;
use serde::{Serialize, Deserialize};
use super::{FederationServer, Connect};

#[derive(Debug, Serialize, Deserialize)]
pub enum FederationMessage {
    Success(Success),
    Error(Error),
}

impl Into<Bytes> for FederationMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for FederationMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Success {
    NoData,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Error {
    UnexpectedText,
}

#[derive(Debug, Serialize, Deserialize, Message)]
struct SendMessage {
    message: FederationMessage,
}

impl Into<Bytes> for SendMessage {
    fn into(self) -> Bytes {
        self.message.into()
    }
}

impl Into<Vec<u8>> for SendMessage {
    fn into(self) -> Vec<u8> {
        self.message.into()
    }
}

#[derive(Message)]
pub enum ServerWsSession {
    Incoming(Addr<IncomingSession>),
    Outgoing(Addr<OutgoingSession>),
}

impl ServerWsSession {
    pub fn start_incoming(
        request: HttpRequest,
        server: Data<Addr<FederationServer>>,
        stream: Payload,
    ) -> Result<HttpResponse, actix_web::Error> {
        ws::start(
            IncomingSession::new(server.get_ref().clone()),
            &request,
            stream,
        )
    }

    pub fn send_message(&mut self, message: FederationMessage) {
        let message = SendMessage { message };
        match self {
            ServerWsSession::Incoming(inc) => inc.do_send(message),
            ServerWsSession::Outgoing(out) => out.do_send(message),
        }
    }

    fn handle_text() -> FederationMessage {
        FederationMessage::Error(Error::UnexpectedText)
    }

    fn handle_binary(
        binary: Bytes,
        _server: &mut Addr<FederationServer>
    ) -> Option<FederationMessage> {
        let cursor = Cursor::new(binary);
        let msg = serde_cbor::from_reader(cursor).expect("invaild bytes"); // TODO <- return properly

        match msg {
            FederationMessage::Success(s) => {
                println!("Msg from federated server: {:?}", s);
                Some(FederationMessage::Success(Success::NoData))
            },
            FederationMessage::Error(e) => {
                eprintln!("Error from federated server: {:?}", e);
                None
            }
        }
    }
}

struct IncomingSession {
    from: Option<Url>,
    server: Addr<FederationServer>,
}

impl IncomingSession {
    fn new(server: Addr<FederationServer>) -> Self {
        IncomingSession { from: None, server, }
    }
}

impl Actor for IncomingSession {
    type Context = WebsocketContext<Self>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for IncomingSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut WebsocketContext<Self>) {
        match msg {
            ws::Message::Ping(msg) => {
                ctx.pong(&msg)
            },
            ws::Message::Text(text) => {
                if self.from.is_none() {
                    println!("url: {}", text); //TODO
                    self.server.do_send(Connect {
                        url: Url::parse(&text).expect("invalid url"), // TODO <- return err properly
                        session: ServerWsSession::Incoming(ctx.address())
                    });
                    ctx.binary(FederationMessage::Success(Success::NoData));
                } else {
                    ctx.binary(ServerWsSession::handle_text()) // todo get rid of
                }
            },
            ws::Message::Binary(bin) => {
                if let Some(bin) = ServerWsSession::handle_binary(bin, &mut self.server) {
                    ctx.binary(bin)
                }
            },
            _ => println!("wat?"),
        }
    }
}

impl Handler<SendMessage> for IncomingSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg);
    }
}

pub struct WsReaderStreamAdapter(pub Reader<TcpStream>);

impl Stream for WsReaderStreamAdapter {
    type Item = OwnedMessage;
    type Error = WebSocketError;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        let res = self.0.recv_message();

        match res {
            Ok(msg) => Ok(Async::Ready(Some(msg))),
            Err(WebSocketError::NoDataAvailable) => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }
}

pub struct OutgoingSession {
    to: Url,
    sender: Writer<TcpStream>,
    server: Addr<FederationServer>,
}

impl OutgoingSession {
    pub fn new(
        to: Url,
        server: Addr<FederationServer>,
        sender: Writer<TcpStream>,
    ) -> Self {
        OutgoingSession { to, sender, server, }
    }
}

impl Actor for OutgoingSession {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        self.server.do_send(Connect {
            url: self.to.clone(),
            session: ServerWsSession::Outgoing(ctx.address()),
        });
    }
}

impl StreamHandler<OwnedMessage, WebSocketError> for OutgoingSession {
    fn handle(&mut self, msg: OwnedMessage, _ctx: &mut Context<Self>) {
        match msg {
            OwnedMessage::Ping(msg) => {
                self.sender.send_message(&OwnedMessage::Ping(msg)).unwrap(); // TODO unwraps
            },
            OwnedMessage::Text(_text) => {
                self.sender.send_message(
                    &OwnedMessage::Binary(ServerWsSession::handle_text().into()),
                ).unwrap();
            },
            OwnedMessage::Binary(bin) => {
                if let Some(bin) = ServerWsSession::handle_binary(bin.into(), &mut self.server) {
                    self.sender.send_message(&OwnedMessage::Binary(bin.into())).unwrap();
                }
            },
            _ => println!("wat?"),
        }
    }
}

impl Handler<SendMessage> for OutgoingSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage, _: &mut Context<Self>) {
        self.sender.send_message(&OwnedMessage::Binary(msg.into())).unwrap();
    }
}
