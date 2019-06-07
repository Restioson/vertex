use std::{ops::Deref, fmt::Debug};
use ccl::dhashmap::DHashMap;
use uuid::Uuid;
use actix::prelude::*;
use actix::dev::{MessageResponse, ResponseChannel};
use vertex_common::*;
use super::ClientWsSession;

struct Room {
    clients: Vec<Uuid>,
}

#[derive(Message)]
pub struct Connect {
    pub session: Addr<ClientWsSession>,
    pub login: Login,
}

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
        tx: Option<R>
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
    init_keys: DHashMap<Uuid, InitKey>,
    sessions: DHashMap<Uuid, Addr<ClientWsSession>>,
    rooms: DHashMap<Uuid, Room>,
}

impl ClientServer {
    pub fn new() -> Self {
        ClientServer {
            init_keys: DHashMap::default(),
            sessions: DHashMap::default(),
            rooms: DHashMap::default(),
        }
    }
}

impl Actor for ClientServer {
    type Context = Context<Self>;
}

impl Handler<PublishInitKey> for ClientServer {
    type Result = ();

    fn handle(&mut self, msg: PublishInitKey, _: &mut Context<Self>) {
        self.init_keys.insert(msg.id, msg.key);
    }
}

impl Handler<RequestInitKey> for ClientServer {
    type Result = Option<InitKey>;

    fn handle(&mut self, req: RequestInitKey, _: &mut Context<Self>) -> Option<InitKey> {
        self.init_keys.get(&req.id).map(|x| x.clone())
    }
}

impl Handler<Connect> for ClientServer {
    type Result = ();

    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) {
        self.sessions.insert(connect.login.id, connect.session);
    }
}

impl Handler<IdentifiedMessage<SentMessage>> for ClientServer {
    type Result = ();

    fn handle(&mut self, msg: IdentifiedMessage<SentMessage>, _: &mut Context<Self>) {
        // TODO
        println!("msg: {:?}", msg);
    }
}

impl Handler<IdentifiedMessage<CreateRoom>> for ClientServer {
    type Result = SendableUuid;

    fn handle(&mut self, _m: IdentifiedMessage<CreateRoom>, _: &mut Context<Self>) -> SendableUuid {
        // TODO
        SendableUuid(Uuid::new_v4())
    }
}

