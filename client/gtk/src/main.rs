#![feature(type_alias_impl_trait)]

use std::cell::{self, RefCell};
use std::rc::Rc;

use gio::prelude::*;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};

pub use crate::client::Client;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod client;
pub mod connect;
pub mod net;
pub mod screen;
pub mod token_store;
pub mod window;

pub fn local_server() -> Server {
    Server("https://localhost:8080/client".to_owned())
}

pub trait TryGetText {
    fn try_get_text(&self) -> Result<String, ()>;
}

impl<E: gtk::EntryExt> TryGetText for E {
    fn try_get_text(&self) -> Result<String, ()> {
        self.get_text().map(|s| s.as_str().to_owned()).ok_or(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server(String);

impl Server {
    pub fn url(&self) -> &str { &self.0 }
}

pub struct UiShared<T>(Rc<RefCell<T>>);

impl<T> Clone for UiShared<T> {
    #[inline]
    fn clone(&self) -> Self { UiShared(self.0.clone()) }
}

impl<T> UiShared<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        UiShared(Rc::new(RefCell::new(value)))
    }

    #[inline]
    pub fn borrow(&self) -> cell::Ref<T> { self.0.borrow() }

    #[inline]
    pub fn borrow_mut(&self) -> cell::RefMut<T> { self.0.borrow_mut() }
}

async fn start() {
    match try_login().await {
        Some(ws) => {
            let screen = screen::active::build(ws);
            window::set_screen(&screen.borrow().ui.main);
        }
        None => {
            let screen = screen::login::build();
            window::set_screen(&screen.borrow().main);
        }
    }
}

async fn try_login() -> Option<auth::AuthenticatedWs> {
    match token_store::get_stored_token() {
        Some((device, token)) => {
            let auth = auth::Client::new(local_server());
            match auth.authenticate(device, token).await {
                Ok(ws) => Some(ws),
                Err(err) => {
                    println!("failed to log in with stored token: {:?}", err);
                    token_store::forget_token();
                    None
                }
            }
        }
        _ => None,
    }
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
    let application = gtk::Application::new(None, Default::default())
        .expect("failed to create application");

    setup_gtk_style();

    application.connect_activate(move |application| {
        let mut window = gtk::ApplicationWindowBuilder::new()
            .application(application)
            .title(&format!("Vertex {}", crate::VERSION))
            .default_width(1280)
            .default_height(720);

        if let Ok(icon) = gdk_pixbuf::Pixbuf::new_from_file("res/icon.png") {
            window = window.icon(&icon);
        }

        window::init(window.build());

        let screen = screen::loading::build();
        window::set_screen(&*screen.borrow());

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(start());
    });

    application.run(&[]);
}
