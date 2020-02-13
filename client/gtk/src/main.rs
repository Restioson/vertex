#![feature(type_alias_impl_trait)]

use std::ops;
use std::rc::{Rc, Weak};

use gio::prelude::*;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub use crate::client::Client;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod client;
pub mod connect;
pub mod net;
pub mod screen;
pub mod token_store;
pub mod window;

pub struct SharedMut<T>(Rc<RwLock<T>>);

impl<T> Clone for SharedMut<T> {
    #[inline]
    fn clone(&self) -> Self { SharedMut(self.0.clone()) }
}

impl<T> SharedMut<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        SharedMut(Rc::new(RwLock::new(value)))
    }

    #[inline]
    pub async fn read<'a>(&'a self) -> impl ops::Deref<Target = T> + 'a {
        self.0.read().await
    }

    #[inline]
    pub async fn write<'a>(&'a self) -> impl ops::Deref<Target = T> + ops::DerefMut + 'a {
        self.0.write().await
    }

    #[inline]
    pub fn downgrade(&self) -> WeakSharedMut<T> {
        WeakSharedMut(Rc::downgrade(&self.0))
    }
}

pub struct WeakSharedMut<T>(Weak<RwLock<T>>);

impl<T> Clone for WeakSharedMut<T> {
    #[inline]
    fn clone(&self) -> Self { WeakSharedMut(self.0.clone()) }
}

impl<T> WeakSharedMut<T> {
    #[inline]
    pub fn upgrade(&self) -> Option<SharedMut<T>> {
        self.0.upgrade().map(|upgrade| SharedMut(upgrade))
    }
}

pub trait TryGetText {
    fn try_get_text(&self) -> Result<String, ()>;
}

impl<E: gtk::EntryExt> TryGetText for E {
    fn try_get_text(&self) -> Result<String, ()> {
        self.get_text().map(|s| s.as_str().to_owned()).ok_or(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Server(String);

impl Server {
    pub fn parse(url: String) -> Server {
        let mut url = url;
        if !url.starts_with("https://") {
            url.insert_str(0, "https://");
        }
        Server(url)
    }
}

impl Server {
    pub fn url(&self) -> &str { &self.0 }
}

async fn start() {
    match try_login().await {
        Some(ws) => {
            let client = screen::active::start(ws).await;
            window::set_screen(&client.ui.main);
        }
        None => {
            let screen = screen::login::build().await;
            window::set_screen(&screen.main);
        }
    }
}

async fn try_login() -> Option<auth::AuthenticatedWs> {
    match token_store::get_stored_token() {
        Some(token_store::StoredToken { instance, device, token }) => {
            let auth = auth::Client::new(instance);
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

// TODO: it freezes if we use <2 threads: why? (something with tokio-tungstenite, maybe?)
#[tokio::main(core_threads = 2)]
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

        let ctx = glib::MainContext::ref_thread_default();
        ctx.spawn_local(async move {
            let screen = screen::loading::build();
            window::set_screen(&screen);

            start().await;
        });
    });

    application.run(&[]);
}
