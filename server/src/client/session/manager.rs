use dashmap::DashMap;
use xtra::Address;

use lazy_static::lazy_static;
use vertex::*;

use super::*;
use futures::TryStreamExt;
use std::collections::HashMap;

lazy_static! {
    static ref USERS: DashMap<UserId, ActiveUser> = DashMap::new();
}

pub struct ActiveUser {
    pub communities: HashMap<CommunityId, UserCommunity>,
    pub sessions: HashMap<DeviceId, Session>,
}

impl ActiveUser {
    pub async fn load_with_new_session(
        db: Database,
        user: UserId,
        device: DeviceId,
        session: Session,
    ) -> DbResult<Self> {
        let communities = db.get_communities_for_user(user).await?;
        let db = &db; // To prevent move
        let communities = communities
            .and_then(|record| async move {
                let community = UserCommunity::load(db, user, record.community).await?;

                Ok((record.community, community))
            })
            .try_collect()
            .await?;

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
    Active {
        actor: Address<ActiveSession>,
        looking_at: Option<(CommunityId, RoomId)>,
    },
}

impl Session {
    #[allow(clippy::option_option)]
    pub fn as_active_looking_at(&self) -> Option<Option<(CommunityId, RoomId)>> {
        match self {
            Session::Upgrading => None,
            Session::Active { looking_at, .. } => Some(*looking_at),
        }
    }

    pub fn as_active_actor(&self) -> Option<Address<ActiveSession>> {
        match self {
            Session::Upgrading => None,
            Session::Active { actor, .. } => Some(actor.clone()),
        }
    }

    pub fn set_looking_at(&mut self, at: Option<(CommunityId, RoomId)>) -> Option<()> {
        match self {
            Session::Upgrading => None,
            Session::Active { actor, .. } => {
                *self = Session::Active {
                    looking_at: at,
                    actor: actor.clone(),
                };
                Some(())
            }
        }
    }
}

#[derive(Debug)]
pub struct UserCommunity {
    pub rooms: HashMap<RoomId, UserRoom>,
}

impl UserCommunity {
    pub async fn load(db: &Database, user: UserId, community: CommunityId) -> DbResult<Self> {
        let rooms = db
            .get_watching_states(user, community)
            .await?
            .map_ok(|(id, watching)| (id, UserRoom { watching }))
            .try_collect()
            .await?;

        Ok(UserCommunity { rooms })
    }
}

#[derive(Debug)]
pub struct UserRoom {
    pub watching: WatchingState,
}

pub async fn insert(db: Database, user: UserId, device: DeviceId) -> DbResult<Result<(), ()>> {
    if let Some(mut active_user) = USERS.get_mut(&user) {
        if active_user.sessions.contains_key(&device) {
            return Ok(Err(()));
        }

        active_user.sessions.insert(device, Session::Upgrading);
    } else {
        let active_user =
            ActiveUser::load_with_new_session(db, user, device, Session::Upgrading).await?;
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
        Some(session) => {
            *session = Session::Active {
                actor: addr,
                looking_at: None,
            };
            Ok(())
        }
        None => Err(()),
    }
}

pub fn remove_and_notify(user: UserId, device: DeviceId) -> Option<Session> {
    let result = remove(user, device);
    if let Some(Session::Active { actor, .. }) = &result {
        actor.do_send(LogoutThisSession).unwrap();
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
            if let Session::Active { actor, .. } = session {
                actor.do_send(LogoutThisSession).unwrap()
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
