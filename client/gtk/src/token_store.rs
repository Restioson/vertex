use keyring::Keyring;
use serde_json;

use crate::AuthParameters;

fn keyring() -> Keyring<'static> {
    Keyring::new("vertex_client_gtk", "")
}

// TODO: pass errors down?
pub fn store_token(parameters: &AuthParameters) {
    let serialized_token = serde_json::to_string(parameters).expect("unable to serialize token");
    keyring().set_password(&serialized_token)
        .expect("unable to store token");
}

pub fn get_stored_token() -> Option<AuthParameters> {
    keyring().get_password().ok()
        .and_then(|token_str| serde_json::from_str::<AuthParameters>(&token_str).ok())
}

pub fn forget_token() {
    keyring().delete_password().expect("unable to forget token");
}
