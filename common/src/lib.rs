//! Some type definitions common between server and client
use bytes::Bytes;

#[cfg(feature = "enable-actix")]
use actix::prelude::*;
use uuid::Uuid;

#[macro_use]
extern crate serde_derive;

pub trait ClientMessageType {}

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RequestId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Debug, Serialize, Deserialize)]
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

// TODO wrapper newtypes for uuid's e.g messageid, roomid
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    Login(Login),
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    CreateRoom,
    JoinRoom(RoomId),
    Delete(Delete),
}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_room: RoomId,
    pub content: String,
}

impl ClientMessageType for ClientSentMessage {}

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

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub message_id: MessageId,
    pub room_id: RoomId,
}

impl ClientMessageType for Edit {}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message_id: MessageId,
    pub room_id: RoomId,
}

impl ClientMessageType for Delete {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub id: UserId,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestResponse {
    Success(Success),
    Error(ServerError),
}

impl RequestResponse {
    pub fn success() -> Self {
        RequestResponse::Success(Success::NoData)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerError {
    InvalidMessage,
    UnexpectedTextFrame,
    IdNotFound,
    Internal,
    InvalidUrl,
    WsConnectionError,
    NotLoggedIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Success {
    NoData,
    Room { id: RoomId },
    MessageSent { id: MessageId },
}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}
