use std::future::Future;
use std::rc::Rc;

use futures::{Stream, StreamExt};
use futures::channel::oneshot;

pub use community::*;
pub use message::*;
pub use room::*;
pub use user::*;
use vertex::*;

use crate::{net, UiEntity, WeakUiEntity};

mod community;
mod room;
mod user;
mod message;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

pub trait ClientUi: Sized {
    type CommunityEntryWidget: CommunityEntryWidget<Self>;
    type RoomEntryWidget: RoomEntryWidget<Self>;

    type MessageListWidget: MessageListWidget<Self>;
    type MessageEntryWidget: MessageEntryWidget<Self>;

    fn add_community(&self, name: String) -> Self::CommunityEntryWidget;

    fn build_message_list(&self) -> Self::MessageListWidget;
}

pub struct Client<Ui: ClientUi + 'static> {
    request: Rc<net::RequestSender>,

    pub ui: Ui,
    pub user: UiEntity<User>,
    pub message_list: UiEntity<MessageList<Ui>>,

    pub communities: Vec<UiEntity<CommunityEntry<Ui>>>,
    selected_community: Option<usize>,
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

impl<Ui: ClientUi + 'static> Client<Ui> {
    pub async fn start(ws: net::AuthenticatedWs, ui: Ui) -> Result<UiEntity<Client<Ui>>> {
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

        let client = UiEntity::new(Client {
            ui,
            request: request.clone(),
            user,
            message_list,
            communities: vec![], // TODO
            selected_community: None,
        });

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(ClientLoop {
            client: client.downgrade(),
            request: request.clone(),
            event_receiver,
        }.run());

        Ok(client)
    }

    pub async fn handle_event(&mut self, event: ServerEvent) {
        // TODO
        match event {
            ServerEvent::Message(message) => {}
            unexpected => println!("unhandled server event: {:?}", unexpected),
        }
    }

    pub async fn handle_err(&mut self, err: net::Error) {
        println!("server error: {:?}", err);
    }

    pub async fn create_community(&mut self, name: &str) -> Result<UiEntity<CommunityEntry<Ui>>> {
        let request = ClientRequest::CreateCommunity { name: name.to_owned() };
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community)),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn join_community(&mut self, invite: InviteCode) -> Result<UiEntity<CommunityEntry<Ui>>> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community)),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    fn add_community(&mut self, community: CommunityStructure) -> UiEntity<CommunityEntry<Ui>> {
        let widget = self.ui.add_community(community.name.clone());

        let entry: UiEntity<CommunityEntry<Ui>> = CommunityEntry::new(
            self.request.clone(),
            self.user.clone(),
            self.message_list.clone(),
            widget,
            community.id,
            community.name,
        );

        &entry.borrow().widget.bind_events(&entry);

        for room in community.rooms {
            entry.borrow_mut().add_room(room);
        }

        self.communities.push(entry);
        self.communities.last().unwrap().clone()
    }

    pub fn select_community(&mut self, index: Option<usize>) {
        self.selected_community = index;
    }

    pub fn selected_community(&self) -> Option<&UiEntity<CommunityEntry<Ui>>> {
        self.selected_community.and_then(move |idx| self.communities.get(idx))
    }

    pub async fn log_out(&self) -> Result<()> {
        let request = self.request.send(ClientRequest::LogOut).await?;
        request.response().await?;
        Ok(())
    }
}

struct ClientLoop<Ui: ClientUi + 'static, S> {
    client: WeakUiEntity<Client<Ui>>,
    request: Rc<net::RequestSender>,
    event_receiver: S,
}

impl<Ui: ClientUi, S> ClientLoop<Ui, S>
    where S: Stream<Item = net::Result<ServerEvent>> + Unpin
{
    async fn run(self) {
        let ClientLoop { client, request, event_receiver } = self;

        let receiver = Box::pin(async move {
            let mut event_receiver = event_receiver;
            while let Some(result) = event_receiver.next().await {
                if let Some(client) = client.upgrade() {
                    let mut client = client.borrow_mut();
                    match result {
                        Ok(event) => client.handle_event(event).await,
                        Err(err) => client.handle_err(err).await,
                    }
                } else {
                    break;
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
