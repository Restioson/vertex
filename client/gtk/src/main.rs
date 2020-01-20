use gtk::prelude::*;
use gio::prelude::*;

use url::Url;
use vertex_client_backend as vertex;
use vertex_common::*;

use std::rc::Rc;
use futures::task::LocalSpawnExt;
use futures::stream::StreamExt;

use crate::screen::{Screen, DynamicScreen};
use std::cell::RefCell;
use crate::token_store::TokenStore;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

pub mod screen;
pub mod token_store;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Vertex(vertex::Error),
}

impl From<vertex::Error> for Error {
    fn from(vertex: vertex::Error) -> Self { Error::Vertex(vertex) }
}

pub struct App {
    window: gtk::ApplicationWindow,
    net: Rc<vertex::net::Sender>,
    token_store: TokenStore,
    screen: RefCell<Option<DynamicScreen>>,
}

impl App {
    pub fn build(application: &gtk::Application, net: vertex::Net) -> App {
        let window = gtk::ApplicationWindowBuilder::new()
            .application(application)
            .title(&format!("Vertex {}", crate::VERSION))
            .icon_name("icon.png")
            .default_width(1280)
            .default_height(720)
            .build();

        window.show_all();

        let context = glib::MainContext::ref_thread_default();

        let (net_send, net_recv) = net.split();

        // TODO: clean these up
        let mut stream = vertex::action_stream(net_recv);
        context.spawn_local(async move {
            while let Some(action) = stream.next().await {}
        });

        let net_send = Rc::new(net_send);

        context.spawn_local({
            let net_send = net_send.clone();
            async move {
                let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(2));
                loop {
                    ticker.tick().await;
                    net_send.dispatch_heartbeat().await.expect("failed to dispatch heartbeat");
                }
            }
        });

        App {
            window,
            net: net_send,
            token_store: TokenStore::new(),
            screen: RefCell::new(None),
        }
    }

    pub fn set_screen(&self, screen: DynamicScreen) {
        for child in self.window.get_children() {
            self.window.remove(&child);
        }
        self.window.add(screen.viewport());

        self.window.show_all();

        *(self.screen.borrow_mut()) = Some(screen);
    }

    pub async fn start(self) {
        let app = Rc::new(self);

        match app.token_store.get_stored_token() {
            Some((device, token)) => {
                // TODO: Some code duplication with auth in login and register ui
                let client = vertex::Client::new(app.net.clone());
                let client = client.login(device, token).await.expect("failed to login");
                let client = Rc::new(client);

                let screen = screen::active::build(app.clone(), client);
                app.set_screen(DynamicScreen::Active(screen));
            },
            None => {
                let screen = screen::login::build(app.clone());
                app.set_screen(DynamicScreen::Login(screen));
            },
        }
    }
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
        .unwrap_or("127.0.0.1:8080".to_string());

    let url = Url::parse(&format!("wss://{}/client/", ip)).unwrap();
    let url = Rc::new(url);

    let application = gtk::Application::new(None, Default::default())
        .expect("failed to create application");

    application.connect_activate(move |app| {
        let url = url.clone();

        glib::MainContext::ref_thread_default().block_on(async move {
            let net = vertex::net::connect((*url).clone()).await.expect("failed to connect");

            let app = App::build(app, net);
            app.start().await;
        });
    });

    application.run(&[]);
}
