use dashmap::DashMap;
use xtra::Address;

use lazy_static::lazy_static;
use vertex::*;

use super::*;

lazy_static! {
    static ref SESSIONS: DashMap<UserId, Vec<(DeviceId, Session)>> = DashMap::new();
}

pub enum Session {
    Upgrading,
    Active(Address<ActiveSession>),
}

pub fn insert(user: UserId, device: DeviceId) -> Result<(), ()> {
    if let Some(mut sessions) = SESSIONS.get_mut(&user) {
        let existing_session = sessions.iter()
            .any(|(cmp_device, _)| *cmp_device == device);
        if existing_session {
            return Err(());
        }

        sessions.push((device, Session::Upgrading));
    } else {
        SESSIONS.insert(user, vec![(device, Session::Upgrading)]);
    }

    Ok(())
}

pub fn upgrade(user: UserId, device: DeviceId, addr: Address<ActiveSession>) -> Result<(), ()> {
    match get(user, device) {
        Some(mut session) => {
            *session = Session::Active(addr);
            Ok(())
        }
        None => Err(()),
    }
}

pub fn remove_and_notify(user: UserId, device: DeviceId) -> Option<Session> {
    let result = remove(user, device);
    if let Some(Session::Active(session)) = &result {
        session.do_send(LogoutThisSession).unwrap();
    }

    result
}

pub fn remove(user: UserId, device: DeviceId) -> Option<Session> {
    if let Some(mut sessions) = SESSIONS.get_mut(&user) {
        if let Some(idx) = sessions.iter().position(|(cmp_device, _)| *cmp_device == device) {
            let (_, session) = sessions.remove(idx);
            if sessions.is_empty() {
                // drop the lock on sessions so that we can remove it without deadlocking
                drop(sessions);
                SESSIONS.remove(&user);
            }

            return Some(session);
        }
    }

    None
}

pub fn remove_all(user: UserId) {
    if let Some((_, sessions)) = SESSIONS.remove(&user) {
        for (_, session) in &sessions {
            match session {
                Session::Active(addr) => addr.do_send(LogoutThisSession).unwrap(),
                _ => (),
            }
        }
    }
}

pub fn get_all<'a>(user: UserId) -> SessionsRef<'a> {
    SessionsRef { sessions: SESSIONS.get(&user) }
}

pub fn get<'a>(user: UserId, device: DeviceId) -> Option<SessionRef<'a>> {
    SESSIONS.get_mut(&user)
        .and_then(|sessions|
            sessions.iter()
                .position(|(cmp_device, _)| *cmp_device == device)
                .map(|idx| SessionRef { sessions, idx })
        )
}

pub struct SessionRef<'a> {
    sessions: dashmap::mapref::one::RefMut<'a, UserId, Vec<(DeviceId, Session)>>,
    idx: usize,
}

impl<'a> Deref for SessionRef<'a> {
    type Target = Session;

    #[inline]
    fn deref(&self) -> &Session { &self.sessions[self.idx].1 }
}

impl<'a> DerefMut for SessionRef<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Session { &mut self.sessions[self.idx].1 }
}

pub struct SessionsRef<'a> {
    sessions: Option<dashmap::mapref::one::Ref<'a, UserId, Vec<(DeviceId, Session)>>>,
}

impl<'a> Deref for SessionsRef<'a> {
    type Target = [(DeviceId, Session)];

    #[inline]
    fn deref(&self) -> &[(DeviceId, Session)] {
        match &self.sessions {
            Some(sessions) => sessions.as_slice(),
            None => &[],
        }
    }
}
