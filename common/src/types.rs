use crate::proto;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;
use uuid::Uuid;

macro_rules! impl_protobuf_conversions {
    ($($name:ident $(,)?)*) => {
        $(
            impl From<$name> for proto::types::$name {
                fn from(id: $name) -> Self {
                    proto::types::$name {
                        bytes: id.0.as_bytes().to_vec(),
                    }
                }
            }

            impl TryFrom<proto::types::$name> for $name {
                type Error = proto::DeserializeError;

                fn try_from(id: proto::types::$name) -> Result<Self, Self::Error> {
                    Uuid::from_slice(&id.bytes).map($name).map_err(Into::into)
                }
            }
        )*
    }
}

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Default, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Default, Serialize, Deserialize)]
pub struct CommunityId(pub Uuid);

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Default, Serialize, Deserialize)]
pub struct RoomId(pub Uuid);

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Default, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[serde(transparent)]
#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

impl_protobuf_conversions! { DeviceId, MessageId, RoomId, CommunityId, UserId }

/// Does not need to be sequential; just unique within a desired time-span (or not, if you're a fan
/// of trying to handle two responses with the same id attached). This exists for the client-side
/// programmer's ease-of-use only - the server is request-id-agnostic.
#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone)]
pub struct RequestId(u32);

impl RequestId {
    pub const fn new(id: u32) -> Self {
        RequestId(id)
    }
}

impl From<RequestId> for proto::types::RequestId {
    fn from(id: RequestId) -> Self {
        proto::types::RequestId { value: id.0 }
    }
}

impl From<proto::types::RequestId> for RequestId {
    fn from(id: proto::types::RequestId) -> Self {
        RequestId(id.value)
    }
}

#[derive(Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Copy, Clone)]
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

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct InviteCode(pub String);
