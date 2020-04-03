use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::time::Duration;

use crate::proto;
use crate::proto::DeserializeError;
use crate::structures::*;
use crate::types::*;

pub type ResponseResult = Result<OkResponse, ErrResponse>;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum OkResponse {
    NoData,
    AddCommunity(CommunityStructure),
    AddRoom {
        community: CommunityId,
        room: RoomStructure,
    },
    ConfirmMessage(MessageConfirmation),
    UserId(UserId),
    Profile(Profile),
    NewInvite(InviteCode),
    RoomUpdate(RoomUpdate),
    MessageHistory(MessageHistory),
}

impl From<OkResponse> for proto::responses::Ok {
    fn from(ok: OkResponse) -> Self {
        use proto::responses::ok::Response;
        use proto::responses::{self, *};
        use OkResponse::*;

        let inner = match ok {
            NoData => Response::NoData(proto::types::None {}),
            AddCommunity(community) => Response::AddCommunity(community.into()),
            AddRoom { community, room } => Response::AddRoom(NewRoom {
                community: Some(community.into()),
                structure: Some(room.into()),
            }),
            ConfirmMessage(confirmation) => Response::ConfirmMessage(confirmation.into()),
            UserId(id) => Response::UserId(id.into()),
            Profile(profile) => Response::Profile(profile.into()),
            OkResponse::NewInvite(code) => {
                Response::NewInvite(responses::NewInvite { code: code.0 })
            }
            RoomUpdate(update) => Response::RoomUpdate(update.into()),
            MessageHistory(history) => Response::MessageHistory(history.into()),
        };

        proto::responses::Ok {
            response: Some(inner),
        }
    }
}

impl TryFrom<proto::responses::Ok> for OkResponse {
    type Error = DeserializeError;

    fn try_from(ok: proto::responses::Ok) -> Result<Self, Self::Error> {
        use proto::responses::ok::Response::*;

        Ok(match ok.response? {
            NoData(_) => OkResponse::NoData,
            AddCommunity(community) => OkResponse::AddCommunity(community.try_into()?),
            AddRoom(new_room) => OkResponse::AddRoom {
                community: new_room.community?.try_into()?,
                room: new_room.structure?.try_into()?,
            },
            ConfirmMessage(confirmation) => OkResponse::ConfirmMessage(confirmation.try_into()?),
            UserId(id) => OkResponse::UserId(id.try_into()?),
            Profile(profile) => OkResponse::Profile(profile.try_into()?),
            NewInvite(new_invite) => OkResponse::NewInvite(InviteCode(new_invite.code)),
            RoomUpdate(update) => OkResponse::RoomUpdate(update.try_into()?),
            MessageHistory(history) => OkResponse::MessageHistory(history.try_into()?),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrResponse {
    Error(Error),
    RateLimited { ready_in: Duration },
}

impl fmt::Display for ErrResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ErrResponse::*;
        match self {
            Error(err) => write!(f, "{}", err),
            RateLimited { ready_in } => write!(f, "Rate limited! Ready in: {}s", ready_in.as_secs()),
        }
    }
}

impl From<ErrResponse> for proto::responses::ErrResponse {
    fn from(resp: ErrResponse) -> Self {
        use proto::responses::err_response::Inner;
        use proto::responses::RateLimited;

        let inner = match resp {
            ErrResponse::Error(err) => {
                let proto_err: proto::responses::Error = err.into();
                let discrim: i32 = proto_err.into();
                Inner::Error(discrim)
            },
            ErrResponse::RateLimited { ready_in } => {
                Inner::RateLimited(RateLimited {
                    ready_in_ms: ready_in.as_millis().try_into().unwrap_or(std::u32::MAX)
                })
            }
        };

        proto::responses::ErrResponse { inner: Some(inner) }
    }
}

impl TryFrom<proto::responses::ErrResponse> for ErrResponse {
    type Error = DeserializeError;

    fn try_from(resp: proto::responses::ErrResponse) -> Result<Self, DeserializeError> {
        use proto::responses::err_response::Inner;
        use proto::responses::RateLimited;

        let resp = match resp.inner? {
            Inner::Error(err) => {
                let err = proto::responses::Error::from_i32(err)
                    .ok_or(DeserializeError::InvalidEnumVariant)?;
                ErrResponse::Error(err.try_into()?)
            },
            Inner::RateLimited(RateLimited { ready_in_ms }) => {
                ErrResponse::RateLimited {
                    ready_in: Duration::from_millis(ready_in_ms as u64)
                }
            }
        };

        Ok(resp)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    Internal,
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidPassword,
    InvalidDisplayName,
    /// Returned when the user that is sending a message is deleted while processing the message
    UserDeleted,
    DeviceDoesNotExist,
    IncorrectUsernameOrPassword,
    /// User is not able to perform said action with current authentication token, or request to
    /// revoke authentication token requires re-entry of password.
    AccessDenied,
    InvalidRoom,
    InvalidCommunity,
    InvalidInviteCode,
    InvalidUser,
    AlreadyInCommunity,
    TooManyInviteCodes,
    InvalidMessageSelector,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            Internal => write!(f, "Internal server error"),
            UsernameAlreadyExists => write!(f, "Username already exists"),
            InvalidUsername => write!(f, "Invalid username"),
            InvalidPassword => write!(f, "Invalid password"),
            InvalidDisplayName => write!(f, "Invalid display name"),
            UserDeleted => write!(f, "User deleted"),
            DeviceDoesNotExist => write!(f, "Device does not exist"),
            IncorrectUsernameOrPassword => write!(f, "Incorrect username or password"),
            AccessDenied => write!(f, "Access denied"),
            InvalidRoom => write!(f, "Invalid room"),
            InvalidCommunity => write!(f, "Invalid community"),
            InvalidInviteCode => write!(f, "Invalid invite code"),
            InvalidUser => write!(f, "Invalid user"),
            AlreadyInCommunity => write!(f, "Already in community"),
            TooManyInviteCodes => write!(f, "Too many invite codes"),
            InvalidMessageSelector => write!(f, "Invalid message selector"),
        }
    }
}

macro_rules! convert_to_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(Error::$variant => proto::responses::Error::$variant,)*
        }
    };
}

macro_rules! convert_from_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(proto::responses::Error::$variant => Ok(Error::$variant),)*
        }
    };
}

impl From<Error> for proto::responses::Error {
    fn from(err: Error) -> Self {
        convert_to_proto! {
            err: {
                Internal,
                UsernameAlreadyExists,
                InvalidUsername,
                InvalidPassword,
                InvalidDisplayName,
                UserDeleted,
                DeviceDoesNotExist,
                IncorrectUsernameOrPassword,
                AccessDenied,
                InvalidRoom,
                InvalidCommunity,
                InvalidInviteCode,
                InvalidUser,
                AlreadyInCommunity,
                TooManyInviteCodes,
                InvalidMessageSelector,
            }
        }
    }
}

impl TryFrom<proto::responses::Error> for Error {
    type Error = DeserializeError;

    fn try_from(err: proto::responses::Error) -> Result<Self, Self::Error> {
        convert_from_proto! {
            err: {
                Internal,
                UsernameAlreadyExists,
                InvalidUsername,
                InvalidPassword,
                InvalidDisplayName,
                UserDeleted,
                DeviceDoesNotExist,
                IncorrectUsernameOrPassword,
                AccessDenied,
                InvalidRoom,
                InvalidCommunity,
                InvalidInviteCode,
                InvalidUser,
                AlreadyInCommunity,
                TooManyInviteCodes,
                InvalidMessageSelector,
            }
        }
    }
}
