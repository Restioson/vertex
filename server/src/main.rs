use actix::prelude::*;
use actix_web::web::{Data, Payload};
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use std::{env, fmt::Debug};

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

fn dispatch_client_ws(
    request: HttpRequest,
    stream: Payload,
    client_server: Data<Addr<ClientServer>>,
    federation_server: Data<Addr<FederationServer>>,
    db_server: Data<Addr<DatabaseServer>>,
) -> Result<HttpResponse, Error> {
    let client_server = client_server.get_ref().clone();
    let federation_server = federation_server.get_ref().clone();
    let db_server = db_server.get_ref().clone();

    ws::start(
        ClientWsSession::new(client_server, federation_server, db_server),
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
    let port = args.get(1).cloned().unwrap_or("127.0.0.1:8080".to_string());

    let mut sys = System::new("vertex_server");
    let db_server = DatabaseServer::new(&mut sys).start();
    let client_server = ClientServer::new(db_server.clone()).start();
    let federation_server = FederationServer::new().start();

    HttpServer::new(move || {
        App::new()
            .data(client_server.clone())
            .data(federation_server.clone())
            .data(db_server.clone())
            .service(web::resource("/client/").route(web::get().to(dispatch_client_ws)))
            .service(web::resource("/server/").route(web::get().to(dispatch_server_ws)))
    })
    .bind(port.clone())?
    .start();

    println!("Server started on port {}", port);

    sys.run()
}
