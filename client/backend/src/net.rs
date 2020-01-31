use vertex::*;

use futures::FutureExt;
use futures::{Stream, StreamExt};
use futures::channel::oneshot;

use std::collections::HashMap;

use std::rc::Rc;
use std::cell::RefCell;
use std::io;

use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;

struct RequestIdGenerator {
    next_request_id: AtomicU32,
}

impl RequestIdGenerator {
    fn new() -> RequestIdGenerator {
        RequestIdGenerator { next_request_id: AtomicU32::new(0) }
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
        RequestTracker { pending_requests: RefCell::new(HashMap::new()) }
    }

    fn enqueue(&self, id: RequestId) -> Option<oneshot::Receiver<ResponseResult>> {
        let mut pending_requests = self.pending_requests.borrow_mut();
        if pending_requests.contains_key(&id) {
            return None;
        }

        let (send, recv) = oneshot::channel();
        pending_requests.insert(id, EnqueuedRequest(send));

        return Some(recv);
    }

    fn complete(&self, id: RequestId, result: ResponseResult) {
        self.pending_requests.borrow_mut().remove(&id).map(|request| request.handle(result));
    }
}

struct EnqueuedRequest(oneshot::Sender<ResponseResult>);

impl EnqueuedRequest {
    fn handle(self, result: ResponseResult) {
        let _ = self.0.send(result); // We don't care if the channel has closed
    }
}

pub struct RequestManager {
    tracker: Rc<RequestTracker>,
    id_gen: Rc<RequestIdGenerator>,
}

impl RequestManager {
    pub fn new() -> RequestManager {
        RequestManager {
            tracker: Rc::new(RequestTracker::new()),
            id_gen: Rc::new(RequestIdGenerator::new()),
        }
    }

    pub fn sender<S: Sender>(&self, net: S) -> RequestSender<S> {
        RequestSender {
            tracker: self.tracker.clone(),
            id_gen: self.id_gen.clone(),
            net: Rc::new(net),
        }
    }

    pub fn receive_from<R: Receiver>(&self, net: R) -> impl Stream<Item=Result<ServerAction>> {
        let tracker = Rc::downgrade(&self.tracker);

        net.stream().filter_map(move |result| futures::future::ready(
            match result {
                Ok(ServerMessage::Action(action)) => Some(Ok(action)),
                Ok(ServerMessage::Response { result, id }) => {
                    if let Some(tracker) = tracker.upgrade() {
                        tracker.complete(id, result);
                    }
                    None
                }
                Err(e) => Some(Err(e)),
            }
        ))
    }
}

pub struct Request(oneshot::Receiver<ResponseResult>);

impl Request {
    pub async fn response(self) -> ResponseResult {
        self.0.map(|result| result.expect("channel closed")).await
    }
}

pub struct RequestSender<S: Sender> {
    tracker: Rc<RequestTracker>,
    id_gen: Rc<RequestIdGenerator>,
    net: Rc<S>,
}

impl<S: Sender> Clone for RequestSender<S> {
    fn clone(&self) -> Self {
        RequestSender {
            tracker: self.tracker.clone(),
            id_gen: self.id_gen.clone(),
            net: self.net.clone(),
        }
    }
}

impl<S: Sender> RequestSender<S> {
    pub async fn request(&self, request: ClientRequest) -> Result<Request> {
        let id = self.id_gen.next();

        let receiver = self.tracker.enqueue(id)
            .expect("unable to enqueue message");

        let message = ClientMessage { id, request };
        self.send(message).await?;

        Ok(Request(receiver))
    }

    #[inline]
    pub async fn send(&self, message: ClientMessage) -> Result<()> {
        self.net.send(message).await
    }

    #[inline]
    pub async fn close(&self) -> Result<()> {
        self.net.close().await
    }

    #[inline]
    pub fn net(&self) -> &S { &self.net }
}

#[async_trait(? Send)]
pub trait Sender {
    async fn send(&self, message: ClientMessage) -> Result<()>;

    async fn close(&self) -> Result<()>;
}

pub trait Receiver {
    type Stream: Stream<Item=Result<ServerMessage>>;

    fn stream(self) -> Self::Stream;
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Generic,
    MalformedMessage,
    Io(io::Error),
    Closed,
}
