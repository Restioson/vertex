use crate::database::{CommunityRecord, UserRecord};
use vertex_common::{UserId, RoomId, CommunityId, ServerError, ClientSentMessage, MessageId};
use crate::client::ClientWsSession;
use actix::{Addr, Actor, Context, Message, ResponseFuture, Handler};
use std::collections::HashMap;
use uuid::Uuid;
use crate::client::IdentifiedMessage;
use vertex_common::RequestResponse;

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Addr<CommunityActor>> = DashMap::new();
}

pub struct UserInCommunity(CommunityId);

pub struct Connect {
    pub user_id: UserId,
    pub session: Addr<ClientWsSession>,
}

impl Message for Connect {
    type Result = ();
}

pub struct Join {
    pub user_id: UserId,
}

impl Message for Join {
    type Result = bool;
}

/// A community is a collection (or "house", if you will) of rooms, as well as some metadata.
/// It is similar to a "server" in Discord.
pub struct CommunityActor {
    rooms: HashMap<RoomId, Room>,
    online_members: HashMap<UserId, OnlineMember>,
}

impl Actor for CommunityActor {
    type Context = Context<Self>;
}

impl CommunityActor {
    fn new(creator: UserId, online_devices: Vec<Addr<ClientWsSession>>) -> CommunityActor {
        let mut rooms = HashMap::new();
        rooms[RoomId(Uuid::new_v4())] = Room {
            name: "general".to_string(),
        };

        let mut online_members = HashMap::new();
        online_devices[creator] = OnlineMember {
            devices: online_devices,
        };

        CommunityActor {
            rooms,
            online_members,
        }
    }
}

impl Handler<Connect> for CommunityActor {
    type Result = ();

    fn handle(&mut self, join: Connect, _: &mut Context<Self>) -> Self::Result {
        self.online_members.entry(join.user_id)
            .and_modify(move |member| member.devices.push(join.session))
            .or_insert_with(|| OnlineMember::new(join.session));
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for CommunityActor {
    type Result = ResponseFuture<Result<MessageId, ServerError>>;

    fn handle(&mut self, m: IdentifiedMessage<ClientSentMessage>, _: &mut Context<Self>) -> Self::Result {
        // TODO
        unimplemented!()
    }
}

impl Handler<Join> for CommunityActor {
    type Result = ResponseFuture<Result<bool, ServerError>>;

    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Self::Result {
        // TODO
        unimplemented!()
    }
}

/// A member and all their online devices
struct OnlineMember {
    pub devices: Vec<Addr<ClientWsSession>>,
}

impl OnlineMember {
    fn new(session: Addr<ClientWsSession>) -> OnlineMember {
        OnlineMember {
            devices: vec![session],
        }
    }
}

/// A room, loaded into memory
struct Room {
    name: String,
}

