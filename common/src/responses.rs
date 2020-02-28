use crate::proto;
use crate::structures::*;
use crate::types::*;
use serde::{Deserialize, Serialize};
use crate::proto::DeserializeError;
use std::convert::{TryFrom, TryInto};

pub type ResponseResult = Result<OkResponse, ErrResponse>;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Profile(UserProfile),
    NewToken {
        device: DeviceId,
        token: AuthToken,
    },
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
            OkResponse::NewToken { device, token } => Response::NewToken(responses::NewToken {
                device_id: Some(device.into()),
                token_string: token.0,
            }),
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
            AddRoom(new_room) => {
                OkResponse::AddRoom {
                    community: new_room.community.try_into()?,
                    room: new_room.structure.try_into()?,
                }
            },
            ConfirmMessage(confirmation) => OkResponse::ConfirmMessage(confirmation.try_into()?),
            UserId(id) => OkResponse::UserId(id.try_into()?),
            Profile(profile) => OkResponse::Profile(profile.try_into()?),
            NewToken(new_token) => {
                OkResponse::NewToken {
                    device: new_token.device_id.try_into()?,
                    token: AuthToken(new_token.token_string),
                }
            },
            NewInvite(new_invite) => OkResponse::NewInvite(InviteCode(new_invite.code)),
            RoomUpdate(update) => OkResponse::RoomUpdate(update.try_into()?),
            MessageHistory(history) => OkResponse::MessageHistory(history.try_into()?),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
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
            $(ErrResponse::$variant => proto::responses::Error::$variant,)*
        }
    };
}

macro_rules! convert_from_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(proto::responses::Error::$variant => Ok(ErrResponse::$variant),)*
        }
    };
}

impl From<ErrResponse> for proto::responses::Error {
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

impl TryFrom<proto::responses::Error> for ErrResponse {
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
