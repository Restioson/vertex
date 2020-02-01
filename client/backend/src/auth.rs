use super::*;

pub struct Client<Net: net::Sender> {
    sender: net::RequestSender<Net>,
}

impl<Net: net::Sender> Client<Net> {
    pub fn new(request_sender: net::RequestSender<Net>) -> Self {
        Client {
            sender: request_sender,
        }
    }

    pub async fn register(
        &self,
        credentials: UserCredentials,
        display_name: String,
    ) -> Result<UserId> {
        let request = ClientRequest::CreateUser {
            credentials,
            display_name,
        };
        let request = self.sender.request(request).await?;

        match request.response().await? {
            OkResponse::User { id } => Ok(id),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn authenticate(
        &self,
        credentials: UserCredentials,
    ) -> Result<(DeviceId, AuthToken)> {
        let request = ClientRequest::CreateToken {
            credentials,
            // TODO: allow user to configure?
            options: TokenCreationOptions {
                device_name: None,
                expiration_date: None,
                permission_flags: TokenPermissionFlags::ALL,
            },
        };
        let request = self.sender.request(request).await?;

        match request.response().await? {
            OkResponse::Token { device, token } => Ok((device, token)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn login(self, device: DeviceId, token: AuthToken) -> Result<crate::Client<Net>> {
        let request = ClientRequest::Login {
            device,
            token: token.clone(),
        };
        let request = self.sender.request(request).await?;

        match request.response().await? {
            OkResponse::User { id: user_id } => Ok(crate::Client {
                sender: self.sender,
                user: user_id,
                device,
                token,
            }),
            _ => Err(Error::UnexpectedResponse),
        }
    }
}
