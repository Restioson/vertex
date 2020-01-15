use crate::client::{ClientWsSession, LogoutThisSession};
use vertex_common::DeviceId;
use actix::Addr;
use lazy_static::lazy_static;
use dashmap::DashMap;
use vertex_common::UserId;

lazy_static! {
    pub static ref USERS: DashMap<UserId, User> = DashMap::new();
}

pub struct User {
    pub sessions: Vec<(DeviceId, Addr<ClientWsSession>)>,
}

impl User {
    pub fn log_out_all(&mut self) {
        for (_, session) in &self.sessions {
            session.do_send(LogoutThisSession)
        }
    }

    pub fn get(&self, id: &DeviceId) -> Option<&Addr<ClientWsSession>> {
        let idx = self.sessions.iter().position(|(device_id, _)| device_id == id)?;
        self.sessions.get(idx).map(|el| &el.1)
    }
}