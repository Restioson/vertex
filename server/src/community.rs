use crate::client::{ClientWsSession, LogoutThisSession};
use crate::{IdentifiedMessage, SendMessage, handle_disconnected};
use dashmap::DashMap;
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet, BTreeSet};
use uuid::Uuid;
use vertex_common::*;
use xtra::prelude::*;
use crate::database::{AddToCommunityError, DbResult, Database};
use futures::Future;
use crate::client::USERS;
use xtra::Disconnected;

lazy_static! {
    pub static ref COMMUNITIES: DashMap<CommunityId, Address<CommunityActor>> = DashMap::new();
}

pub struct UserInCommunity(CommunityId);

pub struct Connect {
    pub user: UserId,
    pub device: DeviceId,
    pub session: Address<ClientWsSession>,
}

impl Message for Connect {
    type Result = ();
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
    pub fn new(
        id: CommunityId,
        database: Database,
        creator: UserId,
    ) -> CommunityActor {
        let mut rooms = HashMap::new();
        rooms.insert(
            RoomId(Uuid::new_v4()),
            Room {
                name: "general".to_string(),
            },
        );

        let mut online_members = BTreeSet::new();
        online_members.insert(creator);

        CommunityActor {
            id,
            database,
            rooms,
            online_members,
        }
    }

    fn for_each_online_device_skip<F>(&mut self, mut f: F, skip: Option<DeviceId>)
        where F: FnMut(&Address<ClientWsSession>) -> Result<(), Disconnected>
    {
        for member in self.online_members.iter() {
            if let Some(user) = USERS.get(&member) {
                for (device, session) in user.sessions.iter() {
                    match skip {
                        Some(skip_device) if skip_device != *device => {
                            if let Err(d) = f(session) {
                                handle_disconnected("ClientWsSession")(d);
                            }
                        },
                        _ => (),
                    }
                }
            }
        }
    }
}

impl SyncHandler<Connect> for CommunityActor {
    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) {
        self.online_members.insert(connect.user); // TODO(connect)
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

        self.for_each_online_device_skip(
            |addr| addr.do_send(send.clone()),
            Some(from_device)
        );

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

        self.for_each_online_device_skip(
            |addr| addr.do_send(send.clone()),
            Some(from_device)
        );

        Ok(())
    }
}

impl Handler<Join> for CommunityActor {
    type Responder<'a> = impl Future<Output = DbResult<Result<(), AddToCommunityError>>>;

    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            let res = self.database.add_to_community(self.id, join.user).await?;

            match res {
                Err(e) => return Ok(Err(e)),
                _ => (),
            }

            let community = match self.database.get_community_metadata(self.id).await? {
                Some(community) => community,
                None => return Ok(Err(AddToCommunityError::InvalidCommunity)),
            };

            self.online_members.insert(join.user);

            Ok(Ok(()))
        }
    }
}

impl SyncHandler<CreateRoom> for CommunityActor {
    fn handle(&mut self, create: CreateRoom, _: &mut Context<Self>) -> RoomId {
        let id = RoomId(Uuid::new_v4());
        self.rooms.insert(id, Room { name: create.name.clone() });

        let send = SendMessage(ServerMessage::Action(ServerAction::AddRoom {
            id,
            name: create.name.clone(),
        }));

        self.for_each_online_device_skip(
            |addr| addr.do_send(send.clone()),
            Some(create.creator),
        );
        id
    }
}

/// A room, loaded into memory
struct Room {
    name: String,
}
