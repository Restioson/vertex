#![feature(type_alias_impl_trait)]

use std::rc::Rc;

use futures::Stream;
use futures::stream::StreamExt;
use gio::prelude::*;
use gtk::prelude::*;

use vertex::*;

pub use crate::client::Client;
use crate::screen::Screen;
use crate::token_store::TokenStore;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

pub mod auth;
pub mod client;
pub mod net;
pub mod screen;
pub mod token_store;

#[derive(Debug, Clone)]
pub struct Server(String);

impl Server {
    pub fn url(&self) -> &str { &self.0 }
}

#[derive(Debug)]
pub struct Community {
    pub id: CommunityId,
    pub name: String,
    pub rooms: Vec<Room>,
}

#[derive(Debug)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
}

pub struct App {
    window: gtk::ApplicationWindow,
    server: Server,
    token_store: TokenStore,
}

impl App {
    pub fn build(application: &gtk::Application, server: Server) -> App {
        let mut window = gtk::ApplicationWindowBuilder::new()
            .application(application)
            .title(&format!("Vertex {}", crate::VERSION))
            .default_width(1280)
            .default_height(720);

        if let Ok(icon) = gdk_pixbuf::Pixbuf::new_from_file("res/icon.png") {
            window = window.icon(&icon);
        }

        let window = window.build();

        window.show_all();

        App {
            window,
            server,
            token_store: TokenStore::new(),
        }
    }

    pub async fn start(self) {
        let app = Rc::new(self);
        match app.clone().try_login().await {
            Some(ws) => app.set_screen(screen::active::build(app.clone(), ws)),
            None => app.set_screen(screen::login::build(app.clone())),
        }
    }

    async fn try_login(self: Rc<Self>) -> Option<auth::AuthenticatedWs> {
        match self.token_store.get_stored_token() {
            Some((device, token)) => {
                let auth = auth::Client::new(self.server.clone());
                match auth.authenticate(device, token).await {
                    Ok(ws) => Some(ws),
                    Err(err) => {
                        println!("failed to log in with stored token: {:?}", err);
                        self.token_store.forget_token();
                        None
                    }
                }
            }
            _ => None,
        }
    }

    pub fn set_screen<M>(&self, screen: Screen<M>) {
        for child in self.window.get_children() {
            self.window.remove(&child);
        }
        self.window.add(screen.widget());

        self.window.show_all();
    }

    pub fn server(&self) -> Server { self.server.clone() }

    pub fn token_store(&self) -> &TokenStore { &self.token_store }
}

fn setup_gtk_style() {
    let screen = gdk::Screen::get_default().expect("unable to get screen");
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_path("res/style.css").expect("unable to load css");

    gtk::StyleContext::add_provider_for_screen(&screen, &css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

// TODO: can we get rid of need for this? (do we need to use tokio-tungstenite or can we just use tungstenite?)
#[tokio::main]
async fn main() {
    let matches = clap::App::new(NAME)
        .version(VERSION)
        .author(AUTHORS)
        .arg(
            clap::Arg::with_name("ip")
                .short("i")
                .long("ip")
                .value_name("IP")
                .help("Sets the homeserver to connect to")
                .takes_value(true),
        )
        .get_matches();

    let ip = matches.value_of("ip")
        .map(|ip| ip.to_string())
        .unwrap_or("https://localhost:8080/client".to_string());

    let server = Server(ip);

    let application = gtk::Application::new(None, Default::default())
        .expect("failed to create application");

    setup_gtk_style();

    application.connect_activate(move |app| {
        let app = App::build(app, server.clone());

        app.set_screen(screen::loading::build());

        glib::MainContext::ref_thread_default().spawn_local(async move {
            app.start().await;
        });
    });

    application.run(&[]);
}
