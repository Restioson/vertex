use ccl::dhashmap::DHashMap;
use uuid::Uuid;
use actix::prelude::*;
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
        self.sessions.insert(connect.login.uuid, connect.session);
    }
}
