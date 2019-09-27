use std::convert::Into;
use url::Url;
use vertex_common::*;
use websocket::WebSocketError;

use net::Net;
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
}

impl Vertex {
    pub fn connect(config: Config) -> Vertex {
        let net = Net::connect(config.url)
            .expect("failed to connect");

        Vertex {
            net,
            user_id: config.client_id,
            logged_in: false,
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
        if Instant::now() - self.net.last_message() > HEARTBEAT_TIMEOUT {
            return Some(Action::Error(Error::ServerTimedOut));
        }

        match self.net.recv() {
            Ok(Some(msg)) => match msg {
                ClientboundMessage::AddRoom(room) => Some(Action::AddRoom(room)),
                ClientboundMessage::AddMessage(message) => Some(Action::AddMessage(message.into())),
                ClientboundMessage::EditMessage(_) => None, // TODO
                ClientboundMessage::DeleteMessage(_) => None,
            },
            Err(err) => Some(Action::Error(err)),
            _ => None,
        }
    }

    pub fn login(&mut self) {
        self.net.send(ServerboundMessage::Login(Login { id: self.user_id }));
    }

    pub fn create_room(&mut self) {
        self.net.send(ServerboundMessage::CreateRoom);
    }

    pub fn join_room(&mut self, room: RoomId) {
        self.net.send(ServerboundMessage::JoinRoom(room));
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, content: String, to_room: RoomId) {
        self.net.send(ServerboundMessage::SendMessage(ClientSentMessage { to_room, content }));
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
