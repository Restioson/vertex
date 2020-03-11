use crate::proto;
use crate::proto::DeserializeError;
use crate::structures::{Credentials, TokenCreationOptions};
use crate::types::*;
use std::convert::{TryFrom, TryInto};
use std::ops::Try;
use serde::{Serialize, Deserialize};

/// Not protobuf, but encoded in the url of the endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub device: DeviceId,
    pub token: AuthToken,
}

#[non_exhaustive]
pub enum AuthRequest {
    CreateToken(CreateToken),
    RefreshToken(RefreshToken),
    RevokeToken(RevokeToken),
    RegisterUser(RegisterUser),
}

impl AuthRequest {
    pub fn from_protobuf_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        use prost::Message;
        let proto = proto::requests::auth::AuthRequest::decode(bytes)?;
        proto.try_into()
    }
}

impl Into<Vec<u8>> for AuthRequest {
    fn into(self) -> Vec<u8> {
        use prost::Message;

        let mut buf = Vec::new();
        proto::requests::auth::AuthRequest::from(self)
            .encode(&mut buf)
            .unwrap();
        buf
    }
}

impl From<AuthRequest> for proto::requests::auth::AuthRequest {
    fn from(req: AuthRequest) -> Self {
        use proto::requests::auth::auth_request::Message;
        use AuthRequest::*;

        let inner = match req {
            CreateToken(create) => Message::CreateToken(create.into()),
            RefreshToken(refresh) => Message::RefreshToken(refresh.into()),
            RevokeToken(revoke) => Message::RevokeToken(revoke.into()),
            RegisterUser(register) => Message::RegisterUser(register.into()),
        };

        proto::requests::auth::AuthRequest {
            message: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::auth::AuthRequest> for AuthRequest {
    type Error = DeserializeError;

    fn try_from(req: proto::requests::auth::AuthRequest) -> Result<Self, Self::Error> {
        use proto::requests::auth::auth_request::Message::*;

        Ok(match req.message? {
            CreateToken(create) => AuthRequest::CreateToken(create.try_into()?),
            RefreshToken(refresh) => AuthRequest::RefreshToken(refresh.try_into()?),
            RevokeToken(revoke) => AuthRequest::RevokeToken(revoke.try_into()?),
            RegisterUser(register) => AuthRequest::RegisterUser(register.try_into()?),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CreateToken {
    pub credentials: Credentials,
    pub options: TokenCreationOptions,
}

impl From<CreateToken> for proto::requests::auth::CreateToken {
    fn from(create: CreateToken) -> Self {
        proto::requests::auth::CreateToken {
            credentials: Some(create.credentials.into()),
            options: Some(create.options.into()),
        }
    }
}

impl TryFrom<proto::requests::auth::CreateToken> for CreateToken {
    type Error = DeserializeError;

    fn try_from(create: proto::requests::auth::CreateToken) -> Result<Self, Self::Error> {
        Ok(CreateToken {
            credentials: create.credentials?.into(),
            options: create.options?.try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RefreshToken {
    pub credentials: Credentials,
    pub device: DeviceId,
}

impl From<RefreshToken> for proto::requests::auth::RefreshToken {
    fn from(refresh: RefreshToken) -> Self {
        proto::requests::auth::RefreshToken {
            credentials: Some(refresh.credentials.into()),
            device: Some(refresh.device.into()),
        }
    }
}

impl TryFrom<proto::requests::auth::RefreshToken> for RefreshToken {
    type Error = DeserializeError;

    fn try_from(refresh: proto::requests::auth::RefreshToken) -> Result<Self, Self::Error> {
        Ok(RefreshToken {
            credentials: refresh.credentials?.into(),
            device: refresh.device?.try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RevokeToken {
    pub credentials: Credentials,
    pub device: DeviceId,
}

impl From<RevokeToken> for proto::requests::auth::RevokeToken {
    fn from(revoke: RevokeToken) -> Self {
        proto::requests::auth::RevokeToken {
            credentials: Some(revoke.credentials.into()),
            device: Some(revoke.device.into()),
        }
    }
}

impl TryFrom<proto::requests::auth::RevokeToken> for RevokeToken {
    type Error = DeserializeError;

    fn try_from(revoke: proto::requests::auth::RevokeToken) -> Result<Self, Self::Error> {
        Ok(RevokeToken {
            credentials: revoke.credentials?.into(),
            device: revoke.device?.try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RegisterUser {
    pub credentials: Credentials,
    pub display_name: Option<String>,
}

impl From<RegisterUser> for proto::requests::auth::RegisterUser {
    fn from(register: RegisterUser) -> Self {
        use proto::requests::auth::register_user::DisplayName;

        proto::requests::auth::RegisterUser {
            credentials: Some(register.credentials.into()),
            display_name: if let Some(name) = register.display_name {
                Some(DisplayName::Present(name))
            } else {
                None
            },
        }
    }
}

impl TryFrom<proto::requests::auth::RegisterUser> for RegisterUser {
    type Error = DeserializeError;

    fn try_from(register: proto::requests::auth::RegisterUser) -> Result<Self, Self::Error> {
        use proto::requests::auth::register_user::DisplayName;

        let display_name = register.display_name.map(|DisplayName::Present(x)| x);

        Ok(RegisterUser {
            credentials: register.credentials?.into(),
            display_name,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewToken {
    pub device: DeviceId,
    pub token: AuthToken,
}

impl From<NewToken> for proto::requests::auth::NewToken {
    fn from(new: NewToken) -> Self {
        proto::requests::auth::NewToken {
            device: Some(new.device.into()),
            token_string: new.token.0,
        }
    }
}

impl TryFrom<proto::requests::auth::NewToken> for NewToken {
    type Error = DeserializeError;

    fn try_from(new: proto::requests::auth::NewToken) -> Result<Self, Self::Error> {
        Ok(NewToken {
            device: new.device?.try_into()?,
            token: AuthToken(new.token_string),
        })
    }
}

#[derive(Debug)]
pub enum AuthResponse {
    Ok(AuthOk),
    Err(AuthError),
}

impl AuthResponse {
    pub fn from_protobuf_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        use prost::Message;
        let proto = proto::requests::auth::AuthResponse::decode(bytes)?;
        proto.try_into()
    }
}

impl Try for AuthResponse {
    type Ok = AuthOk;
    type Error = AuthError;

    fn into_result(self) -> Result<AuthOk, AuthError> {
        match self {
            AuthResponse::Ok(ok) => Ok(ok),
            AuthResponse::Err(err) => Err(err),
        }
    }

    fn from_ok(ok: AuthOk) -> Self {
        AuthResponse::Ok(ok)
    }

    fn from_error(err: AuthError) -> Self {
        AuthResponse::Err(err)
    }
}

impl Into<Vec<u8>> for AuthResponse {
    fn into(self) -> Vec<u8> {
        use prost::Message;

        let mut buf = Vec::new();
        proto::requests::auth::AuthResponse::from(self)
            .encode(&mut buf)
            .unwrap();
        buf
    }
}

impl From<AuthResponse> for proto::requests::auth::AuthResponse {
    fn from(result: AuthResponse) -> Self {
        use proto::requests::auth::auth_response::Response;

        let inner = match result {
            AuthResponse::Ok(ok) => Response::Ok(ok.into()),
            AuthResponse::Err(err) => {
                let error: proto::requests::auth::AuthError = err.into();
                Response::Error(error as i32)
            }
        };

        proto::requests::auth::AuthResponse {
            response: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::auth::AuthResponse> for AuthResponse {
    type Error = DeserializeError;

    fn try_from(response: proto::requests::auth::AuthResponse) -> Result<Self, Self::Error> {
        use proto::requests::auth::auth_response::Response;

        Ok(match response.response? {
            Response::Ok(ok) => AuthResponse::Ok(ok.try_into()?),
            Response::Error(err) => {
                let error = proto::requests::auth::AuthError::from_i32(err)
                    .ok_or(DeserializeError::InvalidEnumVariant)?;
                AuthResponse::Err(error.try_into()?)
            }
        })
    }
}

#[derive(Debug, Clone)]
pub enum AuthOk {
    User(UserId),
    Token(NewToken),
    NoData,
}

impl From<AuthOk> for proto::requests::auth::AuthOk {
    fn from(ok: AuthOk) -> Self {
        use proto::requests::auth::auth_ok::Ok;
        use AuthOk::*;

        let inner = match ok {
            User(user) => Ok::User(user.into()),
            Token(token) => Ok::Token(token.into()),
            NoData => Ok::NoData(proto::types::None {}),
        };

        proto::requests::auth::AuthOk { ok: Some(inner) }
    }
}

impl TryFrom<proto::requests::auth::AuthOk> for AuthOk {
    type Error = DeserializeError;

    fn try_from(ok: proto::requests::auth::AuthOk) -> Result<Self, Self::Error> {
        use proto::requests::auth::auth_ok::Ok::*;

        Ok(match ok.ok? {
            User(user) => AuthOk::User(user.try_into()?),
            Token(token) => AuthOk::Token(token.try_into()?),
            NoData(_) => AuthOk::NoData,
        })
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AuthError {
    Internal,
    WrongEndpoint,
    IncorrectCredentials,
    InvalidToken,
    StaleToken,
    TokenInUse,
    InvalidUser,
    UserCompromised,
    UserLocked,
    UserBanned,
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidPassword,
    InvalidDisplayName,
    InvalidMessage,
}

macro_rules! convert_to_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(AuthError::$variant => proto::requests::auth::AuthError::$variant,)*
        }
    };
}

macro_rules! convert_from_proto {
    ($err:ident: { $($variant:ident$(,)?)* }) => {
        match $err {
            $(proto::requests::auth::AuthError::$variant => Ok(AuthError::$variant),)*
        }
    };
}

impl From<AuthError> for proto::requests::auth::AuthError {
    fn from(err: AuthError) -> Self {
        convert_to_proto! {
            err: {
                Internal,
                WrongEndpoint,
                IncorrectCredentials,
                InvalidToken,
                StaleToken,
                TokenInUse,
                InvalidUser,
                UserCompromised,
                UserLocked,
                UserBanned,
                UsernameAlreadyExists,
                InvalidUsername,
                InvalidPassword,
                InvalidDisplayName,
                InvalidMessage
            }
        }
    }
}

impl TryFrom<proto::requests::auth::AuthError> for AuthError {
    type Error = DeserializeError;

    fn try_from(err: proto::requests::auth::AuthError) -> Result<Self, Self::Error> {
        convert_from_proto! {
            err: {
                Internal,
                WrongEndpoint,
                IncorrectCredentials,
                InvalidToken,
                StaleToken,
                TokenInUse,
                InvalidUser,
                UserCompromised,
                UserLocked,
                UserBanned,
                UsernameAlreadyExists,
                InvalidUsername,
                InvalidPassword,
                InvalidDisplayName,
                InvalidMessage
            }
        }
    }
}

impl From<DeserializeError> for AuthError {
    fn from(_: DeserializeError) -> Self {
        AuthError::InvalidMessage
    }
}
