// configuration framework rewrite time. very epic

use directories::ProjectDirs;
use openssl::pkey::PKey;
use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind, Read};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "max_password_len")]
    pub max_password_len: u16,
    #[serde(default = "min_password_len")]
    pub min_password_len: u16,
    #[serde(default = "max_username_len")]
    pub max_username_len: u16,
    #[serde(default = "min_username_len")]
    pub min_username_len: u16,
    #[serde(default = "max_display_name_len")]
    pub max_display_name_len: u16,
    #[serde(default = "min_display_name_len")]
    pub min_display_name_len: u16,
    #[serde(default = "profile_pictures")]
    pub profile_pictures: PathBuf,
    #[serde(default = "tokens_sweep_interval_secs")]
    pub tokens_sweep_interval_secs: u64,
    #[serde(default = "token_stale_days")]
    pub token_stale_days: u16,
    #[serde(default = "token_expiry_days")]
    pub token_expiry_days: u16,
}

fn max_password_len() -> u16 {
    1000
}

fn min_password_len() -> u16 {
    12
}

fn max_username_len() -> u16 {
    64
}

fn min_username_len() -> u16 {
    1
}

fn max_display_name_len() -> u16 {
    64
}

fn min_display_name_len() -> u16 {
    1
}

fn profile_pictures() -> PathBuf {
    PathBuf::from("./files/images/profile_pictures/")
}

fn tokens_sweep_interval_secs() -> u64 {
    1800 // 30min
}

fn token_stale_days() -> u16 {
    7 // 1 week
}

fn token_expiry_days() -> u16 {
    90 // ~3 months
}

pub fn load_config() -> Config {
    let dirs = ProjectDirs::from("", "vertex_chat", "vertex_server")
        .expect("Error getting project directories");
    let config_dir = dirs.config_dir();
    let config_file = config_dir.join("config.toml");
    let res = fs::read_to_string(&config_file);

    let config_str = match res {
        Ok(s) => s,
        Err(ref e) if e.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(config_dir).expect(&format!(
                "Error creating config dirs ({})",
                config_dir.to_string_lossy(),
            ));

            File::create(&config_file).expect(&format!(
                "Error creating config file ({})",
                config_file.to_string_lossy(),
            ));

            fs::read_to_string(&config_file).expect(&format!(
                "Error reading config file ({}). Error",
                config_file.to_string_lossy(),
            ))
        }
        Err(e) => panic!(
            "Error reading config file. It is expected to be here: {}. Error: {:?}",
            config_file.to_string_lossy(),
            e,
        ),
    };

    let config: Config = toml::from_str(&config_str).expect("Invalid config file");

    // Validate config
    if config.min_password_len < 8 {
        panic!("Minimum password length must be greater than 8");
    }

    if config.max_password_len < config.min_password_len {
        panic!("Maximum password length must be greater or equal to than minimum password length");
    }

    if config.min_username_len < 1 {
        panic!("Minimum username length must be greater than or equal to 1");
    }

    if config.max_username_len < config.min_username_len {
        panic!("Maximum username length must be greater than or equal to minimum username length");
    }

    if config.min_display_name_len < 1 {
        panic!("Minimum display name length must be greater than or equal to 1");
    }

    if config.max_display_name_len < config.min_username_len {
        panic!("Maximum display name length must be greater than or equal to minimum display name length");
    }

    if config.tokens_sweep_interval_secs < 60 {
        panic!("Tokens sweep interval must be greater than 1 minute!");
    }

    config
}

pub fn ssl_config() -> SslAcceptorBuilder {
    let dirs = ProjectDirs::from("", "vertex_chat", "vertex_server")
        .expect("Error getting project directories");
    let dir = dirs.config_dir();

    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let key_file = &mut BufReader::new(File::open(key_path.clone()).expect(&format!(
        "Error opening private key file ({})",
        key_path.to_string_lossy()
    )));
    let mut key_data = Vec::new();
    key_file
        .read_to_end(&mut key_data)
        .expect("Error reading private key file");
    let passphrase = env::var("VERTEX_SERVER_KEY_PASS")
        .expect("Error getting the private key passphrase from $VERTEX_SERVER_KEY_PASS");
    let passphrase = passphrase.as_bytes();
    let key = PKey::private_key_from_pem_passphrase(&key_data, passphrase)
        .expect("Error loading private key");

    let mut acceptor = SslAcceptor::mozilla_modern(SslMethod::tls())
        .expect("Error getting Mozilla modern ssl acceptor");
    acceptor
        .set_certificate_file(cert_path, SslFiletype::PEM)
        .expect("Error setting certificate");
    acceptor
        .set_private_key(&key)
        .expect("Error setting private key");

    acceptor
}
