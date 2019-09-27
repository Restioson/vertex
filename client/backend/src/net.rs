use std::time::Instant;
use websocket::{ClientBuilder, OwnedMessage, WebSocketResult};
use websocket::client::Url;

use super::Error as VertexError;
use super::Result as VertexResult;

use vertex_common::{ServerMessage, ClientMessage, HEARTBEAT_TIMEOUT};

use websocket::sender::Writer;
use websocket::receiver::Reader;
use websocket::stream::sync::TcpStream;
use std::thread;
use std::sync::mpsc;

pub struct Net {
    send: mpsc::Sender<OwnedMessage>,
    recv: mpsc::Receiver<OwnedMessage>,
    last_heartbeat: Instant,
}

impl Net {
    pub fn connect(url: Url) -> WebSocketResult<Net> {
        let client = ClientBuilder::from_url(&url)
            .connect_insecure()?;

        client.stream_ref().set_read_timeout(None)?;

        let (send_in, recv_in) = mpsc::channel();
        let (send_out, recv_out) = mpsc::channel();

        let (reader, writer) = client.split()?;

        thread::spawn(move || {
            NetReader {
                reader,
                send: send_in,
            }.run()
        });

        thread::spawn(move || {
            NetWriter {
                writer,
                recv: recv_out,
            }.run()
        });

        Ok(Net {
            send: send_out,
            recv: recv_in,
            last_heartbeat: Instant::now(),
        })
    }

    pub fn send(&mut self, message: ClientMessage) {
        self.send.send(OwnedMessage::Binary(message.into()))
            .expect("send channel closed")
    }

    pub fn dispatch_heartbeat(&mut self) {
        self.send.send(OwnedMessage::Ping(Vec::new()))
            .expect("send channel closed")
    }

    pub fn next(&mut self) -> VertexResult<Option<ServerMessage>> {
        // TODO: I don't think this should be handled here
        if Instant::now() - self.last_heartbeat > HEARTBEAT_TIMEOUT {
            return Err(VertexError::ServerTimedOut);
        }
        while let Ok(message) = self.recv.try_recv() {
            match message {
                OwnedMessage::Binary(bytes) => {
                    match serde_cbor::from_slice::<ServerMessage>(&bytes) {
                        Ok(message) => return Ok(Some(message)),
                        Err(_) => return Err(VertexError::MalformedResponse),
                    }
                }
                OwnedMessage::Pong(_) => self.last_heartbeat = Instant::now(),
                OwnedMessage::Close(_) => return Err(VertexError::ServerTimedOut),
                _ => eprintln!("received unexpected message type"),
            }
        }
        Ok(None)
    }
}

struct NetReader {
    reader: Reader<TcpStream>,
    send: mpsc::Sender<OwnedMessage>,
}

impl NetReader {
    fn run(mut self) {
        loop {
            match self.reader.recv_message() {
                Ok(message) => {
                    if self.send.send(message).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    // TODO: handle error
                    eprintln!("websocket read error: {:?}", err);
                    break;
                }
            }
        }
    }
}

struct NetWriter {
    writer: Writer<TcpStream>,
    recv: mpsc::Receiver<OwnedMessage>,
}

impl NetWriter {
    fn run(mut self) {
        while let Ok(message) = self.recv.recv() {
            if let Err(err) = self.writer.send_message(&message) {
                // TODO: handle error
                eprintln!("websocket write error: {}", err);
                break;
            }
        }
    }
}
