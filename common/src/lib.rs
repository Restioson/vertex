//! Some definitions common between server and client
#[cfg(feature = "enable-actix")]
use actix::prelude::*;
use bytes::Bytes;
use std::time::Duration;
use uuid::Uuid;

#[macro_use]
extern crate serde_derive;

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);

pub trait ClientMessageType {}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RequestId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

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
    Login(Login),
    CreateUser { name: String, password: String },
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    CreateRoom,
    JoinRoom(RoomId),
    Delete(Delete),
    ChangePassword { new_password: String },
    ChangeUsername { new_username: String },
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
    pub content: String,
}

impl ForwardedMessage {
    pub fn from_message_and_author(msg: ClientSentMessage, author: UserId) -> Self {
        ForwardedMessage {
            room: msg.to_room,
            author,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub name: String,
    pub password: String,
}

impl ClientMessageType for Login {}

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
    IdNotFound,
    Internal,
    InvalidUrl,
    WsConnectionError,
    NotLoggedIn,
    UsernameAlreadyExists,
    UserDoesNotExist,
    IncorrectPassword,
    InvalidPassword,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Success {
    NoData,
    Room { id: RoomId },
    MessageSent { id: MessageId },
    User { id: UserId },
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
