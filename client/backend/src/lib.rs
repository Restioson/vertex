use std::convert::Into;
use std::io::{self, Cursor};
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use vertex_common::*;
use websocket::client::ClientBuilder;
use websocket::stream::sync::TcpStream;
use websocket::sync::Client;
use websocket::{OwnedMessage, WebSocketError};

pub struct Config {
    pub url: Url,
    pub client_id: UserId,
}

pub struct Vertex {
    socket: Client<TcpStream>,
    id: UserId,
    logged_in: bool,
}

impl Vertex {
    pub fn new(config: Config) -> Self {
        let socket = ClientBuilder::from_url(&config.url)
            .connect_insecure()
            .expect("Error connecting to websocket");

        // TODO have a heartbeat
        socket
            .stream_ref()
            .set_read_timeout(Some(Duration::from_micros(1)))
            .unwrap();

        Vertex {
            socket,
            id: config.client_id,
            logged_in: false,
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
        let message = match self.receive() {
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

    fn send(&mut self, msg: ClientRequest) -> Result<(), Error> {
        self.socket
            .send_message(&OwnedMessage::Binary(msg.into()))
            .map_err(Error::WebSocketError)
    }

    fn request(&mut self, msg: ClientMessage) -> Result<RequestId, Error> {
        let request = ClientRequest::new(msg);
        let request_id = request.request_id;
        self.send(request)?;
        Ok(request_id)
    }

    fn receive(&mut self) -> Option<Result<ServerMessage, Error>> {
        let msg = match self.socket.recv_message() {
            Ok(msg) => Ok(msg),
            Err(WebSocketError::NoDataAvailable) => return None,
            Err(WebSocketError::IoError(e)) => {
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
                    return None;
                } else {
                    Err(WebSocketError::IoError(e))
                }
            }
            Err(e) => Err(e),
        };

        let bin = match msg {
            Ok(OwnedMessage::Binary(bin)) => bin,
            Ok(_) => return Some(Err(Error::InvalidServerMessage)),
            Err(e) => return Some(Err(Error::WebSocketError(e))),
        };

        let mut bin = Cursor::new(bin);
        Some(serde_cbor::from_reader(&mut bin).map_err(|_| Error::InvalidServerMessage))
    }

    fn receive_blocking(&mut self) -> Result<ServerMessage, Error> {
        // TODO eventual timeout
        loop {
            match self.receive() {
                Some(res) => return res,
                None => (),
            };
        }
    }

    pub fn login(&mut self) -> Result<(), Error> {
        if !self.logged_in {
            let request_id = self.request(ClientMessage::Login(Login { id: self.id }))?;

            let msg = self.receive_blocking()?;
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

    pub fn create_room(&mut self) -> Result<RoomId, Error> {
        let request_id = self.request(ClientMessage::CreateRoom)?;

        let msg = self.receive_blocking()?;
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

    pub fn join_room(&mut self, room: RoomId) -> Result<(), Error> {
        let request_id = self.request(ClientMessage::JoinRoom(room))?;

        let msg = self.receive_blocking()?;
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
    pub fn send_message(&mut self, msg: String, to_room: RoomId) -> Result<RequestId, Error> {
        if !self.logged_in {
            self.request(ClientMessage::SendMessage(ClientSentMessage {
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
}
