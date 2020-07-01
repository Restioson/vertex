use std::rc::Rc;
use std::sync::Mutex;

use futures::{FutureExt, Stream, StreamExt};
use futures::future::{Abortable, AbortHandle};
use futures::channel::mpsc::{self, UnboundedSender};

pub use chat::*;
pub use community::*;
pub use message::*;
pub use notification::*;
pub use profile::*;
pub use room::*;
pub use user::*;
use vertex::prelude::*;

use crate::{config, net, scheduler, screen, SharedMut, WeakSharedMut, window};
use crate::{Error, Result};
use url::Url;
use crate::screen::active::dialog::show_generic_error;
use crate::screen::active::Ui;
use crate::screen::active::message_scroll::ScrollWidget;

mod community;
mod room;
mod user;
mod message;
mod profile;
mod chat;
mod notification;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

lazy_static::lazy_static! {
    /// Channel through which messages to the invite-listener are sent, to allow for following invite
    /// links from other apps through the `vertex://` protocol
    pub static ref INVITE_SENDER: Mutex<Option<UnboundedSender<Url>>> = Mutex::new(None);
}

// TODO: This is approaching an MVC-like design. We should fully embrace this in terms of naming
//        to make code more easily understandable in that it's a commonly understood pattern.
//       We could also potentially move towards the view always calling the controller, rather
//         than the controller calling the view. Stuff like events can be done by async polling.
//         Do need to properly consider this, though. A problematic case in doing that could be e.g.
//         message adding in the controller; this has to reference the view and thus having multiple
//         references. Then again, should this code be in the controller or in the view?

async fn client_ready<S>(event_receiver: &mut S) -> Result<ClientReady>
    where S: Stream<Item=tungstenite::Result<ServerEvent>> + Unpin
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

pub struct ClientState {
    pub communities: Vec<CommunityEntry>,
    pub chat: Option<Chat>,
    pub selected_room: Option<RoomEntry>,
    pub message_entry_is_empty: bool,
    pub admin_perms: AdminPermissionFlags,
}

#[derive(Clone)]
pub struct Client {
    request: Rc<net::RequestSender>,

    pub ui: Ui,
    pub user: User,
    pub profiles: ProfileCache,
    pub embeds: EmbedCache,

    notifier: Notifier,

    abort_handle: AbortHandle,

    pub state: WeakSharedMut<ClientState>,
    scroll: ScrollWidget,
}

impl Client {
    pub async fn start(ws: net::AuthenticatedWs, ui: Ui, https: bool) -> Result<Client> {
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
            scroll: ScrollWidget::new(),
        };
        client.ui.deselect_room();

        client.ui.bind_events(&client);

        for community in ready.communities {
            client.add_community(community).await;
        }

        scheduler::spawn(ClientLoop {
            client: client.clone(),
            https,
            event_receiver,
            abort_signal,
            _state: state,
        }.run());

        Ok(client)
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
            ServerEvent::AdminPermissionsChanged(new_perms) => {
                let state = self.state.upgrade().unwrap();
                state.write().await.admin_perms = new_perms;
            }
            unexpected => log::warn!("unhandled server event: {:?}", unexpected),
        }
    }

    async fn handle_network_err(&self, err: tungstenite::Error) {
        log::warn!("network error: {:?}", err);

        let error = format!("{}", err);
        let screen = screen::loading::build_error(error, crate::start);
        window::set_screen(&screen);

        self.abort_handle.abort();
    }

    async fn handle_add_room(&self, community: CommunityId, room: RoomStructure) {
        if let Some(community) = self.community_by_id(community).await {
            community.add_room(room).await;
        } else {
            log::warn!("received AddRoom for invalid community: {:?}", community);
        }
    }

    async fn handle_add_message(&self, community: CommunityId, room: RoomId, message: Message) {
        if let Some(community) = self.community_by_id(community).await {
            if let Some(room) = community.room_by_id(room).await {
                let focused = self.ui.window_focused();
                let selected = self.is_selected(room.community, room.id).await;

                // Read it out if looking at the room, but in short form
                let a11y_narration = focused && selected && config::get().narrate_new_messages;

                if (!focused || !selected) || a11y_narration {
                    let profile = self.profiles.get_or_default(message.author, message.author_profile_version).await;
                    self.notifier.notify_message(
                        &profile,
                        &community.state.read().await.name,
                        &room.name,
                        message.content.as_ref().map(|s| s as &str),
                        a11y_narration,
                    ).await;
                }

                if let Some(chat) = self.chat_for(room.id).await {
                    chat.push(message.clone()).await;
                }

                room.push_message(message).await;

                return;
            }
        }

        log::warn!("received message for invalid room: {:?}#{:?}", community, room);
    }

    pub async fn create_community(&self, name: &str) -> Result<CommunityEntry> {
        let request = ClientRequest::CreateCommunity { name: name.to_owned() };
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::AddCommunity(community) => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn join_community(&self, invite: InviteCode) -> Result<CommunityEntry> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::AddCommunity(community) => Ok(self.add_community(community).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    async fn add_community(&self, community: CommunityStructure) -> CommunityEntry {
        let widget = self.ui.add_community(community.name.clone(), community.description.clone());

        let entry: CommunityEntry = CommunityEntry::new(
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

    pub async fn community_by_id(&self, id: CommunityId) -> Option<CommunityEntry> {
        match self.state.upgrade() {
            Some(state) => {
                state.read().await.communities.iter()
                    .find(|&community| community.id == id)
                    .cloned()
            }
            None => None,
        }
    }

    pub async fn select_room(&self, room: RoomEntry) {
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
                log::warn!("failed to get updates for room: {:?}", err);
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

    pub async fn selected_community(&self) -> Option<CommunityEntry> {
        match self.selected_room().await {
            Some(room) => self.community_by_id(room.community).await,
            None => None,
        }
    }

    pub async fn selected_room(&self) -> Option<RoomEntry> {
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

    pub async fn chat_for(&self, room: RoomId) -> Option<Chat> {
        match self.chat().await {
            Some(chat) if chat.accepts(room) => Some(chat),
            _ => None,
        }
    }

    pub async fn chat(&self) -> Option<Chat> {
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

    pub async fn search_users(&self, name: String) -> Result<Vec<ServerUser>> {
        let req = ClientRequest::AdminAction(AdminRequest::SearchUser { name });
        let req = self.request.send(req).await;

        match req.response().await? {
            OkResponse::Admin(AdminResponse::SearchedUsers(users)) => Ok(users),
            _ => Err(Error::UnexpectedMessage)
        }
    }

    pub async fn list_all_server_users(&self) -> Result<Vec<ServerUser>> {
        let req = ClientRequest::AdminAction(AdminRequest::ListAllUsers);
        let req = self.request.send(req).await;

        match req.response().await? {
            OkResponse::Admin(AdminResponse::SearchedUsers(users)) => Ok(users),
            _ => Err(Error::UnexpectedMessage)
        }
    }

    pub async fn list_all_admins(&self) -> Result<Vec<Admin>> {
        let req = ClientRequest::AdminAction(AdminRequest::ListAllAdmins);
        let req = self.request.send(req).await;

        match req.response().await? {
            OkResponse::Admin(AdminResponse::Admins(admins)) => Ok(admins),
            _ => Err(Error::UnexpectedMessage)
        }
    }

    pub async fn search_reports(&self, criteria: SearchCriteria) -> Result<Vec<Report>> {
        let req = ClientRequest::AdminAction(AdminRequest::SearchForReports(criteria));
        let req = self.request.send(req).await;

        match req.response().await? {
            OkResponse::Admin(AdminResponse::Reports(reports)) => Ok(reports),
            _ => Err(Error::UnexpectedMessage)
        }
    }

    async fn do_to_many(
        &self,
        users: Vec<UserId>,
        req: impl Fn(UserId) -> ClientRequest,
    ) -> Result<Vec<(UserId, Error)>> {
        let mut results = Vec::new();
        for user in users {
            let req = self.request.send(req(user)).await;

             match req.response().await {
                Ok(OkResponse::NoData) => {},
                Ok(_) => results.push((user, Error::UnexpectedMessage)),
                Err(e @ Error::ErrorResponse(_)) => results.push((user, e)),
                Err(e) => return Err(e),
            };
        }

        Ok(results)
    }

    pub async fn ban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(
            users,
            |user| ClientRequest::AdminAction(AdminRequest::Ban(user))
        ).await
    }

    pub async fn unban_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(
            users,
            |user| ClientRequest::AdminAction(AdminRequest::Unban(user))
        ).await
    }

    pub async fn unlock_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(
            users,
            |user| ClientRequest::AdminAction(AdminRequest::Unlock(user))
        ).await
    }

    pub async fn demote_users(&self, users: Vec<UserId>) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(
            users,
            |user| ClientRequest::AdminAction(AdminRequest::Demote(user))
        ).await
    }

    pub async fn promote_users(
        &self,
        users: Vec<UserId>,
        permissions: AdminPermissionFlags
    ) -> Result<Vec<(UserId, Error)>> {
        self.do_to_many(
            users,
            |user| ClientRequest::AdminAction(AdminRequest::Promote { user, permissions })
        ).await
    }

    pub async fn report_message(
        &self,
        message: MessageId,
        short_desc: &str,
        extended_desc: &str,
    ) -> Result<()> {
        let request = ClientRequest::ReportUser {
            message,
            short_desc: short_desc.to_string(),
            extended_desc: extended_desc.to_string(),
        };
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn set_report_status(&self, id: i32, status: ReportStatus) -> Result<()> {
        let request = ClientRequest::AdminAction(AdminRequest::SetReportStatus {
            id,
            status,
        });
        let request = self.request.send(request).await;
        match request.response().await? {
            OkResponse::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn set_compromised(&self, typ: SetCompromisedType) -> Result<()> {
        let request = ClientRequest::AdminAction(AdminRequest::SetAccountsCompromised(typ));
        let request = self.request.send(request).await;
        match request.response().await? {
            OkResponse::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }
}

struct ClientLoop<S> {
    client: Client,
    https: bool,
    event_receiver: S,
    abort_signal: Abortable<futures::future::Pending<()>>,
    _state: SharedMut<ClientState>,
}

impl<S> ClientLoop<S>
    where S: Stream<Item=tungstenite::Result<ServerEvent>> + Unpin
{
    async fn run(self) {
        let client = &self.client;
        let https = self.https;
        let event_receiver = self.event_receiver;

        let request = client.request.clone();

        let mut receiver = Box::pin(
            async move {
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
            }.fuse()
        );

        let mut keep_alive = Box::pin(
            async move {
                let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
                loop {
                    request.net().ping().await;
                    ticker.tick().await;
                }
            }.fuse()
        );

        let (invite_tx, mut invite_rx) = mpsc::unbounded();
        *INVITE_SENDER.lock().unwrap() = Some(invite_tx);

        let mut invite_listener = Box::pin(
            async move {
                while let Some(url) = invite_rx.next().await {
                    // Workaround for local dev with http - users should never use http anyway...
                    let scheme = if https {
                        "https"
                    } else {
                        "http"
                    };
                    // https://github.com/servo/rust-url/issues/577#issuecomment-572756577
                    let url = [scheme, &url[url::Position::AfterScheme..]].join("");
                    let meta = get_link_metadata(&url).await.map(|m| m.invite);
                    if let Ok(Some(inv)) = meta {
                        if let Err(err) = client.clone().join_community(inv.code).await {
                            show_generic_error(&err);
                        }
                    } else if let Err(e) = meta {
                        log::warn!("{:?}", e);
                    }
                }
            }.fuse()
        );

        futures::select! {
            _ = keep_alive => {},
            _ = invite_listener => {},
            _ = receiver => {},
            _ = self.abort_signal.fuse() => {}
        }
    }
}
