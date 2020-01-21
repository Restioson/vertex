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
use vertex_client_backend::Action;

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
    net: Option<Rc<vertex::net::Sender>>,
    token_store: TokenStore,
    screen: RefCell<Option<DynamicScreen>>,
}

impl App {
    pub fn build(application: &gtk::Application) -> App {
        let window = gtk::ApplicationWindowBuilder::new()
            .application(application)
            .title(&format!("Vertex {}", crate::VERSION))
            .icon_name("res/icon.png")
            .default_width(1280)
            .default_height(720)
            .build();

        window.show_all();

        App {
            window,
            net: None,
            token_store: TokenStore::new(),
            screen: RefCell::new(None),
        }
    }

    pub async fn start(mut self, net: vertex::Net) {
        let (net_send, net_recv) = net.split();
        self.net = Some(Rc::new(net_send));

        let app = Rc::new(self);

        let context = glib::MainContext::ref_thread_default();
        context.spawn_local({
            let app = app.clone();
            async move { app.run(net_recv).await }
        });

        match app.token_store.get_stored_token() {
            Some((device, token)) => {
                // TODO: Some code duplication with auth in login and register ui
                let client = vertex::AuthClient::new(app.net());
                let client = client.login(device, token).await.expect("failed to login");
                let client = Rc::new(client);

                let screen = screen::active::build(app.clone(), client);
                app.set_screen(DynamicScreen::Active(screen));
            }
            None => {
                let screen = screen::login::build(app.clone());
                app.set_screen(DynamicScreen::Login(screen));
            }
        }
    }

    async fn run(&self, receiver: vertex::net::Receiver) {
        futures::future::join(
            async move {
                let mut stream = vertex::action_stream(receiver);
                while let Some(action) = stream.next().await {
                    let action: vertex::Action = action;
                    match action {
                        Action::AddMessage(_) => {},
                        Action::LoggedOut => {},
                        Action::Error(_) => {},
                    }
                    println!("{:?}", action);
                }
            },
            async {
                let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(2));
                loop {
                    ticker.tick().await;
                    self.net().dispatch_heartbeat().await.expect("failed to dispatch heartbeat");
                }
            },
        ).await;
    }

    pub fn set_screen(&self, screen: DynamicScreen) {
        for child in self.window.get_children() {
            self.window.remove(&child);
        }
        self.window.add(screen.viewport());

        self.window.show_all();

        *(self.screen.borrow_mut()) = Some(screen);
    }

    pub fn net(&self) -> Rc<vertex::net::Sender> {
        self.net.as_ref().unwrap().clone()
    }

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
        .unwrap_or("127.0.0.1:8080".to_string());

    let url = Url::parse(&format!("wss://{}/client/", ip)).unwrap();
    let url = Rc::new(url);

    let application = gtk::Application::new(None, Default::default())
        .expect("failed to create application");

    setup_gtk_style();

    application.connect_activate(move |app| {
        let url = url.clone();
        let app = App::build(app);

        glib::MainContext::ref_thread_default().block_on(async move {
            let net = vertex::net::connect((*url).clone()).await.expect("failed to connect");
            app.start(net).await;
        });
    });

    application.run(&[]);
}
