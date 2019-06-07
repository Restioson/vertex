use wasm_bindgen::prelude::*;
use uuid::Uuid;
use bytes::Bytes;
use vertex_common::*;

// Use wee_alloc when enabled -- it is a smaller (generated code size wise) allocator
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Mockup of a websocket interface
trait WebSocket {
    fn send(&mut self, bin: Bytes) -> Result<(), ()>;
    fn recv(&mut self) -> Bytes;
}

struct Vertex {
    socket: Box<dyn WebSocket>,
    id: Uuid,
    logged_in: bool,
}

// Internal rust methods
impl Vertex{
    fn new(socket: Box<dyn WebSocket>) -> Self {
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
}

#[wasm_bindgen] // Exported methods
impl Vertex {
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
