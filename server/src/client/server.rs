use super::{ClientWsSession, LogoutThisSession};
use crate::SendMessage;
use actix::prelude::*;
use dashmap::DashMap;
use std::fmt::Debug;
use uuid::Uuid;
use vertex_common::{OkResponse, *};

#[derive(Message)]
pub struct Connect {
    pub session: Addr<ClientWsSession>,
    pub user_id: UserId,
    pub device_id: DeviceId,
}

#[derive(Debug, Message)]
pub struct LogoutSessions {
    pub list: Vec<(UserId, DeviceId)>,
}

#[derive(Debug, Message)]
pub struct LogoutUserSessions {
    pub user_id: UserId,
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
    type Result = OkResponse;
}

impl ClientMessageType for Join {}

#[derive(Debug)]
pub struct CreateRoom;

impl ClientMessageType for CreateRoom {}

impl Message for CreateRoom {
    type Result = OkResponse;
}

#[derive(Debug)]
pub struct IdentifiedMessage<T: Message + ClientMessageType + Debug> {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub msg: T,
}

impl<T: Message + ClientMessageType + Debug> Message for IdentifiedMessage<T> {
    type Result = T::Result;
}

pub struct ClientServer {
    sessions: DashMap<DeviceId, Addr<ClientWsSession>>,
    online_devices: DashMap<UserId, Vec<DeviceId>>,
    rooms: DashMap<RoomId, Vec<UserId>>,
}

impl ClientServer {
    pub fn new() -> Self {
        ClientServer {
            sessions: DashMap::default(),
            online_devices: DashMap::default(),
            rooms: DashMap::default(),
        }
    }

    fn logout_user_sessions(&mut self, user_id: &UserId) {
        if let Some(online_devices) = self.online_devices.get(user_id) {
            online_devices
                .iter()
                .map(|id| self.sessions.get_mut(id).unwrap())
                .for_each(|s| s.do_send(LogoutThisSession));
        }
    }

    fn logout_sessions(&mut self, logout: LogoutSessions) {
        // TODO could probably be optimised
        for (user_id, device_id) in logout.list {
            if let Some(online_devices) = self.online_devices.get(&user_id) {
                if online_devices.contains(&device_id) {
                    if let Some(session) = self.sessions.get_mut(&device_id) {
                        session.do_send(LogoutThisSession);
                    }
                }
            }
        }
    }

    fn send_to_room(&mut self, room: &RoomId, message: ClientboundMessage, sender: &DeviceId) {
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
    type Result = OkResponse;

    fn handle(
        &mut self,
        m: IdentifiedMessage<ClientSentMessage>,
        _: &mut Context<Self>,
    ) -> OkResponse {
        println!("msg: {:?}", m);
        let author_id = m.device_id;
        self.send_to_room(
            &m.msg.to_room.clone(),
            ClientboundMessage::Message(ForwardedMessage::from_message_author_device(
                m.msg,
                m.user_id,
                m.device_id,
            )),
            &author_id,
        );
        OkResponse::success()
    }
}

impl Handler<IdentifiedMessage<CreateRoom>> for ClientServer {
    type Result = OkResponse;

    fn handle(
        &mut self,
        m: IdentifiedMessage<CreateRoom>,
        _: &mut Context<Self>,
    ) -> OkResponse {
        let id = RoomId(Uuid::new_v4());
        self.rooms.insert(id, vec![m.user_id]);
        OkResponse::room(id)
    }
}

impl Handler<IdentifiedMessage<Join>> for ClientServer {
    type Result = OkResponse;

    fn handle(&mut self, m: IdentifiedMessage<Join>, _: &mut Context<Self>) -> OkResponse {
        let mut room = match self.rooms.get_mut(&m.msg.room) {
            Some(r) => r,
            // In future, this error can also be used for rooms that the user is banned from/not
            // invited to
            None => return OkResponse::Err(ErrResponse::InvalidRoom),
        };

        if !room.contains(&m.user_id) {
            room.push(m.user_id);
            OkResponse::success()
        } else {
            OkResponse::Err(ErrResponse::AlreadyInRoom)
        }
    }
}

impl Handler<IdentifiedMessage<Edit>> for ClientServer {
    type Result = OkResponse;

    fn handle(&mut self, m: IdentifiedMessage<Edit>, _: &mut Context<Self>) -> OkResponse {
        let room_id = m.msg.room_id;
        self.send_to_room(&room_id, ClientboundMessage::EditMessage(m.msg), &m.device_id);
        OkResponse::success()
    }
}

impl Handler<IdentifiedMessage<Delete>> for ClientServer {
    type Result = OkResponse;

    fn handle(&mut self, m: IdentifiedMessage<Delete>, _: &mut Context<Self>) -> OkResponse {
        let room_id = m.msg.room_id;
        self.send_to_room(&room_id, ClientboundMessage::DeleteMessage(m.msg), &m.device_id);
        OkResponse::success()
    }
}

impl Handler<LogoutUserSessions> for ClientServer {
    type Result = ();

    fn handle(&mut self, logout: LogoutUserSessions, _: &mut Context<Self>) {
        self.logout_user_sessions(&logout.user_id);
    }
}

impl Handler<LogoutSessions> for ClientServer {
    type Result = ();

    fn handle(&mut self, logout: LogoutSessions, _: &mut Context<Self>) {
        self.logout_sessions(logout)
    }
}
