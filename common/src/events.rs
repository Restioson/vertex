use crate::proto;
use crate::proto::DeserializeError;
use crate::responses::*;
use crate::structures::*;
use crate::types::*;
use std::convert::{TryFrom, TryInto};
use std::time::Duration;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ServerMessage {
    Event(ServerEvent),
    Response {
        id: RequestId,
        result: ResponseResult,
    },
    MalformedMessage,
    RateLimited {
        ready_in: Duration,
    },
}

impl ServerMessage {
    pub fn from_protobuf_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        use prost::Message;
        let proto = proto::events::ServerMessage::decode(bytes)?;
        proto.try_into()
    }
}

impl From<ServerMessage> for proto::events::ServerMessage {
    fn from(msg: ServerMessage) -> proto::events::ServerMessage {
        use proto::events::server_message::Message;
        use ServerMessage::*;

        let inner = match msg {
            Event(event) => Message::Event(event.into()),
            Response { id, result } => Message::Response(proto::responses::Response {
                id: Some(id.into()),
                response: Some(match result {
                    Ok(ok) => proto::responses::response::Response::Ok(ok.into()),
                    Err(err) => {
                        let err: proto::responses::Error = err.into();
                        proto::responses::response::Response::Error(err as i32)
                    }
                }),
            }),
            MalformedMessage => Message::MalformedMessage(proto::types::None {}),
            RateLimited { ready_in } => Message::RateLimited(proto::events::RateLimited {
                ready_in_ms: ready_in.as_millis().try_into().unwrap_or(std::u32::MAX),
            }),
        };

        proto::events::ServerMessage {
            message: Some(inner),
        }
    }
}

impl TryFrom<proto::events::ServerMessage> for ServerMessage {
    type Error = DeserializeError;

    fn try_from(msg: proto::events::ServerMessage) -> Result<Self, Self::Error> {
        use proto::events::server_message::Message::*;
        use proto::responses::response::Response;

        Ok(match msg.message? {
            Event(event) => ServerMessage::Event(event.try_into()?),
            Response(res) => match res.response? {
                Response::Ok(ok) => ServerMessage::Response {
                    id: res.id?.into(),
                    result: Ok(ok.try_into()?),
                },
                Response::Error(err) => {
                    let err = proto::responses::Error::from_i32(err)?.try_into()?;

                    ServerMessage::Response {
                        id: res.id?.into(),
                        result: Err(err),
                    }
                }
            },
            MalformedMessage(_) => ServerMessage::MalformedMessage,
            RateLimited(proto::events::RateLimited { ready_in_ms }) => ServerMessage::RateLimited {
                ready_in: Duration::from_millis(ready_in_ms as u64),
            },
        })
    }
}

impl Into<Vec<u8>> for ServerMessage {
    fn into(self) -> Vec<u8> {
        use prost::Message;

        let mut buf = Vec::new();
        proto::events::ServerMessage::from(self)
            .encode(&mut buf)
            .unwrap();
        buf
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ServerEvent {
    ClientReady(ClientReady),
    AddMessage {
        community: CommunityId,
        room: RoomId,
        message: Message,
    },
    NotifyMessageReady {
        community: CommunityId,
        room: RoomId,
    },
    Edit(Edit),
    Delete(Delete),
    SessionLoggedOut,
    AddRoom {
        community: CommunityId,
        structure: RoomStructure,
    },
    AddCommunity(CommunityStructure),
    RemoveCommunity {
        id: CommunityId,
        reason: RemoveCommunityReason,
    },
}

impl From<ServerEvent> for proto::events::ServerEvent {
    fn from(event: ServerEvent) -> Self {
        use proto::events::server_event::Event;
        use ServerEvent::*;

        let inner = match event {
            ClientReady(ready) => Event::ClientReady(ready.into()),
            AddMessage {
                community,
                room,
                message,
            } => Event::AddMessage(proto::events::AddMessage {
                community: Some(community.into()),
                room: Some(room.into()),
                message: Some(message.into()),
            }),
            NotifyMessageReady { community, room } => {
                Event::NotifyMessageReady(proto::events::NotifyMessageReady {
                    community: Some(community.into()),
                    room: Some(room.into()),
                })
            }
            Edit(edit) => Event::Edit(edit.into()),
            Delete(delete) => Event::Delete(delete.into()),
            SessionLoggedOut => Event::SessionLoggedOut(proto::types::None {}),
            AddRoom {
                community,
                structure,
            } => Event::AddRoom(proto::events::AddRoom {
                community: Some(community.into()),
                structure: Some(structure.into()),
            }),
            AddCommunity(structure) => Event::AddCommunity(structure.into()),
            RemoveCommunity { id, reason } => {
                Event::RemoveCommunity(proto::events::RemoveCommunity {
                    id: Some(id.into()),
                    reason: proto::events::RemoveCommunityReason::from(reason) as i32,
                })
            }
        };

        proto::events::ServerEvent { event: Some(inner) }
    }
}

impl TryFrom<proto::events::ServerEvent> for ServerEvent {
    type Error = DeserializeError;

    fn try_from(event: proto::events::ServerEvent) -> Result<Self, Self::Error> {
        use proto::events::server_event::Event::*;

        Ok(match event.event? {
            ClientReady(ready) => ServerEvent::ClientReady(ready.try_into()?),
            AddMessage(add) => ServerEvent::AddMessage {
                community: add.community?.try_into()?,
                room: add.room?.try_into()?,
                message: add.message?.try_into()?,
            },
            NotifyMessageReady(notify) => ServerEvent::NotifyMessageReady {
                community: notify.community?.try_into()?,
                room: notify.room?.try_into()?,
            },
            Edit(edit) => ServerEvent::Edit(edit.try_into()?),
            Delete(delete) => ServerEvent::Delete(delete.try_into()?),
            SessionLoggedOut(_) => ServerEvent::SessionLoggedOut,
            AddRoom(room) => ServerEvent::AddRoom {
                community: room.community?.try_into()?,
                structure: room.structure?.try_into()?,
            },
            AddCommunity(community) => ServerEvent::AddCommunity(community.try_into()?),
            RemoveCommunity(remove) => {
                let reason = proto::events::RemoveCommunityReason::from_i32(remove.reason);
                let reason = reason.ok_or(DeserializeError::InvalidEnumVariant)?;

                ServerEvent::RemoveCommunity {
                    id: remove.id?.try_into()?,
                    reason: reason.try_into()?,
                }
            }
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum RemoveCommunityReason {
    /// The community was deleted
    Deleted,
}

impl From<RemoveCommunityReason> for proto::events::RemoveCommunityReason {
    fn from(delete: RemoveCommunityReason) -> Self {
        use RemoveCommunityReason::*;

        match delete {
            Deleted => proto::events::RemoveCommunityReason::Deleted,
        }
    }
}

impl TryFrom<proto::events::RemoveCommunityReason> for RemoveCommunityReason {
    type Error = DeserializeError;

    fn try_from(delete: proto::events::RemoveCommunityReason) -> Result<Self, Self::Error> {
        use proto::events::RemoveCommunityReason::*;
        match delete {
            Deleted => Ok(RemoveCommunityReason::Deleted),
        }
    }
}
