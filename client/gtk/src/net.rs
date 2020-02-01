use std::cell::RefCell;

use futures::sink::SinkExt;
use futures::stream::{SplitSink, SplitStream, Stream, StreamExt};
use url::Url;

use async_trait::async_trait;
pub use vertex_client::net::{Error, Result};

type WsStream = tokio_tls::TlsStream<tokio::net::TcpStream>;
type WsClient = tokio_tungstenite::WebSocketStream<WsStream>;

/// Module adapted from tokio-tungstenite
/// tokio-tungstenite does not support connection with a custom TlsConnector, so we need to reimplement it
mod ws {
    use tokio::net::TcpStream;
    use tungstenite::{Error, Result};
    use tungstenite::handshake::client::{Request, Response};

    pub(super) async fn connect_async<R>(request: R) -> Result<(super::WsClient, Response)>
        where R: Into<Request<'static>> + Unpin,
    {
        let request: Request = request.into();

        let domain = request.url.host_str()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Url("no host name in the url".into()))?;

        let port = request.url.port_or_known_default().expect("port unknown");

        let socket = TcpStream::connect(format!("{}:{}", domain, port)).await
            .map_err(Error::Io)?;

        let connector = tokio_tls::TlsConnector::from(
            native_tls::TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(Error::Tls)?
        );

        let stream = connector.connect(&domain, socket).await
            .map_err(Error::Tls)?;

        tokio_tungstenite::client_async(request, stream).await
    }
}

pub async fn connect(url: Url) -> Result<(Sender, Receiver)> {
    let (client, _) = ws::connect_async(url)
        .await.map_err(tungstenite_to_net_error)?;

    let (sink, stream) = client.split();

    Ok((
        Sender(RefCell::new(sink)),
        Receiver(stream),
    ))
}

pub struct Sender(RefCell<SplitSink<WsClient, tungstenite::Message>>);

impl Sender {
    #[inline]
    async fn send_raw(&self, message: tungstenite::Message) -> Result<()> {
        self.0.borrow_mut().send(message).await.map_err(tungstenite_to_net_error)?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        self.send_raw(tungstenite::Message::Ping(vec![])).await
    }
}

#[async_trait(?Send)]
impl vertex_client::net::Sender for Sender {
    async fn send(&self, message: vertex::ClientMessage) -> Result<()> {
        self.send_raw(tungstenite::Message::Binary(message.into())).await
    }

    async fn close(&self) -> Result<()> {
        self.send_raw(tungstenite::Message::Close(None)).await
    }
}

pub struct Receiver(SplitStream<WsClient>);

impl vertex_client::net::Receiver for Receiver {
    type Stream = impl Stream<Item = Result<vertex::ServerMessage>>;

    fn stream(self) -> Self::Stream {
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
