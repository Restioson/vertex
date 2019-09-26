use super::*;
use crate::federation::FederationServer;
use crate::SendMessage;
use actix::prelude::*;
use actix_web::web::Bytes;
use actix_web_actors::ws::{self, WebsocketContext};
use std::io::Cursor;
use std::time::Instant;
use uuid::Uuid;
use vertex_common::*;

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

    fn handle_message(&mut self, req: ClientMessage, ctx: &mut WebsocketContext<Self>) {
        let response = match req.request {
            ClientRequest::Login(login) => {
                // Register with the server
                self.client_server.do_send(Connect {
                    session: ctx.address(),
                    session_id: self.session_id,
                    login: login.clone(),
                });
                self.state = SessionState::Ready(login.id);
                Ok(RequestResponse::NoData)
            }
            _ => self.handle_authenticated_message(req.request, ctx),
        };

        let response = ServerMessage::Response {
            response,
            request_id: req.id,
        };

        let binary: Bytes = response.into();
        ctx.binary(binary);
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientRequest,
        _ctx: &mut WebsocketContext<Self>,
    ) -> Result<RequestResponse, ServerError> {
        match self.state {
            SessionState::WaitingForLogin => Err(ServerError::NotLoggedIn),
            SessionState::Ready(id) => {
                match msg {
                    ClientRequest::SendMessage(msg) => {
                        self.client_server.do_send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            msg,
                        });
                        Ok(RequestResponse::NoData)
                    }
                    ClientRequest::EditMessage(edit) => {
                        self.client_server.do_send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            msg: edit,
                        });
                        Ok(RequestResponse::NoData)
                    }
                    ClientRequest::JoinRoom(room) => {
                        // TODO check that it worked lol
                        self.client_server.do_send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            msg: Join { room },
                        });
                        Ok(RequestResponse::NoData)
                    }
                    ClientRequest::CreateRoom => {
                        let id = self
                            .client_server
                            .send(IdentifiedMessage {
                                user_id: id,
                                session_id: self.session_id,
                                msg: CreateRoom,
                            })
                            .wait()
                            .unwrap();
                        Ok(RequestResponse::Room { id })
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
