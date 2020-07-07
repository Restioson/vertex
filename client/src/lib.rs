use async_trait::async_trait;
use embed::{EmbedCache, MessageEmbed};
pub use error::Error;
use event_handler::{EventHandlerActor, HandlerMessage};
use net::{Network, Ready, SendRequest, NetworkMessage};
use profile_cache::{ProfileCache, ProfileResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;
use vertex::prelude::*;
use xtra::prelude::*;
use auth::AuthenticatedWsStream;
use futures::stream::{SplitSink, SplitStream};
use tungstenite::Message as WsMessage;
use futures::StreamExt;

mod auth;
mod embed;
mod error;
mod event_handler;
mod net;
mod profile_cache;
mod message_cache;

pub use auth::AuthClient;
pub use event_handler::EventHandler;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthParameters {
    pub instance: Url,
    pub device: DeviceId,
    pub token: AuthToken,
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
impl Actor for Client {}

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

pub struct ClientBuilder {
    sink: SplitSink<AuthenticatedWsStream, WsMessage>,
    stream: SplitStream<AuthenticatedWsStream>,
    ready: ClientReady,
    device: DeviceId,
}

impl ClientBuilder {
    pub async fn start_with_handler<H>(
        self,
        handler: H,
    ) -> Client
        where H: EventHandler + Send + 'static
    {
        let (address, actor) = EventHandlerActor::new(handler).create();
        tokio::spawn(actor.manage());

        Client::start_with_actor(self, address).await
    }

    pub async fn start_with_actor<A>(
        self,
        address: Address<A>,
    ) -> Client
        where A: Handler<HandlerMessage>
    {
        Client::start_with_actor(self, address).await
    }
}

#[spaad::entangled]
impl Client {
    pub async fn connect(parameters: AuthParameters, bot: bool) -> Result<ClientBuilder> {
        let auth = auth::AuthClient::new(parameters.instance)?;
        let device = parameters.device;
        let (stream, sink, ready) = auth.login(parameters.device, parameters.token, bot).await?;

        Ok(ClientBuilder {
            stream,
            sink,
            ready,
            device,
        })
    }

    async fn start_with_actor<A>(
        builder: ClientBuilder,
        address: Address<A>,
    ) -> crate::Client
        where A: Handler<HandlerMessage>
    {
        let ClientBuilder { sink, mut stream, ready, device } = builder;
        let (network, actor) = Network::new(sink).create();
        tokio::spawn(actor.manage());

        let network_weak = network.downgrade();
        tokio::spawn(async move {
            while let Some(m) = stream.next().await {
                if network_weak.do_send(NetworkMessage(m)).is_err() {
                    return;
                }
            }
        });

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
            device,
            network: network.clone(),
            profiles,
            embeds,
            communities,
            admin_perms: ready.admin_permissions,
            handler: address.channel(),
        };

        let (client_address, mgr) = client.create();

        network.do_send(Ready(client_address.downgrade())).unwrap();
        address.do_send(HandlerMessage::Ready(client_address.clone().into())).unwrap();
        tokio::spawn(mgr.manage());
        client_address.into()
    }

    #[spaad::handler]
    pub async fn user(&self) -> UserId {
        self.id
    }

    #[spaad::handler]
    pub async fn send_message(
        &self,
        to_community: CommunityId,
        to_room: RoomId,
        content: String,
    ) -> Result<MessageConfirmation> {
        let req = ClientRequest::SendMessage(ClientSentMessage {
            to_community,
            to_room,
            content
        });
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect! {
            if let OkResponse::ConfirmMessage(confirmation) = response {
                Ok(confirmation)
            }
        }
    }

    #[spaad::handler]
    pub async fn create_community(&self, name: String) -> Result<CommunityStructure> {
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

    #[spaad::handler]
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

    #[spaad::handler]
    pub async fn get_community(&self, id: CommunityId) -> Option<Community> {
        self.communities.get(&id).cloned()
    }

    #[spaad::handler]
    pub async fn select_room(&self, community: CommunityId, room: RoomId) -> Result<()> {
        let req = ClientRequest::SelectRoom { community, room };
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    #[spaad::handler]
    pub async fn deselect_room(&self) -> Result<()> {
        let req = ClientRequest::DeselectRoom;
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    #[spaad::handler]
    pub async fn log_out(&self) -> Result<()> {
        let req = ClientRequest::LogOut;
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    #[spaad::handler]
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

    #[spaad::handler]
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

    #[spaad::handler]
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

    #[spaad::handler]
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

    #[spaad::handler]
    pub async fn ban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Ban(user))
        })
        .await
    }

    #[spaad::handler]
    pub async fn unban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Unban(user))
        })
        .await
    }

    #[spaad::handler]
    pub async fn unlock_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Unlock(user))
        })
        .await
    }

    #[spaad::handler]
    pub async fn demote_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(users, |user| {
            ClientRequest::AdminAction(AdminRequest::Demote(user))
        })
        .await
    }

    #[spaad::handler]
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

    #[spaad::handler]
    pub async fn report_message(
        &self,
        message: MessageId,
        short_desc: String,
        extended_desc: String,
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

    #[spaad::handler]
    pub async fn set_report_status(&self, id: i32, status: ReportStatus) -> Result<()> {
        let req = ClientRequest::AdminAction(AdminRequest::SetReportStatus { id, status });
        let req = self.network.send(SendRequest(req)).await.unwrap()?;
        let response = req.response().await?;
        expect!(if let OkResponse::NoData = response {
            Ok(())
        })
    }

    #[spaad::handler]
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
    async fn handle(&mut self, event: Event, _ctx: &mut Context<Self>) {
        let _ = self.handler.do_send(HandlerMessage::Event(event.0));
    }
}
