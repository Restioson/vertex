use keyring::Keyring;
use vertex::prelude::*;
use vertex_client::{AuthParameters, Client, EventHandler};

struct Handler;

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&mut self, _client: Client) {
        println!("ready!");
    }

    async fn add_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: Message,
        _client: Client,
    ) {
        println!("{:?} in {:?} in {:?}", message, community, room);
    }
}

pub fn get_stored_token() -> Option<AuthParameters> {
    Keyring::new("vertex_client_gtk", "")
        .get_password()
        .ok()
        .and_then(|token_str| serde_json::from_str::<AuthParameters>(&token_str).ok())
}

#[tokio::main]
async fn main() {
    Client::start(get_stored_token().unwrap(), Handler)
        .await
        .unwrap()
        .1
        .await
}
