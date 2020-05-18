use super::administration::AdminRequest;
use crate::proto;
use crate::proto::DeserializeError;
use crate::structures::*;
use crate::types::*;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone)]
pub struct ClientMessage {
    pub id: RequestId,
    pub request: ClientRequest,
}

impl ClientMessage {
    pub fn from_protobuf_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        use prost::Message;
        let proto = proto::requests::active::ClientMessage::decode(bytes)?;
        proto.try_into()
    }
}

impl From<ClientMessage> for proto::requests::active::ClientMessage {
    fn from(msg: ClientMessage) -> Self {
        proto::requests::active::ClientMessage {
            id: Some(msg.id.into()),
            request: Some(msg.request.into()),
        }
    }
}

impl TryFrom<proto::requests::active::ClientMessage> for ClientMessage {
    type Error = DeserializeError;

    fn try_from(msg: proto::requests::active::ClientMessage) -> Result<Self, Self::Error> {
        Ok(ClientMessage {
            id: msg.id?.into(),
            request: msg.request?.try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ClientSentMessage {
    pub to_community: CommunityId,
    pub to_room: RoomId,
    pub content: String,
}

impl From<ClientSentMessage> for proto::requests::active::ClientSentMessage {
    fn from(msg: ClientSentMessage) -> Self {
        proto::requests::active::ClientSentMessage {
            to_community: Some(msg.to_community.into()),
            to_room: Some(msg.to_room.into()),
            content: msg.content,
        }
    }
}

impl TryFrom<proto::requests::active::ClientSentMessage> for ClientSentMessage {
    type Error = DeserializeError;

    fn try_from(msg: proto::requests::active::ClientSentMessage) -> Result<Self, Self::Error> {
        Ok(ClientSentMessage {
            to_community: msg.to_community?.try_into()?,
            to_room: msg.to_room?.try_into()?,
            content: msg.content,
        })
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ClientRequest {
    LogOut,
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    GetRoomUpdate {
        community: CommunityId,
        room: RoomId,
        last_received: Option<MessageId>,
        message_count: u64,
    },
    GetMessages {
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
        count: u64,
    },
    SelectRoom {
        community: CommunityId,
        room: RoomId,
    },
    DeselectRoom,
    SetAsRead {
        community: CommunityId,
        room: RoomId,
    },
    CreateCommunity {
        name: String,
    },
    CreateRoom {
        name: String,
        community: CommunityId,
    },
    CreateInvite {
        community: CommunityId,
        expiration_datetime: Option<DateTime<Utc>>,
    },
    JoinCommunity(InviteCode),
    Delete(Delete),
    ChangeUsername {
        new_username: String,
    },
    ChangeDisplayName {
        new_display_name: String,
    },
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    GetProfile(UserId),
    ChangeCommunityName {
        community: CommunityId,
        new: String,
    },
    ChangeCommunityDescription {
        community: CommunityId,
        new: String,
    },
    AdminAction(AdminRequest),
    ReportUser {
        message: MessageId,
        short_desc: String,
        extended_desc: String,
    }
}

impl From<ClientRequest> for proto::requests::active::ClientRequest {
    fn from(req: ClientRequest) -> proto::requests::active::ClientRequest {
        use proto::requests::active::{self as request, client_request::Request};
        use ClientRequest::*;

        let inner = match req {
            LogOut => Request::LogOut(proto::types::None {}),
            SendMessage(msg) => Request::SendMessage(msg.into()),
            EditMessage(edit) => Request::Edit(edit.into()),
            GetRoomUpdate {
                community,
                room,
                last_received,
                message_count,
            } => {
                use request::get_room_update::LastReceived::Present;
                Request::GetRoomUpdate(request::GetRoomUpdate {
                    community: Some(community.into()),
                    room: Some(room.into()),
                    last_received: last_received.map(|x| Present(x.into())),
                    message_count,
                })
            }
            GetMessages {
                community,
                room,
                selector,
                count,
            } => Request::GetMessages(request::GetMessages {
                community: Some(community.into()),
                room: Some(room.into()),
                selector: Some(selector.into()),
                message_count: count,
            }),
            SelectRoom { community, room } => Request::SelectRoom(request::SelectRoom {
                community: Some(community.into()),
                room: Some(room.into()),
            }),
            DeselectRoom => Request::DeselectRoom(proto::types::None {}),
            SetAsRead { community, room } => Request::SetAsRead(request::SetAsRead {
                community: Some(community.into()),
                room: Some(room.into()),
            }),
            CreateCommunity { name } => Request::CreateCommunity(request::CreateCommunity { name }),
            CreateRoom { name, community } => Request::CreateRoom(request::CreateRoom {
                name,
                community: Some(community.into()),
            }),
            CreateInvite {
                community,
                expiration_datetime: dt,
            } => {
                use request::create_invite::ExpirationDatetime::Present;
                Request::CreateInvite(request::CreateInvite {
                    community: Some(community.into()),
                    expiration_datetime: dt.map(|x| Present(x.timestamp())),
                })
            }
            JoinCommunity(code) => Request::JoinCommunity(request::JoinCommunity {
                invite_code: code.0,
            }),
            Delete(delete) => Request::Delete(delete.into()),
            ChangeUsername { new_username } => {
                Request::ChangeUsername(request::ChangeUsername { new_username })
            }
            ChangeDisplayName { new_display_name } => {
                Request::ChangeDisplayName(request::ChangeDisplayName { new_display_name })
            }
            ChangePassword {
                old_password,
                new_password,
            } => Request::ChangePassword(request::ChangePassword {
                new_password,
                old_password,
            }),
            GetProfile(id) => Request::GetProfile(request::GetProfile {
                user: Some(id.into()),
            }),
            ChangeCommunityName { new, community } => {
                Request::ChangeCommunityName(request::ChangeCommunityName {
                    new,
                    community: Some(community.into()),
                })
            }
            ChangeCommunityDescription { new, community } => {
                Request::ChangeCommunityDescription(request::ChangeCommunityDescription {
                    new,
                    community: Some(community.into()),
                })
            }
            AdminAction(req) => Request::AdminAction(req.into()),
            ReportUser { message, short_desc, extended_desc } => {
                Request::ReportUser(request::ReportUser {
                    message: Some(message.into()),
                    short_desc,
                    extended_desc,
                })
            }
        };

        request::ClientRequest {
            request: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::active::ClientRequest> for ClientRequest {
    type Error = DeserializeError;

    fn try_from(req: proto::requests::active::ClientRequest) -> Result<Self, Self::Error> {
        use proto::requests::active::{self as request, client_request::Request::*};

        let val = match req.request? {
            LogOut(_) => ClientRequest::LogOut,
            SendMessage(msg) => ClientRequest::SendMessage(msg.try_into()?),
            Edit(edit) => ClientRequest::EditMessage(edit.try_into()?),
            GetRoomUpdate(get) => {
                use request::get_room_update::LastReceived::Present;
                ClientRequest::GetRoomUpdate {
                    community: get.community?.try_into()?,
                    room: get.room?.try_into()?,
                    last_received: if let Some(Present(v)) = get.last_received {
                        Some(v.try_into()?)
                    } else {
                        None
                    },
                    message_count: get.message_count,
                }
            }
            GetMessages(get) => ClientRequest::GetMessages {
                community: get.community?.try_into()?,
                room: get.room?.try_into()?,
                selector: get.selector?.try_into()?,
                count: get.message_count,
            },
            SelectRoom(sel) => ClientRequest::SelectRoom {
                community: sel.community?.try_into()?,
                room: sel.room?.try_into()?,
            },
            DeselectRoom(_) => ClientRequest::DeselectRoom,
            SetAsRead(set) => ClientRequest::SetAsRead {
                community: set.community?.try_into()?,
                room: set.room?.try_into()?,
            },
            CreateCommunity(create) => ClientRequest::CreateCommunity { name: create.name },
            CreateRoom(create) => ClientRequest::CreateRoom {
                name: create.name,
                community: create.community?.try_into()?,
            },
            CreateInvite(create) => {
                use request::create_invite::ExpirationDatetime::Present;
                ClientRequest::CreateInvite {
                    community: create.community?.try_into()?,
                    expiration_datetime: create
                        .expiration_datetime
                        .map(|Present(x)| x)
                        .map(|ts| NaiveDateTime::from_timestamp(ts, 0))
                        .map(|dt| Utc.from_utc_datetime(&dt)),
                }
            }
            JoinCommunity(join) => ClientRequest::JoinCommunity(InviteCode(join.invite_code)),
            Delete(delete) => ClientRequest::Delete(delete.try_into()?),
            ChangeUsername(change) => ClientRequest::ChangeUsername {
                new_username: change.new_username,
            },
            ChangeDisplayName(change) => ClientRequest::ChangeDisplayName {
                new_display_name: change.new_display_name,
            },
            ChangePassword(change) => ClientRequest::ChangePassword {
                old_password: change.old_password,
                new_password: change.new_password,
            },
            GetProfile(get) => ClientRequest::GetProfile(get.user?.try_into()?),
            ChangeCommunityName(change) => ClientRequest::ChangeCommunityName {
                new: change.new,
                community: change.community?.try_into()?,
            },
            ChangeCommunityDescription(change) => ClientRequest::ChangeCommunityDescription {
                new: change.new,
                community: change.community?.try_into()?,
            },
            AdminAction(action) => ClientRequest::AdminAction(action.try_into()?),
            ReportUser(report) => ClientRequest::ReportUser {
                message: report.message?.try_into()?,
                short_desc: report.short_desc,
                extended_desc: report.extended_desc,
            },
        };

        Ok(val)
    }
}

impl ClientMessage {
    pub fn new(request: ClientRequest, id: RequestId) -> Self {
        ClientMessage { request, id }
    }
}

impl Into<Vec<u8>> for ClientMessage {
    fn into(self) -> Vec<u8> {
        use prost::Message;

        let mut buf = Vec::new();
        proto::requests::active::ClientMessage::from(self)
            .encode(&mut buf)
            .unwrap();
        buf
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Bound<T> {
    Inclusive(T),
    Exclusive(T),
}

impl<T> Bound<T> {
    #[inline]
    pub fn get(&self) -> &T {
        match self {
            Bound::Inclusive(bound) => bound,
            Bound::Exclusive(bound) => bound,
        }
    }
}

impl From<Bound<MessageId>> for proto::requests::active::Bound {
    fn from(bound: Bound<MessageId>) -> Self {
        match bound {
            Bound::Inclusive(bound) => proto::requests::active::Bound {
                exclusive: false,
                message: Some(bound.into()),
            },
            Bound::Exclusive(bound) => proto::requests::active::Bound {
                exclusive: true,
                message: Some(bound.into()),
            },
        }
    }
}

impl TryFrom<proto::requests::active::Bound> for Bound<MessageId> {
    type Error = DeserializeError;

    fn try_from(bound: proto::requests::active::Bound) -> Result<Self, Self::Error> {
        let proto::requests::active::Bound { exclusive, message } = bound;
        let message = message?.try_into()?;

        Ok(if exclusive {
            Bound::Exclusive(message)
        } else {
            Bound::Inclusive(message)
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub enum MessageSelector {
    Before(Bound<MessageId>),
    After(Bound<MessageId>),
}

impl From<MessageSelector> for proto::requests::active::MessageSelector {
    fn from(sel: MessageSelector) -> Self {
        match sel {
            MessageSelector::Before(bound) => proto::requests::active::MessageSelector {
                before: true,
                bound: Some(bound.into()),
            },
            MessageSelector::After(bound) => proto::requests::active::MessageSelector {
                before: false,
                bound: Some(bound.into()),
            },
        }
    }
}

impl TryFrom<proto::requests::active::MessageSelector> for MessageSelector {
    type Error = DeserializeError;

    fn try_from(sel: proto::requests::active::MessageSelector) -> Result<Self, Self::Error> {
        let proto::requests::active::MessageSelector { before, bound } = sel;
        let bound = bound?.try_into()?;

        Ok(if before {
            MessageSelector::Before(bound)
        } else {
            MessageSelector::After(bound)
        })
    }
}
