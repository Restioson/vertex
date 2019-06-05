use std::time::Duration;
use std::io::{Write, Cursor};
use actix::prelude::*;
use actix_web_actors::ws;
use awc::Client;
use vertex_common::*;
use super::*;

pub struct ClientWsSession {
    server: Addr<ClientServer>,
}

impl ClientWsSession {
    pub fn new(server: Addr<ClientServer>) -> Self {
        ClientWsSession {
            server,
        }
    }
}

impl Actor for ClientWsSession {
    type Context = ws::WebsocketContext<Self>;
}

/// Handler for ws::Message message
impl StreamHandler<ws::Message, ws::ProtocolError> for ClientWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
            ws::Message::Text(_) => {
                let error = serde_cbor::to_vec(&ServerMessage::Error(Error::UnexpectedTextFrame))
                    .unwrap();
                ctx.binary(error);
            },
            ws::Message::Binary(bin) => {
                let mut bin = Cursor::new(bin);
                let msg = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        let error = serde_cbor::to_vec(&ServerMessage::Error(Error::InvalidMessage))
                            .unwrap();
                        return ctx.binary(error);
                    }
                };

                match msg {
                    ClientMessage::PublishInitKey(publish) => {
                        // TODO check is valid
                        self.server.do_send(publish);
                    },
                    ClientMessage::RequestInitKey(request) => {
                        // TODO check that request is valid
                        match self.server.send(request).wait() {
                            Ok(Some(key)) => ctx.binary(key.bytes()),
                            Ok(None) => ctx.text("no key for index"),
                            Err(e) => ctx.text(format!("error executing: {:?}", e))
                        }
                    },
                    ClientMessage::Federate(federate) => {
                        // TODO check url is valid
                        actix::spawn(
                            Client::build()
                                .timeout(Duration::from_millis(300))
                                .finish()
                                .ws(federate.url)
                                .connect()
                                .map(|(_response, mut socket)| {
                                    println!("ya");
                                    socket.get_mut().write(b"yes").unwrap();
                                })
                                .map_err(|_| eprintln!("oops"))
                        );

                        let success = serde_cbor::to_vec(&ServerMessage::Success).unwrap();
                        ctx.binary(success);
                    },
                }
            },
            _ => (),
        }
    }
}
