use std::io::Cursor;
use std::time::Duration;
use actix::prelude::*;
use actix_web_actors::ws;
use websocket::message::OwnedMessage;
use websocket::client::builder::ClientBuilder;
use url::Url;
use vertex_common::*;
use super::*;
use crate::federation::{OutgoingSession, FederationServer, WsReaderStreamAdapter};

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
                        self.client_server.do_send(publish);

                        let success = serde_cbor::to_vec(&ServerMessage::success()).unwrap();
                        ctx.binary(success);
                    },
                    ClientMessage::RequestInitKey(request) => {
                        match self.client_server.send(request).wait() {
                            Ok(Some(key)) => {
                                let key = serde_cbor::to_vec(
                                    &ServerMessage::Success(Success::Key(key))
                                ).unwrap();
                                ctx.binary(key);
                            },
                            Ok(None) => {
                                let err = serde_cbor::to_vec(
                                    &ServerMessage::Error(Error::IdNotFound)
                                ).unwrap();
                                ctx.binary(err);
                            },
                            Err(_) => {
                                let err = serde_cbor::to_vec(
                                    &ServerMessage::Error(Error::Internal)
                                ).unwrap();
                                ctx.binary(err)
                            }
                        }
                    },
                    ClientMessage::Federate(federate) => {
                        // TODO check url is valid

                        let res = catch! {
                            ClientBuilder::new(&federate.url)
                                .map_err(|_| Error::InvalidUrl)?
                                .connect_insecure()
                                .map_err(|_| Error::WsConnectionError)
                        };

                        let client = match res {
                            Ok(c) => c,
                            Err(e) => {
                                let err = serde_cbor::to_vec(&ServerMessage::Error(e)).unwrap();
                                return ctx.binary(err);
                            }
                        };

                        client.stream_ref()
                            .set_read_timeout(Some(Duration::from_micros(1)))
                            .unwrap();

                        client.stream_ref()
                            .set_write_timeout(Some(Duration::from_millis(1000)))
                            .unwrap();

                        let (reader, mut writer) = client.split().unwrap();

                        let args = std::env::args().collect::<Vec<_>>();
                        let port = args.get(1).cloned().unwrap_or("8080".to_string());
                        writer.send_message(
                            &OwnedMessage::Text(format!("http://127.0.0.1:{}", port))
                        ).unwrap();

                        let addr = self.federation_server.clone();
                        OutgoingSession::create(move |session_ctx| {
                            session_ctx.add_stream(WsReaderStreamAdapter(reader));
                            OutgoingSession::new(
                                Url::parse(&federate.url).expect("url invalid"), // TODO ^
                                addr,
                                writer,
                            )
                        });

                        println!("ya");

                        let success = serde_cbor::to_vec(&ServerMessage::success()).unwrap();
                        ctx.binary(success);
                    },
                }
            },
            _ => (),
        }
    }
}
