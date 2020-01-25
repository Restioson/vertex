use super::*;
use crate::community::COMMUNITIES;
use crate::config::Config;
use crate::database::*;
use crate::{auth, IdentifiedMessage, SendMessage};
use chrono::DateTime;
use chrono::Utc;
use futures::stream::SplitSink;
use futures::{TryFutureExt, Future, SinkExt};
use log::error;
use rand::RngCore;
use std::io::Cursor;
use std::time::Instant;
use uuid::Uuid;
use vertex_common::*;
use xtra::prelude::*;
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use std::sync::Arc;
use xtra::Disconnected;

pub struct WebSocketMessage(pub(crate) Result<ws::Message, warp::Error>);

impl Message for WebSocketMessage {
    type Result = ();
}

struct CheckHeartbeat;

impl Message for CheckHeartbeat {
    type Result = ();
}

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
    sender: SplitSink<WebSocket, ws::Message>,
    database_server: Address<DatabaseServer>,
    communities: Vec<CommunityId>,
    state: SessionState,
    heartbeat: Instant,
    config: Arc<Config>,
}

impl Actor for ClientWsSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_interval(HEARTBEAT_TIMEOUT, || CheckHeartbeat);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        if let Some(_) = self.state.user_and_devices() {
            self.delete();
        }
    }
}

impl Handler<CheckHeartbeat> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: CheckHeartbeat, ctx: &mut Context<Self>) -> Self::Responder<'_> {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            ctx.stop();
        }

        async {}
    }
}

impl Handler<WebSocketMessage> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        msg: WebSocketMessage,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            let msg = if let Ok(msg) = msg.0 {
                msg
            } else {
                self.delete();
                ctx.stop();
                return;
            };

            if msg.is_ping() {
                self.heartbeat = Instant::now();
                if let Err(_) = self.sender.send(ws::Message::ping(vec![])).await {
                    ctx.stop();
                }
            } else if msg.is_close() {
                ctx.stop()
            } else if msg.is_binary() {
                let mut bin = Cursor::new(msg.as_bytes());
                let msg: ClientMessage = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        let msg = serde_cbor::to_vec(&ServerMessage::MalformedMessage).unwrap();
                        if let Err(_) = self.send(msg).await {
                            ctx.stop();
                        }
                        return;
                    }
                };

                self.handle_message(msg, ctx).await;
            } else {
                let msg = serde_cbor::to_vec(&ServerMessage::MalformedMessage).unwrap();
                if let Err(_) = self.send(msg).await {
                    ctx.stop()
                }
            }
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(&'a mut self, msg: SendMessage<ServerMessage>, ctx: &'a mut Context<Self>) -> Self::Responder<'a> {
        async move {
            if let Err(_) = self.send(msg.0).await {
                ctx.stop()
            }
        }
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: LogoutThisSession, _: &mut Context<Self>) -> Self::Responder<'_>  {
        async move {
            let _ = self.send(ServerMessage::Action(ServerAction::SessionLoggedOut)).await;
            self.delete();
        }
    }
}

impl ClientWsSession {
    pub fn new(sender: SplitSink<WebSocket, ws::Message>, database_server: Address<DatabaseServer>, config: Arc<Config>) -> Self {
        ClientWsSession {
            sender,
            database_server,
            communities: Vec::new(),
            state: SessionState::WaitingForLogin,
            heartbeat: Instant::now(),
            config,
        }
    }

    async fn send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.sender.send(ws::Message::binary(msg)).await
    }

    fn logged_in(&self) -> bool {
        self.state.user_and_devices().is_some()
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

    async fn respond(
        &mut self,
        response: Result<OkResponse, ErrResponse>,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) {
        let response = ServerMessage::Response {
            result: response,
            id: request,
        };

        if let Err(_) = self.send(response).await {
            ctx.stop()
        }
    }

    async fn respond_error(
        &mut self,
        error: ErrResponse,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) {
        self.respond(Err(error), request, ctx).await
    }

    async fn handle_message(&mut self, req: ClientMessage, ctx: &mut Context<Self>) {
        let res = match req.request {
            ClientRequest::Login { device, token } => self.login(device, token, req.id, ctx).await,
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
            ).await,
            ClientRequest::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, req.id, ctx).await,
            ClientRequest::RefreshToken {
                device,
                username,
                password,
            } => self.refresh_token(device, username, password, req.id, ctx).await,
            m => self.handle_authenticated_message(m, req.id, ctx).await,
        };

        if let Err(e) = res {
            error!("Xtra send error: {:?}", e);
            self.respond_error(ErrResponse::Internal, req.id, ctx).await;
        }
    }

    async fn handle_authenticated_message(
        &mut self,
        msg: ClientRequest,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        match self.state {
            SessionState::WaitingForLogin => {
                self.respond(Err(ErrResponse::NotLoggedIn), request, ctx).await;
                Ok(())
            }
            SessionState::Ready(user, device, perms) => match msg {
                ClientRequest::SendMessage(msg) => {
                    self.send_message(msg, user, device, perms, request, ctx).await
                }
                ClientRequest::EditMessage(edit) => {
                    self.edit_message(edit, user, device, perms, request, ctx).await
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
                } => self.revoke_token(to_revoke, password, user, device, request, ctx).await,
                ClientRequest::ChangeUsername { new_username } => {
                    self.change_username(new_username, user, perms, request, ctx).await
                }
                ClientRequest::ChangeDisplayName { new_display_name } => {
                    self.change_display_name(new_display_name, user, request, perms, ctx).await
                }
                ClientRequest::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user, request, ctx).await,
                _ => unreachable!(),
            },
        }
    }

    async fn send_message(
        &mut self,
        message: ClientSentMessage,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx).await;
            return Ok(());
        }

        if !self.communities.contains(&message.to_community) {
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx).await;
            return Ok(());
        }

        if let Some(community) = COMMUNITIES.get(&message.to_community) {
            let msg = IdentifiedMessage {
                user,
                device,
                message,
            };
            let res = community.send(msg).map_ok(|res| match res {
                Ok(id) => Ok(OkResponse::MessageId { id }),
                Err(e) => Err(e),
            }).await?;

            self.respond(res, request, ctx).await;
            Ok(())
        } else {
            self.communities.remove_item(&message.to_community);
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx).await;

            let res = self.send(ServerMessage::Action(ServerAction::LeftCommunity(
                LeftCommunityReason::Deleted,
            ))).await;

            if let Err(_) = res {
                ctx.stop();
            }

            Ok(())
        }
    }

    async fn edit_message(
        &mut self,
        edit: Edit,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx).await;
            return Ok(());
        }

        if !self.communities.contains(&edit.community) {
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx).await;
            return Ok(());
        }

        if let Some(community) = COMMUNITIES.get(&edit.community) {
            let msg = IdentifiedMessage {
                user,
                device,
                message: edit,
            };

            let res = community.send(msg).map_ok(|res| match res {
                Ok(()) => Ok(OkResponse::NoData),
                Err(e) => Err(e),
            }).await?;

            self.respond(res, request, ctx).await;
        } else {
            self.communities.remove_item(&edit.community);
            self.respond_error(ErrResponse::InvalidCommunity, request, ctx).await;

            let res = self.send(ServerMessage::Action(ServerAction::LeftCommunity(
                LeftCommunityReason::Deleted,
            ))).await;

            if let Err(_) = res {
                ctx.stop();
            }
        }

        Ok(())
    }

    async fn login(
        &mut self,
        device: DeviceId,
        token_str: AuthToken,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if self.logged_in() {
            self.respond_error(ErrResponse::AlreadyLoggedIn, request, ctx).await;
            return Ok(());
        }

        let token = match self.database_server.send(GetToken { device }).await? {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.respond_error(ErrResponse::InvalidToken, request, ctx).await;
                return Ok(());
            },
            Err(e) => {
                self.respond_error(e, request, ctx).await;
                return Ok(());
            },
        };

        let user = match self.database_server.send(GetUserById(token.user)).await? {
            Ok(Some(user)) => user,
            Ok(None) => {
                self.respond_error(ErrResponse::InvalidUser, request, ctx).await;
                return Ok(());
            },
            Err(e) => {
                self.respond_error(e, request, ctx).await;
                return Ok(());
            },
        };

        // Check if can log in with this token
        if user.locked {
            self.respond_error(ErrResponse::UserLocked, request, ctx).await;
            return Ok(());
        } else if user.banned {
            self.respond_error(ErrResponse::UserBanned, request, ctx).await;
            return Ok(());
        } else if user.compromised {
            self.respond_error(ErrResponse::UserCompromised, request, ctx).await;
            return Ok(());
        } else if (Utc::now() - token.last_used).num_days() > self.config.token_stale_days as i64 {
            self.respond_error(ErrResponse::StaleToken, request, ctx).await;
            return Ok(());
        }

        if token_str.0.len() > auth::MAX_TOKEN_LENGTH {
            self.respond_error(ErrResponse::InvalidToken, request, ctx).await;
            return Ok(());
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
            self.respond_error(ErrResponse::InvalidToken, request, ctx).await;
            return Ok(());
        }

        let res = match self.database_server.send(RefreshToken(device)).await? {
            Ok(true) => (user, permission_flags),
            Ok(false) => {
                self.respond_error(ErrResponse::InvalidToken, request, ctx).await;
                return Ok(());
            },
            Err(e) => {
                self.respond_error(e, request, ctx).await;
                return Ok(());
            },
        };

        let (user, perms) = res;
        let addr = ctx.address().unwrap();
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
                UserSessions::new((device, ctx.address().unwrap()))
            });

        // This token is currently in use on another device
        if !inserted {
            self.respond_error(ErrResponse::TokenInUse, request, ctx).await;
        } else {
            self.state = SessionState::Ready(user, device, perms);
            self.respond(Ok(OkResponse::User { id: user }), request, ctx).await;
        }

        Ok(())
    }

    async fn create_token(
        &mut self,
        username: String,
        password: String,
        device_name: Option<String>,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        let mut token_bytes: [u8; 32] = [0; 32]; // 256 bits
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token_string = base64::encode(&token_bytes);
        let auth_token = AuthToken(token_string.clone());
        let username = auth::process_username(&username, &self.config);

        let verify = self.verify_username_password(username, password);
        let db_server = self.database_server.clone();

        let user = match verify.await {
            Ok(id) => id,
            Err(e) => {
                self.respond_error(e, request, ctx).await;
                return Ok(());
            },
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
            self.respond_error(e, request, ctx).await;
            return Ok(());
        }

        let res = Ok(OkResponse::Token {
            device,
            token: auth_token,
        });

        self.respond(res, request, ctx).await;
        Ok(())
    }

    async fn revoke_token(
        &mut self,
        to_revoke: DeviceId,
        password: Option<String>,
        user: UserId,
        current_device: DeviceId,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if to_revoke != current_device {
            let password = match password {
                Some(password) => password,
                None => {
                    self.respond_error(ErrResponse::AccessDenied, request, ctx).await;
                    return Ok(());
                },
            };

            if let Err(e) = self.verify_user_password(user, password).await {
                self.respond_error(e.into(), request, ctx).await;
                return Ok(());
            }
        }

        let res = self.database_server.send(RevokeToken(to_revoke)).await?;
        match res {
            Ok(false) => {
                self.respond_error(ErrResponse::DeviceDoesNotExist.into(), request, ctx).await;
                return Ok(());
            },
            Err(e) => {
                self.respond_error(e.into(), request, ctx).await;
                return Ok(());
            },
            _ => (),
        }

        if to_revoke == current_device {
            ctx.notify_immediately(LogoutThisSession);
        }

        self.respond(Ok(OkResponse::NoData), request, ctx).await;

        Ok(())
    }

    async fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        request: RequestId,
        ctx: &mut Context<Self>,
    )  -> Result<(), Disconnected> {
        if let Err(e) = self.verify_username_password(username, password).await {
            self.respond_error(e, request, ctx).await;
            return Ok(());
        }

        let res = self.database_server.send(RefreshToken(to_refresh)).await?;
        let res = match res {
            Ok(true) => Ok(OkResponse::NoData),
            Ok(false) => Err(ErrResponse::DeviceDoesNotExist),
            Err(e) => Err(e),
        };

        self.respond(res, request, ctx).await;

        Ok(())
    }

    async fn create_user(
        &mut self,
        username: String,
        display_name: String,
        password: String,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !auth::valid_password(&password, &self.config) {
            self.respond_error(ErrResponse::InvalidPassword, request, ctx).await;
            return Ok(());
        }

        let username = match auth::prepare_username(&username, &self.config) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                self.respond_error(ErrResponse::InvalidUsername, request, ctx).await;
                return Ok(());
            }
        };

        if !auth::valid_display_name(&display_name, &self.config) {
            self.respond_error(ErrResponse::InvalidDisplayName, request, ctx).await;
            return Ok(());
        }

        let (hash, hash_version) = auth::hash(password).await;
        let user = UserRecord::new(username, display_name, hash, hash_version);
        let id = user.id;

        match self.database_server.send(CreateUser(user)).await? {
            Ok(true) => self.respond(Ok(OkResponse::User { id }), request, ctx).await,
            Ok(false) => self.respond_error(ErrResponse::UsernameAlreadyExists, request, ctx).await,
            Err(e) => self.respond_error(e, request, ctx).await,
        }

        Ok(())
    }

    async fn change_username(
        &mut self,
        new_username: String,
        user: UserId,
        perms: TokenPermissionFlags,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx).await;
            return Ok(());
        }

        let new_username = match auth::prepare_username(&new_username, &self.config) {
            Ok(name) => name,
            Err(auth::TooShort) => {
                self.respond_error(ErrResponse::InvalidUsername, request, ctx).await;
                return Ok(());
            }
        };

        let req = self
            .database_server
            .send(ChangeUsername { user, new_username });

        let res = match req.await? {
            Ok(true) => Ok(OkResponse::NoData),
            Ok(false) => Err(ErrResponse::UsernameAlreadyExists),
            Err(e) => {
                self.respond_error(e, request, ctx).await;
                return Ok(());
            }
        };

        self.respond(res, request, ctx).await;

        Ok(())
    }

    async fn change_display_name(
        &mut self,
        new_display_name: String,
        user: UserId,
        request: RequestId,
        perms: TokenPermissionFlags,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !perms.has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME) {
            self.respond_error(ErrResponse::AccessDenied, request, ctx).await;
            return Ok(());
        }
        if !auth::valid_display_name(&new_display_name, &self.config) {
            self.respond_error(ErrResponse::InvalidDisplayName, request, ctx).await;
            return Ok(());
        };

        let req = self.database_server.send(ChangeDisplayName {
            user,
            new_display_name,
        });

        match req.await? {
            Ok(()) => self.respond(Ok(OkResponse::NoData), request, ctx).await,
            Err(e) => self.respond_error(e, request, ctx).await,
        }

        Ok(())
    }

    async fn change_password(
        &mut self,
        old_password: String,
        new_password: String,
        user: UserId,
        request: RequestId,
        ctx: &mut Context<Self>,
    ) -> Result<(), Disconnected> {
        if !auth::valid_password(&new_password, &self.config) {
            self.respond_error(ErrResponse::InvalidPassword, request, ctx).await;
            return Ok(());
        }

        let verify = self.verify_user_password(user, old_password);
        let db_server = self.database_server.clone();

        if let Err(e) = verify.await {
            self.respond_error(e, request, ctx).await;
            return Ok(());
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
            self.respond_error(e, request, ctx).await;
            return Ok(());
        }

        self.respond(Ok(OkResponse::NoData), request, ctx).await;

        Ok(())
    }

    fn create_community(
        &mut self,
        user: UserId,
        device: DeviceId,
        community_name: String,
        request: RequestId,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn join_community(
        &mut self,
        user: UserId,
        device: DeviceId,
        community: CommunityId,
        request: RequestId,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn verify_user_password(
        &mut self,
        user: UserId,
        password: String,
    ) -> impl Future<Output = Result<(), ErrResponse>> {
        let user = self.database_server.send(GetUserById(user));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(e) => {
                    error!("Xtra send error: {:#?}", e);
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
    ) -> impl Future<Output = Result<UserId, ErrResponse>> {
        let username = auth::process_username(&username, &self.config);
        let user = self.database_server.send(GetUserByName(username));

        async {
            let res = match user.await {
                Ok(res) => res,
                Err(e) => {
                    error!("Xtra send error: {:#?}", e);
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
