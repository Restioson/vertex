use chrono::Utc;
use rand::RngCore;
use uuid::Uuid;

use vertex::prelude::*;

use crate::auth;
use crate::database;

pub struct Authenticator {
    pub global: crate::Global,
}

impl Authenticator {
    pub async fn login(
        &self,
        device: DeviceId,
        pass: AuthToken,
    ) -> Result<(UserId, DeviceId, TokenPermissionFlags, AdminPermissionFlags), AuthError> {
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
        } else if (Utc::now() - token.last_used).num_days()
            > self.global.config.token_stale_days as i64
        {
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

        if self.global.database.refresh_token(device).await?.is_err() {
            return Err(AuthError::InvalidToken);
        }

        let admin_perms = self.global.database.get_admin_permissions(user).await?;

        Ok((user, device, permission_flags, admin_perms))
    }

    pub async fn create_user(
        &self,
        credentials: Credentials,
        display_name: String,
    ) -> AuthResponse {
        if !auth::valid_password(&credentials.password, &self.global.config) {
            return AuthResponse::Err(AuthError::InvalidPassword);
        }

        let username = match auth::prepare_username(&credentials.username, &self.global.config) {
            Ok(name) => name,
            Err(auth::TooShort) => return AuthResponse::Err(AuthError::InvalidUsername),
        };

        if !auth::valid_display_name(&display_name, &self.global.config) {
            return AuthResponse::Err(AuthError::InvalidDisplayName);
        }

        let (hash, hash_version) = auth::hash(credentials.password).await;

        let user = database::UserRecord::new(username, display_name, hash, hash_version);
        let user_id = user.id;

        match self.global.database.create_user(user).await? {
            Ok(()) => AuthResponse::Ok(AuthOk::User(user_id)),
            Err(_) => AuthResponse::Err(AuthError::UsernameAlreadyExists),
        }
    }

    pub async fn create_token(
        &self,
        credentials: Credentials,
        options: TokenCreationOptions,
    ) -> AuthResponse {
        let user = match self.verify_credentials(credentials).await? {
            AuthOk::User(user) => user,
            _ => return AuthResponse::Err(AuthError::InvalidMessage),
        };

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
            expiration_date: options.expiration_datetime,
            permission_flags: options.permission_flags,
        };

        if self.global.database.create_token(db_token).await?.is_err() {
            // The chances of a UUID conflict is so abysmally low that we can only assume that a
            // conflict is due to a programming error

            panic!("Newly generated UUID conflicts with another!");
        }

        AuthResponse::Ok(AuthOk::Token(NewToken {
            device,
            token: auth_token,
        }))
    }

    pub async fn refresh_token(
        &self,
        credentials: Credentials,
        to_refresh: DeviceId,
    ) -> AuthResponse {
        self.verify_credentials(credentials).await?;
        match self.global.database.refresh_token(to_refresh).await? {
            Ok(_) => AuthResponse::Ok(AuthOk::NoData),
            Err(_) => AuthResponse::Err(AuthError::InvalidToken),
        }
    }

    pub async fn revoke_token(
        &self,
        credentials: Credentials,
        to_revoke: DeviceId,
    ) -> AuthResponse {
        self.verify_credentials(credentials).await?;
        match self.global.database.revoke_token(to_revoke).await? {
            Ok(_) => AuthResponse::Ok(AuthOk::NoData),
            Err(_) => AuthResponse::Err(AuthError::InvalidToken),
        }
    }

    async fn verify_credentials(&self, credentials: Credentials) -> AuthResponse {
        let username = auth::normalize_username(&credentials.username, &self.global.config);
        let password = credentials.password;

        let user = match self.global.database.get_user_by_name(username).await? {
            Some(user) => user,
            None => return AuthResponse::Err(AuthError::InvalidUser),
        };

        let id = user.id;
        if auth::verify_user(user, password).await {
            AuthResponse::Ok(AuthOk::User(id))
        } else {
            AuthResponse::Err(AuthError::IncorrectCredentials)
        }
    }
}
