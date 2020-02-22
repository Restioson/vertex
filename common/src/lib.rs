//! Some definitions common between server and client
use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use bitflags::bitflags;
pub use request::*;

mod request;

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct CommunityId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ProfileVersion(pub u32);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken(pub String);

impl fmt::Display for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct InviteCode(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCredentials {
    pub username: String,
    pub password: String,
}

impl UserCredentials {
    pub fn new(username: String, password: String) -> UserCredentials {
        UserCredentials { username, password }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_community: CommunityId,
    pub to_room: RoomId,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageConfirmation {
    pub id: MessageId,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageHistory {
    pub messages: Vec<Message>,
}

impl MessageHistory {
    pub fn from_newest_to_oldest(messages: Vec<Message>) -> Self {
        let mut messages = messages;
        messages.reverse();

        MessageHistory { messages }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomUpdate {
    pub last_read: Option<MessageId>,
    pub new_messages: MessageHistory,
    pub continuous: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub author: UserId,
    pub author_profile_version: ProfileVersion,
    pub sent: DateTime<Utc>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub message: MessageId,
    pub community: CommunityId,
    pub room: RoomId,
    pub new_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message: MessageId,
    pub community: CommunityId,
    pub room: RoomId,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TokenCreationOptions {
    pub device_name: Option<String>,
    pub expiration_date: Option<DateTime<Utc>>,
    pub permission_flags: TokenPermissionFlags,
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
    fn default() -> Self { TokenPermissionFlags::ALL }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityStructure {
    pub id: CommunityId,
    pub name: String,
    pub rooms: Vec<RoomStructure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomStructure {
    pub id: RoomId,
    pub name: String,
    pub unread: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RemoveCommunityReason {
    /// The community was deleted
    Deleted,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserProfile {
    pub version: ProfileVersion,
    pub username: String,
    pub display_name: String,
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
