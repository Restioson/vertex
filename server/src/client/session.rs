use super::*;
use crate::auth;
use crate::config::Config;
use crate::database::*;
use crate::federation::FederationServer;
use crate::SendMessage;
use actix::fut;
use actix_web::web::Data;
use actix_web_actors::ws::{self, WebsocketContext};
use chrono::DateTime;
use chrono::Utc;
use futures::future::{self, Either};
use rand::RngCore;
use std::io::Cursor;
use std::time::Instant;
use uuid::Uuid;
use vertex_common::{Response, *};

#[derive(Eq, PartialEq)]
enum SessionState {
    WaitingForLogin,
    Ready(UserId, DeviceId, TokenPermissionFlags),
}

impl SessionState {
    fn user_and_device_ids(&self) -> Option<(UserId, DeviceId)> {
        match self {
            SessionState::WaitingForLogin => None,
            SessionState::Ready(user_id, device_id, _) => Some((*user_id, *device_id)),
        }
    }
}

pub struct ClientWsSession {
    client_server: Addr<ClientServer>,
    database_server: Addr<DatabaseServer>,
    federation_server: Addr<FederationServer>,
    state: SessionState,
    heartbeat: Instant,
    config: Data<Config>,
}

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut WebsocketContext<Self>) {
        self.start_heartbeat(ctx);
    }

    fn stopped(&mut self, _ctx: &mut WebsocketContext<Self>) {
        if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
            self.client_server
                .do_send(Disconnect { user_id, device_id });
        }
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
                let error = serde_cbor::to_vec(&ClientboundMessage::Response(Response::Error(
                    ServerError::UnexpectedTextFrame,
                )))
                .unwrap();
                ctx.binary(error);
            }
            ws::Message::Binary(bin) => {
                let mut bin = Cursor::new(bin);
                let msg = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        let error = serde_cbor::to_vec(&ClientboundMessage::Response(
                            Response::Error(ServerError::InvalidMessage),
                        ))
                        .unwrap();
                        ctx.binary(error);
                        return;
                    }
                };

                self.handle_message(msg, ctx);
            }
            ws::Message::Close(_) => {
                if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
                    self.client_server
                        .do_send(Disconnect { user_id, device_id });
                }
                ctx.stop();
            }
            ws::Message::Nop => (),
        }
    }
}

impl Handler<SendMessage<ClientboundMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ClientboundMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg);
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, _: LogoutThisSession, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(ClientboundMessage::SessionLoggedOut);
        self.state = SessionState::WaitingForLogin;
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
            config,
        }
    }

    fn logged_in(&self) -> bool {
        self.state.user_and_device_ids().is_some()
    }

    fn start_heartbeat(&mut self, ctx: &mut WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_TIMEOUT, |session, ctx| {
            if Instant::now().duration_since(session.heartbeat) > HEARTBEAT_TIMEOUT {
                if let Some((user_id, device_id)) = session.state.user_and_device_ids() {
                    session
                        .client_server
                        .do_send(Disconnect { user_id, device_id });
                }
                ctx.stop();
            }
        });
    }

    /// Responds to a request with a future which will eventually resolve to the request response
    fn respond<F>(&mut self, fut: F, ctx: &mut WebsocketContext<Self>)
    where
        F: ActorFuture<Item = Response, Error = MailboxError, Actor = Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
            let response = ClientboundMessage::Response(if let Ok(r) = response {
                    r
                } else {
                    Response::Error(ServerError::Internal)
                },
            );

            ctx.binary(response);
            fut::ok(())
        })
        .wait(ctx);
    }

    fn respond_error(
        &mut self,
        error: ServerError,
        ctx: &mut WebsocketContext<Self>,
    ) {
        ctx.binary(ClientboundMessage::Response(Response::Error(error)));
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        match req {
            ClientRequest::Login { device_id, token } => {
                self.login(device_id, token, ctx)
            }
            ClientRequest::CreateToken {
                username,
                password,
                device_name,
                expiration_date,
                permission_flags,
            } => self.create_token(
                username,
                password,
                device_name,
                expiration_date,
                permission_flags,
                ctx,
            ),
            ClientRequest::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, ctx),
            ClientRequest::RefreshToken {
                device_id,
                username,
                password,
            } => self.refresh_token(device_id, username, password, ctx),
            m => self.handle_authenticated_message(m, ctx),
        };
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientRequest,
        ctx: &mut WebsocketContext<Self>,
    ) {
        match self.state {
            SessionState::WaitingForLogin => self.respond(
                futures::future::ok(Response::Error(ServerError::NotLoggedIn))
                    .into_actor(self),
                ctx,
            ),
            SessionState::Ready(user_id, device_id, perms) => match msg {
                ClientRequest::SendMessage(msg) => {
                    if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.respond(
                        self.client_server
                            .send(IdentifiedMessage {
                                user_id,
                                device_id,
                                msg,
                            })
                            .into_actor(self),
                        ctx,
                    )
                }
                ClientRequest::EditMessage(edit) => {
                    // TODO when history is implemented, narrow this down according to sender too
                    if !perms.has_perms(TokenPermissionFlags::EDIT_ANY_MESSAGES) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.respond(
                        self.client_server
                            .send(IdentifiedMessage {
                                user_id,
                                device_id,
                                msg: edit,
                            })
                            .into_actor(self),
                        ctx,
                    )
                }
                ClientRequest::JoinRoom(room) => {
                    if !perms.has_perms(TokenPermissionFlags::JOIN_ROOMS) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.respond(
                        self.client_server
                            .send(IdentifiedMessage {
                                user_id,
                                device_id,
                                msg: Join { room },
                            })
                            .into_actor(self),
                        ctx,
                    )
                }
                ClientRequest::CreateRoom => {
                    if !perms.has_perms(TokenPermissionFlags::CREATE_ROOMS) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.respond(
                        self.client_server
                            .send(IdentifiedMessage {
                                user_id,
                                device_id,
                                msg: CreateRoom,
                            })
                            .into_actor(self),
                        ctx,
                    )
                }
                ClientRequest::RevokeToken {
                    device_id: to_revoke,
                    password,
                } => self.revoke_token(to_revoke, password, user_id, device_id, ctx),
                ClientRequest::ChangeUsername { new_username } => {
                    if !perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.change_username(new_username, user_id, ctx)
                }
                ClientRequest::ChangeDisplayName { new_display_name } => {
                    if !perms.has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME) {
                        self.respond_error(ServerError::AccessDenied, ctx);
                        return;
                    }

                    self.change_display_name(new_display_name, user_id, ctx)
                }
                ClientRequest::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user_id, ctx),
                _ => unreachable!(),
            },
        }
    }

    fn login(
        &mut self,
        device_id: DeviceId,
        login_token: AuthToken,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if self.logged_in() {
            ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::AlreadyLoggedIn)));
            return;
        }

        let fut = self
            .database_server
            .send(GetToken { device_id })
            .into_actor(self)
            .and_then(move |token_opt, act, _ctx| match token_opt {
                Ok(Some(token)) => fut::Either::A(
                    act.database_server
                        .send(GetUserById(token.user_id))
                        .and_then(move |user_opt| match user_opt {
                            Ok(Some(user)) => future::ok(Ok((token, user))),
                            Ok(None) => future::ok(Err(ServerError::InvalidToken)),
                            Err(e) => future::ok(Err(e)),
                        })
                        .into_actor(act),
                ),
                Ok(None) => fut::Either::B(fut::ok(Err(ServerError::InvalidToken))),
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            })
            .map(|res, act, _ctx| match res {
                Ok((token, user)) => {
                    let token_stale_days = act.config.token_stale_days as i64;

                    if user.locked {
                        Err(ServerError::UserLocked)
                    } else if user.banned {
                        Err(ServerError::UserBanned)
                    } else if user.compromised {
                        Err(ServerError::UserCompromised)
                    } else if (Utc::now() - token.last_used).num_days() > token_stale_days {
                        Err(ServerError::StaleToken)
                    } else {
                        Ok(token)
                    }
                }
                Err(e) => Err(e),
            })
            .and_then(|res, act, _ctx| match res {
                Ok(token) => {
                    let Token {
                        token_hash,
                        hash_scheme_version,
                        user_id,
                        device_id,
                        permission_flags,
                        ..
                    } = token;

                    if login_token.0.len() > auth::MAX_TOKEN_LENGTH {
                        fut::Either::B(fut::ok(Err(ServerError::InvalidToken)))
                    } else {
                        fut::Either::A(
                            auth::verify(login_token.0, token_hash, hash_scheme_version)
                                .map(move |matches| {
                                    if matches {
                                        Ok((user_id, device_id, permission_flags))
                                    } else {
                                        Err(ServerError::InvalidToken)
                                    }
                                })
                                .into_actor(act),
                        )
                    }
                }
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            })
            .and_then(move |res, act, _ctx| match res {
                Ok((user_id, device_id, perms)) => fut::Either::A(
                    act.database_server
                        .send(RefreshToken(device_id))
                        .map(move |res| match res {
                            Ok(true) => Ok((user_id, device_id, perms)),
                            Ok(false) => Err(ServerError::DeviceDoesNotExist),
                            Err(e) => Err(e),
                        })
                        .into_actor(act),
                ),
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            })
            .and_then(move |res, act, ctx| match res {
                Ok((user_id, device_id, perms)) => fut::Either::A(
                    act.client_server
                        .send(Connect {
                            session: ctx.address(),
                            device_id,
                            user_id,
                        })
                        .map(move |_| Ok((user_id, device_id, perms)))
                        .into_actor(act),
                ),
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            })
            .map(move |res, act, _ctx| match res {
                Ok((user_id, device_id, perms)) => {
                    act.state = SessionState::Ready(user_id, device_id, perms);
                    Response::user(user_id)
                }
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn create_token(
        &mut self,
        username: String,
        password: String,
        device_name: Option<String>,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
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
                            device_name,
                            last_used: Utc::now(),
                            expiration_date,
                            permission_flags,
                        };

                        fut::Either::A(
                            act.database_server
                                .send(CreateToken(token))
                                .map(move |res| match res {
                                    Ok(_) => Ok((device_id, auth_token)),
                                    Err(e) => Err(e),
                                })
                                .into_actor(act),
                        )
                    }
                    Err(e) => fut::Either::B(fut::ok(Err(e))),
                },
            )
            .map(move |res, _act, _ctx| match res {
                Ok((device_id, token)) => Response::token(device_id, token),
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn revoke_token(
        &mut self,
        to_revoke: DeviceId,
        password: Option<String>,
        user_id: UserId,
        current_device_id: DeviceId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let fut = if to_revoke != current_device_id {
            Either::A(self.verify_user_id_password(user_id, password.unwrap()))
        } else {
            Either::B(future::ok(Ok(())))
        }
        .into_actor(self)
        .and_then(move |res, act, _ctx| match res {
            Ok(()) => fut::Either::A(
                act.database_server
                    .send(RevokeToken(to_revoke))
                    .map(|res| match res {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(ServerError::DeviceDoesNotExist),
                        Err(e) => Err(e),
                    })
                    .into_actor(act),
            ),
            Err(e) => fut::Either::B(fut::ok(Err(e))),
        })
        .and_then(move |res, act, _ctx| match res {
            Ok(()) => {
                if to_revoke == current_device_id {
                    act.state = SessionState::WaitingForLogin;
                    fut::Either::A(
                        act.client_server
                            .send(Disconnect {
                                user_id,
                                device_id: current_device_id,
                            })
                            .map(|_| Ok(()))
                            .into_actor(act),
                    )
                } else {
                    fut::Either::B(fut::ok(Ok(())))
                }
            }
            Err(e) => fut::Either::B(fut::ok(Err(e))),
        })
        .map(|res, _act, _ctx| match res {
            Ok(()) => Response::success(),
            Err(e) => Response::Error(e),
        });

        self.respond(fut, ctx)
    }

    fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let fut = self
            .verify_username_password(username, password)
            .into_actor(self)
            .and_then(move |res, act, _ctx| match res {
                Ok(_) => fut::Either::A(
                    act.database_server
                        .send(RefreshToken(to_refresh))
                        .map(|res| match res {
                            Ok(true) => Ok(()),
                            Ok(false) => Err(ServerError::DeviceDoesNotExist),
                            Err(e) => Err(e),
                        })
                        .into_actor(act),
                ),
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            })
            .map(|res, _act, _ctx| match res {
                Ok(()) => Response::success(),
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn create_user(
        &mut self,
        username: String,
        display_name: String,
        password: String,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&password, self.config.get_ref()) {
            ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidPassword)));
            return;
        }

        let username = match auth::prepare_username(&username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidUsername)));
                return;
            }
        };

        if !auth::valid_display_name(&display_name, self.config.get_ref()) {
            ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidDisplayName)));
            return;
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
                        Response::user(id)
                    } else {
                        Response::Error(ServerError::UsernameAlreadyExists)
                    }
                }
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn change_username(
        &mut self,
        new_username: String,
        user_id: UserId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let new_username = match auth::prepare_username(&new_username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidUsername)));
                return;
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
                        Response::success()
                    } else {
                        Response::Error(ServerError::UsernameAlreadyExists)
                    }
                }
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn change_display_name(
        &mut self,
        new_display_name: String,
        user_id: UserId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_display_name(&new_display_name, self.config.get_ref()) {
            ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidDisplayName)));
            return;
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
                Ok(_) => Response::success(),
                Err(e) => Response::Error(e),
            });

        self.respond(fut, ctx)
    }

    fn change_password(
        &mut self,
        old_password: String,
        new_password: String,
        user_id: UserId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&new_password, self.config.get_ref()) {
            ctx.binary(ClientboundMessage::Response(Response::Error(ServerError::InvalidPassword)));
            return;
        }

        let fut = self
            .verify_user_id_password(user_id, old_password)
            .and_then(|res| match res {
                Ok(_) => Either::A(auth::hash(new_password).map(|ok| Ok(ok))),
                Err(error) => Either::B(future::ok(Err(error))),
            })
            .into_actor(self)
            .and_then(move |res, act, _ctx| {
                let fut = match res {
                    Ok((new_password_hash, hash_version)) => {
                        let fut = act
                            .database_server
                            .send(ChangePassword {
                                user_id,
                                new_password_hash,
                                hash_version,
                            })
                            .map(move |res| res.map(|_| ()))
                            .map(|res| match res {
                                Ok(_) => Response::success(),
                                Err(e) => Response::Error(e),
                            });
                        Either::A(fut)
                    }
                    Err(e) => Either::B(future::ok(Response::Error(e))),
                };

                fut.into_actor(act)
            })
            .and_then(move |res, act, _ctx| match res {
                Response::Success(success) => fut::Either::A(
                    act.client_server
                        .send(LogoutUserSessions { user_id })
                        .map(|_| Response::Success(success))
                        .into_actor(act),
                ),
                response => fut::Either::B(fut::ok(response)),
            });

        self.respond(fut, ctx)
    }

    fn verify_user_id_password(
        &mut self,
        user_id: UserId,
        password: String,
    ) -> impl Future<Item = Result<(), ServerError>, Error = MailboxError> {
        self.database_server
            .send(GetUserById(user_id))
            .and_then(move |res| match res {
                Ok(Some(user)) => {
                    Either::A(auth::verify_user_password(user, password).map(|res| res.map(|_| ())))
                }
                Ok(None) => Either::B(future::ok(Err(ServerError::IncorrectUsernameOrPassword))),
                Err(e) => Either::B(future::ok(Err(e))),
            })
    }

    fn verify_username_password(
        &mut self,
        username: String,
        password: String,
    ) -> impl Future<Item = Result<UserId, ServerError>, Error = MailboxError> {
        let username = auth::process_username(&username, self.config.get_ref());
        self.database_server
            .send(GetUserByName(username))
            .and_then(move |res| match res {
                Ok(Some(user)) => Either::A(auth::verify_user_password(user, password)),
                Ok(None) => Either::B(future::ok(Err(ServerError::IncorrectUsernameOrPassword))),
                Err(e) => Either::B(future::ok(Err(e))),
            })
    }
}
