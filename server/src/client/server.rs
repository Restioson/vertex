use ccl::dhashmap::DHashMap;
use uuid::Uuid;
use actix::prelude::*;
use vertex_common::*;

pub struct ClientServer {
    init_keys: DHashMap<Uuid, InitKey>,
}

impl ClientServer {
    pub fn new() -> Self {
        ClientServer {
            init_keys: DHashMap::default()
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

    fn handle(&mut self, msg: RequestInitKey, _: &mut Context<Self>) -> Option<InitKey> {
        self.init_keys.get(&msg.id).map(|x| x.clone())
    }
}
