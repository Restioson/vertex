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
pub struct RequestId(pub Uuid);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRequest {
    pub message: ClientMessage,
    pub request_id: RequestId,
}

impl ClientRequest {
    pub fn new(message: ClientMessage) -> Self {
        ClientRequest {
            message,
            request_id: RequestId(Uuid::new_v4()),
        }
    }
}

impl Into<Bytes> for ClientRequest {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ClientRequest {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Login {
        device_id: DeviceId,
        token: AuthToken,
    },
    CreateToken {
        username: String,
        password: String,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
    },
    RevokeToken {
        device_id: DeviceId,
        // Require re-authentication to revoke a token other than the current
        password: Option<String>,
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
    type Result = RequestResponse;
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
    type Result = RequestResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message_id: MessageId,
    pub room_id: RoomId,
}

impl ClientMessageType for Delete {}

#[cfg(feature = "enable-actix")]
impl Message for Delete {
    type Result = RequestResponse;
}

bitflags! {
    #[derive(Serialize, Deserialize)]
    pub struct TokenPermissionFlags: i64 {
        /// Send messages
        const SEND_MESSAGES = 1 << 0;
        /// Edit any messages sent by this user
        const EDIT_ANY_MESSAGES = 1 << 1;
        /// Edit only messages sent by this device/from this token
        const EDIT_OWN_MESSAGES = 1 << 2;
        /// Delete any messages sent by this user
        const DELETE_ANY_MESSAGES = 1 << 3;
        /// Edit only messages sent by this device/from this token
        const DELETE_OWN_MESSAGES = 1 << 4;
        /// Change the user's name
        const CHANGE_USERNAME = 1 << 5;
        /// Join rooms
        const JOIN_ROOM = 1 << 6;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Response {
        response: RequestResponse,
        request_id: RequestId,
    },
    Error(ServerError),
    Message(ForwardedMessage),
    Edit(Edit),
    Delete(Delete),
    SessionLoggedOut,
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
pub enum RequestResponse {
    Success(Success),
    Error(ServerError),
}

impl RequestResponse {
    pub fn success() -> Self {
        RequestResponse::Success(Success::NoData)
    }
    pub fn room(id: RoomId) -> Self {
        RequestResponse::Success(Success::Room { id })
    }
    pub fn user(id: UserId) -> Self {
        RequestResponse::Success(Success::User { id })
    }
    pub fn token(device_id: DeviceId, token: AuthToken) -> Self {
        RequestResponse::Success(Success::Token { device_id, token })
    }
}

#[cfg(feature = "enable-actix")]
impl<A, M> actix::dev::MessageResponse<A, M> for RequestResponse
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Success {
    NoData,
    Room {
        id: RoomId,
    },
    MessageSent {
        id: MessageId,
    },
    User {
        id: UserId,
    },
    Token {
        device_id: DeviceId,
        token: AuthToken,
    },
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
