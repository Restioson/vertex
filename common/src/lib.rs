//! Some definitions common between server and client
use bitflags::bitflags;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;
use serde::{Serialize, Deserialize};

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

/// Does not need to be sequential; just unique within a desired time-span (or not, if you're a fan
/// of trying to handle two responses with the same id attached). This exists for the client-side
/// programmer's ease-of-use only - the server is request-id-agnostic.
#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RequestId(u32);

impl RequestId {
    pub const fn new(id: u32) -> Self { RequestId(id) }
}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct CommunityId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: RequestId,
    pub request: ClientRequest,
}

impl ClientMessage {
    pub fn new(request: ClientRequest, id: RequestId) -> Self {
        ClientMessage { request, id }
    }
}

impl Into<Bytes> for ClientMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ClientMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientRequest {
    Login {
        device: DeviceId,
        token: AuthToken,
    },
    CreateToken {
        username: String,
        password: String,
        device_name: Option<String>,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
    },
    RevokeToken {
        device: DeviceId,
        // Require re-authentication to revoke a token other than the current
        password: Option<String>,
    },
    RefreshToken {
        device: DeviceId,
        username: String,
        password: String,
    },
    CreateUser {
        username: String,
        display_name: String,
        password: String,
    },
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    CreateCommunity {
        name: String,
    },
    CreateRoom {
        name: String,
        community: CommunityId,
    },
    JoinCommunity(CommunityId),
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_community: CommunityId,
    pub to_room: RoomId,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardedMessage {
    pub community: CommunityId,
    pub room: RoomId,
    pub author: UserId,
    pub device: DeviceId,
    pub content: String,
}

impl ForwardedMessage {
    pub fn from_message_author_device(
        msg: ClientSentMessage,
        author: UserId,
        device: DeviceId,
    ) -> Self {
        ForwardedMessage {
            community: msg.to_community,
            room: msg.to_room,
            author,
            device,
            content: msg.content,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message: MessageId,
    pub community: CommunityId,
    pub room: RoomId,
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TokenPermissionFlags: i64 {
        /// All permissions. Should be used for user devices but not for service logins.
        const ALL = 1 << 0;
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
    }
}

impl TokenPermissionFlags {
    pub fn has_perms(&self, perms: TokenPermissionFlags) -> bool {
        self.contains(TokenPermissionFlags::ALL) || self.contains(perms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Action(ServerAction),
    Response {
        id: RequestId,
        result: Result<OkResponse, ErrResponse>,
    },
    MalformedMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerAction {
    Message(ForwardedMessage),
    Edit(Edit),
    Delete(Delete),
    SessionLoggedOut,
    LeftCommunity(LeftCommunityReason),
}

impl Into<Bytes> for ServerMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ServerMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum LeftCommunityReason {
    /// The community was deleted
    Deleted,
}

#[cfg(feature = "enable-actix")]
impl<A, M> actix::dev::MessageResponse<A, M> for OkResponse
where
    A: actix::Actor,
    M: actix::Message<Result = Self>,
{
    fn handle<R: actix::dev::ResponseChannel<M>>(self, _ctx: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum OkResponse {
    NoData,
    Room { id: RoomId },
    Community { id: CommunityId },
    MessageId { id: MessageId },
    User { id: UserId },
    Token { device: DeviceId, token: AuthToken },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ErrResponse {
    Internal,
    NotLoggedIn,
    AlreadyLoggedIn,
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidDisplayName,
    InvalidToken,
    StaleToken,
    TokenInUse,
    UserCompromised,
    UserLocked,
    UserBanned,
    DeviceDoesNotExist,
    InvalidPassword,
    IncorrectUsernameOrPassword,
    /// User is not able to perform said action with current authentication token, or request to
    /// revoke authentication token requires re-entry of password.
    AccessDenied,
    InvalidRoom,
    InvalidCommunity,
    InvalidUser,
    AlreadyInCommunity,
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
