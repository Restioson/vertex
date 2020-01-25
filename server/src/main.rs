#![feature(type_alias_impl_trait, generic_associated_types)]

use std::{env, fmt::Debug, fs};
use xtra::prelude::*;

mod auth;
mod client;
mod community;
mod config;
mod database;

use crate::config::Config;
use client::ClientWsSession;
use database::DatabaseServer;
use directories::ProjectDirs;
use log::{info, LevelFilter};
use std::fs::OpenOptions;
use std::str::FromStr;
use vertex_common::*;
use std::sync::Arc;
use warp::Filter;
use crate::client::WebSocketMessage;
use futures::StreamExt;
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct SendMessage<T: Debug>(T);

impl<T: Debug + Send + 'static> Message for SendMessage<T> {
    type Result = ();
}

/// Marker trait for `vertex_common` structs that are actor messages too
trait VertexActorMessage: Send + 'static {
    type Result: Send;
}

impl VertexActorMessage for ClientSentMessage {
    type Result = MessageId;
}

impl VertexActorMessage for Edit {
    type Result = ();
}

struct IdentifiedMessage<T: VertexActorMessage> {
    user: UserId,
    device: DeviceId,
    message: T,
}

impl<T> Message for IdentifiedMessage<T>
where
    T: VertexActorMessage,
    T::Result: 'static,
{
    type Result = Result<T::Result, ErrResponse>;
}

fn create_files_directories(config: &Config) {
    let dirs = [config.profile_pictures.clone()];

    for dir in &dirs {
        fs::create_dir_all(dir).expect(&format!(
            "Error creating directory {}",
            dir.to_string_lossy()
        ));
    }
}

fn setup_logging(config: &Config) {
    let dirs = ProjectDirs::from("", "vertex_chat", "vertex_server")
        .expect("Error getting project directories");
    let dir = dirs.data_dir().join("logs");

    fs::create_dir_all(&dir).expect(&format!(
        "Error creating log dirs ({})",
        dir.to_string_lossy(),
    ));

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] [{}] {}",
                chrono::Local::now().to_rfc3339(),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(LevelFilter::from_str(&config.log_level).unwrap())
        .chain(std::io::stdout())
        .chain(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(
                    dir.join(
                        chrono::Local::now()
                            .format("vertex_server_%Y-%m-%d_%H-%M-%S.log")
                            .to_string(),
                    ),
                )
                .expect("Error opening log file"),
        )
        .apply()
        .expect("Error setting logger settings");

    info!("Logging set up");
}

#[tokio::main]
async fn main() {
    println!("Vertex server starting...");

    let config = config::load_config();
    setup_logging(&config);

    let args = env::args().collect::<Vec<_>>();
    let addr = args.get(1).cloned().unwrap_or("127.0.0.1:8080".to_string());

    create_files_directories(&config);

    let (cert_path, key_path) = config::ssl_config();
    let db_server = Actor::spawn(DatabaseServer::new(&config).await);
    let config = Arc::new(config);

    let routes = warp::path("client")
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let db_server = db_server.clone();
            let config = config.clone();

            ws.on_upgrade({
                move |websocket| {
                    let (tx, rx) = websocket.split();
                    let addr = ClientWsSession::new(tx, db_server.clone(), config.clone()).spawn();
                    addr.attach_stream(rx.map(|res| WebSocketMessage(res)));
                    async {}
                }
            })
        });

    info!("Vertex server starting on addr {}", addr);
    warp::serve(routes)
        .tls()
        .cert_path(cert_path)
        .key_path(key_path)
        .run(addr.parse::<SocketAddr>().unwrap()).await;
}
