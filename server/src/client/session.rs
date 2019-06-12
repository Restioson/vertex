use actix::prelude::*;
use actix_web_actors::ws::{self, WebsocketContext};
use std::io::Cursor;
use std::time::Instant;
use uuid::Uuid;
use vertex_common::*;
use super::*;
use crate::federation::FederationServer;
use crate::{SendMessage, LoggedIn};

#[derive(Eq, PartialEq)]
enum SessionState {
    WaitingForLogin,
    Ready(UserId),
}

impl SessionState {
    fn user_id(&self) -> Option<UserId> {
        match self {
            SessionState::WaitingForLogin => None,
            SessionState::Ready(id) => Some(id.clone()),
        }
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct SessionId(pub Uuid);

pub struct ClientWsSession {
    client_server: Addr<ClientServer>,
    federation_server: Addr<FederationServer>,
    state: SessionState,
    heartbeat: Instant,
    session_id: SessionId,
}

impl ClientWsSession {
    pub fn new(
        client_server: Addr<ClientServer>,
        federation_server: Addr<FederationServer>,
    ) -> Self {
        ClientWsSession {
            client_server,
            federation_server,
            state: SessionState::WaitingForLogin,
            heartbeat: Instant::now(),
            session_id: SessionId(Uuid::new_v4()),
        }
    }
}

impl ClientWsSession {
    fn user_id(&self) -> Option<UserId> {
        self.state.user_id()
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        let fut = match req.message {
            ClientMessage::Login(login) => {
                // Register with the server
                Box::new(self.client_server.send(Connect {
                    session: ctx.address(),
                    session_id: self.session_id,
                    request_id: req.request_id,
                    login: login.clone(),
                }))
            },
            _ => self.handle_authenticated_message(req.message, req.request_id, ctx),
        };

        Arbiter::spawn(fut.map_err(|e| panic!("Mailbox error {:?}", e)));
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientMessage,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) -> Box<dyn Future<Item = (), Error = actix::MailboxError>> {
        match self.state {
            SessionState::WaitingForLogin => {
                ctx.binary(ServerMessage::Response {
                    response: RequestResponse::Error(ServerError::NotLoggedIn),
                    request_id,
                });
                Box::new(futures::future::ok(()))
            },
            SessionState::Ready(id) => {
                match msg {
                    ClientMessage::SendMessage(msg) => {
                        Box::new(self.client_server.send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg,
                        }))
                    }
                    ClientMessage::EditMessage(edit) => {
                        Box::new(self.client_server.send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: edit,
                        }))
                    }
                    ClientMessage::JoinRoom(room) => {
                        // TODO check that it worked lol
                        Box::new(self.client_server.send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: Join { room },
                        }))
                    }
                    ClientMessage::CreateRoom => {
                        Box::new(self.client_server
                            .send(IdentifiedMessage {
                                user_id: id,
                                session_id: self.session_id,
                                request_id,
                                msg: CreateRoom,
                        }))
                    }
                    _ => unimplemented!(),
                }
            }
        }
    }

    fn start_heartbeat(&mut self, ctx: &mut WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_TIMEOUT, |session, ctx| {
            if Instant::now().duration_since(session.heartbeat) > HEARTBEAT_TIMEOUT {
                session.client_server.do_send(Disconnect {
                    session_id: session.session_id,
                    user_id: session.user_id(),
                });
                ctx.stop();
            }
        });
    }
}

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut WebsocketContext<Self>) {
        self.start_heartbeat(ctx);
    }

    fn stopped(&mut self, ctx: &mut WebsocketContext<Self>) {
        self.client_server.do_send(Disconnect {
            session_id: self.session_id,
            user_id: self.user_id(),
        });
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for ClientWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut WebsocketContext<Self>) {
        match msg {
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.heartbeat = Instant::now();
            }
            ws::Message::Text(_) => {
                let error =
                    serde_cbor::to_vec(&ServerMessage::Error(ServerError::UnexpectedTextFrame))
                        .unwrap();
                ctx.binary(error);
            }
            ws::Message::Binary(bin) => {
                let mut bin = Cursor::new(bin);
                let msg = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        let error =
                            serde_cbor::to_vec(&ServerMessage::Error(ServerError::InvalidMessage))
                                .unwrap();
                        return ctx.binary(error);
                    }
                };

                self.handle_message(msg, ctx);
            }
            ws::Message::Close(_) => {
                self.client_server.do_send(Disconnect {
                    session_id: self.session_id,
                    user_id: self.user_id(),
                });
                ctx.stop();
            }
            ws::Message::Nop => (),
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ServerMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg);
    }
}

impl Handler<LoggedIn> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, logged_in: LoggedIn, ctx: &mut WebsocketContext<Self>) {
        self.state = SessionState::Ready(logged_in.0);
    }
}
