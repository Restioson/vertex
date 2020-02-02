use std::cell::RefCell;
use std::io;

use futures::sink::SinkExt;
use futures::stream::{SplitSink, SplitStream, Stream, StreamExt};

pub use request::*;
pub use auth::{AuthenticatedWsStream, AuthenticatedWs};

use crate::auth;

mod request;

pub fn from_ws(ws: AuthenticatedWsStream) -> (Sender, Receiver) {
    let (send, recv) = ws.split();
    (
        Sender(RefCell::new(send)),
        Receiver(recv),
    )
}

pub struct Sender(RefCell<SplitSink<AuthenticatedWsStream, tungstenite::Message>>);

impl Sender {
    #[inline]
    async fn send_raw(&self, message: tungstenite::Message) -> Result<()> {
        self.0.borrow_mut().send(message).await.map_err(tungstenite_to_net_error)?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        self.send_raw(tungstenite::Message::Ping(vec![])).await
    }

    pub async fn send(&self, message: vertex::ClientMessage) -> Result<()> {
        self.send_raw(tungstenite::Message::Binary(message.into())).await
    }

    pub async fn close(&self) -> Result<()> {
        self.send_raw(tungstenite::Message::Close(None)).await
    }
}

pub struct Receiver(SplitStream<AuthenticatedWsStream>);

impl Receiver {
    pub fn stream(self) -> impl Stream<Item = Result<vertex::ServerMessage>> {
        self.0.filter_map(move |result| futures::future::ready(
            match result {
                Ok(tungstenite::Message::Binary(bytes)) => {
                    match serde_cbor::from_slice::<vertex::ServerMessage>(&bytes) {
                        Ok(message) => Some(Ok(message)),
                        Err(_) => Some(Err(Error::MalformedMessage)),
                    }
                }
                Ok(tungstenite::Message::Close(_)) => Some(Err(Error::Closed)),
                Err(e) => Some(Err(tungstenite_to_net_error(e))),
                _ => None,
            }
        ))
    }
}

fn tungstenite_to_net_error(error: tungstenite::Error) -> Error {
    match error {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => Error::Closed,
        tungstenite::Error::Io(io) => Error::Io(io),
        _ => Error::Generic,
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Generic,
    MalformedMessage,
    Io(io::Error),
    Closed,
}
