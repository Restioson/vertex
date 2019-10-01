use std::convert::Into;
use vertex_common::*;
use websocket::{WebSocketError, OwnedMessage};

use std::collections::LinkedList;
use std::time::Instant;
use futures::{future, Future, Async};
use futures::stream::Stream;

pub mod net;

pub struct Vertex {
    net: net::Active,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub device_id: Option<DeviceId>,
    logged_in: bool,
    action_futures: LinkedList<Box<dyn Future<Item=Option<Action>, Error=Error>>>,
}

impl Vertex {
    pub fn new(net: net::Active) -> Vertex {
        Vertex {
            net,
            username: None,
            display_name: None,
            device_id: None,
            logged_in: false,
            action_futures: LinkedList::new(),
        }
    }

    #[inline]
    pub fn bind<F, M, E>(&mut self, fut: F, map: M)
        where F: Future<Error=E> + 'static,
              M: FnOnce(F::Item) -> Option<Action> + 'static,
              E: Into<Error>,
    {
        let fut = fut.map(map).map_err(|e| e.into());
        self.action_futures.push_back(Box::new(fut));
    }

    pub fn handle(&mut self) -> Option<Action> {
        if Instant::now() - self.net.last_message() > HEARTBEAT_TIMEOUT {
            return Some(Action::Error(Error::ServerTimedOut));
        }

        // TODO: Can this be cleaned up at all?
        if let Some(future) = self.action_futures.front_mut() {
            match future.poll() {
                Ok(Async::Ready(action)) => {
                    self.action_futures.pop_front();
                    if let Some(action) = action {
                        return Some(action);
                    }
                }
                Err(err) => {
                    self.action_futures.pop_front();
                    return Some(Action::Error(err));
                }
                _ => (),
            }
        }

        loop {
            match self.net.stream().poll() {
                Ok(Async::Ready(Some(action))) => {
                    break match action {
                        ClientboundAction::Message(message) => Some(Action::AddMessage(message.into())),
                        ClientboundAction::EditMessage(_) => None, // TODO
                        ClientboundAction::DeleteMessage(_) => None,
                        ClientboundAction::SessionLoggedOut => Some(Action::LoggedOut),
                    };
                }
                Ok(_) => break None,
                Err(err) => break Some(Action::Error(err)),
            }
        }
    }

    pub fn login(
        &mut self,
        token: Option<(DeviceId, AuthToken)>,
        username: String,
        password: String,
    ) -> Result<()> {
        if self.logged_in {
            return Err(Error::AlreadyLoggedIn);
        }

        let response = match token {
            Some((device_id, token)) => make_login_request(self, device_id, token),
            None => {
                // TODO allow user to configure these parameters?
                let response = self.net.request(ServerboundRequest::CreateToken {
                    username,
                    password,
                    device_name: None,
                    expiration_date: None,
                    permission_flags: TokenPermissionFlags::ALL,
                }).and_then(|response| match response {
                    OkResponse::Token { device_id, token } => {
                        // TODO: Clone `net` so we can call it from in here
                        //   it has other fields than the channels so we might need to do some extra work there
                        make_login_request(vertex, device_id, token);
                        None
                    }
                    _ => Some(Action::Error(Error::UnexpectedResponse)),
                });
            }
        };

        self.bind(response, |response| {
            match response {
                OkResponse::NoData => {
                    vertex.username = Some(username_cloned.clone());
                    vertex.display_name = Some(username_cloned); // TODO configure this
                    vertex.device_id = Some(device_id);
                    vertex.logged_in = true;

                    Some(Action::LoggedIn { device_id, token })
                }
                _ => Some(Action::Error(Error::UnexpectedResponse)),
            }
        });

        Ok(())
    }

    pub fn create_user(&mut self, username: String, display_name: String, password: String) {
        let response = self.net.request(ServerboundRequest::CreateUser {
            username,
            display_name,
            password,
        });

        self.bind(response, |response| match response {
            OkResponse::User(id) => Some(Action::UserCreated(id)),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn change_username(&mut self, new_username: String) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::ChangeUsername { new_username: new_username.clone() });
        self.bind(response, |response| match response {
            OkResponse::NoData => Some(Action::UsernameChanged(new_username)),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn change_display_name(&mut self, new_display_name: String) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::ChangeDisplayName { new_display_name: new_display_name.clone() });
        self.bind(response, |response| match response {
            OkResponse::NoData => Some(Action::DisplayNameChanged(new_display_name)),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn change_password(&mut self, old_password: String, new_password: String) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::ChangePassword { old_password, new_password });
        self.bind(response, |response| match response {
            OkResponse::NoData => Some(Action::PasswordChanged),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn refresh_token(&mut self, to_refresh: DeviceId, username: String, password: String) {
        let response = self.net.request(ServerboundRequest::RefreshToken { device_id: to_refresh, username, password });
        self.bind(response, |response| match response {
            OkResponse::NoData => Some(Action::TokenRefreshed),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn revoke_token(&mut self, to_revoke: DeviceId, password: String) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::RevokeToken { device_id: to_revoke, password: Some(password) });
        self.bind(response, |response| match response {
            OkResponse::NoData => Some(Action::TokenRevoked),
            _ => Some(Action::Error(Error::UnexpectedResponse)),
        });
    }

    pub fn revoke_current_token(&mut self) -> Result<()> {
        if let Some(device_id) = self.device_id {
            let response = self.net.request(ServerboundRequest::RevokeToken { device_id, password: None });
            self.bind(response, |response| match response {
                OkResponse::NoData => Some(Action::TokenRevoked),
                _ => Some(Action::Error(Error::UnexpectedResponse)),
            });
            Ok(())
        } else {
            Err(Error::NotLoggedIn)
        }
    }

    pub fn create_room(&mut self) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::CreateRoom);
        self.bind(response, |response| {
            match response {
                OkResponse::Room(id) => Some(Action::AddRoom(id)),
                _ => Some(Action::Error(Error::UnexpectedResponse)),
            }
        });
    }

    pub fn join_room(&mut self, room: RoomId) {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::JoinRoom(room));
        self.bind(response, |_| None);
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, content: String, to_room: RoomId) {
        if !self.logged_in {
            return future::err(Error::NotLoggedIn);
        }

        let response = self.net.request(ServerboundRequest::SendMessage(ClientSentMessage {
            to_room,
            content,
        }));

        self.bind(response, |_| None);
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    #[inline]
    pub fn dispatch_heartbeat(&mut self) {
        self.net.dispatch_heartbeat();
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
    LoggedOut,
    AddRoom(RoomId),
    Error(Error),
    LoggedIn {
        device_id: DeviceId,
        token: AuthToken,
    },
    TokenRefreshed,
    TokenRevoked,
    UserCreated(UserId),
    UsernameChanged(String),
    DisplayNameChanged(String),
    PasswordChanged,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError(WebSocketError),
    /// A message from the server that doesn't deserialize correctly
    UnexpectedResponse,
    ServerError(ServerError),
    ServerTimedOut,
    ServerClosed,
    MalformedResponse,
    ChannelClosed,
}

impl From<ServerError> for Error {
    fn from(err: ServerError) -> Self {
        Error::ServerError(err)
    }
}

impl From<WebSocketError> for Error {
    fn from(err: WebSocketError) -> Self {
        Error::WebSocketError(err)
    }
}
