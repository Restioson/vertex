use std::convert::Into;
use url::Url;
use vertex_common::*;
use websocket::WebSocketError;

mod net;

use net::Net;

pub struct Config {
    pub url: Url,
    pub client_id: UserId,
}

pub struct Vertex {
    net: Net,
    id: UserId,
    logged_in: bool,
}

impl Vertex {
    pub fn new(config: Config) -> Self {
        Vertex {
            net: Net::connect(&config.url),
            id: config.client_id,
            logged_in: false,
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
        let message = match self.net.receive() {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Some(Action::Error(e)),
            None => return None,
        };

        match message {
            ServerMessage::Response {
                response,
                request_id: _,
            } => {
                match response {
                    // TODO associate with a particular message id: @gegy1000
                    RequestResponse::Success(_) => None,
                    RequestResponse::Error(e) => Some(Action::Error(Error::ServerError(e))),
                }
            }
            ServerMessage::Error(e) => Some(Action::Error(Error::ServerError(e))),
            ServerMessage::Message(m) => Some(Action::AddMessage(m.into())),
            _ => panic!("unimplemented"),
        }
    }

    pub fn login(&mut self) -> Result<()> {
        if !self.logged_in {
            let request_id = self.net.request(ClientMessage::Login(Login { id: self.id }))?;

            let msg = self.net.receive_blocking()?;
            match msg.clone() {
                ServerMessage::Response {
                    response,
                    request_id: response_id,
                } => {
                    match response {
                        // TODO do this more asynchronously @gegy1000
                        RequestResponse::Success(Success::NoData) if response_id == request_id => {
                            Ok(())
                        }
                        RequestResponse::Error(e) => Err(Error::ServerError(e)),
                        _ => Err(Error::IncorrectServerMessage(msg)),
                    }
                }
                msg @ _ => Err(Error::IncorrectServerMessage(msg)),
            }
        } else {
            Err(Error::AlreadyLoggedIn)
        }
    }

    pub fn create_room(&mut self) -> Result<RoomId> {
        let request_id = self.net.request(ClientMessage::CreateRoom)?;

        let msg = self.net.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::Room { id }) if response_id == request_id => {
                        Ok(id)
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            ServerMessage::Error(e) => Err(Error::ServerError(e)),
            _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn join_room(&mut self, room: RoomId) -> Result<()> {
        let request_id = self.net.request(ClientMessage::JoinRoom(room))?;

        let msg = self.net.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            ServerMessage::Error(e) => Err(Error::ServerError(e)),
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, msg: String, to_room: RoomId) -> Result<RequestId> {
        if !self.logged_in {
            self.net.request(ClientMessage::SendMessage(ClientSentMessage {
                to_room,
                content: msg,
            }))
        } else {
            Err(Error::NotLoggedIn)
        }
    }

    pub fn username(&self) -> String {
        format!("{}", self.id.0) // TODO lol
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    #[inline]
    pub fn dispatch_heartbeat(&mut self) -> Result<()> {
        self.net.dispatch_heartbeat()
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
    /// A message from the server that doesn't make sense in a specific context
    IncorrectServerMessage(ServerMessage),
    ServerError(ServerError),
    ServerTimedOut,
}
