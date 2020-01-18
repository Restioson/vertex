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
    fn user_and_devices(&self) -> Option<(UserId, DeviceId)> {
        match self {
            SessionState::WaitingForLogin => None,
            SessionState::Ready(user, device, _) => Some((*user, *device)),
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
        if let Some(_) = self.state.user_and_devices() {
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
                ctx.binary(serde_cbor::to_vec(&ServerMessage::MalformedMessage).unwrap());
            }
            ws::Message::Binary(bin) => {
                let mut bin = Cursor::new(bin);
                let msg = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        return ctx.binary(serde_cbor::to_vec(&ServerMessage::MalformedMessage).unwrap());
                    }
                };

                self.handle_message(msg, ctx);
            }
            ws::Message::Close(_) => {
                if let Some(_) = self.state.user_and_devices() {
                    ctx.stop();
                } else {
                    ctx.stop();
                }
            }
            ws::Message::Continuation(_) => {
                ctx.binary(serde_cbor::to_vec(&ServerMessage::MalformedMessage).unwrap());
            }
            ws::Message::Nop => (),
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ServerMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg.0);
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, _: LogoutThisSession, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(ServerMessage::Action(ServerAction::SessionLoggedOut));
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
        self.state.user_and_devices().is_some()
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
        if let Some((user_id, device)) = self.state.user_and_devices() {
            if let Some(mut user) = USERS.get_mut(&user_id) {
                // Remove the device
                let devices = &mut user.sessions;
                if let Some(idx) = devices.iter().position(|(id, _)| *id == device) {
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
    fn respond<F>(&mut self, fut: F, request: RequestId, ctx: &mut WebsocketContext<Self>)
        where
            F: ActorFuture<Output=Result<Result<OkResponse, ErrResponse>, MailboxError>, Actor=Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
            let response = ServerMessage::Response {
                result: match response {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        Err(ErrResponse::Internal)
                    }
                },
                id: request,
            };

            ctx.binary(response);
            fut::ready(())
        })
            .wait(ctx);
    }

    fn respond_error(
        &mut self,
        error: ErrResponse,
        id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        ctx.binary(ServerMessage::Response {
            result: Err(error),
            id,
        });
    }

    fn handle_message(&mut self, req: ClientMessage, ctx: &mut WebsocketContext<Self>) {
        match req.request {
            ClientRequest::Login { device, token } => self.login(device, token, req.id, ctx),
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
                req.id,
                ctx,
            ),
            ClientRequest::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, req.id, ctx),
            ClientRequest::RefreshToken {
                device,
                username,
                password,
            } => self.refresh_token(device, username, password, req.id, ctx),
            m => self.handle_authenticated_message(m, req.id, ctx),
        };
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientRequest,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        match self.state {
            SessionState::WaitingForLogin => {
                self.respond(fut::ok(Err(ErrResponse::NotLoggedIn)), request, ctx)
            }
            SessionState::Ready(user, device, perms) => match msg {
                ClientRequest::SendMessage(msg) => {
                    self.send_message(msg, user, device, perms, request, ctx);
                }
                ClientRequest::EditMessage(edit) => {
                    self.edit_message(edit, user, device, perms, request, ctx);
                }
                ClientRequest::JoinCommunity(community) => {
                    unimplemented!() // TODO(implement)
                }
                ClientRequest::CreateCommunity { name } => {
                    unimplemented!() // TODO(implement)
                }
                ClientRequest::RevokeToken {
                    device: to_revoke,
                    password,
                } => self.revoke_token(to_revoke, password, user, device, request, ctx),
                ClientRequest::ChangeUsername { new_username } => {
                    self.change_username(new_username, user, perms, request, ctx);
                }
                ClientRequest::ChangeDisplayName { new_display_name } => {
                    self.change_display_name(new_display_name, user, request, perms, ctx);
                }
                ClientRequest::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user, request, ctx),
                _ => unreachable!(),
            },
        }
    }

    fn send_message(
        &mut self,
        message: ClientSentMessage,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx);
            return;
        }

        if !self.communities.contains(&message.to_community) {
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx);
            return;
        }

        if let Some(community) = COMMUNITIES.get(&message.to_community) {
            let msg = IdentifiedMessage {
                user,
                device,
                message,
            };
            let fut = community.send(msg).map_ok(|res| match res {
                Ok(id) => Ok(OkResponse::MessageId { id }),
                Err(e) => Err(e),
            });
            self.respond(fut.into_actor(self), request, ctx);
        } else {
            self.communities.remove_item(&message.to_community);
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx);
            ctx.binary(ServerMessage::Action(ServerAction::LeftCommunity(LeftCommunityReason::Deleted)));
        }
    }

    fn edit_message(
        &mut self,
        edit: Edit,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx);
            return;
        }

        if !self.communities.contains(&edit.community) {
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx);
            return;
        }

        if let Some(community) = COMMUNITIES.get(&edit.community) {
            let msg = IdentifiedMessage {
                user,
                device,
                message: edit,
            };
            let fut = community.send(msg).map_ok(|res| match res {
                Ok(()) => Ok(OkResponse::NoData),
                Err(e) => Err(e),
            });
            self.respond(fut.into_actor(self), request, ctx);
        } else {
            self.communities.remove_item(&edit.community);
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx);
            ctx.binary(ServerMessage::Action(ServerAction::LeftCommunity(LeftCommunityReason::Deleted)));
        }
    }

    fn login(
        &mut self,
        device: DeviceId,
        token_str: AuthToken,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if self.logged_in() {
            return self.respond_error(ErrResponse::AlreadyLoggedIn, request, ctx);
        }

        let token_stale_days = self.config.token_stale_days as i64;
        let db_server = self.database_server.clone();
        let get_token = self.database_server.send(GetToken { device });

        let fut = async move {
            let token = match get_token.await? {
                Ok(Some(token)) => token,
                Ok(None) => return Ok(Err(ErrResponse::InvalidToken)),
                Err(e) => return Ok(Err(e)),
            };

            let user = match db_server.send(GetUserById(token.user)).await? {
                Ok(Some(user)) => user,
                Ok(None) => return Ok(Err(ErrResponse::InvalidUser)),
                Err(e) => return Ok(Err(e)),
            };

            // Check if can log in with this token
            if user.locked {
                return Ok(Err(ErrResponse::UserLocked));
            } else if user.banned {
                return Ok(Err(ErrResponse::UserBanned));
            } else if user.compromised {
                return Ok(Err(ErrResponse::UserCompromised));
            } else if (Utc::now() - token.last_used).num_days() > token_stale_days {
                return Ok(Err(ErrResponse::StaleToken));
            }

            if token_str.0.len() > auth::MAX_TOKEN_LENGTH {
                return Ok(Err(ErrResponse::InvalidToken));
            }

            let Token {
                token_hash,
                hash_scheme_version,
                user,
                permission_flags,
                ..
            } = token;
            let matches = auth::verify(token_str.0, token_hash, hash_scheme_version).await;

            if !matches {
                return Ok(Err(ErrResponse::InvalidToken));
            }

            match db_server.send(RefreshToken(device)).await? {
                Ok(true) => Ok(Ok((user, permission_flags))),
                Ok(false) => Ok(Err(ErrResponse::InvalidToken)),
                Err(e) => Ok(Err(e)),
            }
        };

        let fut = fut
            .into_actor(self)
            .map(move |res: Result<_, MailboxError>, act, ctx| {
                match res {
                    Ok(Ok((user, perms))) => {
                        let addr = ctx.address();

                        let mut inserted = false;

                        // Add this user to the users map
                        USERS
                            .entry(user)
                            .and_modify(move |user| {
                                if user.sessions.iter().find(|(id, _)| *id == device).is_none() {
                                    inserted = true;
                                    user.sessions.push((device, addr));
                                }
                            })
                            .or_insert_with(|| {
                                inserted = true;
                                UserSessions::new((device, ctx.address()))
                            });

                        // This token is currently in use on another device
                        if !inserted {
                            Ok(Err(ErrResponse::TokenInUse))
                        } else {
                            act.state = SessionState::Ready(user, device, perms);

                            Ok(Ok(OkResponse::User { id: user }))
                        }
                    }
                    Ok(Err(e)) => Ok(Err(e)),
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        Ok(Err(ErrResponse::Internal.into()))
                    }
                }
            });

        self.respond(fut, request, ctx);
    }

    fn create_token(
        &mut self,
        username: String,
        password: String,
        device_name: Option<String>,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
        request: RequestId,
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
            let user = match verify.await {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            };

            let (token_hash, hash_scheme_version) = auth::hash(token_string).await;

            let device = DeviceId(Uuid::new_v4());
            let token = Token {
                token_hash,
                hash_scheme_version,
                user,
                device,
                device_name,
                last_used: Utc::now(),
                expiration_date,
                permission_flags,
            };

            if let Err(e) = db_server.send(CreateToken(token)).await? {
                return Ok(Err(e));
            }

            Ok(Ok(OkResponse::Token { device, token: auth_token }))
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn revoke_token(
        &mut self,
        to_revoke: DeviceId,
        password: Option<String>,
        user: UserId,
        current_device: DeviceId,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let initial_future;
        if to_revoke != current_device {
            let password = match password {
                Some(password) => password,
                None => return self.respond_error(ErrResponse::AccessDenied, request, ctx),
            };

            initial_future = Either::Left(self.verify_user_password(user, password));
        } else {
            initial_future = Either::Right(future::ok(()));
        }
        let this_addr = ctx.address();

        let db_server = self.database_server.clone();
        let fut = async move {
            if let Err(e) = initial_future.await {
                return Ok(Err(e.into()));
            }

            let res = db_server.send(RevokeToken(to_revoke)).await?;
            match res {
                Ok(false) => return Ok(Err(ErrResponse::DeviceDoesNotExist.into())),
                Err(e) => return Ok(Err(e.into())),
                _ => (),
            }

            if to_revoke == current_device {
                this_addr.do_send(LogoutThisSession);
            }

            Ok(Ok(OkResponse::NoData))
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        let verify = self.verify_username_password(username, password);
        let db_server = self.database_server.clone();

        let fut = async move {
            if let Err(e) = verify.await {
                return Ok(Err(e));
            }

            let res = db_server.send(RefreshToken(to_refresh)).await?;

            Ok(match res {
                Ok(true) => Ok(OkResponse::NoData),
                Ok(false) => Err(ErrResponse::DeviceDoesNotExist),
                Err(e) => Err(e),
            })
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn create_user(
        &mut self,
        username: String,
        display_name: String,
        password: String,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&password, self.config.get_ref()) {
            return self.respond_error(ErrResponse::InvalidPassword, request, ctx);
        }

        let username = match auth::prepare_username(&username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return self.respond_error(ErrResponse::InvalidUsername, request, ctx);
            }
        };

        if !auth::valid_display_name(&display_name, self.config.get_ref()) {
            return self.respond_error(ErrResponse::InvalidDisplayName, request, ctx);
        }

        let db_server = self.database_server.clone();

        let fut = async move {
            let (hash, hash_version) = auth::hash(password).await;
            let user = UserRecord::new(username, display_name, hash, hash_version);
            let id = user.id;

            Ok(match db_server.send(CreateUser(user)).await? {
                Ok(true) => Ok(OkResponse::User { id }),
                Ok(false) => Err(ErrResponse::UsernameAlreadyExists),
                Err(e) => Err(e),
            })
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn change_username(
        &mut self,
        new_username: String,
        user: UserId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return self.respond_error(ErrResponse::AccessDenied, request, ctx);
        }

        let new_username = match auth::prepare_username(&new_username, self.config.get_ref()) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                return self.respond_error(ErrResponse::InvalidUsername, request, ctx);
            }
        };

        let req = self
            .database_server
            .send(ChangeUsername { user, new_username });

        let fut = async move {
            Ok(match req.await? {
                Ok(true) => Ok(OkResponse::NoData),
                Ok(false) => Err(ErrResponse::UsernameAlreadyExists),
                Err(e) => Err(e),
            })
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn change_display_name(
        &mut self,
        new_display_name: String,
        user: UserId,
        request: RequestId,
        perms: TokenPermissionFlags,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME) {
            return self.respond_error(ErrResponse::AccessDenied, request, ctx);
        }
        if !auth::valid_display_name(&new_display_name, &self.config) {
            return self.respond_error(ErrResponse::InvalidDisplayName, request, ctx);
        };

        let req = self.database_server.send(ChangeDisplayName {
            user,
            new_display_name,
        });

        let fut = async move {
            Ok(match req.await? {
                Ok(()) => Ok(OkResponse::NoData),
                Err(e) => Err(e),
            })
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn change_password(
        &mut self,
        old_password: String,
        new_password: String,
        user: UserId,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        if !auth::valid_password(&new_password, self.config.get_ref()) {
            return self.respond_error(ErrResponse::InvalidPassword, request, ctx);
        }

        let verify = self.verify_user_password(user, old_password);
        let db_server = self.database_server.clone();

        let fut = async move {
            if let Err(e) = verify.await {
                return Ok(Err(e));
            }

            let (new_password_hash, hash_version) = auth::hash(new_password).await;

            let res = db_server
                .send(ChangePassword {
                    user,
                    new_password_hash,
                    hash_version,
                })
                .await?;
            if let Err(e) = res {
                return Ok(Err(e));
            }

            Ok(Ok(OkResponse::NoData))
        };

        self.respond(fut.into_actor(self), request, ctx);
    }

    fn create_community(
        &mut self,
        user: UserId,
        device: DeviceId,
        community_name: String,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn join_community(
        &mut self,
        user: UserId,
        device: DeviceId,
        community: CommunityId,
        request: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn verify_user_password(
        &mut self,
        user: UserId,
        password: String,
    ) -> impl Future<Output=Result<(), ErrResponse>> {
        let user = self.database_server.send(GetUserById(user));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(mailbox_error) => {
                    error!("Actix mailbox error: {:#?}", mailbox_error);
                    return Err(ErrResponse::Internal);
                }
            };

            let user = match res {
                Ok(Some(user)) => user,
                Ok(None) => return Err(ErrResponse::IncorrectUsernameOrPassword),
                Err(server_error) => return Err(server_error),
            };

            auth::verify_user_password(user, password).await.map(|_| ())
        }
    }

    fn verify_username_password(
        &mut self,
        username: String,
        password: String,
    ) -> impl Future<Output=Result<UserId, ErrResponse>> {
        let username = auth::process_username(&username, self.config.get_ref());
        let user = self.database_server.send(GetUserByName(username));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(mailbox_error) => {
                    error!("Actix mailbox error: {:#?}", mailbox_error);
                    return Err(ErrResponse::Internal);
                }
            };

            let user = match res {
                Ok(Some(user)) => user,
                Ok(None) => return Err(ErrResponse::IncorrectUsernameOrPassword),
                Err(server_error) => return Err(server_error),
            };

            auth::verify_user_password(user, password).await
        }
    }
}
