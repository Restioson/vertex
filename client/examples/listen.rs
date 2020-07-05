use vertex::prelude::*;
use vertex_client::{AuthParameters, Client, EventHandler};
use keyring::Keyring;

struct Handler;

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&mut self, _client: &mut Client) {
        println!("ready!");
    }

    async fn add_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: Message,
        _client: &mut Client,
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
    let client = Client::connect(get_stored_token().unwrap(), true).await.unwrap();
    client.start_with_handler(Handler).await
}

