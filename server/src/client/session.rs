use std::time::Instant;

use chrono::{DateTime, Utc};
use futures::stream::SplitSink;
use futures::{Future, SinkExt};
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;

pub use manager::*;
use vertex::*;

use crate::community::{CommunityActor, CreateRoom, GetRoomStructures, Join, COMMUNITIES};
use crate::database::*;
use crate::{auth, handle_disconnected, IdentifiedMessage, SendMessage};

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

struct NotifyClientReady;

impl Message for NotifyClientReady {
    type Result = ();
}

pub struct ActiveSession {
    ws: SplitSink<WebSocket, ws::Message>,
    global: crate::Global,
    heartbeat: Instant,
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
            user,
            device,
            perms,
        }
    }
}

impl Actor for ActiveSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_immediately(NotifyClientReady);
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
                ctx.stop();
            }
        }
    }
}

impl Handler<NotifyClientReady> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        _: NotifyClientReady,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if self.send_ready_event().await.is_err() {
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
                .send(ServerMessage::Event(ServerEvent::SessionLoggedOut))
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

    fn in_community(&self, id: &CommunityId) -> bool {
        manager::get_active_user(self.user)
            .unwrap()
            .communities
            .contains(&id)
    }

    async fn send_ready_event(&mut self) -> Result<(), ()> {
        // TODO: handle errors better

        let user = self
            .global
            .database
            .get_user_by_id(self.user)
            .await
            .map_err(|_| ())?
            .ok_or(())?;

        let active = manager::get_active_user(self.user).unwrap();
        let mut communities = Vec::with_capacity(active.communities.len());

        for id in active.communities.iter() {
            let addr = COMMUNITIES.get(id).unwrap().actor.clone();
            let rooms = addr.send(GetRoomStructures).await.unwrap(); // TODO errors thing

            let structure = CommunityStructure {
                id: *id,
                name: COMMUNITIES.get(id).unwrap().name.clone(),
                rooms,
            };

            communities.push(structure);
        }

        let ready = ClientReady {
            user: self.user,
            username: user.username,
            display_name: user.display_name,
            communities,
        };

        self.send(ServerMessage::Event(ServerEvent::ClientReady(ready)))
            .await
            .map_err(|_| ())
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
            let response = RequestHandler {
                session: self,
                ctx,
                user,
                device,
                perms,
            }
            .handle_request(msg.request)
            .await;

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
            ClientRequest::LogOut => self.log_out().await,
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
            ClientRequest::CreateInvite {
                community,
                expiration_date,
            } => self.create_invite(community, expiration_date).await,
            _ => unimplemented!(),
        }
    }

    async fn verify_password(&mut self, password: String) -> Result<(), ErrResponse> {
        let user = match self
            .session
            .global
            .database
            .get_user_by_id(self.user)
            .await?
        {
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

        if !self.session.in_community(&message.to_community) {
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
                    .actor
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

        if !self.session.in_community(&edit.community) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if let Some(community) = COMMUNITIES.get(&edit.community) {
            let message = IdentifiedMessage {
                user: self.user,
                device: self.device,
                message: edit,
            };
            community
                .actor
                .send(message)
                .await
                .map_err(handle_disconnected("Community"))??;
            Ok(OkResponse::NoData)
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }

    async fn log_out(self) -> ResponseResult {
        if let Err(NonexistentDevice) = self
            .session
            .global
            .database
            .revoke_token(self.device)
            .await?
        {
            return Err(ErrResponse::DeviceDoesNotExist);
        }

        self.ctx.notify_immediately(LogoutThisSession);

        Ok(OkResponse::NoData)
    }

    async fn change_username(self, new_username: String) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return Err(ErrResponse::AccessDenied);
        }

        let new_username = match auth::prepare_username(&new_username, &self.session.global.config)
        {
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
        match database
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
        if !auth::valid_password(&new_password, &self.session.global.config) {
            return Err(ErrResponse::InvalidPassword);
        }

        self.verify_password(old_password).await?;

        let (new_password_hash, hash_version) = auth::hash(new_password).await;

        let database = &self.session.global.database;
        let res = database
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

        let id = self
            .session
            .global
            .database
            .create_community(name.clone())
            .await?;
        CommunityActor::create_and_spawn(name, id, self.session.global.database.clone(), self.user);

        self.join_community_by_id(id).await
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
        // TODO: needs to send ServerAction::AddCommunity to other devices

        if let Some(community) = COMMUNITIES.get(&id) {
            let join = Join {
                user: self.user,
                device_id: self.device,
                session: self.ctx.address().unwrap(),
            };

            let res = community
                .actor
                .send(join)
                .await
                .map_err(handle_disconnected("Community"))??;

            match res {
                Ok(community) => {
                    if let Some(mut user) = manager::get_active_user_mut(self.user) {
                        user.communities.insert(community.id);
                        let community = community.clone();
                        let send = ServerMessage::Event(ServerEvent::AddCommunity(community));
                        let sessions = user.sessions.iter();
                        sessions
                            .filter(|(id, _)| **id != self.device)
                            .filter_map(|(_, session)| session.as_active())
                            .for_each(|addr| {
                                let _ = addr.do_send(SendMessage(send.clone()));
                            });
                    }

                    Ok(OkResponse::AddCommunity { community })
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

    async fn create_room(self, name: String, community_id: CommunityId) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_ROOMS) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.session.in_community(&community_id) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if let Some(community) = COMMUNITIES.get(&community_id) {
            let create = CreateRoom {
                creator: self.device,
                name: name.clone(),
            };
            let id = community
                .actor
                .send(create)
                .await
                .map_err(handle_disconnected("Community"))??;

            // TODO: needs to send ServerAction::AddRoom to other devices

            let room = RoomStructure { id, name };
            Ok(OkResponse::AddRoom {
                community: *community.key(),
                room,
            })
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }

    async fn create_invite(
        self,
        id: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    ) -> ResponseResult {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_INVITES) {
            return Err(ErrResponse::AccessDenied);
        }

        if !self.session.in_community(&id) {
            return Err(ErrResponse::InvalidCommunity);
        }

        if COMMUNITIES.contains_key(&id) {
            let db = &self.session.global.database;
            let max = self.session.global.config.max_invite_codes_per_community as i64;
            let res = db.create_invite_code(id, expiration_date, max).await?;

            match res {
                Ok(code) => Ok(OkResponse::Invite { code }),
                Err(_) => Err(ErrResponse::TooManyInviteCodes),
            }
        } else {
            Err(ErrResponse::InvalidCommunity)
        }
    }
}
