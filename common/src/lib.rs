//! Some type definitions common between server and client
use std::convert::TryFrom;
use bytes::Bytes;
use serde::{Serialize, Deserialize};
use actix::prelude::*;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub enum ClientMessage {
    PublishInitKey(PublishInitKey),
    RequestInitKey(RequestInitKey),
    Federate(Federate),
    Login(Login),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Login {
    pub uuid: Uuid,
}

#[derive(Debug, Message, Serialize, Deserialize)]
pub struct Federate {
    pub url: String,
}

#[derive(Debug, Message, Serialize, Deserialize)]
pub struct PublishInitKey {
    pub id: Uuid,
    pub key: InitKey,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestInitKey {
    pub id: Uuid,
}

impl Message for RequestInitKey {
    type Result = Option<InitKey>;
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    Success(Success),
    Error(Error),
}

impl ServerMessage {
    pub fn success() -> Self {
        ServerMessage::Success(Success::NoData)
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