use super::ClientWsSession;
use crate::SendMessage;
use actix::dev::{MessageResponse, ResponseChannel};
use actix::prelude::*;
use ccl::dhashmap::DHashMap;
use std::{fmt::Debug, ops::Deref};
use uuid::Uuid;
use vertex_common::*;

struct Room {
    clients: Vec<Uuid>,
}

impl Room {
    fn new(creator: Uuid) -> Self {
        Room {
            clients: vec![creator],
        }
    }

    fn add(&mut self, client: Uuid) {
        self.clients.push(client)
    }
}

#[derive(Message)]
pub struct Connect {
    pub session: Addr<ClientWsSession>,
    pub login: Login,
}

#[derive(Debug, Message)]
pub struct Join {
    pub room: Uuid,
}

impl ClientMessageType for Join {}

/// A wrapper for `Uuid` that allows it to be sent as a message
pub struct SendableUuid(Uuid);

impl Deref for SendableUuid {
    type Target = Uuid;

    fn deref(&self) -> &Uuid {
        &self.0
    }
}

impl MessageResponse<ClientServer, IdentifiedMessage<CreateRoom>> for SendableUuid {
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
    pub id: Uuid,
    pub msg: T,
}

impl<T: Message + ClientMessageType + Debug> Message for IdentifiedMessage<T> {
    type Result = T::Result;
}

#[derive(Debug)]
pub struct CreateRoom;

impl Message for CreateRoom {
    type Result = SendableUuid;
}

impl ClientMessageType for CreateRoom {}

pub struct ClientServer {
    sessions: DHashMap<Uuid, Addr<ClientWsSession>>,
    rooms: DHashMap<Uuid, Room>,
}

impl ClientServer {
    pub fn new() -> Self {
        ClientServer {
            sessions: DHashMap::default(),
            rooms: DHashMap::default(),
        }
    }

    fn send_to_room(&mut self, room: &Uuid, message: ServerMessage, sender: Uuid) {
        let room = self.rooms.index(room);
        for client_id in room.clients.iter().filter(|id| **id != sender) {
            // TODO do not unwrap
            if let Some(client) = self.sessions.get_mut(client_id) {
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
        self.sessions.insert(connect.login.id, connect.session);
    }
}

impl Handler<IdentifiedMessage<ClientSentMessage>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<ClientSentMessage>, _: &mut Context<Self>) {
        println!("msg: {:?}", m);
        self.send_to_room(
            &m.msg.to_room.clone(),
            ServerMessage::Message(ForwardedMessage::from_message_and_author(m.msg, m.id)),
            m.id,
        );
    }
}

impl Handler<IdentifiedMessage<CreateRoom>> for ClientServer {
    type Result = SendableUuid;

    fn handle(&mut self, m: IdentifiedMessage<CreateRoom>, _: &mut Context<Self>) -> SendableUuid {
        let id = Uuid::new_v4();
        self.rooms.insert(id.clone(), Room::new(m.id));

        SendableUuid(id)
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
        self.send_to_room(&m.msg.room_id.clone(), ServerMessage::Edit(m.msg), m.id);
    }
}

impl Handler<IdentifiedMessage<Delete>> for ClientServer {
    type Result = ();

    fn handle(&mut self, m: IdentifiedMessage<Delete>, _: &mut Context<Self>) {
        self.send_to_room(&m.msg.room_id.clone(), ServerMessage::Delete(m.msg), m.id);
    }
}
