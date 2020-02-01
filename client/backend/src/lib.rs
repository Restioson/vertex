use vertex::*;

use std::time::Duration;

pub mod auth;
pub mod net;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

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

pub struct Client<Net: net::Sender> {
    sender: net::RequestSender<Net>,
    user: UserId,
    device: DeviceId,
    token: AuthToken,
}

impl<Net: net::Sender> Client<Net> {
    pub async fn change_username(&self, new_username: String) -> Result<()> {
        let request = ClientRequest::ChangeUsername { new_username };
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_display_name(&self, new_display_name: String) -> Result<()> {
        let request = ClientRequest::ChangeDisplayName { new_display_name };
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn change_password(&self, old_password: String, new_password: String) -> Result<()> {
        let request = ClientRequest::ChangePassword {
            old_password,
            new_password,
        };
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn refresh_token(
        &self,
        credentials: UserCredentials,
        to_refresh: DeviceId,
    ) -> Result<()> {
        let request = ClientRequest::RefreshToken {
            credentials,
            device: to_refresh,
        };
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_foreign_token(&self, to_revoke: DeviceId, password: String) -> Result<()> {
        let request = ClientRequest::RevokeForeignToken {
            device: to_revoke,
            password,
        };
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn revoke_token(&self) -> Result<()> {
        let request = self.sender.request(ClientRequest::RevokeToken).await?;
        request.response().await?;
        Ok(())
    }

    pub async fn create_room(&self, name: String, community: CommunityId) -> Result<RoomId> {
        let request = ClientRequest::CreateRoom { name, community };
        let request = self.sender.request(request).await?;
        let response = request.response().await?;

        match response {
            OkResponse::Room { id } => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn send_message(
        &self,
        content: String,
        to_community: CommunityId,
        to_room: RoomId,
    ) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community,
            to_room,
            content,
        });
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn create_community(&self, name: String) -> Result<CommunityId> {
        let request = ClientRequest::CreateCommunity { name };
        let request = self.sender.request(request).await?;

        match request.response().await? {
            OkResponse::Community { id } => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn join_community(&self, community: CommunityId) -> Result<()> {
        let request = ClientRequest::JoinCommunity(community);
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub fn token(&self) -> (DeviceId, AuthToken) {
        (self.device, self.token.clone())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Net(net::Error),
    Response(ErrResponse),
    UnexpectedResponse,
}

impl From<net::Error> for Error {
    fn from(net: net::Error) -> Self {
        Error::Net(net)
    }
}

impl From<ErrResponse> for Error {
    fn from(response: ErrResponse) -> Self {
        Error::Response(response)
    }
}
