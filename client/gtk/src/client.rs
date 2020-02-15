use std::rc::Rc;

use futures::{Stream, StreamExt};
use futures::future::{Abortable, AbortHandle};

pub use chat::*;
pub use community::*;
pub use message::*;
pub use notification::*;
pub use profile::*;
pub use room::*;
pub use user::*;
use vertex::*;

use crate::{net, SharedMut, WeakSharedMut};
use crate::{Error, Result};

mod community;
mod room;
mod user;
mod message;
mod profile;
mod chat;
mod notification;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

pub trait ClientUi: Sized + Clone + 'static {
    type CommunityEntryWidget: CommunityEntryWidget<Self>;
    type RoomEntryWidget: RoomEntryWidget<Self>;

    type ChatWidget: ChatWidget<Self>;

    type MessageEntryWidget: MessageEntryWidget<Self>;

    fn bind_events(&self, client: &Client<Self>);

    fn add_community(&self, name: String) -> Self::CommunityEntryWidget;
    fn build_chat_widget(&self) -> Self::ChatWidget;
    fn window_focused(&self) -> bool;
}

async fn client_ready<S>(event_receiver: &mut S) -> Result<ClientReady>
    where S: Stream<Item = tungstenite::Result<ServerEvent>> + Unpin
{
    if let Some(result) = event_receiver.next().await {
        let event = result?;
        match event {
            ServerEvent::ClientReady(ready) => Ok(ready),
            _ => Err(Error::UnexpectedMessage),
        }
    } else {
        Err(Error::Websocket(tungstenite::Error::ConnectionClosed))
    }
}

pub struct ClientState<Ui: ClientUi> {
    pub communities: Vec<CommunityEntry<Ui>>,
    selected_room: Option<RoomEntry<Ui>>,
}

#[derive(Clone)]
pub struct Client<Ui: ClientUi> {
    request: Rc<net::RequestSender>,

    pub ui: Ui,
    pub user: User,
    pub profiles: ProfileCache,
    pub chat: Chat<Ui>,

    notifier: Notifier,

    abort_handle: AbortHandle,

    state: WeakSharedMut<ClientState<Ui>>,
}

impl<Ui: ClientUi> Client<Ui> {
    pub async fn start(ws: net::AuthenticatedWs, ui: Ui) -> Result<Client<Ui>> {
        let (sender, receiver) = net::from_ws(ws.stream);

        let req_manager = net::RequestManager::new();

        let request = req_manager.sender(sender);
        let request = Rc::new(request);

        let mut event_receiver = req_manager.receive_from(receiver);

        let ready = client_ready(&mut event_receiver).await?;

        let user = User::new(
            request.clone(),
            ready.user,
            ready.profile,
            ws.device,
            ws.token,
        );

        let profiles = ProfileCache::new(request.clone(), user.clone());

        let chat = Chat::new(ui.build_chat_widget());
        chat.set_room(None).await;

        let state = SharedMut::new(ClientState {
            communities: Vec::new(),
            selected_room: None,
        });

        let (abort_signal, abort_handle) = futures::future::abortable(futures::future::pending());

        let client = Client {
            request,
            ui,
            user,
            profiles,
            chat,
            notifier: Notifier::new(),
            abort_handle,
            state: state.downgrade(),
        };

        client.bind_events().await;

        for community in ready.communities {
            client.add_community(community).await;
        }

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(ClientLoop {
            client: client.clone(),
            event_receiver,
            abort_signal,
            _state: state,
        }.run());

        Ok(client)
    }

    async fn bind_events(&self) {
        self.ui.bind_events(&self);
        self.chat.bind_events(&self).await;
    }

    async fn handle_event(&self, event: ServerEvent) {
        match event.clone() {
            ServerEvent::AddCommunity(structure) => {
                self.add_community(structure).await;
            }
            ServerEvent::AddRoom { community, structure } => {
                if let Some(community) = self.community_by_id(community).await {
                    community.add_room(structure).await;
                } else {
                    println!("received AddRoom for invalid community: {:?}", community);
                }
            }
            ServerEvent::AddMessage(message) => {
                let room = match self.community_by_id(message.community).await {
                    Some(community) => community.room_by_id(message.room).await,
                    None => None,
                };

                if let Some(room) = room {
                    room.add_message(MessageSource {
                        author: message.author,
                        author_profile_version: Some(message.author_profile_version),
                        content: message.content,
                    }).await;

                    if !self.ui.window_focused() || self.selected_room().await != Some(room) {
                        self.notifier.send(&event).await;
                    }
                } else {
                    println!("received message for invalid room: {:?}#{:?}", message.community, message.room);
                }
            }
            ServerEvent::SessionLoggedOut => {
                self.abort_handle.abort();
                println!("session logged out");
            }
            unexpected => println!("unhandled server event: {:?}", unexpected),
        }
    }

    async fn handle_network_err(&self, err: tungstenite::Error) {
        println!("network error: {:?}", err);
        self.abort_handle.abort();
    }

    pub async fn create_community(&self, name: &str) -> Result<CommunityEntry<Ui>> {
        let request = ClientRequest::CreateCommunity { name: name.to_owned() };
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn join_community(&self, invite: InviteCode) -> Result<CommunityEntry<Ui>> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    async fn add_community(&self, community: CommunityStructure) -> CommunityEntry<Ui> {
        let widget = self.ui.add_community(community.name.clone());

        let entry: CommunityEntry<Ui> = CommunityEntry::new(
            self.clone(),
            widget,
            community.id,
            community.name,
        );

        entry.widget.bind_events(&entry);

        for room in community.rooms {
            entry.add_room(room).await;
        }

        if let Some(state) = self.state.upgrade() {
            let mut state = state.write().await;
            state.communities.push(entry);
            state.communities.last().unwrap().clone()
        } else {
            entry
        }
    }

    pub async fn community_by_id(&self, id: CommunityId) -> Option<CommunityEntry<Ui>> {
        match self.state.upgrade() {
            Some(state) => {
                state.read().await.communities.iter()
                    .find(|&community| community.id == id)
                    .cloned()
            }
            None => None,
        }
    }

    pub async fn select_room(&self, room: Option<RoomEntry<Ui>>) -> Result<()> {
        self.set_looking_at(room.as_ref()).await?;

        if let Some(state) = self.state.upgrade() {
            let mut state = state.write().await;
            self.chat.set_room(room.as_ref()).await;
            state.selected_room = room;
        }

        Ok(())
    }

    async fn set_looking_at(&self, room: Option<&RoomEntry<Ui>>) -> Result<()> {
        let request = self.request.send(ClientRequest::SetLookingAt(
            room.map(|room| (room.community, room.id))
        )).await?;

        request.response().await?;
        Ok(())
    }

    pub async fn selected_community(&self) -> Option<CommunityEntry<Ui>> {
        match self.selected_room().await {
            Some(room) => self.community_by_id(room.community).await,
            None => None,
        }
    }

    pub async fn selected_room(&self) -> Option<RoomEntry<Ui>> {
        match self.state.upgrade() {
            Some(state) => {
                let state = state.read().await;
                state.selected_room.as_ref().cloned()
            }
            None => None,
        }
    }

    pub async fn log_out(&self) -> Result<()> {
        let request = self.request.send(ClientRequest::LogOut).await?;
        request.response().await?;
        Ok(())
    }
}

struct ClientLoop<Ui: ClientUi, S> {
    client: Client<Ui>,
    event_receiver: S,
    abort_signal: Abortable<futures::future::Pending<()>>,
    _state: SharedMut<ClientState<Ui>>,
}

impl<Ui: ClientUi, S> ClientLoop<Ui, S>
    where S: Stream<Item = tungstenite::Result<ServerEvent>> + Unpin
{
    async fn run(self) {
        let client = self.client;
        let event_receiver = self.event_receiver;

        let request = client.request.clone();

        let main_ctx = glib::MainContext::ref_thread_default();

        let receiver = Box::pin(async move {
            let mut event_receiver = event_receiver;
            while let Some(result) = event_receiver.next().await {
                let client = client.clone();
                main_ctx.spawn_local(async move {
                    match result {
                        Ok(event) => client.handle_event(event).await,
                        Err(err) => client.handle_network_err(err).await,
                    }
                });
            }
        });

        let keep_alive = Box::pin(async move {
            let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
            loop {
                if let Err(_) = request.net().ping().await {
                    break;
                }
                ticker.tick().await;
            }
        });

        let run = futures::future::select(receiver, keep_alive);
        futures::future::select(self.abort_signal, run).await;
    }
}
