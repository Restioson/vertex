use super::*;
use crate::database::*;
use crate::federation::FederationServer;
use crate::SendMessage;
use actix::prelude::*;
use actix_web_actors::ws::{self, WebsocketContext};
use futures::future;
use rand::RngCore;
use std::io::Cursor;
use std::time::Instant;
use tokio_postgres::error::SqlState;
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
    database_server: Addr<DatabaseServer>,
    federation_server: Addr<FederationServer>,
    state: SessionState,
    heartbeat: Instant,
    session_id: SessionId,
}

impl ClientWsSession {
    pub fn new(
        client_server: Addr<ClientServer>,
        federation_server: Addr<FederationServer>,
        database_server: Addr<DatabaseServer>,
    ) -> Self {
        ClientWsSession {
            client_server,
            database_server,
            federation_server,
            state: SessionState::WaitingForLogin,
            heartbeat: Instant::now(),
            session_id: SessionId(Uuid::new_v4()),
        }
    }

    fn user_id(&self) -> Option<UserId> {
        self.state.user_id()
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        let request_id = req.request_id.clone();

        let fut: Box<dyn ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self>> =
            match req.message {
                ClientMessage::Login(login) => Box::new(self.login(login, ctx)),
                ClientMessage::CreateUser { name, password } => {
                    Box::new(self.create_user(name, password, ctx))
                }
                m => self.handle_authenticated_message(m, req.request_id, ctx),
            };

        fut.then(move |response, act, ctx| {
            let response = ServerMessage::Response {
                response: if let Ok(r) = response {
                    r
                } else {
                    RequestResponse::Error(ServerError::Internal)
                },
                request_id,
            };

            ctx.binary(response);
            actix::fut::ok(())
        })
        .wait(ctx);
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientMessage,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) -> Box<dyn ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self>> {
        match self.state {
            SessionState::WaitingForLogin => Box::new(
                futures::future::ok(RequestResponse::Error(ServerError::NotLoggedIn))
                    .into_actor(self),
            ),
            SessionState::Ready(id) => match msg {
                ClientMessage::SendMessage(msg) => Box::new(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg,
                        })
                        .into_actor(self),
                ),
                ClientMessage::EditMessage(edit) => Box::new(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: edit,
                        })
                        .into_actor(self),
                ),
                ClientMessage::JoinRoom(room) => Box::new(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: Join { room },
                        })
                        .into_actor(self),
                ),
                ClientMessage::CreateRoom => Box::new(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: CreateRoom,
                        })
                        .into_actor(self),
                ),
                _ => unimplemented!(),
            },
        }
    }

    fn login(
        &mut self,
        login: Login,
        ctx: &mut WebsocketContext<Self>,
    ) -> impl ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self> {
        let client_server = self.client_server.clone();
        let session_id = self.session_id.clone();
        let session = ctx.address().clone();

        self.database_server
            .send(GetUserByName(login.name.clone()))
            .and_then(move |user_opt| match user_opt {
                Ok(Some(user)) => {
                    let matches =
                        argon2::verify_encoded(&user.password_hash, login.password.as_bytes())
                            .expect("Error verifying password hash");

                    let fut: Box<dyn Future<Item = RequestResponse, Error = MailboxError>> =
                        if matches {
                            Box::new(
                                client_server
                                    .send(Connect {
                                        session,
                                        session_id,
                                        user_id: user.id,
                                    })
                                    .and_then(move |_| future::ok(RequestResponse::user(user.id))),
                            )
                        } else {
                            Box::new(future::ok(RequestResponse::Error(
                                ServerError::IncorrectPassword,
                            )))
                        };
                    fut
                }
                Ok(None) => Box::new(future::ok(RequestResponse::Error(
                    ServerError::UserDoesNotExist,
                ))),
                Err(e) => {
                    eprintln!("Database error: {:?}", e);
                    Box::new(future::ok(RequestResponse::Error(ServerError::Internal)))
                }
            })
            .into_actor(self)
            .and_then(|res, act, ctx| {
                if let RequestResponse::Success(Success::User { id }) = res {
                    act.state = SessionState::Ready(id);
                }

                actix::fut::ok(res)
            })
    }

    fn create_user(
        &mut self,
        name: String,
        password: String,
        ctx: &mut WebsocketContext<Self>,
    ) -> impl ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self> {
        let mut salt: [u8; 32] = [0; 32];
        rand::thread_rng().fill_bytes(&mut salt);

        let mut config = Default::default();
        // TODO configure this more ^? Not really sure what to do...

        let password_hash = argon2::hash_encoded(password.as_bytes(), &salt, &config)
            .expect("Error generating password hash");

        let id = UserId(Uuid::new_v4());

        self.database_server
            .send(CreateUser(User {
                id,
                name,
                password_hash,
            }))
            .map(move |res| match res {
                Ok(_) => RequestResponse::user(id),
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(_) => RequestResponse::Error(ServerError::Internal),
                    l337::Error::External(sql_error) => {
                        if sql_error.code() == Some(&SqlState::INTEGRITY_CONSTRAINT_VIOLATION)
                            || sql_error.code() == Some(&SqlState::UNIQUE_VIOLATION)
                        {
                            RequestResponse::Error(ServerError::UsernameAlreadyExists)
                        } else {
                            eprintln!("Database error: {:?}", sql_error.code());
                            RequestResponse::Error(ServerError::Internal)
                        }
                    }
                },
            })
            .into_actor(self)
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
