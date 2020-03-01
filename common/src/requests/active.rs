use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
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
    GetProfile(UserId),
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
