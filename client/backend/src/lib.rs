#![feature(await_macro, async_await)]

use std::convert::Into;
use url::Url;
use vertex_common::*;
use websocket::{WebSocketError, OwnedMessage};

use net::{Net, MakeRequest};
use actix::{Addr, Actor, Context, Handler};
use futures::{Future, future};
use crate::net::DispatchHeartbeat;

mod net;

pub struct Config {
    pub url: Url,
    pub client_id: UserId,
}

pub struct Vertex {
    net: Addr<Net>,
    user_id: UserId,
    logged_in: bool,
}

impl Vertex {
    pub fn connect(config: Config) -> Vertex {
        let net = Net::connect(config.url)
            .expect("failed to connect");

        Vertex {
            net,
            user_id: config.client_id,
            logged_in: false,
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
//        let message = match self.net.receive() {
//            Some(Ok(m)) => m,
//            Some(Err(e)) => return Some(Action::Error(e)),
//            None => return None,
//        };
//
//        match message {
//            ServerMessage::Response {
//                response,
//                request_id: _,
//            } => {
//                match response {
//                    // TODO associate with a particular message id: @gegy1000
//                    Result<RequestResponse, ServerError>::RequestResponse(_) => None,
//                    Result<RequestResponse, ServerError>::Error(e) => Some(Action::Error(Error::ServerError(e))),
//                }
//            }
//            ServerMessage::Error(e) => Some(Action::Error(Error::ServerError(e))),
//            ServerMessage::Message(m) => Some(Action::AddMessage(m.into())),
//            _ => panic!("unimplemented"),
//        }
        None
    }

    pub fn login(&mut self) -> RequestFuture<()> {
        if !self.logged_in {
            Box::new(self.request(ClientRequest::Login(Login { id: self.user_id }))
                .then(|response| {
                    match response {
                        Ok(RequestResponse::NoData) => Ok(()),
                        Ok(response) => Err(Error::UnexpectedResponse(response)),
                        Err(e) => Err(e),
                    }
                }))
        } else {
            Box::new(future::err(Error::AlreadyLoggedIn))
        }
    }

    pub fn create_room(&mut self) -> RequestFuture<RoomId> {
        Box::new(self.request(ClientRequest::CreateRoom)
            .then(|response| {
                match response {
                    Ok(RequestResponse::Room { id }) => Ok(id),
                    Ok(response) => Err(Error::UnexpectedResponse(response)),
                    Err(e) => Err(e),
                }
            }))
    }

    pub fn join_room(&mut self, room: RoomId) -> RequestFuture<()> {
        Box::new(self.request(ClientRequest::JoinRoom(room))
            .then(|response| {
                match response {
                    Ok(RequestResponse::NoData) => Ok(()),
                    Ok(response) => Err(Error::UnexpectedResponse(response)),
                    Err(e) => Err(e),
                }
            }))
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, content: String, to_room: RoomId) -> RequestFuture<()> {
        if self.logged_in {
            Box::new(self.request(ClientRequest::SendMessage(ClientSentMessage { to_room, content })).map(|_| ()))
        } else {
            Box::new(future::err(Error::NotLoggedIn))
        }
    }

    pub fn username(&self) -> String {
        format!("{}", self.user_id.0) // TODO lol
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    #[inline]
    pub fn dispatch_heartbeat(&mut self)  {
        self.net.send(DispatchHeartbeat);
    }

    #[inline]
    fn request(&self, request: ClientRequest) -> impl Future<Item=RequestResponse, Error=Error> {
        self.net.send(MakeRequest(request))
            .then(|result| {
                result.expect("failed to send request to actor")
            })
    }
}

impl Actor for Vertex {
    type Context = Context<Vertex>;
}

impl Handler<ServerMessage> for Vertex {
    type Result = ();

    fn handle(&mut self, msg: ServerMessage, ctx: &mut Context<Vertex>) {
        println!("bruh! {:?}", msg);
    }
}

#[derive(Debug)]
pub struct Message {
    pub author: String,
    pub room: RoomId,
    pub content: String,
}

impl From<ForwardedMessage> for Message {
    fn from(msg: ForwardedMessage) -> Self {
        Message {
            author: format!("{}", msg.author.0),
            room: msg.room,
            content: msg.content,
        }
    }
}

/// An action that the GUI should take
#[derive(Debug)]
pub enum Action {
    AddMessage(Message),
    Error(Error),
}

pub type Result<T> = std::result::Result<T, Error>;
pub type RequestFuture<T> = Box<dyn Future<Item=T, Error=Error> + Send>;

#[derive(Debug)]
pub enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError(WebSocketError),
    /// A message from the server that doesn't deserialize correctly
    InvalidServerMessage,
    /// A response from the server that doesn't make sense in a specific context
    UnexpectedResponse(RequestResponse),
    ServerError(ServerError),
    ServerTimedOut,
    ResponseCancelled,
}
