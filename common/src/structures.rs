use crate::proto::{self, DeserializeError};
use crate::types::*;
use chrono::{DateTime, Utc, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityStructure {
    pub id: CommunityId,
    pub name: String,
    pub rooms: Vec<RoomStructure>,
}

impl From<CommunityStructure> for proto::structures::CommunityStructure {
    fn from(community: CommunityStructure) -> Self {
        proto::structures::CommunityStructure {
            id: Some(community.id.into()),
            name: community.name,
            rooms: community.rooms.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<proto::structures::CommunityStructure> for CommunityStructure {
    type Error = DeserializeError;

    fn try_from(community: proto::structures::CommunityStructure) -> Result<Self, Self::Error> {
        let rooms = community
            .rooms
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<RoomStructure>, DeserializeError>>()?;

        Ok(CommunityStructure {
            id: community.id?.try_into()?,
            name: community.name,
            rooms,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomStructure {
    pub id: RoomId,
    pub name: String,
    pub unread: bool,
}

impl From<RoomStructure> for proto::structures::RoomStructure {
    fn from(room: RoomStructure) -> Self {
        proto::structures::RoomStructure {
            id: Some(room.id.into()),
            name: room.name,
            unread: room.unread,
        }
    }
}

impl TryFrom<proto::structures::RoomStructure> for RoomStructure {
    type Error = DeserializeError;

    fn try_from(room: proto::structures::RoomStructure) -> Result<Self, Self::Error> {
        Ok(RoomStructure {
            id: room.id?.try_into()?,
            name: room.name,
            unread: room.unread,
        })
    }
}

impl TryFrom<Option<proto::structures::RoomStructure>> for RoomStructure {
    type Error = DeserializeError;

    fn try_from(room: Option<proto::structures::RoomStructure>) -> Result<Self, Self::Error> {
        if let Some(room) = room {
            room.try_into()
        } else {
            Err(DeserializeError::NullField)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageConfirmation {
    pub id: MessageId,
    pub time_sent: DateTime<Utc>,
}

impl From<MessageConfirmation> for proto::structures::MessageConfirmation {
    fn from(confirmation: MessageConfirmation) -> Self {
        proto::structures::MessageConfirmation {
            id: Some(confirmation.id.into()),
            time_sent: confirmation.time_sent.timestamp(),
        }
    }
}

impl TryFrom<proto::structures::MessageConfirmation> for MessageConfirmation {
    type Error = DeserializeError;

    fn try_from(confirmation: proto::structures::MessageConfirmation) -> Result<Self, Self::Error> {
        let dt = &NaiveDateTime::from_timestamp(confirmation.time_sent, 0);
        Ok(MessageConfirmation {
            id: confirmation.id?.try_into()?,
            time_sent: Utc.from_utc_datetime(dt),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHistory {
    pub buffer: Vec<Message>,
}

impl MessageHistory {
    pub fn from_newest_to_oldest(messages: Vec<Message>) -> Self {
        let mut messages = messages;
        messages.reverse();

        MessageHistory { buffer: messages }
    }
}

impl From<MessageHistory> for proto::structures::MessageHistory {
    fn from(history: MessageHistory) -> Self {
        proto::structures::MessageHistory {
            messages: history.buffer.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<proto::structures::MessageHistory> for MessageHistory {
    type Error = DeserializeError;

    fn try_from(history: proto::structures::MessageHistory) -> Result<Self, Self::Error> {
        let buffer = history
            .messages
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Message>, DeserializeError>>()?;

        Ok(MessageHistory { buffer })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomUpdate {
    pub last_read: Option<MessageId>,
    pub new_messages: MessageHistory,
    /// Whether the history returned is continuous with the last message read
    pub continuous: bool,
}

impl From<RoomUpdate> for proto::structures::RoomUpdate {
    fn from(update: RoomUpdate) -> Self {
        proto::structures::RoomUpdate {
            last_read: update.last_read.map(Into::into),
            new_messages: Some(update.new_messages.into()),
            continuous: update.continuous,
        }
    }
}

impl TryFrom<proto::structures::RoomUpdate> for RoomUpdate {
    type Error = DeserializeError;

    fn try_from(update: proto::structures::RoomUpdate) -> Result<Self, Self::Error> {
        Ok(RoomUpdate {
            last_read: if let Some(last_read) = update.last_read {
                Some(last_read.try_into()?)
            } else {
                None
            },
            new_messages: if let Some(new_messages) = update.new_messages {
                new_messages.try_into()?
            } else {
                MessageHistory { buffer: vec![] }
            },
            continuous: update.continuous,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub author: UserId,
    pub author_profile_version: ProfileVersion,
    pub time_sent: DateTime<Utc>,
    pub content: Option<String>,
}

impl From<Message> for proto::structures::Message {
    fn from(msg: Message) -> Self {
        use proto::structures::message::Content;

        proto::structures::Message {
            id: Some(msg.id.into()),
            author: Some(msg.author.into()),
            author_profile_version: msg.author_profile_version.0 as u32,
            time_sent: msg.time_sent.timestamp(),
            content: msg.content.map(Content::Present),
        }
    }
}

impl TryFrom<proto::structures::Message> for Message {
    type Error = DeserializeError;

    fn try_from(message: proto::structures::Message) -> Result<Self, Self::Error> {
        use proto::structures::message::Content;
        let dt = &NaiveDateTime::from_timestamp(message.time_sent, 0);

        Ok(Message {
            id: message.id.try_into()?,
            author: message.author.try_into()?,
            author_profile_version: ProfileVersion(message.author_profile_version),
            time_sent: Utc.from_utc_datetime(&dt),
            content: message.content.map(|c| {
                let Content::Present(content) = c;
                content
            }),
        })
    }
}

impl TryFrom<Option<proto::structures::Message>> for Message {
    type Error = DeserializeError;

    fn try_from(message: Option<proto::structures::Message>) -> Result<Self, Self::Error> {
        if let Some(message) = message {
            message.try_into()
        } else {
            Err(DeserializeError::NullField)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub message: MessageId,
    pub community: CommunityId,
    pub room: RoomId,
    pub new_content: String,
}

impl From<Edit> for proto::structures::Edit {
    fn from(edit: Edit) -> Self {
        proto::structures::Edit {
            message: Some(edit.message.into()),
            community: Some(edit.community.into()),
            room: Some(edit.room.into()),
            new_content: edit.new_content,
        }
    }
}

impl TryFrom<proto::structures::Edit> for Edit {
    type Error = DeserializeError;

    fn try_from(edit: proto::structures::Edit) -> Result<Self, Self::Error> {
        Ok(Edit {
            message: edit.message.try_into()?,
            community: edit.community.try_into()?,
            room: edit.room.try_into()?,
            new_content: edit.new_content,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message: MessageId,
    pub community: CommunityId,
    pub room: RoomId,
}

impl From<Delete> for proto::structures::Delete {
    fn from(delete: Delete) -> Self {
        proto::structures::Delete {
            message: Some(delete.message.into()),
            community: Some(delete.community.into()),
            room: Some(delete.room.into()),
        }
    }
}

impl TryFrom<proto::structures::Delete> for Delete {
    type Error = DeserializeError;

    fn try_from(delete: proto::structures::Delete) -> Result<Self, Self::Error> {
        Ok(Delete {
            message: delete.message.try_into()?,
            community: delete.community.try_into()?,
            room: delete.room.try_into()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientReady {
    pub user: UserId,
    pub profile: UserProfile,
    pub communities: Vec<CommunityStructure>,
}

impl From<ClientReady> for proto::structures::ClientReady {
    fn from(ready: ClientReady) -> Self {
        proto::structures::ClientReady {
            user: Some(ready.user.into()),
            profile: Some(ready.profile.into()),
            communities: ready.communities.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<proto::structures::ClientReady> for ClientReady {
    type Error = DeserializeError;

    fn try_from(ready: proto::structures::ClientReady) -> Result<Self, Self::Error> {
        let communities = ready
            .communities
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<CommunityStructure>, DeserializeError>>()?;

        Ok(ClientReady {
            user: ready.user.try_into()?,
            profile: ready.profile.try_into()?,
            communities,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserProfile {
    pub version: ProfileVersion,
    pub username: String,
    pub display_name: String,
}

impl From<UserProfile> for proto::structures::UserProfile {
    fn from(profile: UserProfile) -> Self {
        proto::structures::UserProfile {
            version: profile.version.0,
            username: profile.username,
            display_name: profile.display_name,
        }
    }
}

impl TryFrom<proto::structures::UserProfile> for UserProfile {
    type Error = DeserializeError;

    fn try_from(profile: proto::structures::UserProfile) -> Result<Self, Self::Error> {
        Ok(UserProfile {
            version: ProfileVersion(profile.version),
            username: profile.username,
            display_name: profile.display_name,
        })
    }
}

impl TryFrom<Option<proto::structures::UserProfile>> for UserProfile {
    type Error = DeserializeError;

    fn try_from(profile: Option<proto::structures::UserProfile>) -> Result<Self, Self::Error> {
        if let Some(profile) = profile {
            profile.try_into()
        } else {
            Err(DeserializeError::NullField)
        }
    }
}
