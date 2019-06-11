use super::ClientWsSession;
use crate::SendMessage;
use actix::dev::{MessageResponse, ResponseChannel};
use actix::prelude::*;
use ccl::dhashmap::DHashMap;
use std::fmt::Debug;
use uuid::Uuid;
use vertex_common::*;

struct Room {
    users: Vec<UserId>,
}

impl Room {
    fn new(creator: UserId) -> Self {
        Room {
            users: vec![creator],
        }
    }

    fn add(&mut self, user: UserId) {
        self.users.push(user)
    }
}

#[derive(Message)]
pub struct Connect {
    pub session: Addr<ClientWsSession>,
    pub login: Login,
}

#[derive(Debug, Message)]
pub struct Join {
    pub room: RoomId,
}

impl ClientMessageType for Join {}

impl MessageResponse<ClientServer, IdentifiedMessage<CreateRoom>> for RoomId {
    fn handle<R: ResponseChannel<IdentifiedMessage<CreateRoom>>>(
        self,
        _: &mut Context<ClientServer>,
        tx: Option<R>,
    ) {
        if let Some(tx) = tx {
            tx.send(self)
        }
    }
}

#[derive(Debug)]
pub struct IdentifiedMessage<T: Message + ClientMessageType + Debug> {
    pub id: UserId,
    pub msg: T,
}

impl<T: Message + ClientMessageType + Debug> Message for IdentifiedMessage<T> {
    type Result = T::Result;
}

#[derive(Debug)]
pub struct CreateRoom;

impl Message for CreateRoom {
    type Result = RoomId;
}

impl ClientMessageType for CreateRoom {}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
struct SessionId(pub Uuid);

pub struct ClientServer {
    sessions: DHashMap<SessionId, Addr<ClientWsSession>>,
    rooms: DHashMap<RoomId, Room>,
}

impl ClientServer {
    pub fn new() -> Self {
        ClientServer {
            sessions: DHashMap::default(),
            rooms: DHashMap::default(),
        }
    }

    fn send_to_room(&mut self, room: &RoomId, message: ServerMessage, sender: &UserId) {
        let room = self.rooms.index(room);
        for user_id in room.users.iter().filter(|id| **id != *sender) {
            // TODO do not unwrap
            if let Some(client) = self.sessions.get_mut(&SessionId(user_id.0)) {
                // TODO multiple clients per user
                client.do_send(SendMessage {
                    message: message.clone(),
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
        self.sessions
            .insert(SessionId(connect.login.id.0), connect.session); // TODO multiple clients per user
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<ClientSentMessage>, _: &mut Context<Self>) {
        println!("msg: {:?}", m);
        let author_id = m.id;
        self.send_to_room(
            &m.msg.to_room.clone(),
            ServerMessage::Message(ForwardedMessage::from_message_and_author(m.msg, m.id)),
            &author_id,
        );
    }
}

impl Handler<IdentifiedMessage<CreateRoom>> for ClientServer {
    type Result = RoomId;

    fn handle(&mut self, m: IdentifiedMessage<CreateRoom>, _: &mut Context<Self>) -> RoomId {
        let id = RoomId(Uuid::new_v4());
        self.rooms.insert(id, Room::new(m.id));

        id
    }
}

impl Handler<IdentifiedMessage<Join>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<Join>, _: &mut Context<Self>) {
        self.rooms.get_mut(&m.msg.room).unwrap().add(m.id); // TODO don't unwrap
    }
}

impl Handler<IdentifiedMessage<Edit>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<Edit>, _: &mut Context<Self>) {
        self.send_to_room(&m.msg.room_id, ServerMessage::Edit(m.msg), &m.id);
    }
}

impl Handler<IdentifiedMessage<Delete>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<Delete>, _: &mut Context<Self>) {
        self.send_to_room(&m.msg.room_id, ServerMessage::Delete(m.msg), &m.id);
    }
}
