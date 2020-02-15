use std::cell::RefCell;

use futures::sink::SinkExt;
use futures::stream::{SplitSink, SplitStream, Stream, StreamExt};

pub use auth::{AuthenticatedWs, AuthenticatedWsStream};
pub use request::*;

use crate::auth;
use std::borrow::Cow;

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
    async fn send_raw(&self, message: tungstenite::Message) -> tungstenite::Result<()> {
        self.0.borrow_mut().send(message).await?;
        Ok(())
    }

    pub async fn ping(&self) -> tungstenite::Result<()> {
        self.send_raw(tungstenite::Message::Ping(vec![])).await
    }

    pub async fn send(&self, message: vertex::ClientMessage) -> tungstenite::Result<()> {
        self.send_raw(tungstenite::Message::Binary(message.into())).await
    }

    pub async fn close(&self) -> tungstenite::Result<()> {
        self.send_raw(tungstenite::Message::Close(None)).await
    }
}

pub struct Receiver(SplitStream<AuthenticatedWsStream>);

impl Receiver {
    pub fn stream(self) -> impl Stream<Item = tungstenite::Result<vertex::ServerMessage>> {
        self.0.filter_map(move |result| futures::future::ready(
            match result {
                Ok(tungstenite::Message::Binary(bytes)) => {
                    match serde_cbor::from_slice::<vertex::ServerMessage>(&bytes) {
                        Ok(message) => Some(Ok(message)),
                        Err(_) => Some(Err(tungstenite::Error::Protocol(Cow::Borrowed("malformed message")))),
                    }
                }
                Ok(tungstenite::Message::Close(_)) => Some(Err(tungstenite::Error::ConnectionClosed)),
                Err(e) => Some(Err(e)),
                _ => None,
            }
        ))
    }
}
