use super::*;
use crate::database::*;
use crate::federation::FederationServer;
use crate::SendMessage;
use actix::prelude::*;
use actix_web_actors::ws::{self, WebsocketContext};
use futures::future::{self, Either};
use lazy_static::lazy_static;
use rand::RngCore;
use std::io::Cursor;
use std::time::Instant;
use tokio_postgres::error::SqlState;
use tokio_threadpool::ThreadPool;
use uuid::Uuid;
use vertex_common::*;

lazy_static! {
    static ref THREAD_POOL: ThreadPool = ThreadPool::new();
}

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

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut WebsocketContext<Self>) {
        self.start_heartbeat(ctx);
    }

    fn stopped(&mut self, _ctx: &mut WebsocketContext<Self>) {
        self.client_server.do_send(Disconnect {
            session_id: self.session_id,
            user_id: self.state.user_id(),
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
                    user_id: self.state.user_id(),
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

    fn start_heartbeat(&mut self, ctx: &mut WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_TIMEOUT, |session, ctx| {
            if Instant::now().duration_since(session.heartbeat) > HEARTBEAT_TIMEOUT {
                session.client_server.do_send(Disconnect {
                    session_id: session.session_id,
                    user_id: session.state.user_id(),
                });
                ctx.stop();
            }
        });
    }

    /// Responds to a request with a future which will eventually resolve to the request response
    fn respond<F>(&mut self, fut: F, request_id: RequestId, ctx: &mut WebsocketContext<Self>)
    where
        F: ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
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

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        match req.message {
            ClientMessage::Login(login) => self.login(login, req.request_id, ctx),
            ClientMessage::CreateUser { name, password } => {
                self.create_user(name, password, req.request_id, ctx)
            }
            m => self.handle_authenticated_message(m, req.request_id, ctx),
        };
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientMessage,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        match self.state {
            SessionState::WaitingForLogin => self.respond(
                futures::future::ok(RequestResponse::Error(ServerError::NotLoggedIn))
                    .into_actor(self),
                request_id,
                ctx,
            ),
            SessionState::Ready(id) => match msg {
                ClientMessage::SendMessage(msg) => self.respond(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg,
                        })
                        .into_actor(self),
                    request_id,
                    ctx,
                ),
                ClientMessage::EditMessage(edit) => self.respond(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: edit,
                        })
                        .into_actor(self),
                    request_id,
                    ctx,
                ),
                ClientMessage::JoinRoom(room) => self.respond(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: Join { room },
                        })
                        .into_actor(self),
                    request_id,
                    ctx,
                ),
                ClientMessage::CreateRoom => self.respond(
                    self.client_server
                        .send(IdentifiedMessage {
                            user_id: id,
                            session_id: self.session_id,
                            request_id,
                            msg: CreateRoom,
                        })
                        .into_actor(self),
                    request_id,
                    ctx,
                ),
                _ => unimplemented!(),
            },
        }
    }

    fn login(&mut self, login: Login, request_id: RequestId, ctx: &mut WebsocketContext<Self>) {
        let client_server = self.client_server.clone();
        let session_id = self.session_id.clone();
        let session = ctx.address().clone();

        let fut = self
            .database_server
            .send(GetUserByName(login.name.clone()))
            .and_then(move |user_opt| match user_opt {
                Ok(Some(user)) => {
                    let User { id: user_id, name: _, password_hash } = user;
                    Either::A(
                        verify(login.password, password_hash)
                            .and_then(move |matches| {
                                if matches {
                                    Either::A(
                                        client_server
                                            .send(Connect {
                                                session,
                                                session_id,
                                                user_id,
                                            })
                                            .and_then(move |_| {
                                                future::ok(RequestResponse::user(user_id))
                                            }),
                                    )
                                } else {
                                    Either::B(future::ok(RequestResponse::Error(
                                        ServerError::IncorrectPassword,
                                    )))
                                }
                            }),
                    )
                }
                Ok(None) => Either::B(future::ok(RequestResponse::Error(
                    ServerError::UserDoesNotExist,
                ))),
                Err(e) => {
                    eprintln!("Database error: {:?}", e);
                    Either::B(future::ok(RequestResponse::Error(ServerError::Internal)))
                }
            })
            .into_actor(self)
            .and_then(|res, act, _ctx| {
                if let RequestResponse::Success(Success::User { id }) = res {
                    act.state = SessionState::Ready(id);
                }

                actix::fut::ok(res)
            });

        self.respond(fut, request_id, ctx);
    }

    fn create_user(
        &mut self,
        name: String,
        password: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {

        // TODO configure this more ^? Not really sure what to do...
        let id = UserId(Uuid::new_v4());

        let fut = hash(password)
            .into_actor(self)
            .and_then(move |password_hash, act, _ctx| {
                act.database_server
                    .send(CreateUser(User {
                        id,
                        name,
                        password_hash,
                    }))
                    .into_actor(act)
            })
            .map(move |res, _act, _ctx| match res {
                Ok(_) => RequestResponse::user(id),
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(e) => {
                        eprintln!("Database connection pooling error: {:?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    },
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
            });

        self.respond(fut, request_id, ctx)
    }
}

fn hash<E: Send + 'static>(pass: String) -> impl Future<Item = String, Error = E> {
    THREAD_POOL
        .spawn_handle(future::poll_fn(move || {
            tokio_threadpool::blocking(|| {
                let mut salt: [u8; 32] = [0; 32];
                rand::thread_rng().fill_bytes(&mut salt);
                let config = Default::default();

                argon2::hash_encoded(pass.as_bytes(), &salt, &config)
                    .expect("Error generating password hash")
            }).map_err(|_| panic!("the threadpool shut down"))
        }))
}

fn verify<E: Send + 'static>(pass: String, hash: String) -> impl Future<Item = bool, Error = E> {
    THREAD_POOL
        .spawn_handle(future::poll_fn(move || {
            tokio_threadpool::blocking(|| {
                argon2::verify_encoded(&hash, pass.as_bytes())
                    .expect("Error verifying password hash")
            }).map_err(|_| panic!("the threadpool shut down"))
        }))
}
