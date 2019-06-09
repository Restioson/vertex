#[macro_use]
extern crate serde_derive;

use std::convert::Into;
use uuid::Uuid;
use vertex_common::*;

trait WebSocket {
    fn send(&mut self, bytes: Vec<u8>) -> Result<(), Error>;
}

pub struct Vertex {
    socket: Box<dyn WebSocket>,
    id: Uuid,
    logged_in: bool,
}

impl Vertex {
    pub fn new(socket: Box<dyn WebSocket>) -> Self {
        Vertex {
            socket,
            id: Uuid::new_v4(),
            logged_in: false,
        }
    }

    fn handle(&mut self, binary: Vec<u8>) {
        // TODO
    }

    fn send(&mut self, msg: ClientMessage) -> Result<(), Error> {
        self.socket.send(msg.into())
    }

    /// Logs in, returning whether it was successful
    fn login(&mut self) -> bool {
        if !self.logged_in {
            self.send(ClientMessage::Login(Login { id: self.id })).is_ok()
        } else {
            false
        }
    }

    /// Sends a message, returning whether it was successful
    fn send_message(&mut self, msg: String, to_room: Uuid) -> Result<(), Error> {
        if self.logged_in {
            self.send(ClientMessage::SendMessage(ClientSentMessage { to_room, content: msg, }))
        } else {
            Err(Error::AlreadyLoggedIn)
        }
    }
}

#[derive(Debug, Serialize)]
pub enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError,
    InvalidUuid,
}
