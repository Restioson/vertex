// TODO: how to split this into backend?

use tokio_tungstenite::WebSocketStream;
use url::Url;

use vertex::prelude::*;

use crate::{Error, Result};
use crate::Server;

pub struct AuthenticatedWs {
    pub stream: AuthenticatedWsStream,
    pub device: DeviceId,
    pub token: AuthToken,
}

pub type AuthenticatedWsStream = WebSocketStream<hyper::upgrade::Upgraded>;

type Connector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

pub struct Client {
    pub server: Server,
    client: hyper::Client<Connector>,
}

impl Client {
    pub fn new(server: Server) -> Client {
        let https = hyper_tls::HttpsConnector::new();
        let client = hyper::client::Client::builder()
            .build(https);

        Client { server, client }
    }

    pub async fn login(
        &self,
        device: DeviceId,
        token: AuthToken,
    ) -> Result<AuthenticatedWs> {
        let request = serde_urlencoded::to_string(Login { device, token: token.clone() })
            .expect("failed to encode authenticate request");

        let url = self.server.url().join(&format!("authenticate?{}", request))?;

        let key: [u8; 16] = rand::random();
        let key = base64::encode(&key);

        let request = hyper::Request::builder()
            .uri(url.as_str().parse::<hyper::Uri>().unwrap())
            .header("upgrade", "websocket")
            .header("connection", "upgrade")
            .header("sec-websocket-key", key)
            .header("sec-websocket-version", "13")
            .body(hyper::Body::empty())
            .unwrap();

        let response = self.client.request(request).await?;

        match response.status() {
            hyper::StatusCode::SWITCHING_PROTOCOLS => {
                let body = response.into_body();
                let upgraded = body.on_upgrade().await?;

                let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
                    upgraded,
                    tungstenite::protocol::Role::Client,
                    None,
                ).await;

                Ok(AuthenticatedWs { stream: ws, device, token })
            }
            _ => {
                let body = response.into_body();
                let bytes = hyper::body::to_bytes(body).await?;

                match AuthResponse::from_protobuf_bytes(&bytes) {
                    Ok(AuthResponse::Ok(_)) => Err(Error::ProtocolError(None)),
                    Ok(AuthResponse::Err(err)) => Err(Error::AuthErrorResponse(err)),
                    Err(e) => Err(e.into()),
                }
            }
        }
    }

    pub async fn register(
        &self,
        credentials: Credentials,
        display_name: Option<String>,
    ) -> Result<UserId> {
        let response = self.post_auth(
            AuthRequest::RegisterUser(RegisterUser { credentials, display_name }),
            self.server.url().join("register")?,
        ).await?;

        match response? {
            AuthOk::User(user) => Ok(user),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn create_token(
        &self,
        credentials: Credentials,
        options: TokenCreationOptions,
    ) -> Result<NewToken> {
        let response = self.post_auth(
            AuthRequest::CreateToken(CreateToken { credentials, options }),
            self.server.url().join("token/create")?,
        ).await?;

        match response? {
            AuthOk::Token(token) => Ok(token),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn refresh_token(
        &self,
        credentials: Credentials,
        device: DeviceId,
    ) -> Result<()> {
        let response = self.post_auth(
            AuthRequest::RefreshToken(RefreshToken { credentials, device }),
            self.server.url().join("token/refresh")?,
        ).await?;

        match response? {
            AuthOk::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn revoke_token(
        &self,
        credentials: Credentials,
        device: DeviceId,
    ) -> Result<()> {
        let response = self.post_auth(
            AuthRequest::RevokeToken(RevokeToken { credentials, device }),
            self.server.url().join("token/revoke")?,
        ).await?;

        match response? {
            AuthOk::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn change_password(
        &self,
        old_credentials: Credentials,
        new_password: String,
    ) -> Result<()> {
        let req = AuthRequest::ChangePassword(ChangePassword {
            username: old_credentials.username,
            old_password: old_credentials.password,
            new_password,
        });
        let url = self.server.url().join("change_password")?;
        let response = self.post_auth(req, url).await?;

        match response? {
            AuthOk::NoData => Ok(()),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    async fn post_auth(&self, request: AuthRequest, url: Url) -> Result<AuthResponse> {
        let request = hyper::Request::builder()
            .uri(url.as_str().parse::<hyper::Uri>()?)
            .method(hyper::Method::POST)
            .body(hyper::Body::from(request.into(): Vec<u8>))
            .unwrap();

        let response = self.client.request(request).await?;
        let bytes = hyper::body::to_bytes(response.into_body()).await?;

        Ok(AuthResponse::from_protobuf_bytes(&bytes)?)
    }
}
