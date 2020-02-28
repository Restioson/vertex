use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use bitflags::bitflags;
use crate::structures::*;
use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: RequestId,
    pub request: ClientRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_community: CommunityId,
    pub to_room: RoomId,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClientRequest {
    LogOut,
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    GetRoomUpdate {
        community: CommunityId,
        room: RoomId,
        last_received: Option<MessageId>,
        message_count: usize,
    },
    GetMessages {
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
        count: usize,
    },
    SelectRoom {
        community: CommunityId,
        room: RoomId,
    },
    DeselectRoom,
    SetAsRead {
        community: CommunityId,
        room: RoomId,
    },
    CreateCommunity {
        name: String,
    },
    CreateRoom {
        name: String,
        community: CommunityId,
    },
    CreateInvite {
        community: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    },
    JoinCommunity(InviteCode),
    Delete(Delete),
    ChangeUsername {
        new_username: String,
    },
    ChangeDisplayName {
        new_display_name: String,
    },
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    GetUserProfile(UserId),
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TokenCreationOptions {
    pub device_name: Option<String>,
    pub expiration_date: Option<DateTime<Utc>>,
    pub permission_flags: TokenPermissionFlags,
}

impl ClientMessage {
    pub fn new(request: ClientRequest, id: RequestId) -> Self {
        ClientMessage { request, id }
    }
}

impl Into<Vec<u8>> for ClientMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TokenPermissionFlags: i64 {
        /// All permissions. Should be used for user devices but not for service logins.
        const ALL = 1;
        /// Send messages
        const SEND_MESSAGES = 1 << 1;
        /// Edit any messages sent by this user
        const EDIT_ANY_MESSAGES = 1 << 2;
        /// Edit only messages sent by this device/from this token
        const EDIT_OWN_MESSAGES = 1 << 3;
        /// Delete any messages sent by this user
        const DELETE_ANY_MESSAGES = 1 << 4;
        /// Edit only messages sent by this device/from this token
        const DELETE_OWN_MESSAGES = 1 << 5;
        /// Change the user's name
        const CHANGE_USERNAME = 1 << 6;
        /// Change the user's display name
        const CHANGE_DISPLAY_NAME = 1 << 7;
        /// Join communities
        const JOIN_COMMUNITIES = 1 << 8;
        /// Create communities
        const CREATE_COMMUNITIES = 1 << 9;
        /// Create rooms
        const CREATE_ROOMS = 1 << 10;
        /// Create invites to communities
        const CREATE_INVITES = 1 << 11;
    }
}

impl TokenPermissionFlags {
    pub fn has_perms(self, perms: TokenPermissionFlags) -> bool {
        self.contains(TokenPermissionFlags::ALL) || self.contains(perms)
    }
}

impl Default for TokenPermissionFlags {
    fn default() -> Self {
        TokenPermissionFlags::ALL
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Bound<T> {
    Inclusive(T),
    Exclusive(T),
}

impl<T> Bound<T> {
    #[inline]
    pub fn get(&self) -> &T {
        match self {
            Bound::Inclusive(bound) => bound,
            Bound::Exclusive(bound) => bound,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum MessageSelector {
    Before(Bound<MessageId>),
    After(Bound<MessageId>),
}
