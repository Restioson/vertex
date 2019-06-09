//! Some type definitions common between server and client
use std::convert::TryFrom;
use bytes::Bytes;

#[cfg(feature = "enable-actix")]
use actix::prelude::*;
use uuid::Uuid;

#[macro_use]
extern crate serde_derive;

pub trait ClientMessageType {}

// TODO wrapper newtypes for uuid's e.g messageid, roomid
#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    PublishInitKey(PublishInitKey),
    RequestInitKey(RequestInitKey),
    Login(Login),
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    CreateRoom,
    JoinRoom(Uuid),
    Delete(Delete),
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

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_room: Uuid,
    pub content: String,
}

impl ClientMessageType for ClientSentMessage {}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardedMessage {
    pub room: Uuid,
    pub author: Uuid,
    pub content: String,
}

impl ForwardedMessage {
    pub fn from_message_and_author(msg: ClientSentMessage, author: Uuid) -> Self {
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
    pub message_id: Uuid,
    pub room_id: Uuid,
}

impl ClientMessageType for Edit {}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delete {
    pub message_id: Uuid,
    pub room_id: Uuid,
}

impl ClientMessageType for Delete {}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub id: Uuid,
}

impl ClientMessageType for Login {}

#[cfg_attr(feature = "enable-actix", derive(Message))]
#[derive(Debug, Serialize, Deserialize)]
pub struct PublishInitKey {
    pub id: Uuid,
    pub key: InitKey,
}

impl ClientMessageType for PublishInitKey {}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestInitKey {
    pub id: Uuid,
}
impl ClientMessageType for RequestInitKey {}

#[cfg(feature = "enable-actix")]
impl Message for RequestInitKey {
    type Result = Option<InitKey>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Success(Success),
    Error(ServerError),
    Message(ForwardedMessage),
    Edit(Edit),
    Delete(Delete),
}

impl ServerMessage {
    pub fn success() -> Self {
        ServerMessage::Success(Success::NoData)
    }
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
    InvalidInitKey,
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
    Key(InitKey),
    Room { id: Uuid, },
    MessageSent { id: Uuid },
}

/// Dummy type for init key
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitKey {
    bin: Bytes,
}

impl InitKey {
    pub fn bytes(&self) -> Bytes {
        self.bin.clone()
    }
}

impl TryFrom<Bytes> for InitKey {
    type Error = InvalidInitKey;

    fn try_from(bin: Bytes) -> Result<InitKey, InvalidInitKey> {
        Ok(InitKey { bin })
    }
}

#[derive(Debug)]
pub enum InvalidInitKey {}

#[macro_export]
macro_rules! catch {
    { $($tt:tt)* } => {
        (||{ $($tt)* })()
    }
}