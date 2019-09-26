use super::{Connect, FederationServer};
use crate::SendMessage;
use actix::prelude::*;
use actix_web::web::{Bytes, Data, HttpRequest, HttpResponse, Payload};
use actix_web_actors::ws::{self, WebsocketContext};
use futures::Async;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    io::{self, Cursor},
};
use url::Url;
use websocket::message::OwnedMessage;
use websocket::receiver::Reader;
use websocket::result::WebSocketError;
use websocket::sender::Writer;
use websocket::stream::sync::TcpStream;

#[derive(Debug, Serialize, Deserialize)]
pub enum FederationMessage {
    RequestResponse(RequestResponse),
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
pub enum RequestResponse {
    NoData,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Error {
    UnexpectedText,
}

impl<'a, T: Into<Bytes> + Debug + Serialize + Deserialize<'a>> Into<Bytes> for SendMessage<T> {
    fn into(self) -> Bytes {
        self.message.into()
    }
}

impl<'a, T: Into<Vec<u8>> + Debug + Serialize + Deserialize<'a>> Into<Vec<u8>> for SendMessage<T> {
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
        _server: &mut Addr<FederationServer>,
    ) -> Option<FederationMessage> {
        let cursor = Cursor::new(binary);
        let msg = serde_cbor::from_reader(cursor).expect("invaild bytes"); // TODO <- return properly

        match msg {
            FederationMessage::RequestResponse(s) => {
                println!("Msg from federated server: {:?}", s);
                Some(FederationMessage::RequestResponse(RequestResponse::NoData))
            }
            FederationMessage::Error(e) => {
                eprintln!("Error from federated server: {:?}", e);
                None
            }
        }
    }
}

pub struct IncomingSession {
    from: Option<Url>,
    server: Addr<FederationServer>,
}

impl IncomingSession {
    fn new(server: Addr<FederationServer>) -> Self {
        IncomingSession { from: None, server }
    }
}

impl Actor for IncomingSession {
    type Context = WebsocketContext<Self>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for IncomingSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut WebsocketContext<Self>) {
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Text(text) => {
                if self.from.is_none() {
                    println!("url: {}", text); //TODO
                    self.server.do_send(Connect {
                        url: Url::parse(&text).expect("invalid url"), // TODO <- return err properly
                        session: ServerWsSession::Incoming(ctx.address()),
                    });
                    ctx.binary(FederationMessage::RequestResponse(RequestResponse::NoData));
                } else {
                    ctx.binary(ServerWsSession::handle_text()) // todo get rid of
                }
            }
            ws::Message::Binary(bin) => {
                if let Some(bin) = ServerWsSession::handle_binary(bin, &mut self.server) {
                    ctx.binary(bin)
                }
            }
            _ => (),
        }
    }
}

impl Handler<SendMessage<FederationMessage>> for IncomingSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<FederationMessage>, ctx: &mut WebsocketContext<Self>) {
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

pub struct OutgoingSession {
    to: Url,
    sender: Writer<TcpStream>,
    server: Addr<FederationServer>,
}

impl OutgoingSession {
    pub fn new(to: Url, server: Addr<FederationServer>, sender: Writer<TcpStream>) -> Self {
        OutgoingSession { to, sender, server }
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
            }
            OwnedMessage::Text(_text) => {
                self.sender
                    .send_message(&OwnedMessage::Binary(ServerWsSession::handle_text().into()))
                    .unwrap();
            }
            OwnedMessage::Binary(bin) => {
                if let Some(bin) = ServerWsSession::handle_binary(bin.into(), &mut self.server) {
                    self.sender
                        .send_message(&OwnedMessage::Binary(bin.into()))
                        .unwrap();
                }
            }
            _ => (),
        }
    }
}

impl Handler<SendMessage<FederationMessage>> for OutgoingSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<FederationMessage>, _: &mut Context<Self>) {
        self.sender
            .send_message(&OwnedMessage::Binary(msg.into()))
            .unwrap();
    }
}
