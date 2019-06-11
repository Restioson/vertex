use super::*;
use crate::federation::FederationServer;
use crate::SendMessage;
use actix::prelude::*;
use actix_web::web::Bytes;
use actix_web_actors::ws::{self, WebsocketContext};
use std::io::Cursor;
use vertex_common::*;

#[derive(Eq, PartialEq)]
enum SessionState {
    WaitingForLogin,
    Ready(UserId), // TODO sessionid?
}

pub struct ClientWsSession {
    client_server: Addr<ClientServer>,
    federation_server: Addr<FederationServer>,
    state: SessionState,
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
        }
    }
}

impl ClientWsSession {
    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        let response = match req.message {
            ClientMessage::Login(login) => {
                // Register with the server
                self.client_server.do_send(Connect {
                    session: ctx.address(),
                    login: login.clone(),
                });
                self.state = SessionState::Ready(login.id);
                RequestResponse::success()
            }
            _ => self.handle_authenticated_message(req.message, ctx),
        };

        let response = ServerMessage::Response {
            response,
            request_id: req.request_id,
        };

        let binary: Bytes = response.into();
        ctx.binary(binary);
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientMessage,
        ctx: &mut WebsocketContext<Self>,
    ) -> RequestResponse {
        match self.state {
            SessionState::WaitingForLogin => RequestResponse::Error(ServerError::NotLoggedIn),
            SessionState::Ready(id) => {
                match msg {
                    ClientMessage::SendMessage(msg) => {
                        self.client_server.do_send(IdentifiedMessage { id, msg });
                        RequestResponse::success()
                    }
                    ClientMessage::EditMessage(edit) => {
                        self.client_server
                            .do_send(IdentifiedMessage { id, msg: edit });
                        RequestResponse::success()
                    }
                    ClientMessage::JoinRoom(room) => {
                        // TODO check that it worked lol
                        self.client_server.do_send(IdentifiedMessage {
                            id,
                            msg: Join { room },
                        });
                        RequestResponse::success()
                    }
                    ClientMessage::CreateRoom => {
                        let id = self
                            .client_server
                            .send(IdentifiedMessage {
                                id,
                                msg: CreateRoom,
                            })
                            .wait()
                            .unwrap();
                        RequestResponse::Success(Success::Room { id })
                    }
                    _ => unimplemented!(),
                }
            }
        }
    }
}

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for ClientWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut WebsocketContext<Self>) {
        match msg {
            ws::Message::Ping(msg) => ctx.pong(&msg),
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
