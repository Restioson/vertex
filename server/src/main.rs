use actix::prelude::*;
use actix_web::dev::ServiceRequest;
use actix_web::web::{Data, Payload};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use std::{env, fmt::Debug, fs};

mod auth;
mod client;
mod config;
mod database;
mod federation;
mod community;

use crate::config::Config;
use actix_web::dev::ServiceResponse;
use client::{ClientServer, ClientWsSession};
use database::DatabaseServer;
use directories::ProjectDirs;
use federation::{FederationServer, ServerWsSession};
use log::info;
use std::fs::OpenOptions;
use crate::database::Init;

#[derive(Debug, Message)]
pub struct SendMessage<T: Debug> {
    message: T,
}

fn dispatch_client_ws(
    request: HttpRequest,
    stream: Payload,
    client_server: Data<Addr<ClientServer>>,
    federation_server: Data<Addr<FederationServer>>,
    db_server: Data<Addr<DatabaseServer>>,
    config: Data<config::Config>,
) -> Result<HttpResponse, Error> {
    let client_server = client_server.get_ref().clone();
    let _federation_server = federation_server.get_ref().clone();
    let db_server = db_server.get_ref().clone();

    ws::start(
        ClientWsSession::new(client_server, db_server, config),
        &request,
        stream,
    )
}

fn dispatch_server_ws(
    request: HttpRequest,
    stream: Payload,
    srv: Data<Addr<FederationServer>>,
) -> Result<HttpResponse, Error> {
    ServerWsSession::start_incoming(request, srv, stream)
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

fn setup_logging() {
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
        .level(log::LevelFilter::Info)
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
    setup_logging();

    let args = env::args().collect::<Vec<_>>();
    let addr = args.get(1).cloned().unwrap_or("127.0.0.1:8080".to_string());

    let config = config::load_config();
    create_files_directories(&config);

    let ssl_config = config::ssl_config();

    let mut sys = System::new("vertex_server");
    let mut db_server = DatabaseServer::new(&mut sys, &config).start();
    let client_server = ClientServer::new(db_server.clone()).start();
    db_server.send(Init(client_server));
    let federation_server = FederationServer::new().start();

    HttpServer::new(move || {
        App::new()
            .data(client_server.clone())
            .data(federation_server.clone())
            .data(db_server.clone())
            .data(config.clone())
            .service(
                actix_files::Files::new(
                    "/images/profile_pictures/",
                    config.profile_pictures.clone(),
                )
                .default_handler(actix_service::service_fn(|req: ServiceRequest| {
                    req.into_response(HttpResponse::NotFound().finish())
                }))
                .files_listing_renderer(|_dir, req| {
                    Ok(ServiceResponse::new(
                        req.clone(),
                        HttpResponse::Forbidden().finish(),
                    ))
                })
                .show_files_listing(),
            )
            .service(web::resource("/client/").route(web::get().to(dispatch_client_ws)))
            .service(web::resource("/server/").route(web::get().to(dispatch_server_ws)))
    })
    .bind_ssl(addr.clone(), ssl_config)
    .expect("Error binding to socket")
    .start();

    info!("Vertex server started on addr {}", addr);

    sys.run()
}
