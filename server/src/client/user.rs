use crate::client::{ClientWsSession, LogoutThisSession};
use dashmap::DashMap;
use lazy_static::lazy_static;
use vertex_common::DeviceId;
use vertex_common::UserId;
use xtra::prelude::*;

lazy_static! {
    pub static ref USERS: DashMap<UserId, UserSessions> = DashMap::new();
}

pub struct UserSessions {
    pub sessions: Vec<(DeviceId, Address<ClientWsSession>)>,
}

impl UserSessions {
    pub fn new(session: (DeviceId, Address<ClientWsSession>)) -> Self {
        UserSessions {
            sessions: vec![session],
        }
    }

    pub fn log_out_all(&mut self) {
        for (_, session) in &self.sessions {
            session.do_send(LogoutThisSession).unwrap();
        }
    }

    pub fn get(&self, id: &DeviceId) -> Option<&Address<ClientWsSession>> {
        let idx = self.sessions.iter().position(|(device, _)| device == id)?;
        self.sessions.get(idx).map(|el| &el.1)
    }
}
