use std::io::{self, Cursor};
use std::convert::Into;
use std::time::Duration;
use uuid::Uuid;
use url::Url;
use websocket::client::ClientBuilder;
use websocket::sync::Client;
use websocket::stream::sync::TcpStream;
use websocket::{OwnedMessage, WebSocketError};
use vertex_common::*;

pub struct Config {
    pub url: Url,
    pub client_id: Uuid,
}

pub struct Vertex {
    socket: Client<TcpStream>,
    id: Uuid,
    logged_in: bool,
}

impl Vertex {
    pub fn new(config: Config) -> Self {
        let socket = ClientBuilder::from_url(&config.url)
            .connect_insecure()
            .expect("Error connecting to websocket");

        // TODO have a heartbeat
        socket.stream_ref()
            .set_read_timeout(Some(Duration::from_micros(1)))
            .unwrap();

        Vertex {
            socket,
            id: config.client_id,
            logged_in: false,
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
        let msg = match self.receive() {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Some(Action::Error(e)),
            None => return None,
        };

        match msg {
            ServerMessage::Success(_) => None,
            ServerMessage::Error(e) => Some(Action::Error(Error::ServerError(e))),
            ServerMessage::Message(m) => Some(Action::AddMessage(m.into())),
            _ => unimplemented!(),
        }
    }

    fn send(&mut self, msg: ClientMessage) -> Result<(), Error> {
        self.socket.send_message(&OwnedMessage::Binary(msg.into()))
            .map_err(Error::WebSocketError)
    }

    fn receive(&mut self) -> Option<Result<ServerMessage, Error>> {
        let msg = match self.socket.recv_message() {
            Ok(msg) => Ok(msg),
            Err(WebSocketError::NoDataAvailable) => return None,
            Err(WebSocketError::IoError(e)) => {
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
                    return None
                } else {
                    Err(WebSocketError::IoError(e))
                }
            },
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

    fn receive_blocking(&mut self) -> Result<ServerMessage, Error> { // TODO eventual timeout
        loop {
            match self.receive() {
                Some(res) => return res,
                None => (),
            };
        };
    }

    pub fn login(&mut self) -> Result<(), Error> {
        if !self.logged_in {
            self.send(ClientMessage::Login(Login { id: self.id }))?;

            match self.receive_blocking()? {
                ServerMessage::Success(Success::NoData) => Ok(()),
                msg @ _ => Err(Error::IncorrectServerMessage(msg)),
            }
        } else {
            Err(Error::AlreadyLoggedIn)
        }
    }

    pub fn create_room(&mut self) -> Result<Uuid, Error> {
        self.send(ClientMessage::CreateRoom)?;

        match self.receive_blocking()? {
            ServerMessage::Success(Success::Room { id }) => Ok(id),
            ServerMessage::Error(e) => Err(Error::ServerError(e)),
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    /// Sends a message, returning whether it was successful
    pub fn send_message(&mut self, msg: String, to_room: Uuid) -> Result<(), Error> {
        if !self.logged_in {
            self.send(ClientMessage::SendMessage(ClientSentMessage { to_room, content: msg, }))
        } else {
            Err(Error::NotLoggedIn)
        }
    }
}

#[derive(Debug)]
pub struct Message {
    pub author: String,
    pub room: Uuid,
    pub content: String,
}

impl From<ForwardedMessage> for Message {
    fn from(msg: ForwardedMessage) -> Self {
        Message {
            author: format!("{}", msg.author),
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
    InvalidUuid,
    WebSocketError(WebSocketError),
    /// A message from the server that doesn't deserialize correctly
    InvalidServerMessage,
    /// A message from the server that doesn't make sense in a specific context
    IncorrectServerMessage(ServerMessage),
    ServerError(ServerError),
}
