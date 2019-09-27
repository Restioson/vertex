use websocket::{ClientBuilder, OwnedMessage, WebSocketResult};
use websocket::client::Url;

use super::Error as VertexError;
use super::Result as VertexResult;

use vertex_common::{ClientboundPayload, ServerboundMessage, ClientboundMessage};

use websocket::sender::Writer;
use websocket::receiver::Reader;
use websocket::stream::sync::TcpStream;
use std::thread;
use std::sync::mpsc;
use std::time::Instant;

pub struct Net {
    send: mpsc::Sender<OwnedMessage>,
    recv: mpsc::Receiver<OwnedMessage>,
    last_message: Instant,
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
            last_message: Instant::now(),
        })
    }

    pub fn send(&mut self, message: ServerboundMessage) {
        self.send.send(OwnedMessage::Binary(message.into()))
            .expect("send channel closed")
    }

    pub fn dispatch_heartbeat(&mut self) {
        self.send.send(OwnedMessage::Ping(Vec::new()))
            .expect("send channel closed")
    }

    pub fn recv(&mut self) -> VertexResult<Option<ClientboundMessage>> {
        while let Ok(message) = self.recv.try_recv() {
            self.last_message = Instant::now();
            match message {
                OwnedMessage::Binary(bytes) => {
                    match serde_cbor::from_slice::<ClientboundPayload>(&bytes) {
                        Ok(ClientboundPayload::Message(msg)) => return Ok(Some(msg)),
                        Ok(ClientboundPayload::Error(err)) => return Err(VertexError::ServerError(err)),
                        Err(_) => return Err(VertexError::MalformedResponse),
                    }
                }
                OwnedMessage::Close(_) => return Err(VertexError::ServerClosed),
                _ => (),
            }
        }
        Ok(None)
    }

    pub fn last_message(&self) -> Instant {
        self.last_message
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
