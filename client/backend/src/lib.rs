use std::convert::Into;
use url::Url;
use vertex_common::*;
use websocket::WebSocketError;

use net::Net;
use std::collections::LinkedList;
use std::time::Instant;

mod net;

pub struct Config {
    pub url: Url,
}

pub struct Vertex {
    net: Net,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub device_id: Option<DeviceId>,
    logged_in: bool,
    request_callbacks: LinkedList<Box<dyn FnOnce(&mut Vertex, Response) -> Option<Action>>>,
}

impl Vertex {
    pub fn connect(config: Config) -> Vertex {
        let net = Net::connect(config.url).expect("failed to connect");

        Vertex {
            net,
            username: None,
            display_name: None,
            device_id: None,
            logged_in: false,
            request_callbacks: LinkedList::new(),
        }
    }

    fn add_callback<F: FnOnce(&mut Vertex, Response) -> Option<Action> + 'static>(&mut self, callback: F) {
        self.request_callbacks.push_back(Box::new(callback))
    }

    fn add_callback_success<F: FnOnce(&mut Vertex, Success) -> Option<Action> + 'static>(
        &mut self,
        callback: F,
    ) {
        self.add_callback(|vertex, res| match res {
            Response::Success(s) => callback(vertex, s),
            Response::Error(e) => Some(Action::Error(Error::ServerError(e))),
        })
    }

    pub fn handle(&mut self) -> Option<Action> {
        if Instant::now() - self.net.last_message() > HEARTBEAT_TIMEOUT {
            return Some(Action::Error(Error::ServerTimedOut));
        }

        match self.net.recv() {
            Ok(Some(msg)) => match msg {
                ClientboundMessage::Response(res) => {
                    self.request_callbacks
                        .pop_front()
                        .expect("No callback for request found")(self, res)
                }
                ClientboundMessage::Message(message) => Some(Action::AddMessage(message.into())),
                ClientboundMessage::EditMessage(_) => None, // TODO
                ClientboundMessage::DeleteMessage(_) => None,
                ClientboundMessage::SessionLoggedOut => Some(Action::LoggedOut),
            },
            Err(err) => Some(Action::Error(err)),
            _ => None,
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

        let username_cloned = username.clone();
        let make_login_request = |vertex: &mut Vertex, device_id, token: AuthToken| {
            vertex.net.send(ClientRequest::Login { device_id, token: token.clone() });
            vertex.add_callback_success(move |vertex, res| {
                match res {
                    Success::NoData => {
                        vertex.username = Some(username_cloned.clone());
                        vertex.display_name = Some(username_cloned); // TODO configure this
                        vertex.device_id = Some(device_id);
                        vertex.logged_in = true;

                        Some(Action::LoggedIn { device_id, token })
                    },
                    _ => Some(Action::Error(Error::InvalidServerMessage)),
                }
            });
        };

        match token {
            Some((device_id, token)) => make_login_request(self, device_id, token),
            None => {
                // TODO allow user to configure these parameters?
                self.net.send(ClientRequest::CreateToken {
                    username,
                    password,
                    device_name: None,
                    expiration_date: None,
                    permission_flags: TokenPermissionFlags::ALL
                });

                self.add_callback_success(|vertex, res| {
                    match res {
                        Success::Token { device_id, token } => {
                            make_login_request(vertex, device_id, token);
                            None
                        },
                        _ => Some(Action::Error(Error::InvalidServerMessage)),
                    }
                })
            },
        };

        Ok(())
    }

    pub fn create_user(&mut self, username: String, display_name: String, password: String) {
        self.net.send(ClientRequest::CreateUser {
            username,
            display_name,
            password,
        });
        self.add_callback_success(|_, success| match success {
            Success::User(id) => Some(Action::UserCreated(id)),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        })
    }

    pub fn change_username(&mut self, new_username: String) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::ChangeUsername { new_username: new_username.clone() });
        self.add_callback_success(|_, success| match success {
            Success::NoData => Some(Action::UsernameChanged(new_username)),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        });
        Ok(())
    }

    pub fn change_display_name(&mut self, new_display_name: String) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::ChangeDisplayName { new_display_name: new_display_name.clone() });
        self.add_callback_success(|vertex, success| match success {
            Success::NoData => {
                vertex.display_name = Some(new_display_name.clone());
                Some(Action::DisplayNameChanged(new_display_name))
            },
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        });
        Ok(())
    }

    pub fn change_password(&mut self, old_password: String, new_password: String) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::ChangePassword { old_password, new_password });
        self.add_callback_success(|_, success| match success {
            Success::NoData => Some(Action::PasswordChanged),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        });
        Ok(())
    }

    pub fn refresh_token(&mut self, to_refresh: DeviceId, username: String, password: String) {
        self.net.send(ClientRequest::RefreshToken { device_id: to_refresh, username, password });
        self.add_callback_success(|_, success| match success {
            Success::NoData => Some(Action::TokenRefreshed),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        })
    }

    pub fn revoke_token(&mut self, to_revoke: DeviceId, password: String) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::RevokeToken { device_id: to_revoke, password: Some(password) });
        self.add_callback_success(|_, success| match success {
            Success::NoData => Some(Action::TokenRevoked),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        });

        Ok(())
    }

    pub fn revoke_current_token(&mut self) -> Result<()> {
        if let Some(device_id) = self.device_id {
            self.net.send(ClientRequest::RevokeToken { device_id, password: None });
            self.add_callback_success(|_, success| match success {
                Success::NoData => Some(Action::TokenRevoked),
                _ => Some(Action::Error(Error::InvalidServerMessage)),
            });
            Ok(())
        } else {
            Err(Error::NotLoggedIn)
        }
    }

    pub fn create_room(&mut self) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::CreateRoom);
        self.add_callback_success(|_, success| match success {
            Success::Room(id) => Some(Action::AddRoom(id)),
            _ => Some(Action::Error(Error::InvalidServerMessage)),
        });
        Ok(())
    }

    pub fn join_room(&mut self, room: RoomId) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::JoinRoom(room));
        self.add_callback_success(|_vertex, _res| None);
        Ok(())
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub fn send_message(&mut self, content: String, to_room: RoomId) -> Result<()> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        self.net.send(ClientRequest::SendMessage(ClientSentMessage {
            to_room,
            content,
        }));
        self.add_callback_success(|_vertex, _res| None);
        Ok(())
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
    InvalidServerMessage,
    ServerError(ServerError),
    ServerTimedOut,
    ServerClosed,
    MalformedResponse,
}

impl From<WebSocketError> for Error {
    fn from(err: WebSocketError) -> Self {
        Error::WebSocketError(err)
    }
}

impl From<ServerError> for Error {
    fn from(err: ServerError) -> Self {
        Error::ServerError(err)
    }
}
