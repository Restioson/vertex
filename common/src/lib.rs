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

#[derive(Debug, Serialize, Deserialize)]
pub struct ClientMessage {
    pub request: ClientRequest,
    pub id: RequestId,
}

impl From<ClientRequest> for ClientMessage {
    #[inline]
    fn from(req: ClientRequest) -> Self {
        ClientMessage::new(req)
    }
}

impl ClientMessage {
    pub fn new(request: ClientRequest) -> Self {
        ClientMessage {
            request,
            id: RequestId(Uuid::new_v4()),
        }
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

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientRequest {
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

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Response {
        response: Result<RequestResponse, ServerError>,
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
pub enum RequestResponse {
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
