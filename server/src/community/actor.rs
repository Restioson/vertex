use crate::database::{CommunityRecord, UserRecord};
use vertex_common::{UserId, RoomId, CommunityId};
use crate::client::ClientWsSession;
use actix::{Addr, Actor, Context};
use std::collections::HashMap;
use uuid::Uuid;

pub struct UserInCommunity(CommunityId)

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

/// A member and any cached data needed while they are online
struct OnlineMember {
    devices: Vec<Addr<ClientWsSession>>,
}

impl OnlineMember {
    fn new(_record: UserRecord, session: Addr<ClientWsSession>) -> OnlineMember {
        OnlineMember {
            devices: vec![session],
        }
    }
}

/// A room, loaded into memory
struct Room {
    name: String,
}

