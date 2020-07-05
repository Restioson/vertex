use crate::{Error, Event, Result};
use async_trait::async_trait;
use futures::{channel::oneshot, stream::SplitSink, FutureExt, SinkExt};
use governor::state::{InMemoryState, NotKeyed};
use governor::{clock::DefaultClock, Quota, RateLimiter};
use std::{collections::HashMap, num::NonZeroU32};
use tokio::time::Duration;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{Error as WsError, Message as WsMessage};
use vertex::prelude::*;
use vertex::RATELIMIT_BURST_PER_MIN;
use xtra::prelude::{Message, *};
use xtra::WeakAddress;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
type AuthenticatedWsStream = WebSocketStream<hyper::upgrade::Upgraded>;

pub struct Network {
    request_manager: RequestManager,
    ratelimiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    ws: SplitSink<AuthenticatedWsStream, WsMessage>,
    client: Option<WeakAddress<crate::__ClientActor::Client>>,
}

impl Network {
    pub fn new(ws: SplitSink<AuthenticatedWsStream, WsMessage>) -> Network {
        Network {
            request_manager: RequestManager::new(),
            ratelimiter: RateLimiter::direct(Quota::per_minute(
                NonZeroU32::new(RATELIMIT_BURST_PER_MIN).unwrap(),
            )),
            ws,
            client: None,
        }
    }
}

impl Actor for Network {}

struct NetworkMessage(tungstenite::Result<WsMessage>);

impl Message for NetworkMessage {
    type Result = ();
}

#[async_trait]
impl Handler<NetworkMessage> for Network {
    async fn handle(&mut self, message: NetworkMessage, _ctx: &mut Context<Self>) {
        let event = match self.handle_network_message(message.0) {
            Ok(Some(event)) => Event(Ok(event)),
            Err(err) => Event(Err(err)),
            _ => return,
        };

        if let Some(client) = &self.client {
            let _ = client.do_send(event);
        }
    }
}

pub struct SendRequest(pub ClientRequest);

impl Message for SendRequest {
    type Result = Result<RequestHandle>;
}

#[async_trait]
impl Handler<SendRequest> for Network {
    async fn handle(
        &mut self,
        request: SendRequest,
        _ctx: &mut Context<Self>,
    ) -> Result<RequestHandle> {
        let (id, handle) = self.request_manager.enqueue();
        let req = ClientMessage {
            id,
            request: request.0,
        };
        self.try_send(req).await?;
        Ok(handle)
    }
}

impl Network {
    async fn try_send<M: Into<Vec<u8>>>(&mut self, msg: M) -> std::result::Result<(), WsError> {
        self.ratelimiter.until_ready().await;
        self.ws.send(WsMessage::binary(msg)).await
    }

    fn handle_network_message(
        &mut self,
        message: tungstenite::Result<WsMessage>,
    ) -> Result<Option<ServerEvent>> {
        let message = match message? {
            WsMessage::Binary(bytes) => ServerMessage::from_protobuf_bytes(&bytes)?,
            WsMessage::Close(_) => return Err(Error::Websocket(WsError::ConnectionClosed)),
            _ => return Ok(None),
        };

        match message {
            ServerMessage::Event(action) => Ok(Some(action)),
            ServerMessage::Response { result, id } => {
                self.request_manager.handle_response(result, id);
                Ok(None)
            }
            ServerMessage::MalformedMessage => {
                log::error!(
                    "Server has informed us that we have sent a malformed message! Out of date?"
                );
                panic!("Malformed message")
            }
            ServerMessage::RateLimited { .. } => {
                log::error!("Ratelimited even after ratelimiting ourselves!");
                panic!("Ratelimited");
            }
            other => {
                log::error!("Unimplemented server message {:#?}", other);
                unimplemented!()
            }
        }
    }
}

struct EnqueuedRequest(oneshot::Sender<Result<OkResponse>>);

struct RequestManager {
    pending_requests: HashMap<RequestId, EnqueuedRequest>,
    next_id: u32,
}

impl RequestManager {
    fn new() -> RequestManager {
        RequestManager {
            pending_requests: HashMap::new(),
            next_id: 0,
        }
    }

    fn enqueue(&mut self) -> (RequestId, RequestHandle) {
        let (send, recv) = oneshot::channel();
        let id = RequestId::new(self.next_id);
        self.pending_requests.insert(id, EnqueuedRequest(send));
        self.next_id += 1;

        (id, RequestHandle(recv))
    }

    fn handle_response(&mut self, response: ResponseResult, id: RequestId) {
        match self.pending_requests.remove(&id) {
            Some(request) => {
                let result = response.map_err(Error::ErrorResponse);
                let _ = request.0.send(result); // We don't care if the channel has closed
            }
            _ => log::warn!(
                "Server sent response for unknown request id: {:#?}",
                (response, id)
            ),
        }
    }
}

pub struct RequestHandle(oneshot::Receiver<Result<OkResponse>>);

impl RequestHandle {
    pub async fn response(self) -> Result<OkResponse> {
        let future = self.0.map(|result| result.expect("channel closed"));
        let result = tokio::time::timeout(REQUEST_TIMEOUT, future)
            .await
            .map_err(|_| Error::Timeout)?;

        Ok(result?)
    }
}
