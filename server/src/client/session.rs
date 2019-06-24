use super::*;
use crate::auth;
use crate::database::*;
use crate::federation::FederationServer;
use crate::Config;
use crate::SendMessage;
use actix::prelude::*;
use actix_web::web::Data;
use actix_web_actors::ws::{self, WebsocketContext};
use chrono::DateTime;
use chrono::Utc;
use futures::future::{self, Either};
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
    config: Data<Config>,
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
        config: Data<Config>,
    ) -> Self {
        ClientWsSession {
            client_server,
            database_server,
            federation_server,
            state: SessionState::WaitingForLogin,
            heartbeat: Instant::now(),
            session_id: SessionId(Uuid::new_v4()),
            config,
        }
    }

    fn logged_in(&self) -> bool {
        self.state.user_id().is_some()
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
        .wait(ctx); // TODO perhaps not wait
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        match req.message {
            ClientMessage::Login { device_id, token } => {
                self.login(device_id, token, req.request_id, ctx)
            }
            ClientMessage::CreateToken {
                username,
                password,
                expiration_date,
                permission_flags,
            } => self.create_token(
                username,
                password,
                expiration_date,
                permission_flags,
                req.request_id,
                ctx,
            ),
            ClientMessage::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, req.request_id, ctx),
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
                ClientMessage::ChangeUsername { new_username } => {
                    self.change_username(new_username, id, request_id, ctx)
                }
                ClientMessage::ChangeDisplayName { new_display_name } => {
                    self.change_display_name(new_display_name, id, request_id, ctx)
                }
                ClientMessage::ChangePassword { new_password } => {
                    self.change_password(new_password, id, request_id, ctx)
                }
                _ => unimplemented!(),
            },
        }
    }

    fn login(
        &mut self,
        device_id: DeviceId,
        login_token: AuthToken,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if self.logged_in() {
            ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::AlreadyLoggedIn),
                request_id,
            })
        }

        let fut = self
            .database_server
            .send(GetToken { device_id })
            .and_then(move |token_opt| match token_opt {
                Ok(Some(token)) => {
                    let Token {
                        token_hash,
                        hash_scheme_version,
                        user_id,
                        ..
                    } = token;
                    Either::A(
                        auth::verify(login_token.0, token_hash, hash_scheme_version).map(
                            move |matches| {
                                if matches {
                                    Ok(user_id)
                                } else {
                                    Err(ServerError::InvalidToken)
                                }
                            },
                        ),
                    )
                }
                Ok(None) => Either::B(future::ok(Err(ServerError::InvalidToken))),
                Err(e) => {
                    eprintln!("Database error: {:?}", e);
                    Either::B(future::ok(Err(ServerError::Internal)))
                }
            })
            .into_actor(self)
            .and_then(|res, act, ctx| match res {
                Ok(user_id) => actix::fut::Either::A(
                    act.client_server
                        .send(Connect {
                            session: ctx.address(),
                            session_id: act.session_id,
                            user_id,
                        })
                        .map(move |_| Ok(user_id))
                        .into_actor(act),
                ),
                Err(e) => actix::fut::Either::B(actix::fut::ok(Err(e))),
            })
            .map(move |res, act, _ctx| match res {
                Ok(id) => {
                    act.state = SessionState::Ready(id);
                    RequestResponse::user(id)
                }
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn create_token(
        &mut self,
        username: String,
        password: String,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let mut token_bytes: [u8; 32] = [0; 32]; // 256 bits
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token_string = base64::encode(&token_bytes);
        let auth_token = AuthToken(token_string.clone());
        let username = auth::process_username(&username, self.config.get_ref());

        let fut = self
            .verify_username_password(username, password)
            .and_then(|user_id| {
                auth::hash(token_string).map(move |(hash, ver)| (hash, ver, user_id))
            })
            .into_actor(self)
            .and_then(
                move |(hash, hash_version, user_id), act, _ctx| match user_id {
                    Ok(user_id) => {
                        let device_id = DeviceId(Uuid::new_v4());
                        let token = Token {
                            token_hash: hash,
                            hash_scheme_version: hash_version,
                            user_id,
                            device_id,
                            last_used: Utc::now(),
                            expiration_date,
                            permission_flags,
                        };

                        actix::fut::Either::A(
                            act.database_server
                                .send(CreateToken(token))
                                .map(move |_| Ok((device_id, auth_token)))
                                .into_actor(act),
                        )
                    }
                    Err(e) => actix::fut::Either::B(actix::fut::ok(Err(e))),
                },
            )
            .map(move |res, _act, _ctx| match res {
                Ok((device_id, token)) => RequestResponse::token(device_id, token),
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn create_user(
        &mut self,
        username: String,
        display_name: String,
        password: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&password, self.config.get_ref()) {
            return ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::InvalidPassword),
                request_id,
            });
        }

        let username = match auth::prepare_username(&username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return ctx.binary(ServerMessage::Response {
                    response: RequestResponse::Error(ServerError::InvalidUsername),
                    request_id,
                })
            }
        };

        if !auth::valid_display_name(&display_name, self.config.get_ref()) {
            return ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::InvalidDisplayName),
                request_id,
            });
        }

        let fut = auth::hash(password)
            .into_actor(self)
            .and_then(move |(hash, hash_version), act, _ctx| {
                let user = User::new(username, display_name, hash, hash_version);
                let id = user.id.clone();

                act.database_server
                    .send(CreateUser(user))
                    .map(move |res| res.map(|success| (success, id)))
                    .into_actor(act)
            })
            .map(move |res, _act, _ctx| match res {
                Ok((success, id)) => {
                    if success {
                        RequestResponse::user(id)
                    } else {
                        RequestResponse::Error(ServerError::UsernameAlreadyExists)
                    }
                }
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(e) => {
                        eprintln!("Database connection pooling error: {:?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                    l337::Error::External(sql_error) => {
                        eprintln!("Database error: {:?}", sql_error);
                        RequestResponse::Error(ServerError::Internal)
                    }
                },
            });

        self.respond(fut, request_id, ctx)
    }

    fn change_username(
        &mut self,
        new_username: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let new_username = match auth::prepare_username(&new_username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return ctx.binary(ServerMessage::Response {
                    response: RequestResponse::Error(ServerError::InvalidUsername),
                    request_id,
                })
            }
        };

        let fut = self
            .database_server
            .send(ChangeUsername {
                user_id,
                new_username,
            })
            .into_actor(self)
            .map(move |res, _act, _ctx| match res {
                Ok(success) => {
                    if success {
                        RequestResponse::success()
                    } else {
                        RequestResponse::Error(ServerError::UsernameAlreadyExists)
                    }
                }
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(e) => {
                        eprintln!("Database connection pooling error: {:?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                    l337::Error::External(sql_error) => {
                        if sql_error.code() == Some(&SqlState::INTEGRITY_CONSTRAINT_VIOLATION)
                            || sql_error.code() == Some(&SqlState::UNIQUE_VIOLATION)
                        {
                            RequestResponse::Error(ServerError::UsernameAlreadyExists)
                        } else {
                            eprintln!("Database error: {:?}", sql_error);
                            RequestResponse::Error(ServerError::Internal)
                        }
                    }
                },
            });

        self.respond(fut, request_id, ctx)
    }

    fn change_display_name(
        &mut self,
        new_display_name: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_display_name(&new_display_name, self.config.get_ref()) {
            return ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::InvalidDisplayName),
                request_id,
            });
        }

        let fut = self
            .database_server
            .send(ChangeDisplayName {
                user_id,
                new_display_name,
            })
            .map(move |res| res.map(|_| ()))
            .into_actor(self)
            .map(move |res, _act, _ctx| match res {
                Ok(_) => RequestResponse::success(),
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(e) => {
                        eprintln!("Database connection pooling error: {:?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                    l337::Error::External(sql_error) => {
                        eprintln!("Database error: {:?}", sql_error);
                        RequestResponse::Error(ServerError::Internal)
                    }
                },
            });

        self.respond(fut, request_id, ctx)
    }

    fn change_password(
        &mut self,
        password: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&password, self.config.get_ref()) {
            return ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::InvalidPassword),
                request_id,
            });
        }

        let fut = auth::hash(password)
            .into_actor(self)
            .and_then(move |(new_password_hash, hash_version), act, _ctx| {
                act.database_server
                    .send(ChangePassword {
                        user_id,
                        new_password_hash,
                        hash_version,
                    })
                    .map(move |res| res.map(|_| ()))
                    .into_actor(act)
            })
            .map(move |res, _act, _ctx| match res {
                Ok(_) => RequestResponse::success(),
                Err(l337_err) => match l337_err {
                    l337::Error::Internal(e) => {
                        eprintln!("Database connection pooling error: {:?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                    l337::Error::External(sql_error) => {
                        eprintln!("Database error: {:?}", sql_error);
                        RequestResponse::Error(ServerError::Internal)
                    }
                },
            });

        self.respond(fut, request_id, ctx)
    }

    fn verify_username_password(
        &mut self,
        username: String,
        password: String,
    ) -> impl Future<Item = Result<UserId, ServerError>, Error = MailboxError> {
        let username = auth::process_username(&username, self.config.get_ref());
        self.database_server.send(GetUserByName(username)).and_then(
            move |user_opt| match user_opt {
                Ok(Some(user)) => {
                    let User {
                        id: user_id,
                        password_hash,
                        hash_scheme_version,
                        ..
                    } = user;

                    Either::A(
                        auth::verify(password, password_hash, hash_scheme_version).map(
                            move |matches| {
                                if matches {
                                    Ok(user_id)
                                } else {
                                    Err(ServerError::IncorrectPassword)
                                }
                            },
                        ),
                    )
                }
                Ok(None) => Either::B(future::ok(Err(ServerError::UserDoesNotExist))),
                Err(e) => {
                    eprintln!("Database error: {:?}", e);
                    Either::B(future::ok(Err(ServerError::Internal)))
                }
            },
        )
    }
}
