#![feature(async_await)]

use std::env;
use actix::prelude::*;
use actix_web::{web, App, HttpServer, HttpRequest, HttpResponse, Error};
use actix_web::web::{Data, Payload};
use actix_web_actors::ws;

mod client;
mod federation;

use client::{ClientWsSession, ClientServer};
use federation::{ServerWsSession, FederationServer};

fn dispatch_client_ws(
    request: HttpRequest,
    stream: Payload,
    srv: Data<Addr<ClientServer>>
) -> Result<HttpResponse, Error> {
    ws::start(ClientWsSession::new(srv.get_ref().clone()), &request, stream)
}

fn dispatch_server_ws(
    request: HttpRequest,
    stream: Payload,
    srv: Data<Addr<FederationServer>>
) -> Result<HttpResponse, Error> {
    println!("wow");
    ws::start(ServerWsSession::new(srv.get_ref().clone()), &request, stream)
}

fn main() -> std::io::Result<()> {
    let args = env::args().collect::<Vec<_>>();
    let port = args.get(1).cloned().unwrap_or("8080".to_string());

    let sys = System::new("chat-server");
    let client_server = ClientServer::new().start();
    let federation_server = FederationServer::new().start();

    HttpServer::new(move ||
        App::new()
            .data(client_server.clone())
            .data(federation_server.clone())
            .service(web::resource("/client/").route(web::get().to(dispatch_client_ws)))
            .service(web::resource("/server/").route(web::get().to(dispatch_server_ws)))
    )
    .bind(format!("127.0.0.1:{}", port))?
    .start();

    sys.run()
}