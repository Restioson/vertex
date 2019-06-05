use actix::prelude::*;

pub struct FederationServer {
}

impl FederationServer {
    pub fn new() -> Self {
        FederationServer {
        }
    }
}

impl Actor for FederationServer {
    type Context = Context<Self>;
}
