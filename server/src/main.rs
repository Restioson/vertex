use actix::prelude::*;
use actix_web::web::{Data, Payload};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use serde::{Deserialize, Serialize};
use std::{env, fmt::Debug};

mod auth;
mod client;
mod database;
mod federation;

use client::{ClientServer, ClientWsSession};
use database::DatabaseServer;
use federation::{FederationServer, ServerWsSession};

#[derive(Debug, Message)]
pub struct SendMessage<T: Debug> {
    message: T,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    max_password_len: u64,
    max_username_len: u64,     // TODO use this
    max_display_name_len: u64, // and this
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_password_len: 1000,
            max_username_len: 64,
            max_display_name_len: 64,
        }
    }
}

fn dispatch_client_ws(
    request: HttpRequest,
    stream: Payload,
    client_server: Data<Addr<ClientServer>>,
    federation_server: Data<Addr<FederationServer>>,
    db_server: Data<Addr<DatabaseServer>>,
    config: Data<Config>,
) -> Result<HttpResponse, Error> {
    let client_server = client_server.get_ref().clone();
    let federation_server = federation_server.get_ref().clone();
    let db_server = db_server.get_ref().clone();

    ws::start(
        ClientWsSession::new(client_server, federation_server, db_server, config),
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

fn main() -> std::io::Result<()> {
    let args = env::args().collect::<Vec<_>>();
    let addr = args.get(1).cloned().unwrap_or("127.0.0.1:8080".to_string());

    let config: Config = confy::load("vertex_server").expect("Error loading config");

    let mut sys = System::new("vertex_server");
    let db_server = DatabaseServer::new(&mut sys).start();
    let client_server = ClientServer::new(db_server.clone()).start();
    let federation_server = FederationServer::new().start();

    HttpServer::new(move || {
        App::new()
            .data(client_server.clone())
            .data(federation_server.clone())
            .data(db_server.clone())
            .data(config.clone())
            .service(web::resource("/client/").route(web::get().to(dispatch_client_ws)))
            .service(web::resource("/server/").route(web::get().to(dispatch_server_ws)))
    })
    .bind(addr.clone())?
    .start();

    println!("Server started on addr {}", addr);

    sys.run()
}
