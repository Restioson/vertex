use crate::client::ClientWsSession;
use crate::{IdentifiedMessage, SendMessage};
use dashmap::DashMap;
use lazy_static::lazy_static;
use std::collections::HashMap;
use uuid::Uuid;
use vertex_common::*;
use xtra::prelude::*;

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
}

impl Message for Join {
    type Result = Result<bool, ErrResponse>;
}

/// A community is a collection (or "house", if you will) of rooms, as well as some metadata.
/// It is similar to a "server" in Discord.
pub struct CommunityActor {
    rooms: HashMap<RoomId, Room>,
    online_members: HashMap<UserId, OnlineMember>,
}

impl Actor for CommunityActor {}

impl CommunityActor {
    fn new(
        creator: UserId,
        online_devices: Vec<(DeviceId, Address<ClientWsSession>)>,
    ) -> CommunityActor {
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

impl SyncHandler<Connect> for CommunityActor {
    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) {
        let user = connect.user;
        let device = connect.device;
        let session = connect.session;
        let session_cloned = session.clone();

        self.online_members
            .entry(user)
            .and_modify(move |member| member.devices.push((device, session_cloned)))
            .or_insert_with(|| OnlineMember::new(session, device));
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

        self.online_members
            .values()
            .flat_map(|member| member.devices.iter())
            .filter(|(device, _)| *device != from_device)
            .for_each(|(_, addr)| addr.do_send(send.clone()).unwrap());

        Ok(MessageId(Uuid::new_v4()))
    }
}

impl SyncHandler<IdentifiedMessage<Edit>> for CommunityActor {
    fn handle(&mut self, m: IdentifiedMessage<Edit>, _: &mut Context<Self>) -> Result<(), ErrResponse> {
        let from_device = m.device;
        let send = SendMessage(ServerMessage::Action(ServerAction::Edit(m.message)));

        self.online_members
            .values()
            .flat_map(|member| member.devices.iter())
            .filter(|(device, _)| *device != from_device)
            .for_each(|(_, addr)| addr.do_send(send.clone()).unwrap());

        Ok(())
    }
}

impl SyncHandler<Join> for CommunityActor {
    fn handle(&mut self, join: Join, _: &mut Context<Self>) -> Result<bool, ErrResponse> {
        // TODO(implement)
        unimplemented!()
    }
}

/// A member and all their online devices
struct OnlineMember {
    pub devices: Vec<(DeviceId, Address<ClientWsSession>)>,
}

impl OnlineMember {
    fn new(session: Address<ClientWsSession>, device: DeviceId) -> OnlineMember {
        OnlineMember {
            devices: vec![(device, session)],
        }
    }
}

/// A room, loaded into memory
struct Room {
    name: String,
}
