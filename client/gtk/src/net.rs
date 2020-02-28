use std::borrow::Cow;
use std::cell::RefCell;

use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::stream::{SplitSink, SplitStream, Stream, StreamExt};

pub use auth::{AuthenticatedWs, AuthenticatedWsStream};
pub use request::*;

use crate::auth;

mod request;

pub fn from_ws(ws: AuthenticatedWsStream) -> (Sender, Receiver) {
    let (sink, stream) = ws.split();
    let (error_send, error_recv) = mpsc::channel(4);

    (
        Sender(RefCell::new(SenderInner {
            sink,
            error: error_send,
        })),
        Receiver {
            stream,
            error: error_recv,
        },
    )
}

struct SenderInner {
    sink: SplitSink<AuthenticatedWsStream, tungstenite::Message>,
    error: mpsc::Sender<tungstenite::Error>,
}

pub struct Sender(RefCell<SenderInner>);

impl Sender {
    async fn send_raw(&self, message: tungstenite::Message) {
        let result = self.0.borrow_mut().sink.send(message).await;
        if let Err(err) = result {
            let _ = self.0.borrow_mut().error.send(err).await;
        }
    }

    pub async fn ping(&self) {
        self.send_raw(tungstenite::Message::Ping(vec![])).await
    }

    pub async fn send(&self, message: vertex::requests::ClientMessage) {
        self.send_raw(tungstenite::Message::Binary(message.into())).await
    }

    pub async fn close(&self) {
        self.send_raw(tungstenite::Message::Close(None)).await
    }
}

pub struct Receiver {
    stream: SplitStream<AuthenticatedWsStream>,
    error: mpsc::Receiver<tungstenite::Error>,
}

impl Receiver {
    pub fn stream(self) -> impl Stream<Item = tungstenite::Result<vertex::prelude::ServerMessage>> {
        let error = self.error.map(Err);

        futures::stream::select(self.stream, error)
            .filter_map(move |result| futures::future::ready(
                match result {
                    Ok(tungstenite::Message::Binary(bytes)) => {
                        match vertex::prelude::ServerMessage::from_protobuf_bytes(&bytes) {
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
