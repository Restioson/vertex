//! Some definitions common between server and client
#[cfg(feature = "enable-actix")]
use actix::prelude::*;
use bitflags::bitflags;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;

#[macro_use]
extern crate serde_derive;

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);

pub trait ClientMessageType {}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken(pub String);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RequestId(pub u64);

impl Into<Bytes> for ServerboundRequest {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ServerboundRequest {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerboundMessage {
    pub id: RequestId,
    pub request: ServerboundRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerboundRequest {
    Login {
        device_id: DeviceId,
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
        device_id: DeviceId,
        // Require re-authentication to revoke a token other than the current
        password: Option<String>,
    },
    RefreshToken {
        device_id: DeviceId,
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
    CreateRoom,
    JoinRoom(RoomId),
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
    pub to_room: RoomId,
    pub content: String,
}

impl ClientMessageType for ClientSentMessage {}

#[cfg(feature = "enable-actix")]
impl Message for ClientSentMessage {
    type Result = OkResponse;
}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardedMessage {
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
            room: msg.to_room,
            author,
            device,
            content: msg.content,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub message_id: MessageId,
    pub room_id: RoomId,
}

impl ClientMessageType for Edit {}

#[cfg(feature = "enable-actix")]
impl Message for Edit {
    type Result = OkResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message_id: MessageId,
    pub room_id: RoomId,
}

impl ClientMessageType for Delete {}

#[cfg(feature = "enable-actix")]
impl Message for Delete {
    type Result = OkResponse;
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
        /// Join rooms
        const JOIN_ROOMS = 1 << 8;
        /// Create rooms
        const CREATE_ROOMS = 1 << 9;
    }
}

impl TokenPermissionFlags {
    pub fn has_perms(&self, perms: TokenPermissionFlags) -> bool {
        self.contains(TokenPermissionFlags::ALL) || self.contains(perms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientboundMessage {
    Action(ClientboundAction),
    Response {
        id: RequestId,
        result: Result<OkResponse, ServerError>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientboundAction {
    Message(ForwardedMessage),
    EditMessage(Edit),
    DeleteMessage(Delete),
    SessionLoggedOut,
}

impl Into<Bytes> for ClientboundMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ClientboundMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum OkResponse {
    NoData,
    Room(RoomId),
    MessageSent(MessageId),
    User(UserId),
    Token {
        device_id: DeviceId,
        token: AuthToken,
    },
}

#[cfg(feature = "enable-actix")]
impl<A, M> actix::dev::MessageResponse<A, M> for OkResponse
    where
        A: actix::Actor,
        M: actix::Message<Result=Self>,
{
    fn handle<R: actix::dev::ResponseChannel<M>>(self, _ctx: &mut A::Context, tx: Option<R>) {
        if let Some(tx) = tx {
            tx.send(self);
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ServerError {
    InvalidMessage,
    UnexpectedTextFrame,
    Internal,
    NotLoggedIn,
    AlreadyLoggedIn,
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidDisplayName,
    InvalidToken,
    StaleToken,
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
    AlreadyInRoom,
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
