use dashmap::DashMap;
use xtra::Address;

use lazy_static::lazy_static;
use vertex::*;

use super::*;
use std::collections::{HashMap, HashSet};

lazy_static! {
    static ref USERS: DashMap<UserId, ActiveUser> = DashMap::new();
}

pub struct ActiveUser {
    pub communities: HashSet<CommunityId>, // Community ID to community name
    pub sessions: HashMap<DeviceId, Session>,
}

impl ActiveUser {
    pub async fn load_with_new_session(
        db: Database,
        user: UserId,
        device: DeviceId,
        session: Session
    ) -> DbResult<Self> {
        let communities = db.get_communities_for_user(user).await?;
        let communities = communities.iter().map(|c| c.id).collect();
        let mut sessions = HashMap::new();
        sessions.insert(device, session);

        Ok(ActiveUser {
            communities,
            sessions,
        })
    }
}

pub enum Session {
    Upgrading,
    Active(Address<ActiveSession>),
}

pub async fn insert(db: Database, user: UserId, device: DeviceId) -> DbResult<Result<(), ()>> {
    if let Some(mut active_user) = USERS.get_mut(&user) {
        if active_user.sessions.contains_key(&device) {
            return Ok(Err(()));
        }

        active_user.sessions.insert(device, Session::Upgrading);
    } else {
        let active_user = ActiveUser::load_with_new_session(db, user, device, Session::Upgrading).await?;
        USERS.insert(user, active_user);
    }

    Ok(Ok(()))
}

pub fn upgrade(user: UserId, device: DeviceId, addr: Address<ActiveSession>) -> Result<(), ()> {
    let mut user = match get_active_user_mut(user) {
        Some(user) => user,
        None => return Err(()),
    };

    match user.sessions.get_mut(&device) {
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
    let mut lock = USERS.get_mut(&user);
    if let Some(ref mut active_user) = lock {
        let sessions = &mut active_user.sessions;
        if let Some(session) = sessions.remove(&device) {
            if sessions.is_empty() {
                // Drop the lock so that we can remove it without deadlocking
                drop(lock);
                USERS.remove(&user);
            }

            return Some(session);
        }
    }

    None
}

pub fn remove_all(user: UserId) {
    if let Some((_, active_user)) = USERS.remove(&user) {
        for session in active_user.sessions.values() {
            if let Session::Active(addr) = session {
                 addr.do_send(LogoutThisSession).unwrap()
            }
        }
    }
}

pub fn get_active_user<'a>(user: UserId) -> Option<ActiveUserRef<'a>> {
    USERS.get(&user)
}

pub fn get_active_user_mut<'a>(user: UserId) -> Option<ActiveUserRefMut<'a>> {
    USERS.get_mut(&user)
}

type ActiveUserRef<'a> = dashmap::mapref::one::Ref<'a, UserId, ActiveUser>;
type ActiveUserRefMut<'a> = dashmap::mapref::one::RefMut<'a, UserId, ActiveUser>;