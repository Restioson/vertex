pub use vertex_common::*;

use std::time::Duration;
use url::Url;

use futures::{Stream, StreamExt};
use std::rc::Rc;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

pub mod net;

pub use net::Net;

pub fn action_stream(receiver: net::Receiver) -> impl Stream<Item=Action> {
    receiver.stream().filter_map(|action| futures::future::ready(
        match action {
            Ok(ServerAction::Message(message)) => Some(Action::AddMessage(message.into())),
            Ok(ServerAction::SessionLoggedOut) => Some(Action::LoggedOut),
            Ok(ServerAction::AddCommunity { id, name }) => {
                // TODO community rooms
                Some(Action::AddCommunity(Community { id, name, rooms: Vec::new() }))
            },
            Ok(ServerAction::RemoveCommunity { id, .. }) => Some(Action::RemoveCommunity { id }),
            Err(e) => Some(Action::Error(e)),
            _ => None,
        }
    ))
}

#[derive(Debug)]
pub struct Community {
    pub id: CommunityId,
    pub name: String,
    pub rooms: Vec<Room>,
}

#[derive(Debug)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
}

pub struct Config {
    pub url: Url,
}

pub struct AuthClient {
    net: Rc<net::Sender>,
}

impl AuthClient {
    pub fn new(net: Rc<net::Sender>) -> AuthClient {
        AuthClient { net }
    }

    pub async fn register(&self, credentials: UserCredentials, display_name: String) -> Result<UserId> {
        let request = ClientRequest::CreateUser { credentials, display_name };
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::User { id } => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn authenticate(&self, credentials: UserCredentials) -> Result<(DeviceId, AuthToken)> {
        let request = ClientRequest::CreateToken {
            credentials,
            // TODO: allow user to configure?
            options: TokenCreationOptions {
                device_name: None,
                expiration_date: None,
                permission_flags: TokenPermissionFlags::ALL,
            },
        };
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::Token { device, token } => Ok((device, token)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn login(self, device: DeviceId, token: AuthToken) -> Result<Client> {
        let request = ClientRequest::Login { device, token: token.clone() };
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::User { id: user_id } => {
                Ok(Client {
                    net: self.net,
                    user: user_id,
                    device,
                    token,
                })
            }
            _ => Err(Error::UnexpectedResponse),
        }
    }
}

pub struct Client {
    net: Rc<net::Sender>,
    user: UserId,
    device: DeviceId,
    token: AuthToken,
}

impl Client {
    pub async fn change_username(&self, new_username: String) -> Result<()> {
        let request = ClientRequest::ChangeUsername { new_username };
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_display_name(&self, new_display_name: String) -> Result<()> {
        let request = ClientRequest::ChangeDisplayName { new_display_name };
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_password(&self, old_password: String, new_password: String) -> Result<()> {
        let request = ClientRequest::ChangePassword { old_password, new_password };
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn refresh_token(&self, credentials: UserCredentials, to_refresh: DeviceId) -> Result<()> {
        let request = ClientRequest::RefreshToken { credentials, device: to_refresh };
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_foreign_token(&self, to_revoke: DeviceId, password: String) -> Result<()> {
        let request = ClientRequest::RevokeForeignToken { device: to_revoke, password };
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_token(&self) -> Result<()> {
        let request = self.net.request(ClientRequest::RevokeToken).await?;
        request.response().await?;
        Ok(())
    }

    pub async fn create_room(&self, name: String, community: CommunityId) -> Result<RoomId> {
        let request = self.net.request(ClientRequest::CreateRoom {
            name,
            community,
        }).await?;
        let response = request.response().await?;

        match response {
            OkResponse::Room { id } => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn send_message(&self, content: String, to_community: CommunityId, to_room: RoomId) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community,
            to_room,
            content,
        });
        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn create_community(&self, name: String) -> Result<CommunityId> {
        let request = self.net.request(ClientRequest::CreateCommunity { name }).await?;

        match request.response().await? {
            OkResponse::Community { id } => Ok(id),
            _ => Err(Error::MalformedResponse),
        }
    }

    pub async fn join_community(&self, community: CommunityId) -> Result<()> {
        let request = self.net.request(ClientRequest::JoinCommunity(community)).await?;
        request.response().await?;

        Ok(())
    }

    pub fn token(&self) -> (DeviceId, AuthToken) {
        (self.device, self.token.clone())
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
    AddCommunity(Community),
    RemoveCommunity { id: CommunityId },
    LoggedOut,
    Error(Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    ErrResponse(ErrResponse),
    WebSocketError(tungstenite::Error),
    UnexpectedResponse,
    ServerClosed,
    MalformedRequest,
    MalformedResponse,
}

impl From<ErrResponse> for Error {
    fn from(err: ErrResponse) -> Self { Error::ErrResponse(err) }
}

impl From<tungstenite::Error> for Error {
    fn from(err: tungstenite::Error) -> Self { Error::WebSocketError(err) }
}
