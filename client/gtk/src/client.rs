use std::rc::Rc;

use futures::{Stream, StreamExt};

pub use community::*;
pub use message::*;
pub use room::*;
pub use user::*;
use vertex::*;

use crate::{net, SharedMut};

mod community;
mod room;
mod user;
mod message;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

pub trait ClientUi: Sized + Clone + 'static {
    type CommunityEntryWidget: CommunityEntryWidget<Self>;
    type RoomEntryWidget: RoomEntryWidget<Self>;

    type MessageListWidget: MessageListWidget<Self>;
    type MessageEntryWidget: MessageEntryWidget<Self>;

    fn add_community(&self, name: String) -> Self::CommunityEntryWidget;

    fn build_message_list(&self) -> Self::MessageListWidget;
}

#[derive(Copy, Clone, Debug)]
pub struct RoomIndex {
    pub community: usize,
    pub room: usize,
}

async fn client_ready<S>(event_receiver: &mut S) -> Result<ClientReady>
    where S: Stream<Item = net::Result<ServerEvent>> + Unpin
{
    if let Some(result) = event_receiver.next().await {
        let event = result?;
        match event {
            ServerEvent::ClientReady(ready) => Ok(ready),
            _ => Err(Error::UnexpectedMessage),
        }
    } else {
        Err(Error::Net(net::Error::Closed))
    }
}

pub struct ClientState<Ui: ClientUi> {
    pub communities: Vec<CommunityEntry<Ui>>,

    selected_room: Option<RoomIndex>,
}

#[derive(Clone)]
pub struct Client<Ui: ClientUi> {
    request: Rc<net::RequestSender>,

    pub ui: Ui,
    pub user: User,
    pub message_list: MessageList<Ui>,

    state: SharedMut<ClientState<Ui>>,
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
            ready.username,
            ready.display_name,
            ws.device,
            ws.token,
        );

        let message_list = MessageList::new(ui.build_message_list());

        let state = SharedMut::new(ClientState {
            communities: vec![], // TODO
            selected_room: None,
        });

        let client = Client { request, ui, user, message_list, state };

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(ClientLoop {
            client: client.clone(),
            event_receiver,
        }.run());

        Ok(client)
    }

    pub async fn handle_event(&self, event: ServerEvent) {
        // TODO
        match event {
            unexpected => println!("unhandled server event: {:?}", unexpected),
        }
    }

    pub async fn handle_err(&self, err: net::Error) {
        println!("server error: {:?}", err);
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
            self.request.clone(),
            self.user.clone(),
            self.message_list.clone(),
            widget,
            community.id,
            community.name,
        );

        entry.widget.bind_events(&entry);

        for room in community.rooms {
            entry.add_room(room).await;
        }

        let mut state = self.state.write().await;
        state.communities.push(entry);
        state.communities.last().unwrap().clone()
    }

    pub async fn select_room(&self, index: Option<RoomIndex>) {
        let mut state = self.state.write().await;
        state.selected_room = index;
    }

    pub async fn selected_room(&self) -> Option<RoomEntry<Ui>> {
        let state = self.state.read().await;
        match state.selected_room {
            Some(RoomIndex { community, room }) => {
                if let Some(community) = state.communities.get(community) {
                    return community.get_room(room).await.clone();
                }
            }
            _ => (),
        }
        None
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
}

impl<Ui: ClientUi, S> ClientLoop<Ui, S>
    where S: Stream<Item = net::Result<ServerEvent>> + Unpin
{
    // TODO: we need to be able to signal this to exit!
    async fn run(self) {
        let ClientLoop { client, event_receiver } = self;
        let request = client.request.clone();

        let receiver = Box::pin(async move {
            let mut event_receiver = event_receiver;
            while let Some(result) = event_receiver.next().await {
                match result {
                    Ok(event) => client.handle_event(event).await,
                    Err(err) => client.handle_err(err).await,
                }
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

        futures::future::select(receiver, keep_alive).await;
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Net(net::Error),
    Response(ErrResponse),
    UnexpectedMessage,
}

impl From<net::Error> for Error {
    fn from(net: net::Error) -> Self {
        Error::Net(net)
    }
}

impl From<ErrResponse> for Error {
    fn from(response: ErrResponse) -> Self {
        Error::Response(response)
    }
}
