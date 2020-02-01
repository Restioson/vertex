use super::*;
use crate::community::{CommunityActor, CreateRoom, Join, COMMUNITIES};
use crate::config::Config;
use crate::database::*;
use crate::{auth, handle_disconnected, IdentifiedMessage, SendMessage};
use chrono::Utc;
use futures::stream::SplitSink;
use futures::{Future, SinkExt};
use rand::RngCore;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;
use vertex::*;
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;

pub struct WebSocketMessage(pub(crate) Result<ws::Message, warp::Error>);

impl Message for WebSocketMessage {
    type Result = ();
}

struct CheckHeartbeat;

impl Message for CheckHeartbeat {
    type Result = ();
}

enum State {
    Unauthenticated,
    Authenticated {
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
    },
}

pub struct ClientWsSession {
    sender: SplitSink<WebSocket, ws::Message>,
    database: Database,
    state: State,
    heartbeat: Instant,
    config: Arc<Config>,
    communities: Vec<CommunityId>,
}

impl ClientWsSession {
    pub fn new(
        sender: SplitSink<WebSocket, ws::Message>,
        database: Database,
        config: Arc<Config>,
    ) -> Self {
        ClientWsSession {
            sender,
            database,
            state: State::Unauthenticated,
            heartbeat: Instant::now(),
            config,
            communities: Vec::new(),
        }
    }
}

impl Actor for ClientWsSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_interval(HEARTBEAT_TIMEOUT, || CheckHeartbeat);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        self.log_out();
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
        message: WebSocketMessage,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if self.handle_ws_message(message, ctx).await.is_err() {
                self.log_out();
                ctx.stop();
            }
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        msg: SendMessage<ServerMessage>,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if self.send(msg.0).await.is_err() {
                ctx.stop()
            }
        }
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: LogoutThisSession, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            let _ = self
                .send(ServerMessage::Action(ServerAction::SessionLoggedOut))
                .await;
            self.log_out();
        }
    }
}

// TODO: Error Handling: should not .unwrap() on `xtra::Disconnected` and `warp::Error`
impl ClientWsSession {
    #[inline]
    async fn send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.sender.send(ws::Message::binary(msg)).await
    }

    /// Remove the device from wherever it is referenced
    fn log_out(&mut self) {
        use std::mem;

        let state = mem::replace(&mut self.state, State::Unauthenticated);
        if let State::Authenticated {
            user: user_id,
            device,
            ..
        } = state
        {
            if let Some(mut user) = USERS.get_mut(&user_id) {
                // Remove the device
                let devices = &mut user.sessions;
                if let Some(idx) = devices.iter().position(|(id, _)| *id == device) {
                    devices.remove(idx);

                    // Remove the entire user entry if they are no longer online
                    if devices.is_empty() {
                        drop(user); // Prevent double lock on USERS
                        USERS.remove(&user_id);
                    }
                }
            }
        }
    }

    async fn handle_ws_message(
        &mut self,
        message: WebSocketMessage,
        ctx: &mut Context<Self>,
    ) -> Result<(), warp::Error> {
        let message = message.0?;

        if message.is_ping() {
            self.heartbeat = Instant::now();
            self.sender.send(ws::Message::ping(vec![])).await?;
        } else if message.is_binary() {
            let msg: ClientMessage = match serde_cbor::from_slice(message.as_bytes()) {
                Ok(m) => m,
                Err(_) => {
                    self.send(ServerMessage::MalformedMessage).await?;
                    return Ok(());
                }
            };

            let response = self.handle_request(ctx, msg.request).await;
            self.send(ServerMessage::Response {
                id: msg.id,
                result: response,
            })
            .await?;
        } else if message.is_close() {
            ctx.stop();
        } else {
            self.send(ServerMessage::MalformedMessage).await?;
        }

        Ok(())
    }

    async fn handle_request(
        &mut self,
        ctx: &mut Context<Self>,
        request: ClientRequest,
    ) -> ResponseResult {
        match request {
            ClientRequest::CreateToken {
                credentials,
                options,
            } => self.create_token(credentials, options).await,
            ClientRequest::CreateUser {
                credentials,
                display_name,
            } => self.create_user(credentials, display_name).await,
            ClientRequest::RefreshToken {
                credentials,
                device,
            } => self.refresh_token(credentials, device).await,
            request => match &mut self.state {
                State::Unauthenticated => {
                    Unauthenticated { client: self, ctx }
                        .handle_request(request)
                        .await
                }
                State::Authenticated {
                    user,
                    device,
                    perms,
                } => {
                    let (user, device, perms) = (*user, *device, *perms);
                    Authenticated {
                        client: self,
                        ctx,
                        user,
                        device,
                        perms,
                    }
                    .handle_request(request)
                    .await
                }
            },
        }
    }

    async fn create_user(
        &mut self,
        credentials: UserCredentials,
        display_name: String,
    ) -> ResponseResult {
        if !auth::valid_password(&credentials.password, &self.config) {
            return Err(ErrResponse::InvalidPassword);
        }

        let username = match auth::prepare_username(&credentials.username, &self.config) {
            Ok(name) => name,
            Err(auth::TooShort) => return Err(ErrResponse::InvalidUsername),
        };

        if !auth::valid_display_name(&display_name, &self.config) {
            return Err(ErrResponse::InvalidDisplayName);
        }

        let (hash, hash_version) = auth::hash(credentials.password).await;

        let user = UserRecord::new(username, display_name, hash, hash_version);
        let id = user.id;

        match self.database.create_user(user).await? {
            Ok(()) => Ok(OkResponse::User { id }),
            Err(UsernameConflict) => Err(ErrResponse::UsernameAlreadyExists),
        }
    }

    async fn create_token(
        &mut self,
        credentials: UserCredentials,
        options: TokenCreationOptions,
    ) -> ResponseResult {
        let user = self.verify_credentials(credentials).await?;

        let mut token_bytes: [u8; 32] = [0; 32]; // 256 bits
        rand::thread_rng().fill_bytes(&mut token_bytes);

        let token_string = base64::encode(&token_bytes);

        let auth_token = AuthToken(token_string.clone());
        let (token_hash, hash_scheme_version) = auth::hash(token_string).await;

        let device = DeviceId(Uuid::new_v4());
        let token = Token {
            token_hash,
            hash_scheme_version,
            user,
            device,
            device_name: options.device_name,
            last_used: Utc::now(),
            expiration_date: options.expiration_date,
            permission_flags: options.permission_flags,
        };

        if let Err(DeviceIdConflict) = self.database.create_token(token).await? {
            // The chances of a UUID conflict is so abysmally low that we can only assume that a
            // conflict is due to a programming error

            panic!("Newly generated UUID conflicts with another!");
        }

        Ok(OkResponse::Token {
            device,
            token: auth_token,
        })
    }

    async fn refresh_token(
        &mut self,
        credentials: UserCredentials,
        to_refresh: DeviceId,
    ) -> ResponseResult {
        self.verify_credentials(credentials).await?;

        match self.database.refresh_token(to_refresh).await? {
            Ok(()) => Ok(OkResponse::NoData),
            Err(NonexistentDevice) => Err(ErrResponse::DeviceDoesNotExist),
        }
    }

    async fn verify_credentials(
        &mut self,
        credentials: UserCredentials,
    ) -> Result<UserId, ErrResponse> {
        let username = auth::normalize_username(&credentials.username, &self.config);
        let password = credentials.password;

        let user = match self.database.get_user_by_name(username).await? {
            Some(user) => user,
            None => return Err(ErrResponse::InvalidUser),
        };

        let id = user.id;
        if auth::verify_user(user, password).await {
            Ok(id)
        } else {
            Err(ErrResponse::IncorrectUsernameOrPassword)
        }
    }
}

struct Unauthenticated<'a> {
    client: &'a mut ClientWsSession,
    ctx: &'a mut Context<ClientWsSession>,
}

impl<'a> Unauthenticated<'a> {
    async fn handle_request(&mut self, request: ClientRequest) -> ResponseResult {
        match request {
            ClientRequest::Login { device, token } => self.login(device, token).await,
            _ => Err(ErrResponse::NotLoggedIn),
        }
    }

    async fn login(&mut self, device: DeviceId, pass: AuthToken) -> ResponseResult {
        let token = match self.client.database.get_token(device).await? {
            Some(token) => token,
            None => return Err(ErrResponse::InvalidToken),
        };

        let user = match self.client.database.get_user_by_id(token.user).await? {
            Some(user) => user,
            None => return Err(ErrResponse::InvalidUser),
        };

        // Check if can log in with this token
        if user.locked {
            return Err(ErrResponse::UserLocked);
        } else if user.banned {
            return Err(ErrResponse::UserBanned);
        } else if user.compromised {
            return Err(ErrResponse::UserCompromised);
        } else if (Utc::now() - token.last_used).num_days()
            > self.client.config.token_stale_days as i64
        {
            return Err(ErrResponse::StaleToken);
        }

        if pass.0.len() > auth::MAX_TOKEN_LENGTH {
            return Err(ErrResponse::InvalidToken);
        }

        let Token {
            token_hash,
            hash_scheme_version,
            user,
            permission_flags,
            ..
        } = token;

        if !auth::verify(pass.0, token_hash, hash_scheme_version).await {
            return Err(ErrResponse::InvalidToken);
        }

        if let Err(NonexistentDevice) = self.client.database.refresh_token(device).await? {
            return Err(ErrResponse::InvalidToken);
        }

        let session = (device, self.ctx.address().unwrap());

        if let Some(mut user_sessions) = USERS.get_mut(&user) {
            let existing_session = user_sessions.sessions.iter().find(|(id, _)| *id == device);
            if existing_session.is_some() {
                return Err(ErrResponse::TokenInUse);
            }
            user_sessions.sessions.push(session);
        } else {
            USERS.insert(user, UserSessions::new(session));
        }

        self.client.state = State::Authenticated {
            user,
            device,
            perms: permission_flags,
        };

        Ok(OkResponse::User { id: user })
    }
}

struct Authenticated<'a> {
    client: &'a mut ClientWsSession,
    ctx: &'a mut Context<ClientWsSession>,
    user: UserId,
    device: DeviceId,
    perms: TokenPermissionFlags,
}

impl<'a> Authenticated<'a> {
    async fn handle_request(self, request: ClientRequest) -> ResponseResult {
        match request {
            ClientRequest::SendMessage(message) => self.send_message(message).await,
            ClientRequest::EditMessage(edit) => self.edit_message(edit).await,
            ClientRequest::JoinCommunity(community) => self.join_community(community).await,
            ClientRequest::CreateCommunity { name } => self.create_community(name).await,
            ClientRequest::RevokeToken => self.revoke_token().await,
            ClientRequest::RevokeForeignToken { device, password } => {
                self.revoke_foreign_token(device, password).await
            }
            ClientRequest::ChangeUsername { new_username } => {
                self.change_username(new_username).await
            }
            ClientRequest::ChangeDisplayName { new_display_name } => {
                self.change_display_name(new_display_name).await
            }
            ClientRequest::ChangePassword {
                old_password,
                new_password,
            } => self.change_password(old_password, new_password).await,
            ClientRequest::Login { .. } => Err(ErrResponse::AlreadyLoggedIn),
            ClientRequest::CreateRoom { name, community } => {
                self.create_room(name, community).await
            }
            _ => unimplemented!(),
        }
    }

    async fn verify_password(&mut self, password: String) -> Result<(), ErrResponse> {
        let user = match self.client.database.get_user_by_id(self.user).await? {
            Some(user) => user,
            None => return Err(ErrResponse::InvalidUser),
        };

        if auth::verify_user(user, password).await {
            Ok(())
        } else {
            Err(ErrResponse::IncorrectUsernameOrPassword)
        }
    }

    async fn send_message(self, message: ClientSentMessage) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.client.communities.contains(&message.to_community) {
            return Err(ErrResponse::InvalidCommunity);
        }

        match COMMUNITIES.get(&message.to_community) {
            Some(community) => {
                let message = IdentifiedMessage {
                    user: self.user,
                    device: self.device,
                    message,
                };
                let id = community
                    .send(message)
                    .await
                    .map_err(handle_disconnected("Community"))??;

                Ok(OkResponse::MessageId { id })
            }
            _ => Err(ErrResponse::InvalidCommunity),
        }
    }

    async fn edit_message(self, edit: Edit) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.client.communities.contains(&edit.community) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if let Some(community) = COMMUNITIES.get(&edit.community) {
            let message = IdentifiedMessage {
                user: self.user,
                device: self.device,
                message: edit,
            };
            community
                .send(message)
                .await
                .map_err(handle_disconnected("Community"))??;
            Ok(OkResponse::NoData)
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }

    async fn revoke_token(self) -> ResponseResult {
        if let Err(NonexistentDevice) = self.client.database.revoke_token(self.device).await? {
            return Err(ErrResponse::DeviceDoesNotExist);
        }

        self.ctx.notify_immediately(LogoutThisSession);

        Ok(OkResponse::NoData)
    }

    async fn revoke_foreign_token(
        mut self,
        to_revoke: DeviceId,
        password: String,
    ) -> ResponseResult {
        self.verify_password(password).await?;
        if let Err(NonexistentDevice) = self.client.database.revoke_token(to_revoke).await? {
            return Err(ErrResponse::DeviceDoesNotExist);
        }

        Ok(OkResponse::NoData)
    }

    async fn change_username(self, new_username: String) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return Err(ErrResponse::AccessDenied);
        }

        let new_username = match auth::prepare_username(&new_username, &self.client.config) {
            Ok(name) => name,
            Err(auth::TooShort) => return Err(ErrResponse::InvalidUsername),
        };

        match self
            .client
            .database
            .change_username(self.user, new_username)
            .await?
        {
            Ok(()) => Ok(OkResponse::NoData),
            Err(ChangeUsernameError::UsernameConflict) => Err(ErrResponse::UsernameAlreadyExists),
            Err(ChangeUsernameError::NonexistentUser) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(ErrResponse::UserDeleted)
            }
        }
    }

    async fn change_display_name(self, new_display_name: String) -> ResponseResult {
        if !self
            .perms
            .has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME)
        {
            return Err(ErrResponse::AccessDenied);
        }

        if !auth::valid_display_name(&new_display_name, &self.client.config) {
            return Err(ErrResponse::InvalidDisplayName);
        }

        match self
            .client
            .database
            .change_display_name(self.user, new_display_name)
            .await?
        {
            Ok(()) => Ok(OkResponse::NoData),
            Err(NonexistentUser) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(ErrResponse::UserDeleted)
            }
        }
    }

    async fn change_password(
        mut self,
        old_password: String,
        new_password: String,
    ) -> ResponseResult {
        if !auth::valid_password(&new_password, &self.client.config) {
            return Err(ErrResponse::InvalidPassword);
        }

        self.verify_password(old_password).await?;

        let (new_password_hash, hash_version) = auth::hash(new_password).await;
        let res = self
            .client
            .database
            .change_password(self.user, new_password_hash, hash_version)
            .await?;

        match res {
            Ok(()) => Ok(OkResponse::NoData),
            Err(NonexistentUser) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(ErrResponse::UserDeleted)
            }
        }
    }

    async fn create_community(self, name: String) -> ResponseResult {
        if !self
            .perms
            .has_perms(TokenPermissionFlags::CREATE_COMMUNITIES)
        {
            return Err(ErrResponse::AccessDenied);
        }

        let id = self.client.database.create_community(name).await?;
        CommunityActor::create_and_spawn(id, self.client.database.clone(), self.user);
        self.join_community(id).await?;

        Ok(OkResponse::Community { id })
    }

    async fn join_community(self, id: CommunityId) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::JOIN_COMMUNITIES) {
            return Err(ErrResponse::AccessDenied);
        }

        if let Some(community) = COMMUNITIES.get(&id) {
            let join = Join {
                user: self.user,
                device_id: self.device,
                session: self.ctx.address().unwrap(),
            };

            let res = community
                .send(join)
                .await
                .map_err(handle_disconnected("Community"))??;

            match res {
                Ok(()) => {
                    self.client.communities.push(id);
                    Ok(OkResponse::NoData)
                }
                Err(AddToCommunityError::AlreadyInCommunity) => {
                    Err(ErrResponse::AlreadyInCommunity)
                }
                Err(AddToCommunityError::InvalidCommunity) => Err(ErrResponse::InvalidCommunity),
                Err(AddToCommunityError::InvalidUser) => Err(ErrResponse::InvalidUser),
            }
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }

    async fn create_room(self, name: String, id: CommunityId) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_ROOMS) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.client.communities.contains(&id) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if let Some(community) = COMMUNITIES.get(&id) {
            let create = CreateRoom {
                creator: self.device,
                name: name.clone(),
            };
            let id = community
                .send(create)
                .await
                .map_err(handle_disconnected("Community"))?;

            self.client
                .send(ServerMessage::Action(ServerAction::AddRoom { id, name }))
                .await
                .unwrap();

            Ok(OkResponse::Room { id })
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }
}
