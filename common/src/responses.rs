use crate::proto;
use crate::proto::DeserializeError;
use crate::structures::*;
use crate::types::*;
use std::convert::{TryFrom, TryInto};

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

macro_rules! convert_to_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(ErrResponse::$variant => proto::responses::ErrResponse::$variant,)*
        }
    };
}

macro_rules! convert_from_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(proto::responses::ErrResponse::$variant => Ok(ErrResponse::$variant),)*
        }
    };
}

impl From<ErrResponse> for proto::responses::ErrResponse {
    fn from(err: ErrResponse) -> Self {
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

impl TryFrom<proto::responses::ErrResponse> for ErrResponse {
    type Error = DeserializeError;

    fn try_from(err: proto::responses::ErrResponse) -> Result<Self, Self::Error> {
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
