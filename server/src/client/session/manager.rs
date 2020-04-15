use dashmap::DashMap;
use xtra::Address;

use lazy_static::lazy_static;
use vertex::prelude::*;

use super::*;
use futures::TryStreamExt;
use std::collections::HashMap;

lazy_static! {
    static ref USERS: DashMap<UserId, ActiveUser> = DashMap::new();
}

/// Stuff that is shared between sessions.
pub struct ActiveUser {
    pub communities: HashMap<CommunityId, UserCommunity>,
    pub sessions: HashMap<DeviceId, Session>,
    pub admin_perms: AdminPermissionFlags,
}

impl ActiveUser {
    pub async fn load_with_new_session(
        db: Database,
        user: UserId,
        device: DeviceId,
        session: Session,
    ) -> DbResult<Self> {
        let communities = db.get_communities_for_user(user).await?;
        let admin_perms = db.get_admin_permissions(user).await?;
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
            admin_perms,
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
        let stream = db
            .get_user_room_states(user, community)
            .await?
            .map_ok(|state| {
                (
                    state.room,
                    UserRoom {
                        watch_level: state.watch_level,
                        unread: state.unread,
                    },
                )
            });

        let rooms = stream.try_collect().await?;

        Ok(UserCommunity { rooms })
    }
}

#[derive(Debug)]
pub struct UserRoom {
    pub watch_level: WatchLevel,
    pub unread: bool,
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

// TODO handle errors better
pub fn upgrade(user: UserId, device: DeviceId, addr: Address<ActiveSession>) -> Result<(), ()> {
    let mut user = match get_active_user_mut(user) {
        Ok(user) => user,
        Err(_) => return Err(()),
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

pub fn remove_and_notify(user: UserId, device: DeviceId) -> Result<(), Error> {
    match remove(user, device) {
        Some(Session::Active { actor, .. }) => actor
            .do_send(LogoutThisSession)
            .map_err(handle_disconnected("ClientSession")),
        _ => Ok(()),
    }
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

pub fn get_active_user<'a>(user: UserId) -> Result<ActiveUserRef<'a>, Error> {
    USERS.get(&user).ok_or(Error::LoggedOut)
}

pub fn get_active_user_mut<'a>(user: UserId) -> Result<ActiveUserRefMut<'a>, Error> {
    USERS.get_mut(&user).ok_or(Error::LoggedOut)
}

type ActiveUserRef<'a> = dashmap::mapref::one::Ref<'a, UserId, ActiveUser>;
type ActiveUserRefMut<'a> = dashmap::mapref::one::RefMut<'a, UserId, ActiveUser>;
