use crate::{Error, Result};
use tokio_tungstenite::WebSocketStream;
use url::Url;
use vertex::prelude::*;

pub struct AuthenticatedWs {
    pub stream: AuthenticatedWsStream,
    pub device: DeviceId,
    pub token: AuthToken,
}

pub type AuthenticatedWsStream = WebSocketStream<hyper::upgrade::Upgraded>;

type Connector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

pub struct AuthClient {
    server_url: Url,
    client: hyper::Client<Connector>,
}

impl AuthClient {
    pub fn new(server_url: Url) -> Result<AuthClient> {
        let https = hyper_tls::HttpsConnector::new();
        let client = hyper::client::Client::builder().build(https);

        let path = server_url.path();
        let has_api = path.ends_with("vertex/client/") || path.ends_with("vertex/client");
        let server_url = if !has_api {
            server_url.join("vertex/client/")?
        } else {
            server_url
        };

        Ok(AuthClient { server_url, client })
    }

    pub async fn login(&self, device: DeviceId, token: AuthToken) -> Result<AuthenticatedWs> {
        let request = serde_urlencoded::to_string(Login {
            device,
            token: token.clone(),
        })
        .expect("failed to encode authenticate request");

        let url = self.server_url.join(&format!("authenticate?{}", request))?;

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
                )
                .await;

                Ok(AuthenticatedWs {
                    stream: ws,
                    device,
                    token,
                })
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
        let response = self
            .post_auth(
                AuthRequest::RegisterUser(RegisterUser {
                    credentials,
                    display_name,
                }),
                self.server_url.join("register")?,
            )
            .await?;

        match response? {
            AuthOk::User(user) => Ok(user),
            other => Err(Error::UnexpectedMessage {
                expected: "AuthOk::User",
                got: Box::new(other),
            }),
        }
    }

    pub async fn create_token(
        &self,
        credentials: Credentials,
        options: TokenCreationOptions,
    ) -> Result<NewToken> {
        let response = self
            .post_auth(
                AuthRequest::CreateToken(CreateToken {
                    credentials,
                    options,
                }),
                self.server_url.join("token/create")?,
            )
            .await?;

        match response? {
            AuthOk::Token(token) => Ok(token),
            other => Err(Error::UnexpectedMessage {
                expected: "AuthOk::Token",
                got: Box::new(other),
            }),
        }
    }

    pub async fn refresh_token(&self, credentials: Credentials, device: DeviceId) -> Result<()> {
        let response = self
            .post_auth(
                AuthRequest::RefreshToken(RefreshToken {
                    credentials,
                    device,
                }),
                self.server_url.join("token/refresh")?,
            )
            .await?;

        match response? {
            AuthOk::NoData => Ok(()),
            other => Err(Error::UnexpectedMessage {
                expected: "AuthOk::NoData",
                got: Box::new(other),
            }),
        }
    }

    pub async fn revoke_token(&self, credentials: Credentials, device: DeviceId) -> Result<()> {
        let response = self
            .post_auth(
                AuthRequest::RevokeToken(RevokeToken {
                    credentials,
                    device,
                }),
                self.server_url.join("token/revoke")?,
            )
            .await?;

        match response? {
            AuthOk::NoData => Ok(()),
            other => Err(Error::UnexpectedMessage {
                expected: "AuthOk::NoData",
                got: Box::new(other),
            }),
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
        let url = self.server_url.join("change_password")?;
        let response = self.post_auth(req, url).await?;

        match response? {
            AuthOk::NoData => Ok(()),
            other => Err(Error::UnexpectedMessage {
                expected: "AuthOk::NoData",
                got: Box::new(other),
            }),
        }
    }

    async fn post_auth(&self, request: AuthRequest, url: Url) -> Result<AuthResponse> {
        let bytes: Vec<u8> = request.into();
        let request = hyper::Request::builder()
            .uri(url.as_str().parse::<hyper::Uri>()?)
            .method(hyper::Method::POST)
            .body(hyper::Body::from(bytes))
            .unwrap();

        let response = self.client.request(request).await?;
        let bytes = hyper::body::to_bytes(response.into_body()).await?;

        Ok(AuthResponse::from_protobuf_bytes(&bytes)?)
    }
}
