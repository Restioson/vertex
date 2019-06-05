use actix::prelude::*;
use actix_web_actors::ws;
use super::FederationServer;

pub struct ServerWsSession {
    server: Addr<FederationServer>,
}

impl ServerWsSession {
    pub fn new(server: Addr<FederationServer>) -> Self {
        ServerWsSession { server, }
    }
}

impl Actor for ServerWsSession {
    type Context = ws::WebsocketContext<Self>;
}

impl StreamHandler<ws::Message, ws::ProtocolError> for ServerWsSession {
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Ping(msg) => {
                println!("ping {:?}", msg);
                ctx.pong(&msg)
            },
            ws::Message::Text(text) => {
                println!("text {:?}", text);
                ctx.text(text)
            },
            ws::Message::Binary(bin) => {
                println!("binary {:?}", bin);
                ctx.binary(bin)
            },
            _ => (),
        }
    }
}
