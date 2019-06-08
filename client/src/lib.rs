use wasm_bindgen::prelude::*;
use uuid::Uuid;
use vertex_common::*;
use std::convert::Into;

#[macro_use]
extern crate serde_derive;

// Use wee_alloc when enabled -- it is a smaller (generated code size wise) allocator
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
    pub type WebSocket;

    /// Sends a message, and returns whether it was successful
    #[wasm_bindgen(structural, method)]
    pub fn send(this: &WebSocket, binary: Vec<u8>) -> bool;

    #[wasm_bindgen(structural, method)]
    fn recv(this: &WebSocket) -> JsValue;
}

#[wasm_bindgen]
pub struct Vertex {
    socket: WebSocket,
    id: Uuid,
    logged_in: bool,
}

// Internal rust methods
impl Vertex {
    pub fn new(socket: WebSocket) -> Self {
        Vertex {
            socket,
            id: Uuid::new_v4(),
            logged_in: false,
        }
    }

    fn send(&mut self, msg: ClientMessage) -> Result<(), Error> {
        if !self.socket.send(msg.into()) {
            Err(Error::WebSocketError)
        } else {
            Ok(())
        }
    }

    pub fn handle(&mut self, binary: Vec<u8>) {
       // TODO
    }
}

#[wasm_bindgen] // Exported methods
impl Vertex {
    /// Logs in, returning whether it was successful
    pub fn login(&mut self) -> bool {
        if !self.logged_in {
            self.send(ClientMessage::Login(Login { id: self.id })).is_ok()
        } else {
            false
        }
    }

    /// Sends a message, returning whether it was successful
    pub fn send_message(&mut self, msg: String, room_id: String) -> bool {
        let to_room = match Uuid::parse_str(&room_id) {
            Ok(id) => id,
            Err(_) => return false,
        };

        if self.logged_in {
            self.send(ClientMessage::SendMessage(ClientSentMessage { to_room, content: msg, })).is_ok()
        } else {
            false
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

impl Into<JsValue> for Error {
    fn into(self) -> JsValue {
        JsValue::from_serde(&self).unwrap()
    }
}
