use crate::client::session::{AddRoom, ForwardMessage};
use crate::client::{self, ActiveSession, Session};
use crate::database::{AddToCommunityError, CommunityRecord, Database, DbResult};
use crate::{handle_disconnected, IdentifiedMessage};
use chrono::Utc;
use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use futures::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;
use vertex::prelude::*;
use xtra::prelude::*;
use xtra::Disconnected;
use async_trait::async_trait;

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Community> = DashMap::new();
}

pub fn get<'a>(id: CommunityId) -> Result<Ref<'a, CommunityId, Community>, Error> {
    COMMUNITIES.get(&id).ok_or(Error::InvalidCommunity)
}

pub fn get_mut<'a>(id: CommunityId) -> Result<RefMut<'a, CommunityId, Community>, Error> {
    COMMUNITIES.get_mut(&id).ok_or(Error::InvalidCommunity)
}

pub fn address_of(id: CommunityId) -> Result<Address<CommunityActor>, Error> {
    get(id).map(|c| c.actor.clone())
}

/// Community info that is just read/updated very quickly (no logic like in the actor). Used to avoid
/// calls back and forth to the actor for simple things like getting the community name.
pub struct Community {
    pub actor: Address<CommunityActor>,
    pub name: String,
    pub description: Option<String>,
}

impl Community {
    pub fn description(&self) -> String {
        Community::desc_or_default(&self.description)
    }

    pub fn desc_or_default(desc: &Option<String>) -> String {
        desc.clone()
            .unwrap_or_else(|| "A Vertex community".to_string())
    }
}

pub struct Connect {
    pub user: UserId,
    pub device: DeviceId,
    pub session: ActiveSession,
}

impl xtra::Message for Connect {
    type Result = DbResult<Result<(), ConnectError>>;
}

pub enum ConnectError {
    NotInCommunity,
}

pub struct Join {
    pub user: UserId,
    pub device_id: DeviceId,
    pub session: ActiveSession,
}

impl xtra::Message for Join {
    type Result = DbResult<Result<CommunityStructure, AddToCommunityError>>;
}

pub struct CreateRoom {
    pub creator: DeviceId,
    pub name: String,
}

impl xtra::Message for CreateRoom {
    type Result = DbResult<RoomId>;
}

pub struct GetRoomInfo;

impl xtra::Message for GetRoomInfo {
    type Result = Vec<RoomInfo>;
}

pub struct RoomInfo {
    pub id: RoomId,
    pub name: String,
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
        let community = Community {
            actor: addr,
            name,
            description: None,
        };
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
            description: record.description,
        };

        COMMUNITIES.insert(record.id, community);

        Ok(())
    }

    fn for_each_online_device_except<F>(&mut self, mut f: F, except: Option<DeviceId>)
    where
        F: FnMut(&ActiveSession) -> Result<(), Disconnected>,
    {
        for member in self.online_members.iter() {
            let user = match client::session::get_active_user(*member) {
                Ok(user) => user,
                Err(_) => continue, // Assume that this is a timing anomaly which will be corrected soon
            };

            for (device, session) in user.sessions.iter() {
                if let Session::Active { actor, .. } = session {
                    let send_to_this_device = match except {
                        Some(except) => except != *device,
                        None => true,
                    };

                    if send_to_this_device {
                        if let Err(d) = f(actor) {
                            handle_disconnected("ClientSession")(d);
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Handler<Connect> for CommunityActor {
    async fn handle(
        &mut self,
        connect: Connect,
        _: &mut Context<Self>
    ) -> DbResult<Result<(), ConnectError>> {
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

#[async_trait]
impl Handler<IdentifiedMessage<ClientSentMessage>> for CommunityActor {
    async fn handle(
        &mut self,
        identified: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> Result<MessageConfirmation, Error> {
        let id = MessageId(Uuid::new_v4());

        let message = identified.message;
        let author = identified.user;
        let time_sent = Utc::now();

        let (_ord, profile_version) = self
            .database
            .create_message(
                id,
                author,
                message.to_community,
                message.to_room,
                time_sent,
                message.content.clone(),
            )
            .await?;

        let from_device = identified.device;
        let send = ForwardMessage {
            community: message.to_community,
            room: message.to_room,
            message: vertex::structures::Message {
                id,
                author,
                author_profile_version: profile_version,
                time_sent,
                content: Some(message.content),
            },
        };

        self.for_each_online_device_except(
            |session| {
                let _ = session.forward_message(send.clone());
                Ok(())
            },
            Some(from_device),
        );

        Ok(MessageConfirmation { id, time_sent })
    }
}

impl SyncHandler<IdentifiedMessage<Edit>> for CommunityActor {
    fn handle(&mut self, m: IdentifiedMessage<Edit>, _: &mut Context<Self>) -> Result<(), Error> {
        let from_device = m.device;
        let send = ServerMessage::Event(ServerEvent::Edit(m.message));

        self.for_each_online_device_except(
            |session| {
                let _ = session.send(send.clone());
                Ok(())
            },
            Some(from_device)
        );

        Ok(())
    }
}

#[async_trait]
impl Handler<Join> for CommunityActor {
    async fn handle(
        &mut self,
        join: Join,
        _: &mut Context<Self>
    ) -> DbResult<Result<CommunityStructure, AddToCommunityError>> {
        if let Err(e) = self.database.add_to_community(self.id, join.user).await? {
            return Ok(Err(e)); // TODO(banning): check if user is not banned
        }

        self.online_members.insert(join.user);

        let info = match get_mut(self.id) {
            Ok(i) => i,
            Err(_) => return Ok(Err(AddToCommunityError::InvalidCommunity)),
        };
        let description = info.description();

        Ok(Ok(CommunityStructure {
            id: self.id,
            name: info.name.clone(),
            description,
            rooms: self
                .rooms
                .iter()
                .map(|(id, room)| RoomStructure {
                    id: *id,
                    name: room.name.clone(),
                    unread: true,
                })
                .collect(),
        }))
    }
}

#[async_trait]
impl Handler<CreateRoom> for CommunityActor {
    async fn handle(&mut self, create: CreateRoom, _: &mut Context<Self>) -> DbResult<RoomId> {
        let db = &self.database;
        let id = db.create_room(self.id, create.name.clone()).await?;

        db.create_default_user_room_states_for_room(self.id, id)
            .await?
            .expect("Error creating default user room states for new room");

        self.rooms.insert(
            id,
            Room {
                name: create.name.clone(),
            },
        );

        let send = AddRoom {
            community: self.id,
            structure: RoomStructure {
                id,
                name: create.name.clone(),
                unread: false,
            },
        };

        self.for_each_online_device_except(
            |addr| {
                let _ = addr.add_room(send.clone());
                Ok(())
            },
            Some(create.creator),
        );

        Ok(id)
    }
}

impl SyncHandler<GetRoomInfo> for CommunityActor {
    fn handle(&mut self, _get: GetRoomInfo, _: &mut Context<Self>) -> Vec<RoomInfo> {
        self.rooms
            .iter()
            .map(move |(id, room)| RoomInfo {
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
