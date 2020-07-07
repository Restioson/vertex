#![feature(try_blocks)]

use std::env;
use vertex::prelude::{Message as VertexMessage, *};
use vertex_client::{AuthParameters, Client, EventHandler};
use std::sync::{Arc, Mutex};
use panda::client::SessionData;
use panda::models::{ExecuteWebhook, WebhookPayload};
use url::Url;
use xtra::prelude::*;
use serde::{Serialize, Deserialize};
use bimap::BiHashMap;
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;
use either::Either;

struct Handler(Bridge);

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&mut self, _client: &mut Client) {
        println!("vertex ready!");
    }

    async fn add_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: VertexMessage,
        client: &mut Client,
    ) {
        if message.author == client.user().await {
            return;
        }

        self.0.vertex_message(community, room, message).await;
    }

    async fn error(&mut self, err: vertex_client::Error, _client: Option<&mut Client>) {
        panic!("{:#?}", err);
    }
}

#[spaad::entangled]
struct Bridge {
    vertex: Option<Client>,
    discord: Option<Arc<SessionData<DiscordState>>>,
    channel_map: BiHashMap<DiscordChannelId, VertexChannelId>,
    webhooks: HashMap<DiscordChannelId, Webhook>,
    bridge_requests: HashMap<Uuid, DiscordChannelId>,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct Webhook {
    id: String,
    token: String,
}

#[serde(transparent)]
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct DiscordChannelId {
    channel: String,
}

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
struct VertexChannelId {
    community: CommunityId,
    room: RoomId,
}

#[derive(Debug, Copy, Clone)]
pub enum Error {
    AlreadyBridged,
    NotBridged,
    InvalidBridgeId,
}

impl From<uuid::Error> for Error {
    fn from(_: uuid::Error) -> Error {
        Error::InvalidBridgeId
    }
}

impl From<Error> for &'static str {
    fn from(err: Error) -> &'static str {
        match err {
            Error::AlreadyBridged => "Channel already bridged.",
            Error::NotBridged => "Channel was not bridged.",
            Error::InvalidBridgeId => "Invalid bridging request id.",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.clone().into())
    }
}

impl std::error::Error for Error {}

#[spaad::entangled]
impl Actor for Bridge {}

#[spaad::entangled]
impl Bridge {
    #[spaad::spawn]
    fn new() -> Self {
        let (webhooks, channel_map) = Self::load();

        Bridge {
            vertex: None,
            discord: None,
            channel_map,
            webhooks,
            bridge_requests: HashMap::new(),
        }
    }

    #[spaad::handler]
    fn init_vertex(&mut self, vertex: Client) {
        self.vertex = Some(vertex);
    }

    #[spaad::handler]
    fn init_discord(&mut self, discord: Arc<SessionData<DiscordState>>) {
        self.discord = Some(discord);
    }

    #[spaad::handler]
    async fn discord_message(&mut self, message: DiscordMessage) {
        let chan = self.channel_map.get_by_left(&message.channel);
        if let (Some(chan), Some(vertex)) = (chan, self.vertex.as_ref()) {
            let content = format!("{}: {}", message.author, message.content);
            vertex
                .send_message(chan.community, chan.room, content)
                .await
                .unwrap();
        }
    }

    #[spaad::handler]
    async fn vertex_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: VertexMessage,
    ) {
        let vertex_channel = VertexChannelId { room, community };
        let content = message.content.clone().unwrap_or_default();
        dbg!(&content);
        let res: Result<Option<&str>, Error> = try {
            if content.starts_with("!db") {
                let args: Vec<&str> = content.split(' ').collect();
                match args.get(1) {
                    Some(&"confirm") => {
                        let id = args.get(2).ok_or(Error::InvalidBridgeId)?;
                        let id = id.parse()?;
                        self.bridge_request_complete(id, vertex_channel).await?;
                        Some("Rooms successfully bridged")
                    },
                    _ => Some("Unknown command. Commands: `!db confirm [id]`."),
                }

            } else {
                None
            }
        };

        let vertex = self.vertex.as_ref().unwrap();

        let msg = match res {
            Ok(Some(msg)) => Some(msg),
            Err(e) => Some(e.into()),
            Ok(None) => None,
        };

        if let Some(msg) = msg {
            vertex
                .send_message(community, room, msg.to_string())
                .await
                .unwrap();
        }

        let chan = self.channel_map.get_by_right(&vertex_channel);
        if let (Some(chan), Some(discord), Some(content)) = (chan, &self.discord, &message.content) {
            let webhook = self.webhooks.get(chan).unwrap();
            let profile: Result<Profile, vertex_client::Error> = vertex
                .get_profile(message.author, message.author_profile_version)
                .await
                .into();
            let profile = profile.unwrap();
            let payload = ExecuteWebhook {
                payload: WebhookPayload::MessageContent(content.clone()),
                username: Some(profile.username),
                avatar_url: None,
                tts: None,
            };
            discord.http.execute_webhook(&webhook.id, &webhook.token, payload).await.unwrap();
        }

        println!("{:?} in {:?} in {:?}", message, community, room);
    }

    #[spaad::handler]
    fn bridge_request_start(
        &mut self,
        channel: DiscordChannelId
    ) -> Result<Uuid, Error> {
        if self.channel_map.get_by_left(&channel).is_some() {
            return Err(Error::AlreadyBridged);
        }

        let id = Uuid::new_v4();
        self.bridge_requests.insert(id, channel);
        Ok(id)
    }

    async fn bridge_request_complete(
        &mut self,
        req_id: Uuid,
        vertex: VertexChannelId,
    ) -> Result<(), Error> {
        const AVATAR: &[u8] = include_bytes!("../../../client/gtk/res/icon.png");

        let channel = self.bridge_requests.remove(&req_id).ok_or(Error::InvalidBridgeId)?;
        self.channel_map.insert(channel.clone(), vertex);

        let b64 = base64::encode(AVATAR);
        let discord = self.discord.as_ref().unwrap();
        let webhook = discord.http.create_webhook(
            channel.channel.clone(),
            "vertex_bridge".to_string(),
            Some(format!("data:image/png;base64,{}", b64))
        ).await.unwrap();
        self.webhooks.insert(
            channel,
            Webhook { id: webhook.id, token: webhook.token.unwrap() }
        );
        self.save();
        Ok(())
    }

    #[spaad::handler]
    async fn unbridge(&mut self, channel: DiscordChannelId) -> Result<(), Error> {
        self.channel_map.remove_by_left(&channel).ok_or(Error::NotBridged)?;
        let webhook = self.webhooks.get(&channel).unwrap();
        let discord = self.discord.as_ref().unwrap();
        discord.http.delete_webhook(webhook.id.to_string()).await.unwrap();
        self.save();
        Ok(())
    }

    fn save(&self) {
        let channel_map = serde_json::to_string(&self.channel_map).unwrap();
        let webhook_map = serde_json::to_string(&self.webhooks).unwrap();
        let dirs = directories_next::ProjectDirs::from("", "vertex_chat", "vertex_discord_bridge")
            .expect("Error getting project directories");
        let file_channels = dirs.data_dir().join("channel_map.json");
        let file_webhooks = dirs.data_dir().join("webhooks.json");
        std::fs::write(file_channels, channel_map).unwrap();
        std::fs::write(file_webhooks, webhook_map).unwrap();
    }

    fn load() -> (HashMap<DiscordChannelId, Webhook>, BiHashMap<DiscordChannelId, VertexChannelId>) {
        let dirs = directories_next::ProjectDirs::from("", "vertex_chat", "vertex_discord_bridge")
            .expect("Error getting project directories");
        let file = dirs.data_dir().join("channel_map.json");
        let json = match std::fs::read_to_string(&file) {
            Ok(js) => js,
            Err(_) => {
                std::fs::write(&file, b"{}").unwrap();
                "{}".to_string()
            },
        };
        let channel_map = serde_json::from_str(&json).unwrap();
        let file = dirs.data_dir().join("webhooks.json");
        let json = match std::fs::read_to_string(&file) {
            Ok(js) => js,
            Err(_) => {
                std::fs::write(&file, b"{}").unwrap();
                "{}".to_string()
            },
        };
        let webhooks = serde_json::from_str(&json).unwrap();
        (webhooks, channel_map)
    }
}

pub fn get_auth_params() -> AuthParameters {
    let dev = env::var("VERTEX_BOT_DEVICE").unwrap().parse().unwrap();
    let token = env::var("VERTEX_BOT_TOKEN").unwrap();

    AuthParameters {
        instance: Url::parse("https://vertex.cf").unwrap(),
        device: DeviceId(dev),
        token: AuthToken(token),
    }
}

struct DiscordState {
    user_id: Mutex<Option<String>>,
    bridge: Bridge,
}

#[derive(Debug, Clone)]
struct DiscordMessage {
    content: String,
    channel: DiscordChannelId,
    author: String,
}

impl DiscordState {
    fn new(bridge: Bridge) -> Self {
        DiscordState {
            user_id: Mutex::new(None),
            bridge,
        }
    }
}

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{}", info);
        std::process::exit(1);
    }));

    #[allow(unused_mut)]
    let mut bridge = Bridge::new();
    let mut discord: panda::client::Client<DiscordState> = panda::new_with_state(
        env::var("DISCORD_TOKEN").unwrap(),
        DiscordState::new(bridge.clone()),
    ).await.unwrap();
    let builder = Client::connect(get_auth_params(), true).await.unwrap();
    let client = builder.start_with_handler(Handler(bridge.clone())).await;

    discord.on_ready(|data, ready| {
        let data_cloned = data.clone();
        let _ = data.state.bridge.init_discord(data_cloned);
        *data.state.user_id.lock().unwrap() = Some(ready.user.id);
        println!("discord ready!");
        async { Ok(()) }
    });

    discord.on_message_create(|data, message| async move {
        let id = {
            let lock = data.state.user_id.lock().unwrap();
            lock.as_ref().unwrap().clone()
        };

        if message.0.author.id == id {
            return Ok(());
        }

        let res: Result<_, &'static str> = try {
            let msg = DiscordMessage {
                content: message.0.content.clone(),
                channel: DiscordChannelId { channel: message.0.channel_id.clone() },
                author: message.0.author.username.clone(),
            };
            data.state.bridge.discord_message(msg).await;

            if message.0.mentions.iter().any(|user| user.id == id)
                || message.0.content.starts_with("!vb")
            {
                let args: Vec<&str> = message.0.content.split(' ').collect();

                match args.get(1) {
                    Some(&"bridge") => {
                        let channel = message.0.channel_id.clone();
                        if message.0.guild_id.is_none() {
                            Err("Only guild channels supported")?;
                        }

                        // lol
                        if message.0.author.id != "160780685190103040" {
                            Err("Only Restioson can run this command")?;
                        }

                        let uuid = data.state.bridge
                            .bridge_request_start(DiscordChannelId { channel })
                            .await?;
                        Some(Either::Left(uuid))
                    },
                    Some(&"unbridge") => {
                        let channel = message.0.channel_id.clone();
                        if message.0.guild_id.is_none() {
                            Err("Only guild channels supported")?;
                        }

                        data.state.bridge
                            .unbridge(DiscordChannelId { channel })
                            .await?;
                        Some(Either::Right("Successfully unbridged."))
                    }
                    _ => Err("Unknown command. Commands: `!vb bridge`, `!vb unbridge`.")?,
                }
            } else {
                None
            }
        };

        match res {
            Ok(Some(Either::Left(id))) => {
                data.http.send_message(
                    &message.0.channel_id,
                    &format!("Successfully began bridge. Id: {}", id),
                ).await.unwrap();
            }
            Ok(Some(Either::Right(txt))) => {
                data.http.send_message(&message.0.channel_id, txt).await.unwrap();
            }
            Ok(None) => {}
            Err(e) => {
                data.http.send_message(&message.0.channel_id, e).await.unwrap();
            }
        }

        Ok(())
    });

    let _ = bridge.init_vertex(client);
    discord.start().await.unwrap();
}
