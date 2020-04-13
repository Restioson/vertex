//! Methods that can be executed by regular users

use log::warn;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use xtra::Context;

use crate::client::session::{manager, UserCommunity, UserRoom};
use crate::client::ActiveSession;
use crate::community::CommunityActor;
use crate::community::COMMUNITIES;
use crate::{auth, community, handle_disconnected, IdentifiedMessage};

use super::*;

pub struct RequestHandler<'a> {
    pub session: &'a mut ActiveSession,
    pub ctx: &'a mut Context<ActiveSession>,
    pub user: UserId,
    pub device: DeviceId,
    pub perms: TokenPermissionFlags,
}

impl<'a> RequestHandler<'a> {
    pub async fn handle_request(self, request: ClientRequest) -> Result<OkResponse, Error> {
        match request {
            ClientRequest::SendMessage(message) => self.send_message(message).await,
            ClientRequest::EditMessage(edit) => self.edit_message(edit).await,
            ClientRequest::JoinCommunity(code) => self.join_community(code).await,
            ClientRequest::CreateCommunity { name } => self.create_community(name).await,
            ClientRequest::LogOut => self.log_out().await,
            ClientRequest::GetProfile(id) => self.get_user_profile(id).await,
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
                expiration_datetime,
            } => self.create_invite(community, expiration_datetime).await,
            ClientRequest::GetRoomUpdate {
                community,
                room,
                last_received,
                message_count,
            } => {
                self.get_room_update(community, room, last_received, message_count)
                    .await
            }
            ClientRequest::SelectRoom { community, room } => {
                self.select_room(community, room).await
            }
            ClientRequest::DeselectRoom => self.deselect_room().await,
            ClientRequest::GetMessages {
                community,
                room,
                selector,
                count,
            } => self.get_messages(community, room, selector, count).await,
            ClientRequest::SetAsRead { community, room } => self.set_as_read(community, room).await,
            ClientRequest::ChangeCommunityName { new, community } => {
                self.change_community_name(new, community).await
            }
            ClientRequest::ChangeCommunityDescription { new, community } => {
                self.change_community_description(new, community).await
            }
            _ => Err(Error::Unimplemented),
        }
    }

    async fn verify_password(&mut self, password: String) -> Result<(), Error> {
        let user = match self
            .session
            .global
            .database
            .get_user_by_id(self.user)
            .await?
        {
            Some(user) => user,
            None => return Err(Error::InvalidUser),
        };

        if auth::verify_user(user, password).await {
            Ok(())
        } else {
            Err(Error::IncorrectUsernameOrPassword)
        }
    }

    async fn send_message(self, message: ClientSentMessage) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            return Err(Error::AccessDenied);
        }

        if !self.session.in_community(&message.to_community) {
            return Err(Error::InvalidCommunity);
        }

        if message.content.len() > self.session.global.config.max_message_len as usize {
            return Err(Error::TextTooLong);
        }

        match community::address_of(message.to_community) {
            Some(community) => {
                let message = IdentifiedMessage {
                    user: self.user,
                    device: self.device,
                    message,
                };

                let confirmation = community
                    .send(message)
                    .await
                    .map_err(handle_disconnected("Community"))??;

                Ok(OkResponse::ConfirmMessage(confirmation))
            }
            _ => Err(Error::InvalidCommunity),
        }
    }

    async fn edit_message(self, edit: Edit) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::SEND_MESSAGES) {
            return Err(Error::AccessDenied);
        }

        if !self.session.in_community(&edit.community) {
            return Err(Error::InvalidCommunity);
        }

        if edit.new_content.len() > self.session.global.config.max_message_len as usize {
            return Err(Error::TextTooLong);
        }

        if let Some(community) = community::address_of(edit.community) {
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
            Err(Error::InvalidCommunity)
        }
    }

    async fn log_out(self) -> Result<OkResponse, Error> {
        if let Err(NonexistentDevice) = self
            .session
            .global
            .database
            .revoke_token(self.device)
            .await?
        {
            return Err(Error::DeviceDoesNotExist);
        }

        self.ctx.notify_immediately(LogoutThisSession);

        Ok(OkResponse::NoData)
    }

    async fn get_user_profile(self, id: UserId) -> Result<OkResponse, Error> {
        match self.session.global.database.get_user_profile(id).await? {
            Some(profile) => Ok(OkResponse::Profile(profile)),
            None => Err(Error::InvalidUser),
        }
    }

    async fn change_username(self, new_username: String) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::CHANGE_USERNAME) {
            return Err(Error::AccessDenied);
        }

        let new_username = match auth::prepare_username(&new_username, &self.session.global.config)
        {
            Ok(name) => name,
            Err(auth::TooShort) => return Err(Error::InvalidUsername),
        };

        let database = &self.session.global.database;
        match database.change_username(self.user, new_username).await? {
            Ok(()) => Ok(OkResponse::NoData),
            Err(ChangeUsernameError::UsernameConflict) => Err(Error::UsernameAlreadyExists),
            Err(ChangeUsernameError::NonexistentUser) => {
                warn!("Nonexistent user! Is this a timing anomaly? Client: {:#?}", self.session);
                self.ctx.stop(); // The user did not exist at the time of request
                Err(Error::UserDeleted)
            }
        }
    }

    async fn change_display_name(self, new_display_name: String) -> Result<OkResponse, Error> {
        if !self
            .perms
            .has_perms(TokenPermissionFlags::CHANGE_DISPLAY_NAME)
        {
            return Err(Error::AccessDenied);
        }

        if !auth::valid_display_name(&new_display_name, &self.session.global.config) {
            return Err(Error::InvalidDisplayName);
        }

        let database = &self.session.global.database;
        match database
            .change_display_name(self.user, new_display_name)
            .await?
        {
            Ok(()) => Ok(OkResponse::NoData),
            Err(_) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(Error::UserDeleted)
            }
        }
    }

    async fn change_password(
        mut self,
        old_password: String,
        new_password: String,
    ) -> Result<OkResponse, Error> {
        if !auth::valid_password(&new_password, &self.session.global.config) {
            return Err(Error::InvalidPassword);
        }

        self.verify_password(old_password).await?;

        let (new_password_hash, hash_version) = auth::hash(new_password).await;

        let database = &self.session.global.database;
        let res = database
            .change_password(self.user, new_password_hash, hash_version)
            .await?;

        match res {
            Ok(()) => Ok(OkResponse::NoData),
            Err(_) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(Error::UserDeleted)
            }
        }
    }

    async fn create_community(self, name: String) -> Result<OkResponse, Error> {
        if !self
            .perms
            .has_perms(TokenPermissionFlags::CREATE_COMMUNITIES)
        {
            return Err(Error::AccessDenied);
        }

        let db = &self.session.global.database;
        let id = db.create_community(name.clone()).await?;
        let res = db
            .create_default_user_room_states_for_user(id, self.user)
            .await?;

        match res {
            Ok(_) => {
                CommunityActor::create_and_spawn(name, id, db.clone(), self.user);
                self.join_community_by_id(id).await
            }
            Err(_) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(Error::UserDeleted)
            }
        }
    }

    async fn join_community(self, code: InviteCode) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::JOIN_COMMUNITIES) {
            return Err(Error::AccessDenied);
        }

        if code.0.len() > 11 {
            return Err(Error::InvalidInviteCode);
        }

        let database = &self.session.global.database;
        let id = match database.get_community_from_invite_code(code).await? {
            Ok(Some(id)) => id,
            Ok(None) | Err(_) => return Err(Error::InvalidInviteCode),
        };

        self.join_community_by_id(id).await
    }

    async fn join_community_by_id(self, id: CommunityId) -> Result<OkResponse, Error> {
        if let Some(community) = community::address_of(id) {
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
                Ok(community) => {
                    let db = &self.session.global.database;
                    let user_community = UserCommunity::load(db, self.user, id).await?;

                    if let Some(mut user) = manager::get_active_user_mut(self.user) {
                        user.communities.insert(community.id, user_community);

                        let community = community.clone();
                        let send = ServerMessage::Event(ServerEvent::AddCommunity(community));
                        let sessions = user.sessions.iter();

                        sessions
                            .filter(|(id, _)| **id != self.device)
                            .filter_map(|(_, session)| session.as_active_actor())
                            .for_each(|addr| {
                                let _ = addr.do_send(SendMessage(send.clone()));
                            });
                    }

                    Ok(OkResponse::AddCommunity(community))
                }
                Err(AddToCommunityError::AlreadyInCommunity) => Err(Error::AlreadyInCommunity),
                Err(AddToCommunityError::InvalidCommunity) => Err(Error::InvalidCommunity),
                Err(AddToCommunityError::InvalidUser) => Err(Error::InvalidUser),
            }
        } else {
            Err(Error::InvalidCommunity)
        }
    }

    async fn create_room(self, name: String, community: CommunityId) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_ROOMS) {
            return Err(Error::AccessDenied);
        }

        if !self.session.in_community(&community) {
            return Err(Error::InvalidCommunity);
        }

        let community_id = community;

        if let Some(community) = community::address_of(community) {
            let create = CreateRoom {
                creator: self.device,
                name: name.clone(),
            };
            let id = community
                .send(create)
                .await
                .map_err(handle_disconnected("Community"))??;

            let mut user = manager::get_active_user_mut(self.user).unwrap();

            if let Some(community) = user.communities.get_mut(&community_id) {
                let room = RoomStructure {
                    id,
                    name,
                    unread: true,
                };

                community.rooms.insert(
                    room.id,
                    UserRoom {
                        watch_level: WatchLevel::default(),
                        unread: true,
                    },
                );

                return Ok(OkResponse::AddRoom {
                    community: community_id,
                    room,
                });
            }
        }

        Err(Error::InvalidCommunity)
    }

    async fn create_invite(
        self,
        id: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    ) -> Result<OkResponse, Error> {
        if !self.perms.has_perms(TokenPermissionFlags::CREATE_INVITES) {
            return Err(Error::AccessDenied);
        }

        if !self.session.in_community(&id) {
            return Err(Error::InvalidCommunity);
        }

        if COMMUNITIES.contains_key(&id) {
            let db = &self.session.global.database;
            let max = self.session.global.config.max_invite_codes_per_community as i64;
            let res = db.create_invite_code(id, expiration_date, max).await?;

            match res {
                Ok(code) => Ok(OkResponse::NewInvite(code)),
                Err(_) => Err(Error::TooManyInviteCodes),
            }
        } else {
            Err(Error::InvalidCommunity)
        }
    }

    async fn get_room_update(
        self,
        community: CommunityId,
        room: RoomId,
        last_received: Option<MessageId>,
        message_count: u64,
    ) -> Result<OkResponse, Error> {
        if !self.session.in_room(&community, &room) {
            return Err(Error::InvalidRoom);
        }

        let db = &self.session.global.database;

        let newest_message = db.get_newest_message(community, room).await?;
        let last_read = db.get_last_read(self.user, room).await?;

        let selector = match (last_received, newest_message) {
            (Some(last_received), _) => {
                Some(MessageSelector::After(Bound::Exclusive(last_received)))
            }
            (_, Some(newest_message)) => {
                Some(MessageSelector::Before(Bound::Inclusive(newest_message)))
            }
            _ => None,
        };

        let new_messages = match selector {
            Some(selector) => {
                let messages = db
                    .get_messages(community, room, selector, message_count as usize)
                    .await?
                    .map_err(|_| Error::InvalidMessageSelector)?;
                messages.map_messages().try_collect().await?
            }
            None => Vec::new(),
        };

        let continuous = new_messages.len() < (message_count as usize);

        let new_messages = MessageHistory::from_newest_to_oldest(new_messages);

        Ok(OkResponse::RoomUpdate(RoomUpdate {
            last_read,
            continuous,
            new_messages,
        }))
    }

    async fn select_room(self, community: CommunityId, room: RoomId) -> Result<OkResponse, Error> {
        if !self.session.in_room(&community, &room) {
            return Err(Error::InvalidRoom);
        }

        self.set_looking_at(Some((community, room))).await;
        Ok(OkResponse::NoData)
    }

    async fn deselect_room(self) -> Result<OkResponse, Error> {
        self.set_looking_at(None).await;
        Ok(OkResponse::NoData)
    }

    async fn set_looking_at(self, looking_at: Option<(CommunityId, RoomId)>) {
        let mut active_user = manager::get_active_user_mut(self.user).unwrap();
        let session = active_user.sessions.get_mut(&self.device).unwrap();
        session.set_looking_at(looking_at).unwrap();
    }

    async fn get_messages(
        self,
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
        count: u64,
    ) -> Result<OkResponse, Error> {
        if !self.session.in_room(&community, &room) {
            return Err(Error::InvalidRoom);
        }

        let db = &self.session.global.database;
        let stream = db
            .get_messages(community, room, selector, count as usize)
            .await?
            .map_err(|_| Error::InvalidMessageSelector)?;

        let messages = stream.map_messages().try_collect().await?;
        Ok(OkResponse::MessageHistory(
            MessageHistory::from_newest_to_oldest(messages),
        ))
    }

    async fn set_as_read(self, community: CommunityId, room: RoomId) -> Result<OkResponse, Error> {
        let mut active_user = manager::get_active_user_mut(self.user).unwrap();
        if let Some(user_community) = active_user.communities.get_mut(&community) {
            if let Some(user_room) = user_community.rooms.get_mut(&room) {
                user_room.unread = false;
            } else {
                return Err(Error::InvalidRoom);
            }
        } else {
            return Err(Error::InvalidCommunity);
        }
        drop(active_user); // Drop lock

        let db = &self.session.global.database;
        let res = db.set_room_read(room, self.user).await?;

        match res {
            Ok(_) => Ok(OkResponse::NoData),
            Err(SetUserRoomStateError::InvalidRoom) => Err(Error::InvalidRoom),
            Err(SetUserRoomStateError::InvalidUser) => {
                self.ctx.stop(); // The user did not exist at the time of request
                Err(Error::UserDeleted)
            }
        }
    }

    async fn change_community_name(
        self,
        new: String,
        id: CommunityId,
    ) -> Result<OkResponse, Error> {
        if !self.session.in_community(&id) {
            return Err(Error::InvalidCommunity);
        }

        if let Some(mut community) = COMMUNITIES.get_mut(&id) {
            community.name = new.clone();
            drop(community); // Drop lock
            let db = &self.session.global.database;
            db.change_community_name(id, new).await?;
            Ok(OkResponse::NoData)
        } else {
            Err(Error::InvalidCommunity)
        }
    }

    async fn change_community_description(
        self,
        new: String,
        id: CommunityId,
    ) -> Result<OkResponse, Error> {
        if !self.session.in_community(&id) {
            return Err(Error::InvalidCommunity);
        }

        if let Some(mut community) = COMMUNITIES.get_mut(&id) {
            community.description = Some(new.clone());
            drop(community); // Drop lock
            let db = &self.session.global.database;
            db.change_community_description(id, new).await?;
            Ok(OkResponse::NoData)
        } else {
            Err(Error::InvalidCommunity)
        }
    }
}
