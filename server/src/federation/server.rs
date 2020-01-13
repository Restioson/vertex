use super::ServerWsSession;
use actix::prelude::*;
use url::Url;
use std::collections::HashMap;

#[derive(Message)]
pub struct Connect {
    pub url: Url,
    pub session: ServerWsSession,
}

pub struct FederationServer {
    servers: HashMap<Url, ServerWsSession>,
}

impl FederationServer {
    pub fn new() -> Self {
        FederationServer {
            servers: HashMap::default(),
        }
    }
}

impl Actor for FederationServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for FederationServer {
    type Result = ();

    fn handle(&mut self, connect: Connect, _: &mut Context<Self>) {
        self.servers.insert(connect.url, connect.session);
    }
}
