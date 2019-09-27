use crate::config::Config;
use crate::database::User;
use futures::{future, Future};
use lazy_static::lazy_static;
use rand::RngCore;
use tokio_threadpool::ThreadPool;
use unicode_normalization::UnicodeNormalization;
use vertex_common::{ServerError, UserId};

pub const MAX_TOKEN_LENGTH: usize = 45;

lazy_static! {
    static ref THREAD_POOL: ThreadPool = ThreadPool::new();
}

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

pub fn hash<E: Send + 'static>(
    pass: String,
) -> impl Future<Item = (String, HashSchemeVersion), Error = E> {
    THREAD_POOL.spawn_handle(future::poll_fn(move || {
        tokio_threadpool::blocking(|| {
            let mut salt: [u8; 32] = [0; 32]; // 256 bits
            rand::thread_rng().fill_bytes(&mut salt);
            let config = Default::default();

            let hash = argon2::hash_encoded(pass.as_bytes(), &salt, &config)
                .expect("Error generating password hash");

            (hash, HashSchemeVersion::Argon2V1)
        })
        .map_err(|_| panic!("the threadpool shut down"))
    }))
}

pub fn verify<E: Send + 'static>(
    pass: String,
    hash: String,
    scheme_version: HashSchemeVersion,
) -> impl Future<Item = bool, Error = E> {
    THREAD_POOL.spawn_handle(future::poll_fn(move || {
        tokio_threadpool::blocking(|| {
            use HashSchemeVersion::*;

            match scheme_version {
                Argon2V1 => argon2::verify_encoded(&hash, pass.as_bytes())
                    .expect("Error verifying password hash"),
            }
        })
        .map_err(|_| panic!("the threadpool shut down"))
    }))
}

pub fn verify_user_password<E: Send + 'static>(
    user: User,
    password: String,
) -> impl Future<Item = Result<UserId, ServerError>, Error = E> {
    let User {
        id: user_id,
        password_hash,
        hash_scheme_version,
        ..
    } = user;

    verify(password, password_hash, hash_scheme_version).map(move |matches| {
        if matches {
            Ok(user_id)
        } else {
            Err(ServerError::IncorrectUsernameOrPassword)
        }
    })
}
