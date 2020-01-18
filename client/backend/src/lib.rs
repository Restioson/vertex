use vertex_common::*;

use futures::stream::{Stream, StreamExt};

use url::Url;
use std::cell::RefCell;

pub mod net;

pub struct Vertex {
    net_sender: net::Sender,
    // TODO: does this need a refcell? is there another type we can use?
    net_receiver: RefCell<Option<net::Receiver>>,
    pub identity: RefCell<Option<UserIdentity>>,
}

#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub username: String,
    pub display_name: String,
    pub device_id: DeviceId,
}

impl Vertex {
    pub async fn connect(url: Url) -> Result<Vertex> {
        let (net_sender, net_receiver) = net::connect(url).await?;
        Ok(Vertex {
            net_sender,
            net_receiver: RefCell::new(Some(net_receiver)),
            identity: RefCell::new(None),
        })
    }

    pub fn action_stream(&self) -> Option<impl Stream<Item=Action>> {
        self.net_receiver.borrow_mut().take().map(|receiver| {
            receiver.stream().filter_map(|action| futures::future::ready(
                match action {
                    Ok(ClientboundAction::Message(message)) => Some(Action::AddMessage(message.into())),
                    Ok(ClientboundAction::SessionLoggedOut) => Some(Action::LoggedOut),
                    Err(e) => Some(Action::Error(e)),
                    _ => None,
                }
            ))
        })
    }

    pub async fn login(
        &self,
        token: Option<(DeviceId, AuthToken)>,
        username: String,
        password: String,
    ) -> Result<(DeviceId, AuthToken)> {
        let (device_id, token) = match token {
            Some(token) => token,
            None => {
                // TODO allow user to configure these parameters?
                let request = ServerboundRequest::CreateToken {
                    username: username.clone(),
                    password,
                    device_name: None,
                    expiration_date: None,
                    permission_flags: TokenPermissionFlags::ALL,
                };
                let request = self.net_sender.request(request).await?;

                match request.response().await? {
                    OkResponse::Token { device_id, token } => (device_id, token),
                    _ => return Err(Error::UnexpectedResponse),
                }
            }
        };

        let request = ServerboundRequest::Login { device_id, token: token.clone() };
        let request = self.net_sender.request(request).await?;

        match request.response().await? {
            OkResponse::NoData => {
                *(self.identity.borrow_mut()) = Some(UserIdentity {
                    username: username.clone(),
                    display_name: username,
                    device_id
                });

                Ok((device_id, token))
            }
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn create_user(&self, username: String, display_name: String, password: String) -> Result<UserId> {
        let request = ServerboundRequest::CreateUser {
            username,
            display_name,
            password,
        };
        let request = self.net_sender.request(request).await?;

        match request.response().await? {
            OkResponse::User(id) => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn change_username(&self, new_username: String) -> Result<()> {
        let request = ServerboundRequest::ChangeUsername { new_username };
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_display_name(&self, new_display_name: String) -> Result<()> {
        let request = ServerboundRequest::ChangeDisplayName { new_display_name };
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_password(&self, old_password: String, new_password: String) -> Result<()> {
        let request = ServerboundRequest::ChangePassword { old_password, new_password };
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn refresh_token(&self, to_refresh: DeviceId, username: String, password: String) -> Result<()> {
        let request = ServerboundRequest::RefreshToken { device_id: to_refresh, username, password };
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_token(&self, to_revoke: DeviceId, password: String) -> Result<()> {
        let request = ServerboundRequest::RevokeToken { device_id: to_revoke, password: Some(password) };
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_current_token(&self) -> Result<()> {
        if let Some(identity) = self.identity.borrow().as_ref() {
            let request = ServerboundRequest::RevokeToken { device_id: identity.device_id, password: None };
            let request = self.net_sender.request(request).await?;
            request.response().await?;
            Ok(())
        } else {
            Err(Error::NotLoggedIn)
        }
    }

    pub async fn create_room(&self) -> Result<RoomId> {
        let request = self.net_sender.request(ServerboundRequest::CreateRoom).await?;
        let response = request.response().await?;

        match response {
            OkResponse::Room(id) => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn join_room(&self, room: RoomId) -> Result<()> {
        let request = self.net_sender.request(ServerboundRequest::JoinRoom(room)).await?;
        request.response().await?;

        Ok(())
    }

    /// Sends a message, returning the request id if it was sent successfully
    pub async fn send_message(&self, content: String, to_room: RoomId) -> Result<()> {
        let request = ServerboundRequest::SendMessage(ClientSentMessage {
            to_room,
            content,
        });
        let request = self.net_sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    #[inline]
    pub async fn dispatch_heartbeat(&self) -> Result<()> {
        self.net_sender.dispatch_heartbeat().await
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
    WebSocketError(tungstenite::Error),
    /// A message from the server that doesn't deserialize correctly
    UnexpectedResponse,
    ServerError(ErrResponse),
    ServerTimedOut,
    ServerClosed,
    MalformedResponse,
    ChannelClosed,
}

impl From<ErrResponse> for Error {
    fn from(err: ErrResponse) -> Self {
        Error::ServerError(err)
    }
}

impl From<tungstenite::Error> for Error {
    fn from(err: tungstenite::Error) -> Self {
        Error::WebSocketError(err)
    }
}
