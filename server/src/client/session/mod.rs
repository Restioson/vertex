use std::fmt::Debug;
use std::time::Instant;

use futures::stream::SplitSink;
use futures::{Future, SinkExt};
use log::{debug, error, warn};
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;

pub use manager::*;
use vertex::prelude::*;

use crate::community::{self, Connect, CreateRoom, GetRoomInfo, Join, COMMUNITIES};
use crate::database::*;
use crate::handle_disconnected;
use regular_user::*;
use std::fmt;

mod administrator;
mod manager;
mod regular_user;

#[derive(Debug)]
pub struct LogoutThisSession;

impl xtra::Message for LogoutThisSession {
    type Result = ();
}

pub struct WebSocketMessage(pub(crate) Result<ws::Message, warp::Error>);

impl xtra::Message for WebSocketMessage {
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

impl xtra::Message for ForwardMessage {
    type Result = ();
}

#[derive(Debug, Clone)]
pub struct AddRoom {
    pub community: CommunityId,
    pub structure: RoomStructure,
}

impl xtra::Message for AddRoom {
    type Result = ();
}

pub struct ActiveSession {
    pub ws: SplitSink<WebSocket, ws::Message>,
    pub global: crate::Global,
    pub heartbeat: Instant,
    pub user: UserId,
    pub device: DeviceId,
    pub perms: TokenPermissionFlags,
}

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

impl ActiveSession {
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
}

impl Actor for ActiveSession {
    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.notify_immediately(NotifyClientReady);
        ctx.notify_interval(HEARTBEAT_TIMEOUT, || CheckHeartbeat);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        self.log_out();
    }
}

impl Handler<CheckHeartbeat> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: CheckHeartbeat, ctx: &mut Context<Self>) -> Self::Responder<'_> {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            ctx.stop();
        }

        async {}
    }
}

impl Handler<WebSocketMessage> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        message: WebSocketMessage,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if let Err(e) = self.handle_ws_message(message, ctx).await {
                debug!(
                    "Error handling websocket message. Error: {:?}\nClient: {:#?}",
                    e, self
                );
                ctx.stop();
            }
        }
    }
}

impl Handler<NotifyClientReady> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        _: NotifyClientReady,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if let Err(e) = self.ready(ctx).await {
                // Probably non-recoverable
                let _ = self
                    .send(ServerMessage::Event(ServerEvent::InternalError))
                    .await;
                error!("Error in client ready. Error: {:?}\nClient: {:#?}", e, self);
                ctx.stop();
            }
        }
    }
}

impl Handler<ForwardMessage> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        fwd: ForwardMessage,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
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
                Err(Error::InvalidUser) => {
                    warn!(
                        "Nonexistent user! Is this a timing anomaly? Client: {:#?}",
                        self
                    );
                    ctx.stop(); // The user did not exist at the time of request
                    ServerEvent::InternalError
                }
                Err(_) => return, // It's *probably* a timing anomaly.
            };

            if let Err(e) = self.send(ServerMessage::Event(msg)).await {
                error!(
                    "Error forwarding message. Error: {:?}\nClient: {:#?}",
                    e, self
                );
                ctx.stop()
            }
        }
    }
}

impl Handler<AddRoom> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(&'a mut self, add: AddRoom, ctx: &'a mut Context<Self>) -> Self::Responder<'a> {
        async move {
            let mut user = match manager::get_active_user_mut(self.user) {
                Ok(user) => user,
                Err(_) => {
                    let _ = self.send(ServerMessage::Event(ServerEvent::SessionLoggedOut));
                    warn!(
                        "Nonexistent user! Is this a timing anomaly? Client: {:#?}",
                        self
                    );
                    ctx.stop(); // The user did not exist at the time of request
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
                if let Err(e) = self.send(msg).await {
                    error!(
                        "Error adding room in client actor. Error: {:?}\nClient: {:#?}",
                        e, self
                    );
                    ctx.stop()
                }
            } // Else case is *probably* a timing anomaly
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        msg: SendMessage<ServerMessage>,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
        async move {
            if let Err(e) = self.send(msg.0).await {
                error!(
                    "Error sending server message. Error: {:?}\nClient: {:#?}",
                    e, self
                );
                ctx.stop()
            }
        }
    }
}

impl Handler<LogoutThisSession> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle(&mut self, _: LogoutThisSession, _: &mut Context<Self>) -> Self::Responder<'_> {
        async move {
            let _ = self
                .send(ServerMessage::Event(ServerEvent::SessionLoggedOut))
                .await;
            self.log_out();
        }
    }
}

impl ActiveSession {
    #[inline]
    async fn send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.ws.send(ws::Message::binary(msg)).await
    }

    /// Remove the device from wherever it is referenced
    fn log_out(&mut self) {
        manager::remove(self.user, self.device);
    }

    fn in_community(&self, id: &CommunityId) -> Result<bool, Error> {
        Ok(manager::get_active_user(self.user)?
            .communities
            .contains_key(&id))
    }

    fn in_room(&self, community: &CommunityId, room: &RoomId) -> Result<bool, Error> {
        let user = manager::get_active_user(self.user)?;
        Ok(if let Some(community) = user.communities.get(community) {
            community.rooms.contains_key(room)
        } else {
            false
        })
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
                session: ctx.address().unwrap(),
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
        };

        let msg = ServerMessage::Event(ServerEvent::ClientReady(ready));
        if let Err(e) = self.send(msg).await {
            error!(
                "Error sending websocket message. Error: {:?}\nClient: {:#?}",
                e, self
            );
            ctx.stop()
        }

        Ok(())
    }

    async fn handle_ws_message(
        &mut self,
        message: WebSocketMessage,
        ctx: &mut Context<Self>,
    ) -> Result<(), warp::Error> {
        let message = message.0?;

        {
            let ratelimiter = self.global.ratelimiter.load();

            if let Err(not_until) = ratelimiter.check_key(&self.device) {
                self.send(ServerMessage::RateLimited {
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
                Err(_) => {
                    self.send(ServerMessage::MalformedMessage).await?;
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
                warn!(
                    "Nonexistent user! Is this a timing anomaly? Client: {:#?}",
                    self
                );
                ctx.stop();
            }

            self.send(ServerMessage::Response { id: msg.id, result })
                .await?;
        } else if message.is_close() {
            ctx.stop();
        } else {
            self.send(ServerMessage::MalformedMessage).await?;
        }

        Ok(())
    }
}
