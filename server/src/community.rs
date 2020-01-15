use crate::client::ClientWsSession;
use crate::IdentifiedMessage;
use actix::{Actor, Addr, Context, Handler, Message, ResponseFuture};
use dashmap::DashMap;
use lazy_static::lazy_static;
use std::collections::HashMap;
use uuid::Uuid;
use vertex_common::{ClientSentMessage, CommunityId, MessageId, RoomId, ServerError, UserId};

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Addr<CommunityActor>> = DashMap::new();
}

pub struct UserInCommunity(CommunityId);

#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: UserId,
    pub session: Addr<ClientWsSession>,
}

#[derive(Message)]
#[rtype(result = "Result<bool, ServerError>")]
pub struct Join {
    pub user_id: UserId,
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
        rooms.insert(
            RoomId(Uuid::new_v4()),
            Room {
                name: "general".to_string(),
            },
        );

        let mut online_members = HashMap::new();
        online_members.insert(
            creator,
            OnlineMember {
                devices: online_devices,
            },
        );

        CommunityActor {
            rooms,
            online_members,
        }
    }
}

impl Handler<Connect> for CommunityActor {
    type Result = ();

    fn handle(&mut self, join: Connect, _: &mut Context<Self>) -> Self::Result {
        let user_id = join.user_id;
        let session = join.session;
        let session_cloned = session.clone();

        self.online_members
            .entry(user_id)
            .and_modify(move |member| member.devices.push(session_cloned))
            .or_insert_with(|| OnlineMember::new(session));
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for CommunityActor {
    type Result = ResponseFuture<Result<MessageId, ServerError>>;

    fn handle(
        &mut self,
        m: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> Self::Result {
        // TODO(implement)
        unimplemented!()
    }
}

impl Handler<Join> for CommunityActor {
    type Result = ResponseFuture<Result<bool, ServerError>>;

    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Self::Result {
        // TODO(implement)
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
