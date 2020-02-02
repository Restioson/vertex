use std::sync::Arc;

use chrono::Utc;
use rand::RngCore;
use uuid::Uuid;

use vertex::*;

use crate::auth;
use crate::config::Config;
use crate::database::{self, Database};

pub struct Authenticator {
    pub global: crate::Global,
}

impl Authenticator {
    pub async fn authenticate(&self, device: DeviceId, pass: AuthToken) -> AuthResult<(UserId, DeviceId, TokenPermissionFlags)> {
        let token = match self.global.database.get_token(device).await? {
            Some(token) => token,
            None => return Err(AuthError::InvalidToken),
        };

        let user = match self.global.database.get_user_by_id(token.user).await? {
            Some(user) => user,
            None => return Err(AuthError::InvalidUser),
        };

        // Check if can log in with this token
        if user.locked {
            return Err(AuthError::UserLocked);
        } else if user.banned {
            return Err(AuthError::UserBanned);
        } else if user.compromised {
            return Err(AuthError::UserCompromised);
        } else if (Utc::now() - token.last_used).num_days() > self.global.config.token_stale_days as i64 {
            return Err(AuthError::StaleToken);
        }

        if pass.0.len() > auth::MAX_TOKEN_LENGTH {
            return Err(AuthError::InvalidToken);
        }

        let database::Token {
            token_hash,
            hash_scheme_version,
            user,
            permission_flags,
            ..
        } = token;

        if !auth::verify(pass.0, token_hash, hash_scheme_version).await {
            return Err(AuthError::InvalidToken);
        }

        if let Err(_) = self.global.database.refresh_token(device).await? {
            return Err(AuthError::InvalidToken);
        }

        Ok((user, device, permission_flags))
    }

    pub  async fn create_user(
        &self,
        credentials: UserCredentials,
        display_name: String,
    ) -> AuthResult<RegisterUserResponse> {
        if !auth::valid_password(&credentials.password, &self.global.config) {
            return Err(AuthError::InvalidPassword);
        }

        let username = match auth::prepare_username(&credentials.username, &self.global.config) {
            Ok(name) => name,
            Err(auth::TooShort) => return Err(AuthError::InvalidUsername),
        };

        if !auth::valid_display_name(&display_name, &self.global.config) {
            return Err(AuthError::InvalidDisplayName);
        }

        let (hash, hash_version) = auth::hash(credentials.password).await;

        let user = database::UserRecord::new(username, display_name, hash, hash_version);
        let user_id = user.id;

        match self.global.database.create_user(user).await? {
            Ok(()) => Ok(RegisterUserResponse { user: user_id }),
            Err(_) => Err(AuthError::UsernameAlreadyExists),
        }
    }

    pub async fn create_token(
        &self,
        credentials: UserCredentials,
        options: TokenCreationOptions,
    ) -> AuthResult<CreateTokenResponse> {
        let user = self.verify_credentials(credentials).await?;

        let mut token_bytes: [u8; 32] = [0; 32]; // 256 bits
        rand::thread_rng().fill_bytes(&mut token_bytes);

        let token = base64::encode(&token_bytes);

        let auth_token = AuthToken(token.clone());
        let (token_hash, hash_scheme_version) = auth::hash(token).await;

        let device = DeviceId(Uuid::new_v4());
        let db_token = database::Token {
            token_hash,
            hash_scheme_version,
            user,
            device,
            device_name: options.device_name,
            last_used: Utc::now(),
            expiration_date: options.expiration_date,
            permission_flags: options.permission_flags,
        };

        if let Err(_) = self.global.database.create_token(db_token).await? {
            // The chances of a UUID conflict is so abysmally low that we can only assume that a
            // conflict is due to a programming error

            panic!("Newly generated UUID conflicts with another!");
        }

        Ok(CreateTokenResponse { device, token: auth_token })
    }

    pub async fn refresh_token(&self, credentials: UserCredentials, to_refresh: DeviceId) -> AuthResult<()> {
        self.verify_credentials(credentials).await?;
        match self.global.database.refresh_token(to_refresh).await? {
            Ok(_) => Ok(()),
            Err(_) => Err(AuthError::InvalidToken)
        }
    }

    pub async fn revoke_token(&self, credentials: UserCredentials, to_revoke: DeviceId) -> AuthResult<()> {
        self.verify_credentials(credentials).await?;
        match self.global.database.revoke_token(to_revoke).await? {
            Ok(_) => Ok(()),
            Err(_) => Err(AuthError::InvalidToken)
        }
    }

    async fn verify_credentials(&self, credentials: UserCredentials) -> AuthResult<UserId> {
        let username = auth::normalize_username(&credentials.username, &self.global.config);
        let password = credentials.password;

        let user = match self.global.database.get_user_by_name(username).await? {
            Some(user) => user,
            None => return Err(AuthError::InvalidUser),
        };

        let id = user.id;
        if auth::verify_user(user, password).await {
            Ok(id)
        } else {
            Err(AuthError::IncorrectCredentials)
        }
    }
}
