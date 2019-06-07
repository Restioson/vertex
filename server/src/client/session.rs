use std::io::Cursor;
use std::time::Duration;
use actix::prelude::*;
use actix_web_actors::ws::{self, WebsocketContext};
use websocket::message::OwnedMessage;
use websocket::client::builder::ClientBuilder;
use url::Url;
use uuid::Uuid;
use vertex_common::*;
use super::*;
use crate::SendMessage;
use crate::federation::{OutgoingSession, FederationServer, WsReaderStreamAdapter};

#[derive(Eq, PartialEq)]
enum SessionState {
    WaitingForLogin,
    Ready(Uuid),
}

impl SessionState {
    fn logged_in(&self) -> bool {
        match self {
            SessionState::WaitingForLogin => false,
            SessionState::Ready(_) => true,
        }
    }
}

pub struct ClientWsSession {
    client_server: Addr<ClientServer>,
    federation_server: Addr<FederationServer>,
    state: SessionState,
}

impl ClientWsSession {
    pub fn new(client_server: Addr<ClientServer>, federation_server: Addr<FederationServer>) -> Self {
        ClientWsSession {
            client_server,
            federation_server,
            state: SessionState::WaitingForLogin,
        }
    }

    fn logged_in(&self) -> bool {
        self.state.logged_in()
    }
}

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;
}

// TODO break up handle into smaller methods
impl StreamHandler<ws::Message, ws::ProtocolError> for ClientWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut WebsocketContext<Self>) {
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
                    // Log in to the server
                    ClientMessage::Login(login) => {
                        // Register with the server
                        self.client_server.do_send(Connect {
                            session: ctx.address(),
                            login: login.clone(),
                        });
                        self.state = SessionState::Ready(login.id);

                        let success = serde_cbor::to_vec(&ServerMessage::success()).unwrap();
                        ctx.binary(success);
                    },
                    // Send a message to the server
                    ClientMessage::SendMessage(msg) => {
                        match self.state {
                            SessionState::WaitingForLogin => {
                                let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                    .unwrap();
                                ctx.binary(err);
                            },
                            SessionState::Ready(id) => {
                                self.client_server.do_send(IdentifiedMessage { id, msg });

                                let success = serde_cbor::to_vec(&ServerMessage::success()).unwrap();
                                ctx.binary(success);
                            },
                        }
                    },
                    ClientMessage::JoinRoom(room) => {
                        match self.state {
                            SessionState::WaitingForLogin => {
                                let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                    .unwrap();
                                ctx.binary(err);
                            },
                            SessionState::Ready(id) => {
                                self.client_server.do_send(
                                    IdentifiedMessage { id, msg: Join { room } }
                                ); // TODO check that it worked lol

                                let success = serde_cbor::to_vec(
                                    &ServerMessage::Success(Success::NoData)
                                ).unwrap();

                                ctx.binary(success);
                            },
                        }
                    }
                    // Create a room
                    ClientMessage::CreateRoom => {
                        match self.state {
                            SessionState::WaitingForLogin => {
                                let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                    .unwrap();
                                ctx.binary(err);
                            },
                            SessionState::Ready(id) => {
                                let id = self.client_server.send(
                                    IdentifiedMessage { id, msg: CreateRoom }
                                )
                                    .wait()
                                    .unwrap();

                                let success = serde_cbor::to_vec(
                                    &ServerMessage::Success(Success::Room { id: *id })
                                ).unwrap();

                                ctx.binary(success);
                            },
                        }
                    }
                    // Publish an initialisation key from the server
                    ClientMessage::PublishInitKey(publish) => {
                        if !self.logged_in() {
                            let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                .unwrap();
                            return ctx.binary(err);
                        }

                        self.client_server.do_send(publish);

                        let success = serde_cbor::to_vec(&ServerMessage::success()).unwrap();
                        ctx.binary(success);
                    },
                    // Request an initialisation key from the server
                    ClientMessage::RequestInitKey(request) => {
                        if !self.logged_in() {
                            let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                .unwrap();
                            return ctx.binary(err);
                        }

                        match self.client_server.send(request).wait() {
                            Ok(Some(key)) => { // Key returned
                                let key = serde_cbor::to_vec(
                                    &ServerMessage::Success(Success::Key(key))
                                ).unwrap();
                                ctx.binary(key);
                            },
                            Ok(None) => { // No key
                                let err = serde_cbor::to_vec(
                                    &ServerMessage::Error(Error::IdNotFound)
                                ).unwrap();
                                ctx.binary(err);
                            },
                            Err(_) => { // Internal error (with actor?)
                                let err = serde_cbor::to_vec(
                                    &ServerMessage::Error(Error::Internal)
                                ).unwrap();
                                ctx.binary(err)
                            }
                        }
                    },
                    // Federate with another server
                    ClientMessage::Federate(federate) => {
                        if !self.logged_in() {
                            let err = serde_cbor::to_vec(&ServerMessage::Error(Error::NotLoggedIn))
                                .unwrap();
                            return ctx.binary(err);
                        }

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

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ServerMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg);
    }
}
