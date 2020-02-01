#![feature(type_alias_impl_trait)]

use std::cell::RefCell;
use std::rc::Rc;

use futures::Stream;
use futures::stream::StreamExt;
use gio::prelude::*;
use gtk::prelude::*;
use url::Url;

use vertex_client::Client;
use vertex_client::net::{RequestManager, RequestSender};

use crate::net::Sender;
use crate::screen::DynamicScreen;
use crate::token_store::TokenStore;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

pub mod net;
pub mod screen;
pub mod token_store;

pub struct App {
    window: gtk::ApplicationWindow,
    request_sender: Option<RequestSender<net::Sender>>,
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
            request_sender: None,
            token_store: TokenStore::new(),
            screen: RefCell::new(None),
        }
    }

    pub async fn start(mut self, sender: net::Sender, receiver: net::Receiver) {
        let request_manager = RequestManager::new();

        let sender = request_manager.sender(sender);
        let stream = request_manager.receive_from(receiver);

        self.request_sender = Some(sender.clone());

        self.window.connect_delete_event(move |_window, _event| {
            let _ = futures::executor::block_on(sender.close());
            gtk::Inhibit(false)
        });

        let app = Rc::new(self);

        let context = glib::MainContext::ref_thread_default();
        context.spawn_local({
            let app = app.clone();
            async move { app.run(stream).await }
        });

        app.set_screen(app.clone().try_login().await);
    }

    async fn try_login(self: Rc<Self>) -> DynamicScreen {
        match self.token_store.get_stored_token() {
            Some((device, token)) => {
                let client = vertex_client::auth::Client::new(self.request_sender());

                match client.login(device, token).await {
                    Ok(client) => {
                        let screen = screen::active::build(self.clone(), Rc::new(client));
                        return DynamicScreen::Active(screen);
                    }
                    Err(err) => {
                        println!("failed to log in with stored token: {:?}", err);
                        self.token_store.forget_token();
                    }
                }
            }
            _ => (),
        }

        let screen = screen::login::build(self.clone());
        DynamicScreen::Login(screen)
    }

    async fn run<S>(&self, stream: S)
        where S: Stream<Item = net::Result<vertex::ServerAction>> + Unpin
    {
        futures::future::join(
            async move {
                let mut stream = stream;
                while let Some(result) = stream.next().await {
                    println!("{:?}", result);
                }
            },
            async {
                let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(2));
                loop {
                    let result = self.request_sender().net().ping().await;
                    if result.is_err() {
                        break;
                    }

                    ticker.tick().await;
                }
            },
        ).await;
    }

    pub fn set_screen(&self, screen: DynamicScreen) {
        for child in self.window.get_children() {
            self.window.remove(&child);
        }
        self.window.add(screen.widget());

        self.window.show_all();

        *(self.screen.borrow_mut()) = Some(screen);
    }

    pub fn request_sender(&self) -> RequestSender<net::Sender> {
        self.request_sender.clone().unwrap()
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
        .unwrap_or("localhost:8080".to_string());

    let url = Url::parse(&format!("wss://{}/client/", ip)).unwrap();

    let application = gtk::Application::new(None, Default::default())
        .expect("failed to create application");

    setup_gtk_style();

    application.connect_activate(move |app| {
        let url = url.clone();
        let app = App::build(app);

        app.set_screen(DynamicScreen::Loading(screen::loading::build()));

        glib::MainContext::ref_thread_default().spawn_local(async move {
            let (send, recv) = net::connect(url.clone()).await
                .expect("failed to connect");

            app.start(send, recv).await;
        });
    });

    application.run(&[]);
}
