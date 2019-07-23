use super::ClientWsSession;
use crate::{database::DatabaseServer, SendMessage};
use actix::prelude::*;
use ccl::dhashmap::DHashMap;
use std::fmt::Debug;
use uuid::Uuid;
use vertex_common::*;

#[derive(Message)]
pub struct Connect {
    pub session: Addr<ClientWsSession>,
    pub user_id: UserId,
    pub device_id: DeviceId,
}

#[derive(Debug, Message)]
pub struct Disconnect {
    pub user_id: UserId,
    pub device_id: DeviceId,
}

#[derive(Debug)]
pub struct Join {
    pub room: RoomId,
}

impl Message for Join {
    type Result = RequestResponse;
}

impl ClientMessageType for Join {}

#[derive(Debug)]
pub struct IdentifiedMessage<T: Message + ClientMessageType + Debug> {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub request_id: RequestId,
    pub msg: T,
}

impl<T: Message + ClientMessageType + Debug> Message for IdentifiedMessage<T> {
    type Result = T::Result;
}

#[derive(Debug)]
pub struct CreateRoom;

impl ClientMessageType for CreateRoom {}

impl Message for CreateRoom {
    type Result = RequestResponse;
}

pub struct ClientServer {
    db: Addr<DatabaseServer>,
    sessions: DHashMap<DeviceId, Addr<ClientWsSession>>,
    online_devices: DHashMap<UserId, Vec<DeviceId>>,
    rooms: DHashMap<RoomId, Vec<UserId>>,
}

impl ClientServer {
    pub fn new(db: Addr<DatabaseServer>) -> Self {
        ClientServer {
            db,
            sessions: DHashMap::default(),
            online_devices: DHashMap::default(),
            rooms: DHashMap::default(),
        }
    }

    fn send_to_room(&mut self, room: &RoomId, message: ServerMessage, sender: &DeviceId) {
        let room = self.rooms.index(room);
        for user_id in room.iter() {
            if let Some(online_devices) = self.online_devices.get(user_id) {
                online_devices
                    .iter()
                    .filter(|id| **id != *sender)
                    .map(|id| self.sessions.get_mut(id).unwrap())
                    .for_each(|s| {
                        s.do_send(SendMessage {
                            message: message.clone(),
                        })
                    });
            }
        }
    }
}

impl Actor for ClientServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for ClientServer {
    type Result = ();

    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) {
        if let Some(mut online_devices) = self.online_devices.get_mut(&connect.user_id) {
            online_devices.push(connect.device_id);
        } else {
            self.online_devices
                .insert(connect.user_id, vec![connect.device_id]);
        }

        self.sessions.insert(connect.device_id, connect.session);
    }
}

impl Handler<Disconnect> for ClientServer {
    type Result = ();

    fn handle(&mut self, disconnect: Disconnect, _: &mut Context<Self>) {
        println!("received discon: {:?}", disconnect);

        let mut online_devices = self.online_devices.get_mut(&disconnect.user_id).unwrap();

        let idx = online_devices
            .iter()
            .position(|i| *i == disconnect.device_id)
            .unwrap();
        online_devices.remove(idx);

        if online_devices.len() == 0 {
            drop(online_devices); // Necessary to stop double lock
            self.online_devices.remove(&disconnect.user_id);
        }

        self.sessions.remove(&disconnect.device_id);
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for ClientServer {
    type Result = RequestResponse;

    fn handle(
        &mut self,
        m: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> RequestResponse {
        println!("msg: {:?}", m);
        let author_id = m.device_id;
        self.send_to_room(
            &m.msg.to_room.clone(),
            ServerMessage::Message(ForwardedMessage::from_message_and_author(m.msg, m.user_id)),
            &author_id,
        );
        RequestResponse::success()
    }
}

impl Handler<IdentifiedMessage<CreateRoom>> for ClientServer {
    type Result = RequestResponse;

    fn handle(
        &mut self,
        m: IdentifiedMessage<CreateRoom>,
        _: &mut Context<Self>,
    ) -> RequestResponse {
        let id = RoomId(Uuid::new_v4());
        self.rooms.insert(id, vec![m.user_id]);
        RequestResponse::room(id)
    }
}

impl Handler<IdentifiedMessage<Join>> for ClientServer {
    type Result = RequestResponse;

    fn handle(&mut self, m: IdentifiedMessage<Join>, _: &mut Context<Self>) -> RequestResponse {
        let mut room = match self.rooms.get_mut(&m.msg.room) {
            Some(r) => r,
            // In future, this error can also be used for rooms that the user is banned from/not
            // invited to
            None => return RequestResponse::Error(ServerError::InvalidRoom),
        };

        if room.contains(&m.user_id) {
            room.push(m.user_id);
            RequestResponse::success()
        } else {
            RequestResponse::Error(ServerError::AlreadyInRoom)
        }
    }
}

impl Handler<IdentifiedMessage<Edit>> for ClientServer {
    type Result = RequestResponse;

    fn handle(&mut self, m: IdentifiedMessage<Edit>, _: &mut Context<Self>) -> RequestResponse {
        let room_id = m.msg.room_id;
        self.send_to_room(&room_id, ServerMessage::Edit(m.msg), &m.device_id);
        RequestResponse::success()
    }
}

impl Handler<IdentifiedMessage<Delete>> for ClientServer {
    type Result = RequestResponse;

    fn handle(&mut self, m: IdentifiedMessage<Delete>, _: &mut Context<Self>) -> RequestResponse {
        let room_id = m.msg.room_id;
        self.send_to_room(&room_id, ServerMessage::Delete(m.msg), &m.device_id);
        RequestResponse::success()
    }
}
