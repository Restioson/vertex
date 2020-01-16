use actix::prelude::*;
use actix_web::web::{Data, Payload};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use std::{env, fmt::Debug, fs};

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

#[derive(Debug, Message)]
#[rtype(result = "()")]
pub struct SendMessage<T: Debug> {
    message: T,
}

/// Marker trait for `vertex_common` structs that are Actix messages too
trait VertexActixMessage {
    type Result;
}

impl VertexActixMessage for ClientSentMessage {
    type Result = MessageId;
}

impl VertexActixMessage for Edit {
    type Result = ();
}

struct IdentifiedMessage<T: VertexActixMessage> {
    user: UserId,
    device: DeviceId,
    message: T,
}

impl<T> Message for IdentifiedMessage<T>
    where T: VertexActixMessage,
          T::Result: 'static
{
    type Result = Result<T::Result, ServerError>;
}

async fn dispatch_client_ws(
    request: HttpRequest,
    stream: Payload,
    db_server: Data<Addr<DatabaseServer>>,
    config: Data<config::Config>,
) -> Result<HttpResponse, Error> {
    let db_server = db_server.get_ref().clone();

    ws::start(ClientWsSession::new(db_server, config), &request, stream)
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

fn main() -> std::io::Result<()> {
    println!("Vertex server starting...");

    let config = config::load_config();
    setup_logging(&config);

    let args = env::args().collect::<Vec<_>>();
    let addr = args.get(1).cloned().unwrap_or("127.0.0.1:8080".to_string());

    create_files_directories(&config);

    let ssl_config = config::ssl_config();

    let mut sys = System::new("vertex_server");
    let db_server = DatabaseServer::new(&mut sys, &config).start();

    HttpServer::new(move || {
        App::new()
            .data(db_server.clone())
            .data(config.clone())
            .service(web::resource("/client/").route(web::get().to(dispatch_client_ws)))
    })
    .bind_openssl(addr.clone(), ssl_config)
    .expect("Error binding to socket")
    .run();

    info!("Vertex server started on addr {}", addr);

    sys.run()
}
