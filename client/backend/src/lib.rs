use std::convert::Into;
use url::Url;
use vertex_common::*;
use websocket::WebSocketError;

use net::Net;
use std::collections::LinkedList;
use std::time::Instant;

mod net;

pub struct Config {
    pub url: Url,
    pub client_id: UserId,
}

pub struct Vertex {
    net: Net,
    user_id: UserId,
    logged_in: bool,
    request_callbacks: LinkedList<Box<dyn FnOnce(Response) -> Option<Action>>>,
}

impl Vertex {
    pub fn connect(config: Config) -> Vertex {
        let net = Net::connect(config.url).expect("failed to connect");

        Vertex {
            net,
            user_id: config.client_id,
            logged_in: false,
            request_callbacks: LinkedList::new(),
        }
    }

    fn add_callback<F: FnOnce(Response) -> Option<Action> + 'static>(&mut self, callback: F) {
        self.request_callbacks.push_back(Box::new(callback))
    }

    fn add_callback_success<F: FnOnce(Success) -> Option<Action> + 'static>(
        &mut self,
        callback: F,
    ) {
        self.request_callbacks.push_back(Box::new(|res| match res {
            Response::Success(s) => callback(s),
            Response::Error(e) => Some(Action::Error(Error::ServerError(e))),
        }))
    }

    pub fn handle(&mut self) -> Option<Action> {
        if Instant::now() - self.net.last_message() > HEARTBEAT_TIMEOUT {
            return Some(Action::Error(Error::ServerTimedOut));
        }

        match self.net.recv() {
            Ok(Some(msg)) => match msg {
                ClientboundMessage::Response(res) => {
                    self.request_callbacks
                        .pop_front()
                        .expect("No callback for request found")(res)
                }
                ClientboundMessage::Message(message) => Some(Action::AddMessage(message.into())),
                ClientboundMessage::EditMessage(_) => None, // TODO
                ClientboundMessage::DeleteMessage(_) => None,
            },
            Err(err) => Some(Action::Error(err)),
            _ => None,
        }
    }

    pub fn login(&mut self) {
        self.net
            .send(ClientRequest::Login(Login { id: self.user_id }));
        self.add_callback_success(|_| None);
    }

    pub fn create_room(&mut self) {
        self.net.send(ClientRequest::CreateRoom);
        self.add_callback_success(|success| match success {
            Success::Room(id) => Some(Action::AddRoom(id)),
            Success::NoData => Some(Action::Error(Error::InvalidServerMessage)),
        });
    }

    pub fn join_room(&mut self, room: RoomId) {
        self.net.send(ClientRequest::JoinRoom(room));
        self.add_callback_success(|_| None);
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, content: String, to_room: RoomId) {
        self.net.send(ClientRequest::SendMessage(ClientSentMessage {
            to_room,
            content,
        }));
        self.add_callback_success(|_| None);
    }

    pub fn username(&self) -> String {
        format!("{}", self.user_id.0) // TODO lol
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    #[inline]
    pub fn dispatch_heartbeat(&mut self) {
        self.net.dispatch_heartbeat();
    }
}

#[derive(Debug)]
pub struct Message {
    pub author: String,
    pub room: RoomId,
    pub content: String,
}

impl From<ForwardedMessage> for Message {
    fn from(msg: ForwardedMessage) -> Self {
        Message {
            author: format!("{}", msg.author.0),
            room: msg.room,
            content: msg.content,
        }
    }
}

/// An action that the GUI should take
#[derive(Debug)]
pub enum Action {
    AddMessage(Message),
    AddRoom(RoomId),
    Error(Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError(WebSocketError),
    /// A message from the server that doesn't deserialize correctly
    InvalidServerMessage,
    ServerError(ServerError),
    ServerTimedOut,
    ServerClosed,
    MalformedResponse,
}

impl From<WebSocketError> for Error {
    fn from(err: WebSocketError) -> Self {
        Error::WebSocketError(err)
    }
}

impl From<ServerError> for Error {
    fn from(err: ServerError) -> Self {
        Error::ServerError(err)
    }
}
