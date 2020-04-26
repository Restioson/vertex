use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use futures::channel::oneshot;
use futures::FutureExt;
use futures::stream::{Stream, StreamExt};

use vertex::prelude::*;

use crate::net;
use crate::{Result, Error};

const REQUEST_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(5);

struct RequestIdGenerator {
    next_request_id: AtomicU32,
}

impl RequestIdGenerator {
    fn new() -> RequestIdGenerator {
        RequestIdGenerator {
            next_request_id: AtomicU32::new(0),
        }
    }

    fn next(&self) -> RequestId {
        RequestId::new(self.next_request_id.fetch_add(1, Ordering::SeqCst))
    }
}

struct RequestTracker {
    pending_requests: RefCell<HashMap<RequestId, EnqueuedRequest>>,
}

impl RequestTracker {
    fn new() -> RequestTracker {
        RequestTracker {
            pending_requests: RefCell::new(HashMap::new()),
        }
    }

    fn enqueue(&self, id: RequestId) -> Option<oneshot::Receiver<Result<OkResponse>>> {
        let mut pending_requests = self.pending_requests.borrow_mut();
        if pending_requests.contains_key(&id) {
            return None;
        }

        let (send, recv) = oneshot::channel();
        pending_requests.insert(id, EnqueuedRequest(send));

        Some(recv)
    }

    fn complete(&self, id: RequestId, result: ResponseResult) {
        if let Some(request) = self.pending_requests.borrow_mut().remove(&id) {
            request.handle(result);
        }
    }
}

struct EnqueuedRequest(oneshot::Sender<Result<OkResponse>>);

impl EnqueuedRequest {
    fn handle(self, result: ResponseResult) {
        let result = result.map_err(Error::ErrorResponse);
        let _ = self.0.send(result); // We don't care if the channel has closed
    }
}

pub struct RequestManager {
    tracker: Rc<RequestTracker>,
    id_gen: Rc<RequestIdGenerator>,
}

impl Default for RequestManager {
    fn default() -> Self {
        RequestManager::new()
    }
}

impl RequestManager {
    pub fn new() -> RequestManager {
        RequestManager {
            tracker: Rc::new(RequestTracker::new()),
            id_gen: Rc::new(RequestIdGenerator::new()),
        }
    }

    pub fn sender(&self, net: net::Sender) -> RequestSender {
        RequestSender {
            tracker: self.tracker.clone(),
            id_gen: self.id_gen.clone(),
            net: Rc::new(net),
        }
    }

    pub fn receive_from(&self, net: net::Receiver) -> impl Stream<Item = tungstenite::Result<ServerEvent>> {
        let tracker = Rc::downgrade(&self.tracker);

        net.stream().filter_map(move |result| {
            futures::future::ready(match result {
                Ok(ServerMessage::Event(action)) => Some(Ok(action)),
                Ok(ServerMessage::Response { result, id }) => {
                    if let Some(tracker) = tracker.upgrade() {
                        tracker.complete(id, result);
                    }
                    None
                }
                Ok(ServerMessage::MalformedMessage) => panic!("Malformed message"),
                Ok(e) => unimplemented!("{:#?}", e),
                Err(e) => Some(Err(e)),
            })
        })
    }
}

pub struct Request(oneshot::Receiver<Result<OkResponse>>);

impl Request {
    pub async fn response(self) -> Result<OkResponse> {
        let future = self.0.map(|result| result.expect("channel closed"));
        let result = tokio::time::timeout(REQUEST_TIMEOUT, future).await
            .map_err(|_| Error::Timeout)?;

        Ok(result?)
    }
}

#[derive(Clone)]
pub struct RequestSender {
    tracker: Rc<RequestTracker>,
    id_gen: Rc<RequestIdGenerator>,
    net: Rc<net::Sender>,
}

impl RequestSender {
    pub async fn send(&self, request: ClientRequest) -> Request {
        let id = self.id_gen.next();

        let receiver = self.tracker.enqueue(id).expect("unable to enqueue message");

        let message = ClientMessage { id, request };
        self.net.send(message).await;

        Request(receiver)
    }

    #[inline]
    pub fn net(&self) -> &net::Sender {
        &self.net
    }
}
