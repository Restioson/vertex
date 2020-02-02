use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Instant;

use futures::{Future, SinkExt};
use futures::stream::SplitSink;
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;

pub use manager::*;
use vertex::*;

use crate::{auth, handle_disconnected, IdentifiedMessage, SendMessage};
use crate::community::{COMMUNITIES, CommunityActor, CreateRoom, Join};
use crate::config::Config;
use crate::database::*;

use super::*;

mod manager;

pub struct WebSocketMessage(pub(crate) Result<ws::Message, warp::Error>);

impl Message for WebSocketMessage {
    type Result = ();
}

struct CheckHeartbeat;

impl Message for CheckHeartbeat {
    type Result = ();
}

pub struct ActiveSession {
    ws: SplitSink<WebSocket, ws::Message>,
    global: crate::Global,
    heartbeat: Instant,
    communities: Vec<CommunityId>,
    user: UserId,
    device: DeviceId,
    perms: TokenPermissionFlags,
}

impl ActiveSession {
    pub fn new(
        ws: SplitSink<WebSocket, ws::Message>,
        global: crate::Global,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
    ) -> Self {
        ActiveSession {
            ws,
            global,
            heartbeat: Instant::now(),
            communities: Vec::new(),
            user,
            device,
            perms,
        }
    }
}

impl Actor for ActiveSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_interval(HEARTBEAT_TIMEOUT, || CheckHeartbeat);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        self.log_out();
    }
}

impl Handler<CheckHeartbeat> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: CheckHeartbeat, ctx: &mut Context<Self>) -> Self::Responder<'_> {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            ctx.stop();
        }

        async {}
    }
}

impl Handler<WebSocketMessage> for ActiveSession {
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

impl Handler<SendMessage<ServerMessage>> for ActiveSession {
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

impl Handler<LogoutThisSession> for ActiveSession {
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
impl ActiveSession {
    #[inline]
    async fn send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.ws.send(ws::Message::binary(msg)).await
    }

    /// Remove the device from wherever it is referenced
    fn log_out(&mut self) {
        manager::remove(self.user, self.device);
    }

    async fn handle_ws_message(
        &mut self,
        message: WebSocketMessage,
        ctx: &mut Context<Self>,
    ) -> Result<(), warp::Error> {
        let message = message.0?;

        if message.is_ping() {
            self.heartbeat = Instant::now();
            self.ws.send(ws::Message::ping(vec![])).await?;
        } else if message.is_binary() {
            let msg: ClientMessage = match serde_cbor::from_slice(message.as_bytes()) {
                Ok(m) => m,
                Err(_) => {
                    self.send(ServerMessage::MalformedMessage).await?;
                    return Ok(());
                }
            };

            let (user, device, perms) = (self.user, self.device, self.perms);
            let response = RequestHandler { session: self, ctx, user, device, perms }
                .handle_request(msg.request).await;

            self.send(ServerMessage::Response {
                id: msg.id,
                result: response,
            }).await?;
        } else if message.is_close() {
            ctx.stop();
        } else {
            self.send(ServerMessage::MalformedMessage).await?;
        }

        Ok(())
    }
}

struct RequestHandler<'a> {
    session: &'a mut ActiveSession,
    ctx: &'a mut Context<ActiveSession>,
    user: UserId,
    device: DeviceId,
    perms: TokenPermissionFlags,
}

impl<'a> RequestHandler<'a> {
    async fn handle_request(self, request: ClientRequest) -> ResponseResult {
        match request {
            ClientRequest::SendMessage(message) => self.send_message(message).await,
            ClientRequest::EditMessage(edit) => self.edit_message(edit).await,
            ClientRequest::JoinCommunity(code) => self.join_community(code).await,
            ClientRequest::CreateCommunity { name } => self.create_community(name).await,
            ClientRequest::RevokeToken => self.revoke_token().await,
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
            ClientRequest::CreateRoom { name, community } => {
                self.create_room(name, community).await
            }
            ClientRequest::CreateInvite { community } => self.create_invite(community).await,
            _ => unimplemented!(),
        }
    }

    async fn verify_password(&mut self, password: String) -> Result<(), ErrResponse> {
        let user = match self.session.global.database.get_user_by_id(self.user).await? {
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

        if !self.session.communities.contains(&message.to_community) {
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

        if !self.session.communities.contains(&edit.community) {
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
        if let Err(NonexistentDevice) = self.session.global.database.revoke_token(self.device).await? {
            return Err(ErrResponse::DeviceDoesNotExist);
        }

        self.ctx.notify_immediately(LogoutThisSession);

        Ok(OkResponse::NoData)
    }

    async fn change_username(self, new_username: String) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return Err(ErrResponse::AccessDenied);
        }

        let new_username = match auth::prepare_username(&new_username, &self.session.global.config) {
            Ok(name) => name,
            Err(auth::TooShort) => return Err(ErrResponse::InvalidUsername),
        };

        let database = &self.session.global.database;
        match database.change_username(self.user, new_username).await? {
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

        if !auth::valid_display_name(&new_display_name, &self.session.global.config) {
            return Err(ErrResponse::InvalidDisplayName);
        }

        let database = &self.session.global.database;
        match database.change_display_name(self.user, new_display_name).await? {
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
        if !auth::valid_password(&new_password, &self.session.global.config) {
            return Err(ErrResponse::InvalidPassword);
        }

        self.verify_password(old_password).await?;

        let (new_password_hash, hash_version) = auth::hash(new_password).await;

        let database = &self.session.global.database;
        let res = database.change_password(self.user, new_password_hash, hash_version).await?;

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

        let id = self.session.global.database.create_community(name).await?;
        CommunityActor::create_and_spawn(id, self.session.global.database.clone(), self.user);
        self.join_community_by_id(id).await?;

        Ok(OkResponse::Community { id })
    }

    async fn join_community(self, code: InviteCode) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::JOIN_COMMUNITIES) {
            return Err(ErrResponse::AccessDenied);
        }

        if code.0.len() > 11 {
            return Err(ErrResponse::InvalidInviteCode);
        }

        let database = &self.session.global.database;
        let id = match database.get_community_from_invite_code(code).await? {
            Ok(Some(id)) => id,
            Ok(None) | Err(_) => return Err(ErrResponse::InvalidInviteCode),
        };

        self.join_community_by_id(id).await
    }

    async fn join_community_by_id(self, id: CommunityId) -> ResponseResult {
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
                    self.session.communities.push(id);
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

        if !self.session.communities.contains(&id) {
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

            self.session
                .send(ServerMessage::Action(ServerAction::AddRoom { id, name }))
                .await
                .unwrap();

            Ok(OkResponse::Room { id })
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }

    async fn create_invite(self, id: CommunityId) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_INVITES) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.session.communities.contains(&id) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if COMMUNITIES.contains_key(&id) {
            let code = self.session.global.database.create_invite_code(id).await?;

            Ok(OkResponse::Invite { code })
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }
}
