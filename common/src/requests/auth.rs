use serde::{Deserialize, Serialize};
use crate::types::*;
use crate::requests::TokenCreationOptions;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateRequest {
    pub device: DeviceId,
    pub token: AuthToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenRequest {
    pub credentials: UserCredentials,
    pub options: TokenCreationOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTokenResponse {
    pub device: DeviceId,
    pub token: AuthToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenRequest {
    pub credentials: UserCredentials,
    pub device: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeTokenRequest {
    pub credentials: UserCredentials,
    pub device: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterUserRequest {
    pub credentials: UserCredentials,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterUserResponse {
    pub user: UserId,
}

pub type AuthResult<T> = Result<T, AuthError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AuthError {
    Internal,
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
}
