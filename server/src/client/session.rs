use super::*;
use crate::community::COMMUNITIES;
use crate::config::Config;
use crate::database::*;
use crate::{auth, IdentifiedMessage, SendMessage};
use actix::fut;
use actix_web::web::Data;
use actix_web_actors::ws::{self, WebsocketContext};
use chrono::DateTime;
use chrono::Utc;
use futures::future::Either;
use futures::{future, TryFutureExt};
use log::error;
use rand::RngCore;
use std::io::Cursor;
use std::time::Instant;
use uuid::Uuid;
use vertex_common::*;

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

    fn stopping(&mut self, ctx: &mut WebsocketContext<Self>) -> Running {
        if let Some(_) = self.state.user_and_device_ids() {
            self.delete();
        }

        Running::Stop
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClientWsSession {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            self.delete();
            return;
        };

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
                if let Some(_) = self.state.user_and_device_ids() {
                    ctx.stop();
                } else {
                    ctx.stop();
                }
            }
            ws::Message::Continuation(_) => {
                let error = serde_cbor::to_vec(&ServerMessage::Error(
                    ServerError::UnexpectedContinuationFrame,
                ))
                .unwrap();
                ctx.binary(error);
            }
            ws::Message::Nop => (),
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ServerMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg.message);
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, _: LogoutThisSession, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(ServerMessage::SessionLoggedOut);
        self.delete();
    }
}

impl ClientWsSession {
    pub fn new(database_server: Addr<DatabaseServer>, config: Data<Config>) -> Self {
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
                ctx.stop();
            }
        });
    }

    /// Remove the device from wherever it is referenced
    fn delete(&mut self) {
        if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
            if let Some(mut user) = USERS.get_mut(&user_id) {
                // Remove the device
                let devices = &mut user.sessions;
                if let Some(idx) = devices.iter().position(|(id, _)| *id == device_id) {
                    devices.remove(idx);

                    // Remove the entire user entry if they are no longer online
                    if devices.len() == 0 {
                        drop(user); // Prevent double lock on USERS
                        USERS.remove(&user_id);
                    }
                }
            }
        }
    }

    /// Responds to a request with a future which will eventually resolve to the request response
    fn respond<F>(&mut self, fut: F, request_id: RequestId, ctx: &mut WebsocketContext<Self>)
    where
        F: ActorFuture<Output = Result<RequestResponse, MailboxError>, Actor = Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
            let response = ServerMessage::Response {
                response: match response {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        ServerError::Internal.into()
                    }
                },
                request_id,
            };

            ctx.binary(response);
            fut::ready(())
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
            SessionState::WaitingForLogin => {
                self.respond(fut::ok(ServerError::NotLoggedIn.into()), request_id, ctx)
            }
            SessionState::Ready(user_id, device_id, perms) => match msg {
                ClientMessage::SendMessage(msg) => {
                    self.send_message(msg, user_id, device_id, perms, request_id, ctx);
                }
                ClientMessage::EditMessage(edit) => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::JoinCommunity(community) => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::CreateCommunity { name } => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::RevokeToken {
                    device_id: to_revoke,
                    password,
                } => self.revoke_token(to_revoke, password, user_id, device_id, request_id, ctx),
                ClientMessage::ChangeUsername { new_username } => {
                    self.change_username(new_username, user_id, perms, request_id, ctx);
                }
                ClientMessage::ChangeDisplayName { new_display_name } => {
                    self.change_display_name(new_display_name, user_id, request_id, perms, ctx);
                }
                ClientMessage::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user_id, request_id, ctx),
                _ => unreachable!(),
            },
        }
    }

    fn send_message(
        &mut self,
        message: ClientSentMessage,
        user_id: UserId,
        device_id: DeviceId,
        perms: TokenPermissionFlags,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            self.respond_error(ServerError::AccessDenied, request_id, ctx);
            return;
        }

        if !self.communities.contains(&message.to_community) {
            self.respond_error(ServerError::InvalidCommunity, request_id, ctx);
            return;
        }

        if let Some(community) = COMMUNITIES.get(&message.to_community) {
            let msg = IdentifiedMessage {
                user_id,
                device_id,
                message,
            };
            let fut = community.send(msg).map_ok(|res| match res {
                Ok(id) => RequestResponse::message_id(id),
                Err(e) => RequestResponse::Error(e),
            });
            self.respond(fut.into_actor(self), request_id, ctx);
        } else {
            self.communities.remove_item(&message.to_community);
            self.respond_error(ServerError::InvalidCommunity, request_id, ctx);
            ctx.binary(ServerMessage::LeftCommunity(LeftCommunityReason::Deleted));
        }
    }

    fn login(
        &mut self,
        device_id: DeviceId,
        token_str: AuthToken,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if self.logged_in() {
            return self.respond_error(ServerError::AlreadyLoggedIn, request_id, ctx);
        }

        let token_stale_days = self.config.token_stale_days as i64;
        let db_server = self.database_server.clone();
        let get_token = self.database_server.send(GetToken { device_id });

        let fut = async move {
            let token = match get_token.await? {
                Ok(Some(token)) => token,
                Ok(None) => return Ok(Err(ServerError::InvalidToken)),
                Err(e) => return Ok(Err(e)),
            };

            let user = match db_server.send(GetUserById(token.user_id)).await? {
                Ok(Some(user)) => user,
                Ok(None) => return Ok(Err(ServerError::InvalidUser)),
                Err(e) => return Ok(Err(e)),
            };

            // Check if can log in with this token
            if user.locked {
                return Ok(Err(ServerError::UserLocked));
            } else if user.banned {
                return Ok(Err(ServerError::UserBanned));
            } else if user.compromised {
                return Ok(Err(ServerError::UserCompromised));
            } else if (Utc::now() - token.last_used).num_days() > token_stale_days {
                return Ok(Err(ServerError::StaleToken));
            }

            if token_str.0.len() > auth::MAX_TOKEN_LENGTH {
                return Ok(Err(ServerError::InvalidToken));
            }

            let Token {
                token_hash,
                hash_scheme_version,
                user_id,
                permission_flags,
                ..
            } = token;
            let matches = auth::verify(token_str.0, token_hash, hash_scheme_version).await;

            if !matches {
                return Ok(Err(ServerError::InvalidToken));
            }

            match db_server.send(RefreshToken(device_id)).await? {
                Ok(true) => Ok(Ok((user_id, permission_flags))),
                Ok(false) => Ok(Err(ServerError::InvalidToken)),
                Err(e) => Ok(Err(e)),
            }
        };

        let fut = fut
            .into_actor(self)
            .map(move |res: Result<_, MailboxError>, act, ctx| {
                match res {
                    Ok(Ok((user_id, perms))) => {
                        let addr = ctx.address();

                        let mut inserted = false;

                        // Add this user to the users map
                        USERS
                            .entry(user_id)
                            .and_modify(move |user| {
                                if user
                                    .sessions
                                    .iter()
                                    .find(|(id, _)| *id == device_id)
                                    .is_none()
                                {
                                    inserted = true;
                                    user.sessions.push((device_id, addr));
                                }
                            })
                            .or_insert_with(|| {
                                inserted = true;
                                UserSessions::new((device_id, ctx.address()))
                            });

                        // This token is currently in use on another device
                        if !inserted {
                            Ok(ServerError::TokenInUse.into())
                        } else {
                            act.state = SessionState::Ready(user_id, device_id, perms);

                            Ok(RequestResponse::user(user_id))
                        }
                    }
                    Ok(Err(e)) => Ok(e.into()),
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        Ok(ServerError::Internal.into())
                    }
                }
            });

        self.respond(fut, request_id, ctx);
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

        let verify = self.verify_username_password(username, password);
        let db_server = self.database_server.clone();

        let fut = async move {
            let user_id = match verify.await {
                Ok(id) => id,
                Err(e) => return Ok(e.into()),
            };

            let (token_hash, hash_scheme_version) = auth::hash(token_string).await;

            let device_id = DeviceId(Uuid::new_v4());
            let token = Token {
                token_hash,
                hash_scheme_version,
                user_id,
                device_id,
                device_name,
                last_used: Utc::now(),
                expiration_date,
                permission_flags,
            };

            if let Err(e) = db_server.send(CreateToken(token)).await? {
                return Ok(e.into());
            };

            Ok(RequestResponse::token(device_id, auth_token))
        };

        self.respond(fut.into_actor(self), request_id, ctx);
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
        let initial_future;
        if to_revoke != current_device_id {
            let password = match password {
                Some(password) => password,
                None => return self.respond_error(ServerError::AccessDenied, request_id, ctx),
            };

            initial_future = Either::Left(self.verify_user_id_password(user_id, password));
        } else {
            initial_future = Either::Right(future::ok(()));
        }
        let this_addr = ctx.address();

        let db_server = self.database_server.clone();
        let fut = async move {
            if let Err(e) = initial_future.await {
                return Ok(e.into());
            }

            let res = db_server.send(RevokeToken(to_revoke)).await?;
            match res {
                Ok(false) => return Ok(ServerError::DeviceDoesNotExist.into()),
                Err(e) => return Ok(e.into()),
                _ => (),
            }

            if to_revoke == current_device_id {
                this_addr.do_send(LogoutThisSession);
            }

            Ok(RequestResponse::success())
        };

        self.respond(fut.into_actor(self), request_id, ctx);
    }

    fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let verify = self.verify_username_password(username, password);
        let db_server = self.database_server.clone();

        let fut = async move {
            if let Err(e) = verify.await {
                return Ok(e.into());
            }

            let res = db_server.send(RefreshToken(to_refresh)).await?;

            Ok(match res {
                Ok(true) => RequestResponse::success(),
                Ok(false) => ServerError::DeviceDoesNotExist.into(),
                Err(e) => e.into(),
            })
        };

        self.respond(fut.into_actor(self), request_id, ctx);
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
            return self.respond_error(ServerError::InvalidPassword, request_id, ctx);
        }

        let username = match auth::prepare_username(&username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return self.respond_error(ServerError::InvalidUsername, request_id, ctx);
            }
        };

        if !auth::valid_display_name(&display_name, self.config.get_ref()) {
            return self.respond_error(ServerError::InvalidDisplayName, request_id, ctx);
        }

        let db_server = self.database_server.clone();

        let fut = async move {
            let (hash, hash_version) = auth::hash(password).await;
            let user = UserRecord::new(username, display_name, hash, hash_version);
            let id = user.id;

            Ok(match db_server.send(CreateUser(user)).await? {
                Ok(true) => RequestResponse::user(id),
                Ok(false) => ServerError::UsernameAlreadyExists.into(),
                Err(e) => e.into(),
            })
        };

        self.respond(fut.into_actor(self), request_id, ctx);
    }

    fn change_username(
        &mut self,
        new_username: String,
        user_id: UserId,
        perms: TokenPermissionFlags,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return self.respond_error(ServerError::AccessDenied, request_id, ctx);
        }

        let new_username = match auth::prepare_username(&new_username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return self.respond_error(ServerError::InvalidUsername, request_id, ctx)
            }
        };

        let req = self.database_server.send(ChangeUsername {
            user_id,
            new_username,
        });

        let fut = async move {
            Ok(match req.await? {
                Ok(true) => RequestResponse::success(),
                Ok(false) => ServerError::UsernameAlreadyExists.into(),
                Err(e) => e.into(),
            })
        };

        self.respond(fut.into_actor(self), request_id, ctx);
    }

    fn change_display_name(
        &mut self,
        new_display_name: String,
        user_id: UserId,
        request_id: RequestId,
        perms: TokenPermissionFlags,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME) {
            return self.respond_error(ServerError::AccessDenied, request_id, ctx);
        }
        if !auth::valid_display_name(&new_display_name, &self.config) {
            return self.respond_error(ServerError::InvalidDisplayName, request_id, ctx);
        };

        let req = self.database_server.send(ChangeDisplayName {
            user_id,
            new_display_name,
        });

        let fut = async move {
            Ok(match req.await? {
                Ok(()) => RequestResponse::success(),
                Err(e) => e.into(),
            })
        };

        self.respond(fut.into_actor(self), request_id, ctx);
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
            return self.respond_error(ServerError::InvalidPassword, request_id, ctx);
        }

        let verify = self.verify_user_id_password(user_id, old_password);
        let db_server = self.database_server.clone();

        let fut = async move {
            if let Err(e) = verify.await {
                return Ok(e.into());
            }

            let (new_password_hash, hash_version) = auth::hash(new_password).await;

            let res = db_server
                .send(ChangePassword {
                    user_id,
                    new_password_hash,
                    hash_version,
                })
                .await?;
            if let Err(e) = res {
                return Ok(e.into());
            }

            Ok(RequestResponse::success())
        };

        self.respond(fut.into_actor(self), request_id, ctx);
    }

    fn create_community(
        &mut self,
        user_id: UserId,
        device_id: DeviceId,
        community_name: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn join_community(
        &mut self,
        user_id: UserId,
        device_id: DeviceId,
        community: CommunityId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn verify_user_id_password(
        &mut self,
        user_id: UserId,
        password: String,
    ) -> impl Future<Output = Result<(), ServerError>> {
        let user = self.database_server.send(GetUserById(user_id));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(mailbox_error) => {
                    error!("Actix mailbox error: {:#?}", mailbox_error);
                    return Err(ServerError::Internal);
                }
            };

            let user = match res {
                Ok(Some(user)) => user,
                Ok(None) => return Err(ServerError::IncorrectUsernameOrPassword),
                Err(server_error) => return Err(server_error),
            };

            auth::verify_user_password(user, password).await.map(|_| ())
        }
    }

    fn verify_username_password(
        &mut self,
        username: String,
        password: String,
    ) -> impl Future<Output = Result<UserId, ServerError>> {
        let username = auth::process_username(&username, self.config.get_ref());
        let user = self.database_server.send(GetUserByName(username));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(mailbox_error) => {
                    error!("Actix mailbox error: {:#?}", mailbox_error);
                    return Err(ServerError::Internal);
                }
            };

            let user = match res {
                Ok(Some(user)) => user,
                Ok(None) => return Err(ServerError::IncorrectUsernameOrPassword),
                Err(server_error) => return Err(server_error),
            };

            auth::verify_user_password(user, password).await
        }
    }
}
