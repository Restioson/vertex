//! Some type definitions common between server and client
use std::convert::TryFrom;
use bytes::Bytes;
use serde::{Serialize, Deserialize};
use actix::prelude::*;
use uuid::Uuid;

pub trait ClientMessageType {}

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    PublishInitKey(PublishInitKey),
    RequestInitKey(RequestInitKey),
    Login(Login),
    SendMessage(ClientSentMessage),
    CreateRoom,
    JoinRoom(Uuid),
}

#[derive(Debug, Message, Serialize, Deserialize)]
pub struct ClientSentMessage {
    pub to_room: Uuid,
    pub content: String,
}

impl ClientMessageType for ClientSentMessage {}

#[derive(Debug, Clone, Message, Serialize, Deserialize)]
pub struct ForwardedMessage {
    pub from_room: Uuid,
    pub content: String,
}

impl From<ClientSentMessage> for ForwardedMessage {
    fn from(msg: ClientSentMessage) -> ForwardedMessage {
        ForwardedMessage {
            from_room: msg.to_room,
            content: msg.content,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub id: Uuid,
}
impl ClientMessageType for Login {}

#[derive(Debug, Message, Serialize, Deserialize)]
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

impl Message for RequestInitKey {
    type Result = Option<InitKey>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    Success(Success),
    Error(Error),
    Message(ForwardedMessage),
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

#[derive(Debug, Serialize, Deserialize)]
pub enum Error {
    InvalidMessage,
    InvalidInitKey,
    UnexpectedTextFrame,
    IdNotFound,
    Internal,
    InvalidUrl,
    WsConnectionError,
    NotLoggedIn,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Success {
    NoData,
    Key(InitKey),
    Room { id: Uuid, },
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