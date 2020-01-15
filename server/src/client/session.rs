use super::*;
use crate::config::Config;
use crate::database::*;
use crate::SendMessage;
use actix::fut;
use actix_web::web::Data;
use actix_web_actors::ws::{self, WebsocketContext};
use chrono::DateTime;
use chrono::Utc;
use std::io::Cursor;
use std::time::Instant;
use vertex_common::*;
use log::error;

// TODO(room_persistence): make sure device isnt online when try to login

#[derive(Eq, PartialEq)]
enum SessionState {
    WaitingForLogin,
    Ready(UserId, DeviceId, TokenPermissionFlags),
}

impl SessionState {
    fn user_and_device_ids(&self) -> Option<(UserId, DeviceId)> {
        match self {
            SessionState::WaitingForLogin => None,
            SessionState::Ready(user_id, device_id, _) => Some((*user_id, *device_id)),
        }
    }
}

pub struct ClientWsSession {
    database_server: Addr<DatabaseServer>,
    communities: Vec<CommunityId>,
    state: SessionState,
    heartbeat: Instant,
    config: Data<Config>,
}

impl Actor for ClientWsSession {
    type Context = WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut WebsocketContext<Self>) {
        self.start_heartbeat(ctx);
    }

    fn stopped(&mut self, ctx: &mut WebsocketContext<Self>) {
        if let Some(_) = self.state.user_and_device_ids() {
            self.delete(ctx); // TODO(room_persistence)
        }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ClientWsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut WebsocketContext<Self>) {
        let msg = if let Ok(msg) = msg {
            msg
        } else {
            self.delete(ctx);
            return;
        };

        match msg {
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.heartbeat = Instant::now();
            }
            ws::Message::Text(_) => {
                let error =
                    serde_cbor::to_vec(&ServerMessage::Error(ServerError::UnexpectedTextFrame))
                        .unwrap();
                ctx.binary(error);
            }
            ws::Message::Binary(bin) => {
                let mut bin = Cursor::new(bin);
                let msg = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(_) => {
                        let error =
                            serde_cbor::to_vec(&ServerMessage::Error(ServerError::InvalidMessage))
                                .unwrap();
                        return ctx.binary(error);
                    }
                };

                self.handle_message(msg, ctx);
            }
            ws::Message::Close(_) => {
                if let Some(_) = self.state.user_and_device_ids() {
                    self.delete(ctx);
                } else {
                    ctx.stop();
                }
            }
            ws::Message::Continuation(_) => {
                if let Some(_) = self.state.user_and_device_ids() {
                    self.delete(ctx);
                } else {
                    ctx.stop();
                }
            }
            ws::Message::Nop => (),
        }
    }
}

impl Handler<SendMessage<ServerMessage>> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, msg: SendMessage<ServerMessage>, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(msg.message);
    }
}

impl Handler<LogoutThisSession> for ClientWsSession {
    type Result = ();

    fn handle(&mut self, _: LogoutThisSession, ctx: &mut WebsocketContext<Self>) {
        ctx.binary(ServerMessage::SessionLoggedOut);
        self.delete(ctx);
    }
}

impl ClientWsSession {
    pub fn new(
        database_server: Addr<DatabaseServer>,
        config: Data<Config>,
    ) -> Self {
        ClientWsSession {
            database_server,
            communities: Vec::new(),
            state: SessionState::WaitingForLogin,
            heartbeat: Instant::now(),
            config,
        }
    }

    fn logged_in(&self) -> bool {
        self.state.user_and_device_ids().is_some()
    }

    fn start_heartbeat(&mut self, ctx: &mut WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_TIMEOUT, |session, ctx| {
            if Instant::now().duration_since(session.heartbeat) > HEARTBEAT_TIMEOUT {
                session.delete(ctx);
            }
        });
    }

    /// Remove the device from wherever it is referenced
    fn delete(&mut self, ctx: &mut WebsocketContext<Self>) {
        if let Some((user_id, device_id)) = self.state.user_and_device_ids() {
            if let Some(mut user) = USERS.get_mut(&user_id) {
                // Remove the device
                let devices = &mut user.sessions;
                if let Some(idx) = devices.iter().position(|(id, _)| *id == device_id) {
                    devices.remove(idx);

                    // Remove the entire user entry if they are no longer online
                    if devices.len() == 0 {
                        USERS.remove(&user_id);
                    }
                }
            }
        }

        ctx.stop();
    }

    /// Responds to a request with a future which will eventually resolve to the request response
    fn respond<F>(&mut self, fut: F, request_id: RequestId, ctx: &mut WebsocketContext<Self>)
    where
        F: ActorFuture<Output = Result<RequestResponse, MailboxError>, Actor = Self> + 'static,
    {
        fut.then(move |response, _act, ctx| {
            let response = ServerMessage::Response {
                response: match response {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Actix mailbox error: {:#?}", e);
                        RequestResponse::Error(ServerError::Internal)
                    }
                },
                request_id,
            };

            ctx.binary(response);
            fut::ready(())
        }).wait(ctx);
    }

    fn respond_error(
        &mut self,
        error: ServerError,
        id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        ctx.binary(ServerMessage::Response {
            response: RequestResponse::Error(error),
            request_id: id,
        });
    }

    fn handle_message(&mut self, req: ClientRequest, ctx: &mut WebsocketContext<Self>) {
        match req.message {
            ClientMessage::Login { device_id, token } => {
                self.login(device_id, token, req.request_id, ctx)
            }
            ClientMessage::CreateToken {
                username,
                password,
                device_name,
                expiration_date,
                permission_flags,
            } => self.create_token(
                username,
                password,
                device_name,
                expiration_date,
                permission_flags,
                req.request_id,
                ctx,
            ),
            ClientMessage::CreateUser {
                username,
                display_name,
                password,
            } => self.create_user(username, display_name, password, req.request_id, ctx),
            ClientMessage::RefreshToken {
                device_id,
                username,
                password,
            } => self.refresh_token(device_id, username, password, req.request_id, ctx),
            m => self.handle_authenticated_message(m, req.request_id, ctx),
        };
    }

    fn handle_authenticated_message(
        &mut self,
        msg: ClientMessage,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        match self.state {
            SessionState::WaitingForLogin => self.respond(
                futures::future::ok(RequestResponse::Error(ServerError::NotLoggedIn))
                    .into_actor(self),
                request_id,
                ctx,
            ),
            SessionState::Ready(user_id, device_id, perms) => match msg {
                ClientMessage::SendMessage(msg) => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::EditMessage(edit) => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::JoinCommunity(community) => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::CreateCommunity { name } => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::RevokeToken {
                    device_id: to_revoke,
                    password,
                } => self.revoke_token(to_revoke, password, user_id, device_id, request_id, ctx),
                ClientMessage::ChangeUsername { new_username } => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::ChangeDisplayName { new_display_name } => {
                    unimplemented!() // TODO(implement)
                }
                ClientMessage::ChangePassword {
                    old_password,
                    new_password,
                } => self.change_password(old_password, new_password, user_id, request_id, ctx),
                _ => unreachable!(),
            },
        }
    }

    fn login(
        &mut self,
        device_id: DeviceId,
        login_token: AuthToken,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn create_token(
        &mut self,
        username: String,
        password: String,
        device_name: Option<String>,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn revoke_token(
        &mut self,
        to_revoke: DeviceId,
        password: Option<String>,
        user_id: UserId,
        current_device_id: DeviceId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn refresh_token(
        &mut self,
        to_refresh: DeviceId,
        username: String,
        password: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn create_user(
        &mut self,
        username: String,
        display_name: String,
        password: String,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn change_username(
        &mut self,
        new_username: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn change_display_name(
        &mut self,
        new_display_name: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn change_password(
        &mut self,
        old_password: String,
        new_password: String,
        user_id: UserId,
        request_id: RequestId,
        ctx: &mut WebsocketContext<Self>,
    ) {
        unimplemented!() // TODO(implement)
    }

    fn create_community(&mut self, user_id: UserId, device_id: DeviceId, community_name: String, request_id: RequestId, ctx: &mut WebsocketContext<Self>) {
        unimplemented!() // TODO(implement)
    }

    fn join_community(&mut self, user_id: UserId, device_id: DeviceId, community: CommunityId, request_id: RequestId, ctx: &mut WebsocketContext<Self>) {
        unimplemented!() // TODO(implement)
    }

    async fn verify_user_id_password(
        &mut self,
        user_id: UserId,
        password: String,
    ) -> Result<(), ServerError> {
        unimplemented!() // TODO(implement)
    }

    async fn verify_username_password(
        &mut self,
        username: String,
        password: String,
    ) -> Result<UserId, ServerError> {
        unimplemented!() // TODO(implement)
    }
}
