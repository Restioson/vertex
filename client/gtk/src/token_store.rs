use keyring::Keyring;
use serde::{Deserialize, Serialize};
use serde_json;

use vertex::*;

use crate::Server;

#[derive(Serialize, Deserialize, Clone)]
struct StoredToken {
    server: Server,
    device: DeviceId,
    token: String,
}

fn keyring() -> Keyring<'static> {
    Keyring::new("vertex_client_gtk", "")
}

// TODO: pass errors down?
pub fn store_token(server: Server, device: DeviceId, token: AuthToken) {
    let stored_token = StoredToken {
        server,
        device,
        token: token.0,
    };
    let serialized_token = serde_json::to_string(&stored_token).expect("unable to serialize token");
    keyring().set_password(&serialized_token)
        .expect("unable to store token");
}

pub fn get_stored_token() -> Option<(DeviceId, AuthToken)> {
    keyring().get_password().ok()
        .and_then(|token_str| serde_json::from_str::<StoredToken>(&token_str).ok())
        .map(|stored| (stored.device, AuthToken(stored.token)))
}

pub fn forget_token() {
    keyring().delete_password().expect("unable to forget token");
}
