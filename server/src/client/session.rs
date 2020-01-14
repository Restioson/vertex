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
use vertex_common::*;
use crate::community::{CommunityActor, Join};
use log::error;
use std::collections::HashMap;

// TODO(room_persistence): make sure device isnt online when try to login

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
    database_server: Addr<DatabaseServer>,
    communities: Vec<CommunityId>,
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
            self.delete(ctx); // TODO(room_persistence)
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
                if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
                    self.delete();
                }
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

impl Handler<LogoutThisSession> for ClientWsSession {
    type Result = ResponseFuture<(), ()>;

    fn handle(&mut self, _: LogoutThisSession, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(ServerMessage::SessionLoggedOut);
        self.delete(ctx);
    }
}

impl ClientWsSession {
    pub fn new(
        database_server: Addr<DatabaseServer>,
        config: Data<Config>,
    ) -> Self {
        ClientWsSession {
            database_server,
            communities: Vec::new(),
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
                session.delete(ctx);
            }
        });
    }

    /// Remove the device from wherever it is referenced
    fn delete(&mut self, ctx: &mut WebsocketContext<Self>) {
        if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
            if let Some(mut devices) = USERS.get_mut(user_id) {
                // Remove the device
                let idx = devices.position(|(id, _)| id == device_id);
                device.remove(idx);

                // Remove the entire user entry if they are no longer online
                if devices.len() == 0 {
                    USERS.remove(user_id);
                }
            }
        }

        ctx.stop();
    }

    /// Responds to a request with a future which will eventually resolve to the request response
    fn respond<F>(&mut self, fut: F, request_id: RequestId, ctx: &mut WebsocketContext<Self>)
    where
        F: ActorFuture<Item = RequestResponse, Error = MailboxError, Actor = Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
            let response = ServerMessage::Response {
                response: match response {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                },
                request_id,
            };

            ctx.binary(response);
            fut::ok(())
        })
        .wait(ctx);
    }

    fn respond_error(
        &mut self,
        error: ServerError,
        id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        ctx.binary(ServerMessage::Response {
            response: RequestResponse::Error(error),
            request_id: id,
        });
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        match req.message {
            ClientMessage::Login { device_id, token } => {
                self.login(device_id, token, req.request_id, ctx)
            }
            ClientMessage::CreateToken {
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
                req.request_id,
                ctx,
            ),
            ClientMessage::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, req.request_id, ctx),
            ClientMessage::RefreshToken {
                device_id,
                username,
                password,
            } => self.refresh_token(device_id, username, password, req.request_id, ctx),
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
            SessionState::Ready(user_id, device_id, perms) => match msg {
                ClientMessage::SendMessage(msg) => {
                    if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    let community = match self.communities.get(&msg.to_community) {
                        Some(c) => c,
                        None => return self.respond_error(ServerError::InvalidCommunity, request_id, ctx),
                    };

                    self.respond(
                        community.send(IdentifiedMessage {
                            user_id,
                            device_id,
                            request_id,
                            msg,
                        }).then(|res| {
                                let r = match res {
                                    Ok(r) => match r {
                                        Ok(id) => RequestResponse::message_id(id),
                                        Err(e) => RequestResponse::Error(e),
                                    }
                                    Err(e) => {
                                        error!("Actix mailbox error: {:#?}", e);
                                        RequestResponse::Error(ServerError::Internal)
                                    }
                                };

                                future::ok(r)
                        })
                            .into_actor(self),
                        request_id,
                        ctx,
                    )
                }
                ClientMessage::EditMessage(edit) => {
                    // TODO when history is implemented, narrow this down according to sender too
                    if !perms.has_perms(TokenPermissionFlags::EDIT_ANY_MESSAGES) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    if !self.communities.contains(&edit.community_id) {
                        self.respond_error(ServerError::InvalidCommunity, request_id, ctx);
                        return;
                    }

                    self.respond(
                        COMMUNITIES.get(&edit.community_id)
                            .and_then(|opt| match opt {
                                Some(community) => Either::A(community.send(IdentifiedMessage {
                                    user_id,
                                    device_id,
                                    request_id,
                                    msg: edit,
                                })),
                                None => Either::B(future::ok(ServerError::InvalidCommunity)),
                            }),
                        request_id,
                        ctx,
                    )
                }
                ClientMessage::JoinCommunity(community) => {
                    if !perms.has_perms(TokenPermissionFlags::JOIN_COMMUNITIES) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    self.join_community(user_id, device_id, community, request_id, ctx)
                }
                ClientMessage::CreateCommunity { name } => {
                    if !perms.has_perms(TokenPermissionFlags::CREATE_COMMUNITIES) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    self.create_community(user_id, device_id, name,  request_id, ctx);
                }
                ClientMessage::RevokeToken {
                    device_id: to_revoke,
                    password,
                } => self.revoke_token(to_revoke, password, user_id, device_id, request_id, ctx),
                ClientMessage::ChangeUsername { new_username } => {
                    if !perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    self.change_username(new_username, user_id, request_id, ctx)
                }
                ClientMessage::ChangeDisplayName { new_display_name } => {
                    if !perms.has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME) {
                        self.respond_error(ServerError::AccessDenied, request_id, ctx);
                        return;
                    }

                    self.change_display_name(new_display_name, user_id, request_id, ctx)
                }
                ClientMessage::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user_id, request_id, ctx),
                _ => unreachable!(),
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
                Ok((user_id, device_id, perms)) => {
                    USERS.entry(user_id)
                        .or_insert_with(|| Vec::with_capacity(1))
                        .and_modify(|user| user.push((device_id, ctx.address())));

                    fut::ok(Ok((user_id, device_id, perms)))
                },
                Err(e) => fut::ok(Err(e)),
            })
            .map(move |res, act, _ctx| match res {
                Ok((user_id, device_id, perms)) => {
                    act.state = SessionState::Ready(user_id, device_id, perms);
                    RequestResponse::user(user_id)
                }
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn create_token(
        &mut self,
        username: String,
        password: String,
        device_name: Option<String>,
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
                Ok((device_id, token)) => RequestResponse::token(device_id, token),
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn revoke_token(
        &mut self,
        to_revoke: DeviceId,
        password: Option<String>,
        user_id: UserId,
        current_device_id: DeviceId,
        request_id: RequestId,
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
        .and_then(move |res, act, ctx| match res {
            Ok(()) => {
                if to_revoke == current_device_id {
                    act.state = SessionState::WaitingForLogin;
                    ctx.notify(LogoutThisSession);
                }
                fut::ok(Ok(()))
            }
            Err(e) => fut::ok(Err(e)),
        })
        .map(|res, _act, _ctx| match res {
            Ok(()) => RequestResponse::success(),
            Err(e) => RequestResponse::Error(e),
        });

        self.respond(fut, request_id, ctx)
    }

    fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        request_id: RequestId,
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
                Ok(()) => RequestResponse::success(),
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
                let user = UserRecord::new(username, display_name, hash, hash_version);
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
                Err(e) => RequestResponse::Error(e),
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
                Err(e) => RequestResponse::Error(e),
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
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn change_password(
        &mut self,
        old_password: String,
        new_password: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&new_password, self.config.get_ref()) {
            return ctx.binary(ServerMessage::Response {
                response: RequestResponse::Error(ServerError::InvalidPassword),
                request_id,
            });
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
                                Ok(_) => RequestResponse::success(),
                                Err(e) => RequestResponse::Error(e),
                            });
                        Either::A(fut)
                    }
                    Err(e) => Either::B(future::ok(RequestResponse::Error(e))),
                };

                fut.into_actor(act)
            })
            .and_then(move |res, act, _ctx| match res {
                RequestResponse::Success(success) => {
                    USERS.get_mut(user_id).map(|user| user.log_out_all());
                    fut::ok(RequestResponse::Success(success))
                },
                response => fut::ok(response),
            });

        self.respond(fut, request_id, ctx)
    }

    fn create_community(&mut self, user_id: UserId, device_id: DeviceId, community_name: String, request_id: RequestId, ctx: &mut WebsocketContext<Self>) {
        // TODO check perms ?
        let fut = self.database_server.send(CreateCommunity { name: community_name })
            .into_actor(self)
            .and_then(move |res, act, _ctx| {
                match res {
                    Ok(community) => {
                        let community_id = community.id;
                        let community = CommunityActor::new(user_id, vec![]); // TODO(room_persistence) populate to all online devices?
                        COMMUNITIES.insert(community_id, community.start());

                        fut::ok(Ok(community_id))
                    }
                    Err(e) => fut::ok(Err(e)),
                }
            })
            .map(move |res, _act, _ctx| match res {
                Ok(community_id) => RequestResponse::community(community_id),
                Err(e) => RequestResponse::Error(e),
            });

        self.respond(fut, request_id, ctx)
    }

    fn join_community(&mut self, user_id: UserId, device_id: DeviceId, community: CommunityId, request_id: RequestId, ctx: &mut WebsocketContext<Self>) {
        let fut = self.database_server.send(AddToCommunity { community, user: user_id })
        .into_actor(self)
        .and_then(move |res, act, _ctx| {
            match res {
                Ok(()) => {
                    COMMUNITIES.get(community)
                        .send(Join { user_id })
                }
                Err(e) => fut::Either::B(fut::ok(Err(e))),
            }
        })
        .map(move |res, _act, _ctx| match res {
            Ok(true) => RequestResponse::success(),
            Ok(false) => RequestResponse::Error(ServerError::InvalidCommunity),
            Err(e) => RequestResponse::Error(e),
        });

        self.respond(fut, request_id, ctx)
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
