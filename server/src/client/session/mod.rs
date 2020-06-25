use std::fmt::Debug;
use std::time::Instant;

use futures::stream::SplitSink;
use futures::SinkExt;
use log::{debug, error, warn};
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;
use async_trait::async_trait;

pub use manager::*;
use vertex::prelude::*;

use crate::community::{self, Connect, CreateRoom, GetRoomInfo, Join, COMMUNITIES};
use crate::database::*;
use crate::{handle_disconnected, Global};
use regular_user::*;
use std::fmt;
use xtra::KeepRunning;

mod administrator;
mod manager;
mod regular_user;

#[derive(Debug)]
pub struct LogoutThisSession;

impl xtra::Message for LogoutThisSession {
    type Result = ();
}

struct CheckHeartbeat;

impl xtra::Message for CheckHeartbeat {
    type Result = ();
}

struct NotifyClientReady;

impl xtra::Message for NotifyClientReady {
    type Result = ();
}

#[derive(Debug, Clone)]
pub struct SendMessage<T: Debug>(pub T);

impl<T: Debug + Send + 'static> xtra::Message for SendMessage<T> {
    type Result = ();
}

#[derive(Debug, Clone)]
pub struct ForwardMessage {
    pub community: CommunityId,
    pub room: RoomId,
    pub message: vertex::structures::Message,
}

#[derive(Debug, Clone)]
pub struct AddRoom {
    pub community: CommunityId,
    pub structure: RoomStructure,
}

#[derive(Debug)]
pub struct WsMessage(pub Result<ws::Message, warp::Error>);

impl xtra::Message for WsMessage {
    type Result = KeepRunning;
}

#[spaad::entangled]
#[async_trait]
impl Handler<WsMessage> for ActiveSession {
    async fn handle(&mut self, m: WsMessage, ctx: &mut Context<Self>) -> KeepRunning {
        match self.handle_ws_message(m.0, ctx).await {
            Ok(_) => KeepRunning::Yes,
            Err(e) => {
                debug!(
                    "Error handling websocket message. Error: {:?}\nClient: {:#?}",
                    e, self
                );
                ctx.stop();
                KeepRunning::No
            },
        }
    }
}

#[spaad::entangled]
pub struct ActiveSession {
    pub ws: SplitSink<WebSocket, ws::Message>,
    pub global: crate::Global,
    pub heartbeat: Instant,
    pub user: UserId,
    pub device: DeviceId,
    pub perms: TokenPermissionFlags,
}

#[spaad::entangled]
impl fmt::Debug for ActiveSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActiveSession")
            .field("heartbeat", &self.heartbeat)
            .field("user", &self.user)
            .field("device", &self.device)
            .field("perms", &self.perms)
            .finish()
    }
}

#[spaad::entangled]
impl Actor for ActiveSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_immediately(NotifyClientReady);
        ctx.notify_interval(HEARTBEAT_TIMEOUT, || CheckHeartbeat);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        self.log_out();
    }
}

#[spaad::entangled]
impl SyncHandler<CheckHeartbeat> for ActiveSession {
    fn handle(&mut self, _: CheckHeartbeat, ctx: &mut Context<Self>) {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            dbg!("heartbeat timeout");
            ctx.stop();
        }
    }
}

#[spaad::entangled]
#[async_trait]
impl Handler<NotifyClientReady> for ActiveSession {
    async fn handle(&mut self, _: NotifyClientReady, ctx: &mut Context<Self>) {
        if let Err(e) = self.ready(ctx).await {
            // Probably non-recoverable
            let _ = self
                .try_send(ServerMessage::Event(ServerEvent::InternalError))
                .await;
            error!("Error in client ready. Error: {:?}\nClient: {:#?}", e, self);
            ctx.stop();
        }
    }
}

#[spaad::entangled]
#[async_trait]
impl Handler<LogoutThisSession> for ActiveSession {
    async fn handle(&mut self, _: LogoutThisSession, ctx: &mut Context<Self>) {
        self.send(ServerMessage::Event(ServerEvent::SessionLoggedOut), ctx)
            .await;
        self.log_out();
    }
}

#[spaad::entangled]
impl ActiveSession {
    #[spaad::spawn]
    pub fn new(
        ws: SplitSink<WebSocket, ws::Message>,
        global: Global,
        user: UserId,
        device: DeviceId,
        perms: TokenPermissionFlags,
    ) -> Self {
        ActiveSession {
            ws,
            global,
            heartbeat: Instant::now(),
            user,
            device,
            perms,
        }
    }

    async fn try_send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.ws.send(ws::Message::binary(msg)).await
    }

    #[spaad::handler]
    pub async fn send<M>(&mut self, msg: M, ctx: &mut Context<Self>)
        where M: Into<Vec<u8>> + Send + 'static
    {
        if let Err(e) = self.try_send(msg).await {
            error!(
                "Error sending websocket message. Error: {:?}\nClient: {:#?}",
                e, self
            );
            ctx.stop()
        }
    }

    /// Remove the device from wherever it is referenced
    fn log_out(&mut self) {
        manager::remove_device(self.user, self.device);
    }

    fn in_community(&self, id: &CommunityId) -> Result<bool, Error> {
        Ok(manager::get_active_user(self.user)?
            .communities
            .contains_key(&id))
    }

    // in future, this will change with permissioning
    fn in_room(&self, community: &CommunityId, room: &RoomId) -> Result<bool, Error> {
        let user = manager::get_active_user(self.user)?;
        Ok(if let Some(community) = user.communities.get(community) {
            community.rooms.contains_key(room)
        } else {
            false
        })
    }

    /// Returns whether the client should be notified and whether the room had unread messages. It
    /// also sets the room to unread.
    fn should_notify_client(
        &self,
        community: CommunityId,
        room: RoomId,
    ) -> Result<(bool, bool), Error> {
        let mut active_user = manager::get_active_user_mut(self.user)?;
        let session = &active_user.sessions[&self.device];
        let looking_at = session.as_active_looking_at().unwrap();

        if let Some(user_community) = active_user.communities.get_mut(&community) {
            if let Some(user_room) = user_community.rooms.get_mut(&room) {
                let notify = looking_at == Some((community, room))
                    || user_room.watch_level == WatchLevel::Watching;
                let was_unread = user_room.unread;
                user_room.unread = true;
                Ok((notify, was_unread))
            } else {
                Err(Error::InvalidRoom)
            }
        } else {
            Err(Error::InvalidCommunity)
        }
    }

    async fn ready(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
        let user = self
            .global
            .database
            .get_user_by_id(self.user)
            .await?
            .ok_or(Error::InvalidUser)?;

        let active = manager::get_active_user(self.user)?;
        let mut communities = Vec::with_capacity(active.communities.len());

        for (id, user_community) in active.communities.iter() {
            let addr = community::address_of(*id)?;
            let rooms = addr.send(GetRoomInfo).await.map_err(|_| Error::Internal)?;
            let rooms = rooms
                .into_iter()
                .map(|info| {
                    let room = user_community
                        .rooms
                        .get(&info.id)
                        .ok_or(Error::InvalidRoom)?;
                    Ok(RoomStructure {
                        id: info.id,
                        name: info.name,
                        unread: room.unread,
                    })
                })
                .collect::<Result<Vec<RoomStructure>, Error>>()?;
            addr.do_send(Connect {
                user: self.user,
                device: self.device,
                session: ctx.address().unwrap().into(),
            })
            .map_err(handle_disconnected("Community"))?;

            let info = COMMUNITIES.get(id).ok_or(Error::InvalidCommunity)?;
            let structure = CommunityStructure {
                id: *id,
                name: info.name.clone(),
                description: info.description(),
                rooms,
            };

            communities.push(structure);
        }

        let ready = ClientReady {
            user: self.user,
            profile: Profile {
                version: user.profile_version,
                username: user.username,
                display_name: user.display_name,
            },
            communities,
            permissions: self.perms,
            admin_permissions: active.admin_perms,
        };

        let msg = ServerMessage::Event(ServerEvent::ClientReady(ready));
        self.send(msg, ctx).await;

        Ok(())
    }

    async fn handle_ws_message(
        &mut self,
        message: Result<ws::Message, warp::Error>,
        ctx: &mut Context<Self>,
    ) -> Result<(), warp::Error> {
        let message = message?;
        {
            let ratelimiter = self.global.ratelimiter.load();

            if let Err(not_until) = ratelimiter.check_key(&self.device) {
                self.try_send(ServerMessage::RateLimited {
                    ready_in: not_until.wait_time_from(Instant::now()),
                })
                .await?;

                return Ok(());
            }
        }

        if message.is_ping() {
            self.heartbeat = Instant::now();
            self.ws.send(ws::Message::ping(vec![])).await?;
        } else if message.is_binary() {
            let msg = match ClientMessage::from_protobuf_bytes(message.as_bytes()) {
                Ok(m) => m,
                Err(e) => {
                    log::debug!("Malformed message: {:#?}", e);
                    self.try_send(ServerMessage::MalformedMessage).await?;
                    return Ok(());
                }
            };

            let (user, device, perms) = (self.user, self.device, self.perms);
            let handler = RequestHandler {
                session: self,
                ctx,
                user,
                device,
                perms,
            };
            let result = handler.handle_request(msg.request).await;

            if let Err(Error::LoggedOut) = result {
                own_user_nonexistent(self, ctx);
            }

            self.try_send(ServerMessage::Response { id: msg.id, result })
                .await?;
        } else if message.is_close() {
            dbg!("asked for close");
            ctx.stop();
        } else {
            log::debug!("Malformed message: {:#?}", message);
            self.try_send(ServerMessage::MalformedMessage).await?;
        }

        Ok(())
    }

    #[spaad::handler]
    pub async fn forward_message(&mut self, fwd: ForwardMessage, ctx: &mut Context<Self>) {
        // Ok path is (notify, unread messages)
        let msg = match self.should_notify_client(fwd.community, fwd.room) {
            // If the user is watching the room, always forward the message
            Ok((true, _)) => ServerEvent::AddMessage {
                community: fwd.community,
                room: fwd.room,
                message: fwd.message,
            },
            // If the user is not watching but it wasn't unread, tell the client that there are new msgs
            Ok((false, false)) => ServerEvent::NotifyMessageReady {
                room: fwd.room,
                community: fwd.community,
            },
            // It was unread, so we don't need to tell the client about the new messages.
            Ok((false, true)) => return,
            Err(Error::InvalidUser) => own_user_nonexistent(self, ctx),
            Err(_) => return, // It's *probably* a timing anomaly.
        };

        self.send(ServerMessage::Event(msg), ctx).await;
    }

    #[spaad::handler]
    pub async fn add_room(&mut self, add: AddRoom, ctx: &mut Context<Self>) {
        let mut user = match manager::get_active_user_mut(self.user) {
            Ok(user) => user,
            Err(_) => {
                let _ = self.send(ServerMessage::Event(ServerEvent::SessionLoggedOut), ctx);
                own_user_nonexistent(self, ctx);
                return;
            }
        };

        if let Some(community) = user.communities.get_mut(&add.community) {
            community.rooms.insert(
                add.structure.id,
                UserRoom {
                    watch_level: WatchLevel::default(),
                    unread: true,
                },
            );

            let msg = ServerMessage::Event(ServerEvent::AddRoom {
                community: add.community,
                structure: add.structure,
            });

            drop(user); // Drop lock
            self.send(msg, ctx).await;
        } // Else case is *probably* a timing anomaly
    }
}

fn own_user_nonexistent<T: Debug, S: xtra::Actor>(client: &T, ctx: &mut Context<S>) -> ServerEvent {
    warn!(
        "Nonexistent user! Is this a timing anomaly? Client: {:#?}",
        client
    );
    ctx.stop(); // The user did not exist at the time of request
    ServerEvent::InternalError
}
