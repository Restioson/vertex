use std::rc::Rc;

use futures::{Stream, StreamExt};

pub use community::*;
pub use room::*;
pub use user::*;
use vertex::*;

use crate::{net, UiShared};

mod community;
mod room;
mod user;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

pub trait ClientUi: Sized {
    type CommunityEntryWidget: CommunityEntryWidget<Self>;
    type RoomEntryWidget: RoomEntryWidget<Self>;

    fn add_community(&self, name: String) -> Self::CommunityEntryWidget;
}

pub struct Client<Ui: ClientUi> {
    pub ui: Ui,
    net: Rc<net::RequestSender>,
    close: Option<futures::channel::oneshot::Sender<()>>,

    pub user: User,

    pub communities: Vec<UiShared<CommunityEntry<Ui>>>,
    selected_community: Option<usize>,
}

impl<Ui: ClientUi> Client<Ui> {
    pub fn spawn(ws: net::AuthenticatedWs, ui: Ui) -> UiShared<Client<Ui>> {
        let (sender, receiver) = net::from_ws(ws.stream);

        let req_manager = net::RequestManager::new();

        let req_sender = req_manager.sender(sender);
        let req_sender = Rc::new(req_sender);

        let req_receiver = req_manager.receive_from(receiver);

        let (close_send, close_recv) = futures::channel::oneshot::channel();

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(ClientLoop {
            send: req_sender.clone(),
            stream: req_receiver,
            close: close_recv,
        }.run());

        let client = Client {
            ui,
            net: req_sender.clone(),
            close: Some(close_send),

            user: User::new(
                req_sender.clone(),
                // TODO
                "You".to_string(),
                "You".to_string(),
                ws.device,
                ws.token,
            ),

            communities: vec![], // TODO
            selected_community: None,
        };

        UiShared::new(client)
    }

    pub async fn create_community(&mut self, name: &str) -> Result<&UiShared<CommunityEntry<Ui>>> {
        let request = ClientRequest::CreateCommunity { name: name.to_owned() };
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn join_community(&mut self, invite: InviteCode) -> Result<&UiShared<CommunityEntry<Ui>>> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::AddCommunity { community } => Ok(self.add_community(community)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    fn add_community(&mut self, community: CommunityStructure) -> &UiShared<CommunityEntry<Ui>> {
        let widget = self.ui.add_community(community.name.clone());

        let entry: UiShared<CommunityEntry<Ui>> = CommunityEntry::new(
            self.net.clone(),
            widget,
            community.id,
            community.name,
        );

        &entry.borrow().widget.bind_events(&entry);

        for room in community.rooms {
            entry.borrow_mut().add_room(room);
        }

        self.communities.push(entry);
        self.communities.last().unwrap()
    }

    pub fn select_community(&mut self, index: Option<usize>) {
        self.selected_community = index;
    }

    pub fn selected_community(&self) -> Option<&UiShared<CommunityEntry<Ui>>> {
        self.selected_community.and_then(move |idx| self.communities.get(idx))
    }

    pub async fn log_out(&self) -> Result<()> {
        let request = self.net.request(ClientRequest::LogOut).await?;
        request.response().await?;
        Ok(())
    }
}

impl<Ui: ClientUi> Drop for Client<Ui> {
    fn drop(&mut self) {
        // make sure the client loop stops. we don't care if it's already stopped
        if let Some(close) = self.close.take() {
            let _ = close.send(());
        }
    }
}

struct ClientLoop<S> {
    send: Rc<net::RequestSender>,
    stream: S,
    close: futures::channel::oneshot::Receiver<()>,
}

impl<S> ClientLoop<S>
    where S: Stream<Item = net::Result<ServerAction>> + Unpin
{
    async fn run(self) {
        let ClientLoop { send, stream, close } = self;

        let receiver = Box::pin(async move {
            let mut stream = stream;
            while let Some(result) = stream.next().await {
                // TODO
                println!("{:?}", result);
            }
        });

        let keep_alive = Box::pin(async move {
            let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
            loop {
                if let Err(_) = send.net().ping().await {
                    break;
                }
                ticker.tick().await;
            }
        });

        // run until either the client is closed or the loop exits
        futures::future::select(
            close,
            futures::future::join(receiver, keep_alive),
        ).await;
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Net(net::Error),
    Response(ErrResponse),
    UnexpectedResponse,
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
