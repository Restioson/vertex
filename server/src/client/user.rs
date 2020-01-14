use crate::client::{ClientWsSession, LogoutThisSession};
use std::collections::HashMap;
use vertex_common::DeviceId;
use actix::Addr;

lazy_static! {
    pub static ref USERS: DashMap<UserId, User> = DashMap::new();
}

pub struct User {
    pub sessions: Vec<(DeviceId, Addr<ClientWsSession>)>,
}

impl User {
    fn log_out_all(&mut self) {
        for (_, session) in sessions {
            session.do_send(LogoutThisSession)
        }
    }
}