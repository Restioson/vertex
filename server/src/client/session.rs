use std::io::Cursor;
use std::time::Duration;
use actix::prelude::*;
use actix_web_actors::ws;
use websocket::message::OwnedMessage;
use websocket::client::builder::ClientBuilder;
use vertex_common::*;
use super::*;
use crate::federation::{OutgoingServerWsSession, FederationServer, WsReaderStreamAdapter};

pub struct ClientWsSession {
    client_server: Addr<ClientServer>,
    federation_server: Addr<FederationServer>,
}

impl ClientWsSession {
    pub fn new(client_server: Addr<ClientServer>, federation_server: Addr<FederationServer>) -> Self {
        ClientWsSession {
            client_server,
            federation_server,
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
                        self.client_server.do_send(publish);
                        // TODO return
                    },
                    ClientMessage::RequestInitKey(request) => { // TODO do with actual api
                        // TODO check that request is valid
                        match self.client_server.send(request).wait() {
                            Ok(Some(key)) => ctx.binary(key.bytes()),
                            Ok(None) => ctx.text("no key for id"),
                            Err(e) => ctx.text(format!("error executing: {:?}", e))
                        }
                    },
                    ClientMessage::Federate(federate) => {
                        // TODO check url is valid
                        let mut client = ClientBuilder::new(&federate.url)
                            .expect("error making builder")
                            .connect_insecure()
                            .expect("Error connecting to server"); // TODO wss/https

                        client.stream_ref()
                            .set_read_timeout(Some(Duration::from_micros(1)))
                            .unwrap();

                        client.stream_ref()
                            .set_write_timeout(Some(Duration::from_millis(1000)))
                            .unwrap();

                        let (mut reader, mut writer) = client.split().unwrap();
                        writer.send_message(&OwnedMessage::Text("hi".to_string())).unwrap();

                        let addr = self.federation_server.clone();
                        let session = OutgoingServerWsSession::create(move |session_ctx| {
                            session_ctx.add_stream(WsReaderStreamAdapter(reader));
                            OutgoingServerWsSession::new(
                                addr,
                                writer,
                            )
                        });

                        println!("ya");

                        let success = serde_cbor::to_vec(&ServerMessage::Success).unwrap();
                        ctx.binary(success);
                    },
                }
            },
            _ => (),
        }
    }
}
