use futures::Stream;

use vertex::*;

use crate::net;

pub const HEARTBEAT_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(2);

pub struct Client {
    sender: net::RequestSender,
    device: DeviceId,
    token: AuthToken,
}

impl Client {
    pub fn new(ws: net::AuthenticatedWs) -> (Client, impl Stream<Item = net::Result<ServerAction>>) {
        let (sender, receiver) = net::from_ws(ws.stream);

        let req_manager = net::RequestManager::new();

        let req_sender = req_manager.sender(sender);
        let req_receiver = req_manager.receive_from(receiver);

        (
            Client {
                sender: req_sender,
                device: ws.device,
                token: ws.token,
            },
            req_receiver,
        )
    }

    pub async fn keep_alive_loop(&self) {
        let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            if let Err(_) = self.sender.net().ping().await {
                break;
            }
            ticker.tick().await;
        }
    }

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

    pub async fn join_community(&self, invite: InviteCode) -> Result<()> {
        let request = ClientRequest::JoinCommunity(invite);
        let request = self.sender.request(request).await?;
        request.response().await?;

        Ok(())
    }

    pub async fn create_invite(&self, community: CommunityId) -> Result<InviteCode> {
        let request = ClientRequest::CreateInvite { community, expiration_date: None };
        let request = self.sender.request(request).await?;

        match request.response().await? {
            OkResponse::Invite { code } => Ok(code),
            _ => Err(Error::UnexpectedResponse),
        }
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
