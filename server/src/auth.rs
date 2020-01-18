use crate::config::Config;
use crate::database::UserRecord;
use futures::Future;
use futures::FutureExt;
use rand::RngCore;
use unicode_normalization::UnicodeNormalization;
use vertex_common::{ErrResponse, UserId};

pub const MAX_TOKEN_LENGTH: usize = 45;

#[derive(Debug)]
#[repr(u8)]
pub enum HashSchemeVersion {
    Argon2V1 = 1,
}

impl From<i16> for HashSchemeVersion {
    fn from(v: i16) -> Self {
        match v {
            1 => HashSchemeVersion::Argon2V1,
            _ => panic!("Invalid hash scheme version {}"),
        }
    }
}

pub fn valid_password(password: &str, config: &Config) -> bool {
    password.len() <= config.max_password_len as usize
        && password.len() >= config.min_password_len as usize
}

pub fn valid_display_name(display_name: &str, config: &Config) -> bool {
    display_name.len() <= config.max_display_name_len as usize
        && display_name.len() >= config.min_display_name_len as usize
}

fn valid_username(username: &str, config: &Config) -> bool {
    username.len() <= config.max_username_len as usize
        && username.len() >= config.min_username_len as usize
}

pub struct TooShort;

pub fn process_username(username: &str, _config: &Config) -> String {
    username.nfkc().flat_map(|c| c.to_lowercase()).collect()
}

pub fn prepare_username(username: &str, config: &Config) -> Result<String, TooShort> {
    if valid_username(username, config) {
        Ok(process_username(username, config))
    } else {
        Err(TooShort)
    }
}

// The `<E: Send + 'static>`s here are to allow the caller to specify an error type for easier use,
// since this will never return an error

pub fn hash(pass: String) -> impl Future<Output = (String, HashSchemeVersion)> {
    tokio::task::spawn_blocking(move || {
        let mut salt: [u8; 32] = [0; 32]; // 256 bits
        rand::thread_rng().fill_bytes(&mut salt);
        let config = Default::default();

        let hash = argon2::hash_encoded(pass.as_bytes(), &salt, &config)
            .expect("Error generating password hash");

        (hash, HashSchemeVersion::Argon2V1)
    })
    .map(|r| r.expect("auth error"))
}

pub fn verify(
    pass: String,
    hash: String,
    scheme_version: HashSchemeVersion,
) -> impl Future<Output = bool> {
    tokio::task::spawn_blocking(move || {
        use HashSchemeVersion::*;

        match scheme_version {
            Argon2V1 => argon2::verify_encoded(&hash, pass.as_bytes())
                .expect("Error verifying password hash"),
        }
    })
    .map(|r| r.expect("auth error"))
}

pub async fn verify_user_password(
    user: UserRecord,
    password: String,
) -> Result<UserId, ErrResponse> {
    let UserRecord {
        id: user,
        password_hash,
        hash_scheme_version,
        ..
    } = user;
    let matches = verify(password, password_hash, hash_scheme_version).await;
    if matches {
        Ok(user)
    } else {
        Err(ErrResponse::IncorrectUsernameOrPassword)
    }
}
