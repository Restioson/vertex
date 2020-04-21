use std::convert::{TryFrom, TryInto};
use std::fmt;

use crate::proto;
use crate::proto::DeserializeError;
use crate::structures::*;
use crate::types::*;
use crate::requests::AdminResponse;

pub type ResponseResult = Result<OkResponse, Error>;

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
    Admin(AdminResponse),
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
            Admin(admin) => Response::Admin(admin.into()),
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
            Admin(admin) => OkResponse::Admin(admin.try_into()?),
        })
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
    /// Returned when the user that is sending a message is deleted or logged out while processing
    /// the message
    LoggedOut,
    DeviceDoesNotExist,
    IncorrectUsernameOrPassword,
    /// User is not able to perform said action with current authentication token, or request to
    /// revoke authentication token requires re-entry of password.
    AccessDenied,
    InvalidRoom,
    InvalidCommunity,
    InvalidInviteCode,
    InvalidUser,
    /// The given string field value was too long.
    TooLong,
    AlreadyInCommunity,
    TooManyInviteCodes,
    InvalidMessageSelector,
    MessageTooLong,
    Unimplemented,
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
            LoggedOut => write!(f, "User deleted"),
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
            MessageTooLong => write!(f, "Message too long"),
            TooLong => write!(f, "Text field too long"),
            Unimplemented => write!(f, "Unimplemented API"),
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
                LoggedOut,
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
                MessageTooLong,
                Unimplemented,
                TooLong,
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
                LoggedOut,
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
                MessageTooLong,
                Unimplemented,
                TooLong,
            }
        }
    }
}
