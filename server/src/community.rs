use crate::client::{self, ActiveSession, Session};
use crate::database::{AddToCommunityError, CommunityRecord, Database, DbResult};
use crate::{handle_disconnected, IdentifiedMessage, SendMessage};
use dashmap::DashMap;
use futures::Future;
use futures::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;
use vertex::*;
use xtra::prelude::*;
use xtra::Disconnected;
use chrono::Utc;

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Community> = DashMap::new();
}

pub struct Community {
    pub actor: Address<CommunityActor>,
    pub name: String,
}

pub struct Connect {
    pub user: UserId,
    pub device: DeviceId,
    pub session: Address<ActiveSession>,
}

impl Message for Connect {
    type Result = DbResult<Result<(), ConnectError>>;
}

pub enum ConnectError {
    NotInCommunity,
}

pub struct Join {
    pub user: UserId,
    pub device_id: DeviceId,
    pub session: Address<ActiveSession>,
}

impl Message for Join {
    type Result = DbResult<Result<CommunityStructure, AddToCommunityError>>;
}

pub struct CreateRoom {
    pub creator: DeviceId,
    pub name: String,
}

impl Message for CreateRoom {
    type Result = DbResult<RoomId>;
}

pub struct GetRoomStructures;

impl Message for GetRoomStructures {
    type Result = Vec<RoomStructure>;
}

/// A community is a collection (or "house", if you will) of rooms, as well as some metadata.
/// It is similar to a "server" in Discord.
pub struct CommunityActor {
    id: CommunityId,
    database: Database,
    rooms: HashMap<RoomId, Room>,
    /// BTreeSet gives us efficient iteration and checking, compared to HashSet which has O(capacity)
    /// iteration.
    online_members: BTreeSet<UserId>,
}

impl Actor for CommunityActor {}

impl CommunityActor {
    pub fn new(id: CommunityId, database: Database, creator: UserId) -> CommunityActor {
        let mut online_members = BTreeSet::new();
        online_members.insert(creator);

        CommunityActor {
            id,
            database,
            rooms: HashMap::new(),
            online_members,
        }
    }

    pub fn create_and_spawn(name: String, id: CommunityId, database: Database, creator: UserId) {
        let addr = CommunityActor::new(id, database, creator).spawn();
        let community = Community { actor: addr, name };
        COMMUNITIES.insert(id, community);
    }

    pub async fn load_and_spawn(record: CommunityRecord, database: Database) -> DbResult<()> {
        let rooms = database.get_rooms_in_community(record.id).await?;
        let rooms = rooms
            .map_ok(|record| (record.id, Room { name: record.name }))
            .try_collect()
            .await?;

        let addr = CommunityActor {
            id: record.id,
            database,
            rooms,
            online_members: BTreeSet::new(),
        }
        .spawn();

        let community = Community {
            actor: addr,
            name: record.name,
        };

        COMMUNITIES.insert(record.id, community);

        Ok(())
    }

    fn for_each_online_device_except<F>(&mut self, mut f: F, except: Option<DeviceId>)
    where
        F: FnMut(&Address<ActiveSession>) -> Result<(), Disconnected>,
    {
        for member in self.online_members.iter() {
            let user = match client::session::get_active_user(*member) {
                Some(user) => user,
                None => continue, // Assume that this is a timing anomaly which will be corrected soon
            };

            for (device, session) in user.sessions.iter() {
                if let Session::Active(session) = session {
                    let send_to_this_device = match except {
                        Some(except) => except != *device,
                        None => true,
                    };

                    if send_to_this_device {
                        if let Err(d) = f(session) {
                            handle_disconnected("ClientSession")(d);
                        }
                    }
                }
            }
        }
    }
}

impl Handler<Connect> for CommunityActor {
    type Responder<'a> = impl Future<Output = DbResult<Result<(), ConnectError>>>;
    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            let membership = self
                .database
                .get_community_membership(self.id, connect.user)
                .await?;
            if membership.is_some() {
                // TODO(banning): check if user is not banned
                self.online_members.insert(connect.user);
                Ok(Ok(()))
            } else {
                Ok(Err(ConnectError::NotInCommunity))
            }
        }
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for CommunityActor {
    type Responder<'a> = impl Future<Output = Result<MessageId, ErrResponse>> + 'a;
    fn handle(
        &mut self,
        m: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> Self::Responder<'_> {
        async move {
            let id = MessageId(Uuid::new_v4());

            let msg = m.message.clone();
            let db = self.database.clone();
            let author = m.user;

            let (_ord, profile_version) = db.create_message(
                id,
                author,
                msg.to_community,
                msg.to_room,
                Utc::now(),
                msg.content
            ).await?;

            let from_device = m.device;
            let fwd = ForwardedMessage::new(id, m.message, m.user, profile_version);
            let send = SendMessage(ServerMessage::Event(ServerEvent::AddMessage(fwd)));

            self.for_each_online_device_except(|addr| addr.do_send(send.clone()), Some(from_device));

            Ok(id)
        }
    }
}

impl SyncHandler<IdentifiedMessage<Edit>> for CommunityActor {
    fn handle(
        &mut self,
        m: IdentifiedMessage<Edit>,
        _: &mut Context<Self>,
    ) -> Result<(), ErrResponse> {
        let from_device = m.device;
        let send = SendMessage(ServerMessage::Event(ServerEvent::Edit(m.message)));

        self.for_each_online_device_except(|addr| addr.do_send(send.clone()), Some(from_device));

        Ok(())
    }
}

impl Handler<Join> for CommunityActor {
    type Responder<'a> =
        impl Future<Output = DbResult<Result<CommunityStructure, AddToCommunityError>>>;

    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            if let Err(e) = self.database.add_to_community(self.id, join.user).await? {
                return Ok(Err(e)); // TODO(banning): check if user is not banned
            }

            self.online_members.insert(join.user);

            Ok(Ok(CommunityStructure {
                id: self.id,
                name: COMMUNITIES.get(&self.id).unwrap().name.clone(),
                rooms: self
                    .rooms
                    .iter()
                    .map(|(id, room)| RoomStructure {
                        id: *id,
                        name: room.name.clone(),
                    })
                    .collect(),
            }))
        }
    }
}

impl Handler<CreateRoom> for CommunityActor {
    type Responder<'a> = impl Future<Output = DbResult<RoomId>> + 'a;

    fn handle(&mut self, create: CreateRoom, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            let id = self
                .database
                .create_room(self.id, create.name.clone())
                .await?;

            self.rooms.insert(
                id,
                Room {
                    name: create.name.clone(),
                },
            );

            let send = SendMessage(ServerMessage::Event(ServerEvent::AddRoom {
                community: self.id,
                structure: RoomStructure {
                    id,
                    name: create.name.clone(),
                },
            }));

            self.for_each_online_device_except(
                |addr| addr.do_send(send.clone()),
                Some(create.creator),
            );

            Ok(id)
        }
    }
}

impl SyncHandler<GetRoomStructures> for CommunityActor {
    fn handle(&mut self, _get: GetRoomStructures, _: &mut Context<Self>) -> Vec<RoomStructure> {
        self.rooms
            .iter()
            .map(move |(id, room)| RoomStructure {
                id: *id,
                name: room.name.clone(),
            })
            .collect()
    }
}

/// A room, loaded into memory
#[derive(Debug)]
struct Room {
    name: String,
}
