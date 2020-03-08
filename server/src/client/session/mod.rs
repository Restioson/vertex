use std::fmt::Debug;
use std::time::Instant;

use futures::stream::SplitSink;
use futures::{Future, SinkExt};
use warp::filters::ws;
use warp::filters::ws::WebSocket;
use xtra::prelude::*;

pub use manager::*;
use vertex::prelude::*;

use crate::community::{Connect, CreateRoom, GetRoomInfo, Join, COMMUNITIES};
use crate::database::*;
use regular_user::*;

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
    ws: SplitSink<WebSocket, ws::Message>,
    global: crate::Global,
    heartbeat: Instant,
    user: UserId,
    device: DeviceId,
    perms: TokenPermissionFlags,
}

impl ActiveSession {
    pub fn new(
        ws: SplitSink<WebSocket, ws::Message>,
        global: crate::Global,
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
            if self.handle_ws_message(message, ctx).await.is_err() {
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
            if self.ready(ctx).await.is_err() {
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
            let mut active_user = manager::get_active_user_mut(self.user).unwrap();
            let (community, room) = (fwd.community, fwd.room);
            let session = &active_user.sessions[&self.device];
            let looking_at = session.as_active_looking_at().unwrap();

            if let Some(user_community) = active_user.communities.get_mut(&community) {
                if let Some(user_room) = user_community.rooms.get_mut(&room) {
                    let msg = if looking_at == Some((community, room))
                        || user_room.watch_level == WatchLevel::Watching
                    {
                        Some(ServerMessage::Event(ServerEvent::AddMessage {
                            community: fwd.community,
                            room: fwd.room,
                            message: fwd.message,
                        }))
                    } else if !user_room.unread {
                        user_room.unread = true;
                        Some(ServerMessage::Event(ServerEvent::NotifyMessageReady {
                            room,
                            community,
                        }))
                    } else {
                        None
                    };

                    if let Some(msg) = msg {
                        if self.send(msg).await.is_err() {
                            ctx.stop()
                        }
                    }
                }
            };

            // Just ignore any errors as probable timing anomalies
        }
    }
}

impl Handler<AddRoom> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(&'a mut self, add: AddRoom, ctx: &'a mut Context<Self>) -> Self::Responder<'a> {
        async move {
            let mut user = manager::get_active_user_mut(self.user).unwrap();

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

                if self.send(msg).await.is_err() {
                    ctx.stop()
                }
            }
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
            if self.send(msg.0).await.is_err() {
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

// TODO: Error Handling: should not .unwrap() on `xtra::Disconnected` and `warp::Error`
impl ActiveSession {
    #[inline]
    async fn send<M: Into<Vec<u8>>>(&mut self, msg: M) -> Result<(), warp::Error> {
        self.ws.send(ws::Message::binary(msg)).await
    }

    /// Remove the device from wherever it is referenced
    fn log_out(&mut self) {
        manager::remove(self.user, self.device);
    }

    fn in_community(&self, id: &CommunityId) -> bool {
        manager::get_active_user(self.user)
            .unwrap()
            .communities
            .contains_key(&id)
    }

    fn in_room(&self, community: &CommunityId, room: &RoomId) -> bool {
        let user = manager::get_active_user(self.user).unwrap();
        if let Some(community) = user.communities.get(community) {
            community.rooms.contains_key(room)
        } else {
            false
        }
    }

    async fn ready(&mut self, ctx: &mut Context<Self>) -> Result<(), ()> {
        // TODO: handle errors better

        let user = self
            .global
            .database
            .get_user_by_id(self.user)
            .await
            .map_err(|_| ())?
            .ok_or(())?;

        let active = manager::get_active_user(self.user).unwrap();
        let mut communities = Vec::with_capacity(active.communities.len());

        for (id, user_community) in active.communities.iter() {
            let addr = COMMUNITIES.get(id).unwrap().actor.clone();
            let rooms = addr.send(GetRoomInfo).await.unwrap(); // TODO errors thing
            let rooms = rooms
                .into_iter()
                .map(|info| RoomStructure {
                    id: info.id,
                    name: info.name,
                    unread: user_community.rooms[&info.id].unread, // TODO errors thing
                })
                .collect();
            addr.do_send(Connect {
                user: self.user,
                device: self.device,
                session: ctx.address().unwrap(),
            })
            .unwrap();

            let structure = CommunityStructure {
                id: *id,
                name: COMMUNITIES.get(id).unwrap().name.clone(),
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

        self.send(ServerMessage::Event(ServerEvent::ClientReady(ready)))
            .await
            .map_err(|_| ())
    }

    async fn handle_ws_message(
        &mut self,
        message: WebSocketMessage,
        ctx: &mut Context<Self>,
    ) -> Result<(), warp::Error> {
        let message = message.0?;

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

            {
                let ratelimiter = self.global.ratelimiter.load();

                if let Err(not_until) = ratelimiter.check_key(&self.device) {
                    let resp = ErrResponse::RateLimited {
                        ready_in: not_until.wait_time_from(Instant::now())
                    };

                    self.send(ServerMessage::Response {
                        id: msg.id,
                        result: Err(resp),
                    }).await?;
                }
            }

            let (user, device, perms) = (self.user, self.device, self.perms);
            let handler = RequestHandler {
                session: self,
                ctx,
                user,
                device,
                perms,
            };
            let response = handler.handle_request(msg.request).await;

            self.send(ServerMessage::Response {
                id: msg.id,
                result: response.map_err(ErrResponse::Error),
            })
            .await?;
        } else if message.is_close() {
            ctx.stop();
        } else {
            self.send(ServerMessage::MalformedMessage).await?;
        }

        Ok(())
    }
}
