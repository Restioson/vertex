use actix::prelude::*;
use actix_web_actors::ws;
use websocket::message::OwnedMessage;
use websocket::result::WebSocketError;
use websocket::sender::Writer;
use websocket::receiver::Reader;
use websocket::stream::sync::TcpStream;
use futures::Async;
use super::FederationServer;

pub struct IncomingServerWsSession {
    server: Addr<FederationServer>,
}

impl IncomingServerWsSession {
    pub fn new(server: Addr<FederationServer>) -> Self {
        IncomingServerWsSession { server, }
    }
}

impl Actor for IncomingServerWsSession {
    type Context = ws::WebsocketContext<Self>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for IncomingServerWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Ping(msg) => {
                println!("ping {:?}", msg);
                ctx.pong(&msg)
            },
            ws::Message::Text(text) => {
                println!("inc from inc text {:?}", text);
                ctx.text(text)
            },
            ws::Message::Binary(bin) => {
                println!("binary {:?}", bin);
                ctx.binary(bin)
            },
            _ => println!("wat?"),
        }
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

pub struct OutgoingServerWsSession {
    sender: Writer<TcpStream>,
    server: Addr<FederationServer>,
}

impl OutgoingServerWsSession {
    pub fn new(
        server: Addr<FederationServer>,
        sender: Writer<TcpStream>,
    ) -> Self {
        OutgoingServerWsSession { sender, server, }
    }
}

impl Actor for OutgoingServerWsSession {
    type Context = Context<Self>;
}

impl StreamHandler<OwnedMessage, WebSocketError> for OutgoingServerWsSession {
    fn handle(&mut self, msg: OwnedMessage, _ctx: &mut Self::Context) {
        match msg {
            OwnedMessage::Ping(msg) => {
                println!("ping {:?}", msg);
                self.sender.send_message(&OwnedMessage::Ping(msg)).unwrap(); // TODO unwraps
            },
            OwnedMessage::Text(text) => {
                println!("inc from out text {:?}", text);
                self.sender.send_message(&OwnedMessage::Text(text)).unwrap();
            },
            OwnedMessage::Binary(bin) => {
                println!("binary {:?}", bin);
                self.sender.send_message(&OwnedMessage::Binary(bin)).unwrap();
            },
            _ => println!("wat?"),
        }
    }
}
