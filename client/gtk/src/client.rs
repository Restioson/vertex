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

use vertex::prelude::*;
use crate::{net, scheduler, screen, SharedMut, WeakSharedMut, window};
use crate::{Error, Result};

mod community;
mod room;
mod user;
mod message;
mod profile;
mod chat;
mod notification;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

// TODO: This is approaching an MVC-like design. We should fully embrace this in terms of naming
//        to make code more easily understandable in that it's a commonly understood pattern.
//       We could also potentially move towards the view always calling the controller, rather
//         than the controller calling the view. Stuff like events can be done by async polling.
//         Do need to properly consider this, though. A problematic case in doing that could be e.g.
//         message adding in the controller; this has to reference the view and thus having multiple
//         references. Then again, should this code be in the controller or in the view?
pub trait ClientUi: Sized + Clone + 'static {
    type CommunityEntryWidget: CommunityEntryWidget<Self>;
    type RoomEntryWidget: RoomEntryWidget<Self>;

    type ChatWidget: ChatWidget<Self>;

    type MessageEntryWidget: MessageEntryWidget<Self>;

    fn bind_events(&self, client: &Client<Self>);

    fn select_room(&self, room: &RoomEntry<Self>) -> Self::ChatWidget;

    fn deselect_room(&self);

    fn add_community(&self, name: String, description: String) -> Self::CommunityEntryWidget;

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
    pub chat: Option<Chat<Ui>>,
    selected_room: Option<RoomEntry<Ui>>,
    pub message_entry_is_empty: bool,
    pub admin_perms: AdminPermissionFlags,
}

#[derive(Clone)]
pub struct Client<Ui: ClientUi> {
    request: Rc<net::RequestSender>,

    pub ui: Ui,
    pub user: User,
    pub profiles: ProfileCache,
    pub embeds: EmbedCache,

    notifier: Notifier,

    abort_handle: AbortHandle,

    pub state: WeakSharedMut<ClientState<Ui>>,
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
        let embeds = EmbedCache::new();

        let state = SharedMut::new(ClientState {
            communities: Vec::new(),
            chat: None,
            selected_room: None,
            message_entry_is_empty: true,
            admin_perms: ready.admin_permissions,
        });

        let (abort_signal, abort_handle) = futures::future::abortable(futures::future::pending());

        let client = Client {
            request,
            ui,
            user,
            profiles,
            embeds,
            notifier: Notifier::new(),
            abort_handle,
            state: state.downgrade(),
        };
        client.ui.deselect_room();

        client.bind_events().await;

        for community in ready.communities {
            client.add_community(community).await;
        }

        scheduler::spawn(ClientLoop {
            client: client.clone(),
            event_receiver,
            abort_signal,
            _state: state,
        }.run());

        Ok(client)
    }

    async fn bind_events(&self) {
        self.ui.bind_events(&self);
    }

    async fn handle_event(&self, event: ServerEvent) {
        match event.clone() {
            ServerEvent::AddCommunity(structure) => {
                self.add_community(structure).await;
            }
            ServerEvent::AddRoom { community, structure } => self.handle_add_room(community, structure).await,
            ServerEvent::AddMessage { community, room, message } => self.handle_add_message(community, room, message).await,
            ServerEvent::SessionLoggedOut => {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
                self.abort_handle.abort();
            }
            unexpected => println!("unhandled server event: {:?}", unexpected),
        }
    }

    async fn handle_network_err(&self, err: tungstenite::Error) {
        println!("network error: {:?}", err);

        let error = format!("{}", err);
        let screen = screen::loading::build_error(error, crate::start);
        window::set_screen(&screen);

        self.abort_handle.abort();
    }

    async fn handle_add_room(&self, community: CommunityId, room: RoomStructure) {
        if let Some(community) = self.community_by_id(community).await {
            community.add_room(room).await;
        } else {
            println!("received AddRoom for invalid community: {:?}", community);
        }
    }

    async fn handle_add_message(&self, community: CommunityId, room: RoomId, message: Message) {
        if let Some(community) = self.community_by_id(community).await {
            if let Some(room) = community.room_by_id(room).await {
                if !self.ui.window_focused() || !self.is_selected(room.community, room.id).await {
                    let profile = self.profiles.get_or_default(message.author, message.author_profile_version).await;
                    self.notifier.notify_message(
                        &profile,
                        &community.state.read().await.name,
                        &room.name,
                        message.content.as_ref().map(|s| s as &str),
                    ).await;
                }

                if let Some(chat) = self.chat_for(room.id).await {
                    chat.push(message.clone()).await;
                }

                room.push_message(message).await;

                return;
            }
        }

        println!("received message for invalid room: {:?}#{:?}", community, room);
    }

    pub async fn create_community(&self, name: &str) -> Result<CommunityEntry<Ui>> {
        let request = ClientRequest::CreateCommunity { name: name.to_owned() };
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::AddCommunity(community) => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn join_community(&self, invite: InviteCode) -> Result<CommunityEntry<Ui>> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::AddCommunity(community) => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    async fn add_community(&self, community: CommunityStructure) -> CommunityEntry<Ui> {
        let widget = self.ui.add_community(community.name.clone(), community.description.clone());

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

    pub async fn select_room(&self, room: RoomEntry<Ui>) {
        let chat = self.ui.select_room(&room);
        let chat = Chat::new(
            self.clone(),
            chat,
            room.clone(),
        ).await;

        if let Some(state) = self.state.upgrade() {
            let mut state = state.write().await;
            state.selected_room = Some(room.clone());
            state.chat = Some(chat.clone());
        }

        match room.get_updates().await {
            Ok(update) => chat.update(update).await,
            Err(err) => {
                println!("failed to get updates for room: {:?}", err);
            }
        }

        self.request.send(ClientRequest::SelectRoom {
            community: room.community,
            room: room.id,
        }).await;
    }

    pub async fn deselect_room(&self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state.write().await;
            state.selected_room = None;
            state.chat = None;
        }

        self.ui.deselect_room();

        self.request.send(ClientRequest::DeselectRoom).await;
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

    pub async fn is_selected(&self, community: CommunityId, room: RoomId) -> bool {
        match self.selected_room().await {
            Some(selected) => selected.id == room && selected.community == community,
            None => false,
        }
    }

    pub async fn chat_for(&self, room: RoomId) -> Option<Chat<Ui>> {
        match self.chat().await {
            Some(chat) if chat.accepts(room) => Some(chat),
            _ => None,
        }
    }

    pub async fn chat(&self) -> Option<Chat<Ui>> {
        match self.state.upgrade() {
            Some(state) => {
                let state = state.read().await;
                state.chat.as_ref().cloned()
            }
            None => None,
        }
    }

    pub async fn log_out(&self) {
        self.request.send(ClientRequest::LogOut).await;
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

        let receiver = Box::pin(async move {
            let mut event_receiver = event_receiver;
            while let Some(result) = event_receiver.next().await {
                let client = client.clone();
                scheduler::spawn(async move {
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
                request.net().ping().await;
                ticker.tick().await;
            }
        });

        let run = futures::future::select(receiver, keep_alive);
        futures::future::select(self.abort_signal, run).await;
    }
}
