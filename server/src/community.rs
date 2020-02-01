use crate::client::ClientWsSession;
use crate::client::USERS;
use crate::database::{AddToCommunityError, CommunityRecord, Database, DbResult};
use crate::{handle_disconnected, IdentifiedMessage, SendMessage};
use dashmap::DashMap;
use futures::Future;
use lazy_static::lazy_static;
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;
use vertex::*;
use xtra::prelude::*;
use xtra::Disconnected;

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Address<CommunityActor>> = DashMap::new();
}

pub struct Connect {
    pub user: UserId,
    pub device: DeviceId,
    pub session: Address<ClientWsSession>,
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
    pub session: Address<ClientWsSession>,
}

impl Message for Join {
    type Result = DbResult<Result<(), AddToCommunityError>>;
}

pub struct CreateRoom {
    pub creator: DeviceId,
    pub name: String,
}

impl Message for CreateRoom {
    type Result = RoomId;
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

    pub fn create_and_spawn(id: CommunityId, database: Database, creator: UserId) {
        let addr = CommunityActor::new(id, database, creator).spawn();
        COMMUNITIES.insert(id, addr);
    }

    pub fn load_and_spawn(record: CommunityRecord, database: Database) {
        let addr = CommunityActor {
            id: record.id,
            database,
            rooms: HashMap::new(), // TODO(room_persistence) load rooms
            online_members: BTreeSet::new(),
        }
        .spawn();

        COMMUNITIES.insert(record.id, addr);
    }

    fn for_each_online_device_skip<F>(&mut self, mut f: F, skip: Option<DeviceId>)
    where
        F: FnMut(&Address<ClientWsSession>) -> Result<(), Disconnected>,
    {
        for member in self.online_members.iter() {
            if let Some(user) = USERS.get(&member) {
                for (device, session) in user.sessions.iter() {
                    match skip {
                        Some(skip_device) if skip_device != *device => {
                            if let Err(d) = f(session) {
                                handle_disconnected("ClientWsSession")(d);
                            }
                        }
                        None => {
                            if let Err(d) = f(session) {
                                handle_disconnected("ClientWsSession")(d);
                            }
                        }
                        _ => (),
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

impl SyncHandler<IdentifiedMessage<ClientSentMessage>> for CommunityActor {
    fn handle(
        &mut self,
        m: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> Result<MessageId, ErrResponse> {
        let from_device = m.device;
        let fwd = ForwardedMessage::from_message_author_device(m.message, m.user, m.device);
        let send = SendMessage(ServerMessage::Action(ServerAction::Message(fwd)));

        self.for_each_online_device_skip(|addr| addr.do_send(send.clone()), Some(from_device));

        Ok(MessageId(Uuid::new_v4()))
    }
}

impl SyncHandler<IdentifiedMessage<Edit>> for CommunityActor {
    fn handle(
        &mut self,
        m: IdentifiedMessage<Edit>,
        _: &mut Context<Self>,
    ) -> Result<(), ErrResponse> {
        let from_device = m.device;
        let send = SendMessage(ServerMessage::Action(ServerAction::Edit(m.message)));

        self.for_each_online_device_skip(|addr| addr.do_send(send.clone()), Some(from_device));

        Ok(())
    }
}

impl Handler<Join> for CommunityActor {
    type Responder<'a> = impl Future<Output = DbResult<Result<(), AddToCommunityError>>>;

    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            if let Err(e) = self.database.add_to_community(self.id, join.user).await? {
                return Ok(Err(e)); // TODO(banning): check if user is not banned
            }

            self.online_members.insert(join.user);

            Ok(Ok(()))
        }
    }
}

impl SyncHandler<CreateRoom> for CommunityActor {
    fn handle(&mut self, create: CreateRoom, _: &mut Context<Self>) -> RoomId {
        let id = RoomId(Uuid::new_v4());
        self.rooms.insert(
            id,
            Room {
                name: create.name.clone(),
            },
        );

        let send = SendMessage(ServerMessage::Action(ServerAction::AddRoom {
            id,
            name: create.name.clone(),
        }));

        self.for_each_online_device_skip(|addr| addr.do_send(send.clone()), Some(create.creator));
        id
    }
}

/// A room, loaded into memory
struct Room {
    name: String,
}
