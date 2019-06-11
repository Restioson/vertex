use std::time::{Duration, Instant};
use websocket::{ClientBuilder, OwnedMessage, WebSocketError};
use websocket::client::sync::Client;
use websocket::client::Url;
use websocket::stream::sync::TcpStream;
use vertex_common::*;
use std::io::{self, Cursor};

use super::{Result, Error};

pub struct Net {
    socket: Client<TcpStream>,
    heartbeat: Instant,
}

impl Net {
    pub fn connect(url: &Url) -> Net {
        let socket = ClientBuilder::from_url(url)
            .connect_insecure()
            .expect("Error connecting to websocket");

        socket.stream_ref()
            .set_read_timeout(Some(Duration::from_micros(1)))
            .unwrap();

        Net {
            socket,
            heartbeat: Instant::now(),
        }
    }

    pub fn send(&mut self, msg: ClientRequest) -> Result<()> {
        self.socket
            .send_message(&OwnedMessage::Binary(msg.into()))
            .map_err(Error::WebSocketError)
    }

    pub fn request(&mut self, msg: ClientMessage) -> Result<RequestId> {
        let request = ClientRequest::new(msg);
        let request_id = request.request_id;
        self.send(request)?;
        Ok(request_id)
    }

    pub fn receive(&mut self) -> Option<Result<ServerMessage>> {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            return Some(Err(Error::ServerTimedOut));
        }

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
            Ok(OwnedMessage::Pong(_)) => {
                self.heartbeat = Instant::now();
                return None;
            }
            Ok(_) => return Some(Err(Error::InvalidServerMessage)),
            Err(e) => return Some(Err(Error::WebSocketError(e))),
        };

        let mut bin = Cursor::new(bin);
        Some(serde_cbor::from_reader(&mut bin).map_err(|_| Error::InvalidServerMessage))
    }

    pub fn receive_blocking(&mut self) -> Result<ServerMessage> {
        // TODO: Match message ids
        // TODO eventual timeout
        loop {
            match self.receive() {
                Some(res) => return res,
                None => (),
            }
        }
    }

    pub fn dispatch_heartbeat(&mut self) -> Result<()> {
        self.socket
            .send_message(&OwnedMessage::Ping(vec![]))
            .map_err(Error::WebSocketError)
    }
}
