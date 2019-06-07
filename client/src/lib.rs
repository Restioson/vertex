use uuid::Uuid;
use bytes::Bytes;
use vertex_common::*;

/// Mockup of a websocket interface
trait WebSocket {
    fn send(&mut self, bin: Bytes) -> Result<(), ()>;
    fn recv(&mut self) -> Bytes;
}

struct Vertex<W: WebSocket> {
    socket: W,
    id: Uuid,
    logged_in: bool,
}

impl<W: WebSocket> Vertex<W> {
    fn new(socket: W) -> Self {
        Vertex {
            socket,
            id: Uuid::new_v4(),
            logged_in: false,
        }
    }

    fn send(&mut self, msg: ClientMessage) -> Result<(), Error> {
        let bin = serde_cbor::to_vec(&msg).unwrap().into();
        self.socket.send(bin).map_err(|_| Error::WebSocketError)
    }

    pub fn login(&mut self) -> Result<(), Error> {
        if !self.logged_in {
            self.send(ClientMessage::Login(Login { id: self.id }))
        } else {
            Err(Error::AlreadyLoggedIn)
        }
    }

    pub fn send_message(&mut self, msg: String, to_room: Uuid) -> Result<(), Error> {
        if self.logged_in {
            self.send(ClientMessage::SendMessage(ClientSentMessage { to_room, content: msg, }))
                .map_err(|_| Error::WebSocketError)
        } else {
            Err(Error::NotLoggedIn)
        }
    }
}

#[derive(Debug)]
enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError,
}
