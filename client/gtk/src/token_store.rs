use keyring::Keyring;
use serde::{Deserialize, Serialize};
use serde_json;

use vertex::*;

#[derive(Serialize, Deserialize, Clone)]
struct StoredToken {
    device: DeviceId,
    token: String,
}

pub struct TokenStore {
    keyring: Keyring<'static>,
}

impl TokenStore {
    pub(crate) fn new() -> TokenStore {
        TokenStore {
            keyring: Keyring::new("vertex_client_gtk", ""),
        }
    }

    // TODO: pass errors down?
    pub fn store_token(&self, device: DeviceId, token: AuthToken) {
        let stored_token = StoredToken {
            device,
            token: token.0,
        };
        let serialized_token = serde_json::to_string(&stored_token).expect("unable to serialize token");
        self.keyring.set_password(&serialized_token)
            .expect("unable to store token");
    }

    pub fn get_stored_token(&self) -> Option<(DeviceId, AuthToken)> {
        self.keyring.get_password().ok()
            .and_then(|token_str| serde_json::from_str::<StoredToken>(&token_str).ok())
            .map(|stored| (stored.device, AuthToken(stored.token)))
    }

    pub fn forget_token(&self) {
        self.keyring.delete_password().expect("unable to forget token");
    }
}
