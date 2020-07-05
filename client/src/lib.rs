use async_trait::async_trait;
use embed::{EmbedCache, MessageEmbed};
pub use error::Error;
use futures::{Future, StreamExt};
use net::{Network, SendRequest};
use profile_cache::{ProfileCache, ProfileResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tungstenite::{Error as WsError, Message as WsMessage};
use url::Url;
use vertex::prelude::{Message, *};
use xtra::prelude::*;

mod auth;
mod embed;
mod error;
mod net;
mod profile_cache;

pub use auth::AuthClient;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthParameters {
    pub instance: Url,
    pub device: DeviceId,
    pub token: AuthToken,
    pub username: String, // TODO(change_username): update
}

#[spaad::entangled]
pub struct Client {
    id: UserId,
    device: DeviceId,

    network: Address<Network>,

    profiles: ProfileCache,
    embeds: EmbedCache,

    communities: HashMap<CommunityId, Community>,
    admin_perms: AdminPermissionFlags,

    handler: MessageChannel<HandlerMessage>,
}

#[spaad::entangled]
impl Actor for Client {
    fn started(&mut self, ctx: &mut Context<Self>) {
        let client = ctx.address().unwrap().into();
        self.handler.do_send(HandlerMessage::Ready(client)).unwrap();
    }
}

#[derive(Clone)]
pub struct Community {
    pub name: String,
    pub description: String,
    pub rooms: HashMap<RoomId, Room>,
}

#[derive(Clone)]
pub struct Room {
    pub name: String,
    pub unread: bool,
}

#[spaad::entangled]
impl Client {
    pub async fn start<H: EventHandler + Send + 'static>(
        parameters: AuthParameters,
        handler: H,
    ) -> Result<(crate::Client, impl Future<Output = ()>)> {
        let auth = auth::AuthClient::new(parameters.instance)?;
        let ws = auth.login(parameters.device, parameters.token).await?;
        let (sink, mut stream) = ws.stream.split();

        let message = match stream.next().await {
            Some(Ok(WsMessage::Binary(bytes))) => ServerMessage::from_protobuf_bytes(&bytes)?,
            Some(Err(e)) => return Err(Error::Websocket(e)),
            Some(other) => {
                return Err(Error::UnexpectedMessage {
                    expected: "WsMessage::Binary",
                    got: Box::new(other),
                })
            }
            None => return Err(Error::Websocket(WsError::ConnectionClosed)),
        };

        let ready = expect! {
            if let ServerMessage::Event(ServerEvent::ClientReady(ready)) = message {
                Ok(ready)
            }
        }?;

        let (network, actor) = Network::new(sink).create();
        tokio::spawn(actor.manage());

        let (handler, actor) = EventHandlerActor(handler).create();
        tokio::spawn(actor.manage());

        let profiles = ProfileCache::new();
        let embeds = EmbedCache::new();

        let communities = ready
            .communities
            .into_iter()
            .map(|community| {
                let id = community.id;
                let rooms = community
                    .rooms
                    .into_iter()
                    .map(|room| {
                        let id = room.id;
                        let room = Room {
                            name: room.name,
                            unread: room.unread,
                        };

                        (id, room)
                    })
                    .collect();

                let community = Community {
                    name: community.name,
                    description: community.description,
                    rooms,
                };
                (id, community)
            })
            .collect();

        let client = Client {
            id: ready.user,
            device: parameters.device,
            network,
            profiles,
            embeds,
            communities,
            admin_perms: ready.admin_permissions,
            handler: handler.into_channel(),
        };

        let (addr, mgr) = client.create();
        Ok((addr.into(), mgr.manage()))
    }

    pub async fn create_community(&self, name: &str) -> Result<CommunityStructure> {
        let req = ClientRequest::CreateCommunity {
            name: name.to_owned(),
        };
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::AddCommunity(community) = response {
                Ok(community)
            }
        }
    }

    pub async fn join_community(&self, invite: InviteCode) -> Result<CommunityStructure> {
        let req = ClientRequest::JoinCommunity(invite);
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::AddCommunity(community) = response {
                Ok(community)
            }
        }
    }

    pub async fn get_community(&self, id: CommunityId) -> Option<Community> {
        self.communities.get(&id).cloned()
    }

    pub async fn select_room(&self, community: CommunityId, room: RoomId) -> Result<()> {
        let req = ClientRequest::SelectRoom { community, room };
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    pub async fn deselect_room(&self) -> Result<()> {
        let req = ClientRequest::DeselectRoom;
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    pub async fn log_out(&self) -> Result<()> {
        let req = ClientRequest::LogOut;
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    pub async fn search_users(&self, name: String) -> Result<Vec<ServerUser>> {
        let req = ClientRequest::AdminAction(AdminRequest::SearchUser { name });
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::Admin(AdminResponse::SearchedUsers(users)) = response {
                Ok(users)
            }
        }
    }

    pub async fn list_all_server_users(&self) -> Result<Vec<ServerUser>> {
        let req = ClientRequest::AdminAction(AdminRequest::ListAllUsers);
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::Admin(AdminResponse::SearchedUsers(users)) = response {
                Ok(users)
            }
        }
    }

    pub async fn list_all_admins(&self) -> Result<Vec<Admin>> {
        let req = ClientRequest::AdminAction(AdminRequest::ListAllAdmins);
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::Admin(AdminResponse::Admins(admins)) = response {
                Ok(admins)
            }
        }
    }

    pub async fn search_reports(&self, criteria: SearchCriteria) -> Result<Vec<Report>> {
        let req = ClientRequest::AdminAction(AdminRequest::SearchForReports(criteria));
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::Admin(AdminResponse::Reports(reports)) = response {
                Ok(reports)
            }
        }
    }

    async fn do_to_many(
        &self,
        users: Vec<UserId>,
        req: impl Fn(UserId) -> ClientRequest,
    ) -> Result<Vec<(UserId, Error)>> {
        let mut results = Vec::new();
        for user in users {
            let req = self.network.send(SendRequest(req(user))).await.unwrap()?;

            match req.response().await {
                Ok(OkResponse::NoData) => {}
                Ok(other) => {
                    let err = Error::UnexpectedMessage {
                        expected: "OkResponse::NoData",
                        got: Box::new(other),
                    };
                    results.push((user, err))
                }
                Err(e @ Error::ErrorResponse(_)) => results.push((user, e)),
                Err(e) => return Err(e),
            };
        }

        Ok(results)
    }

    pub async fn ban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Ban(user))
        })
        .await
    }

    pub async fn unban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Unban(user))
        })
        .await
    }

    pub async fn unlock_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Unlock(user))
        })
        .await
    }

    pub async fn demote_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Demote(user))
        })
        .await
    }

    pub async fn promote_users(
        &self,
        users: Vec<UserId>,
        permissions: AdminPermissionFlags,
    ) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Promote { user, permissions })
        })
        .await
    }

    pub async fn report_message(
        &self,
        message: MessageId,
        short_desc: &str,
        extended_desc: &str,
    ) -> Result<()> {
        let req = ClientRequest::ReportUser {
            message,
            short_desc: short_desc.to_string(),
            extended_desc: extended_desc.to_string(),
        };
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    pub async fn set_report_status(&self, id: i32, status: ReportStatus) -> Result<()> {
        let req = ClientRequest::AdminAction(AdminRequest::SetReportStatus { id, status });
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    pub async fn set_compromised(&self, typ: SetCompromisedType) -> Result<()> {
        let req = ClientRequest::AdminAction(AdminRequest::SetAccountsCompromised(typ));
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    #[spaad::handler]
    pub async fn get_profile(&mut self, user: UserId, version: ProfileVersion) -> ProfileResult {
        self.profiles.get(&self.network, user, version).await
    }

    #[spaad::handler]
    pub async fn get_embed(&mut self, url: Url) -> Option<MessageEmbed> {
        self.embeds.get(&url).await.cloned()
    }
}

#[derive(Debug)]
struct Event(Result<ServerEvent>);

impl xtra::Message for Event {
    type Result = ();
}

#[spaad::entangled]
#[async_trait]
impl Handler<Event> for Client {
    async fn handle(&mut self, event: Event, ctx: &mut Context<Self>) {
        let client = ctx.address().unwrap().into();
        self.handler
            .do_send(HandlerMessage::Event(event.0, client))
            .unwrap();
    }
}

#[async_trait]
#[allow(unused_variables)]
pub trait EventHandler {
    async fn ready(&mut self, client: Client) {}
    async fn error(&mut self, error: Error, client: Client) {}
    async fn internal_error(&mut self, client: Client) {}
    async fn add_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: Message,
        client: Client,
    ) {
    }
    async fn message_ready(&mut self, community: CommunityId, room: RoomId, client: Client) {}
    async fn edit_message(&mut self, edit: Edit, client: Client) {}
    async fn delete_message(&mut self, delete: Delete, client: Client) {}
    async fn logged_out(&mut self) {}
    async fn add_room(&mut self, community: CommunityId, room: RoomStructure, client: Client) {}
    async fn add_community(&mut self, community: CommunityStructure, client: Client) {}
    async fn remove_community(
        &mut self,
        id: CommunityId,
        reason: RemoveCommunityReason,
        client: Client,
    ) {
    }
    async fn admin_permissions_changed(&mut self, new: AdminPermissionFlags, client: Client) {}
}

struct EventHandlerActor<H: EventHandler + 'static>(H);

impl<H: EventHandler + Send + 'static> Actor for EventHandlerActor<H> {}

enum HandlerMessage {
    Event(Result<ServerEvent>, Client),
    Ready(Client),
}

impl xtra::Message for HandlerMessage {
    type Result = ();
}

#[async_trait]
impl<H: EventHandler + Send + 'static> Handler<HandlerMessage> for EventHandlerActor<H> {
    async fn handle(&mut self, msg: HandlerMessage, ctx: &mut Context<Self>) {
        use ServerEvent::*;

        let (client, event) = match msg {
            HandlerMessage::Event(Ok(event), client) => (client, event),
            HandlerMessage::Event(Err(err), client) => return self.0.error(err, client).await,
            HandlerMessage::Ready(client) => return self.0.ready(client).await,
        };

        match event {
            ClientReady(ready) => log::error!("Client sent ready at wrong time: {:#?}", ready),
            AddMessage {
                community,
                room,
                message,
            } => self.0.add_message(community, room, message, client).await,
            InternalError => self.0.internal_error(client).await,
            NotifyMessageReady { community, room } => {
                self.0.message_ready(community, room, client).await
            }
            Edit(edit) => self.0.edit_message(edit, client).await,
            Delete(delete) => self.0.delete_message(delete, client).await,
            SessionLoggedOut => {
                self.0.logged_out().await;
                ctx.stop();
            }
            AddRoom {
                community,
                structure,
            } => self.0.add_room(community, structure, client).await,
            AddCommunity(community) => self.0.add_community(community, client).await,
            RemoveCommunity { id, reason } => self.0.remove_community(id, reason, client).await,
            AdminPermissionsChanged(new) => self.0.admin_permissions_changed(new, client).await,
            other => log::error!("Unimplemented server event {:#?}", other),
        };
    }
}
