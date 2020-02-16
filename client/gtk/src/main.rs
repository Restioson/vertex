#![feature(type_alias_impl_trait)]

use std::ops;
use std::rc::{Rc, Weak};

use gio::prelude::*;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

use vertex::*;

pub use crate::client::Client;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod client;
pub mod connect;
pub mod net;
pub mod screen;
pub mod token_store;
pub mod window;

// TODO: remove
pub fn https_ignore_invalid_certs() -> hyper_tls::HttpsConnector<hyper::client::HttpConnector> {
    let tls = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("failed to build tls connector");

    let tls = tokio_tls::TlsConnector::from(tls);

    let mut http = hyper::client::HttpConnector::new();
    http.enforce_http(false);

    (http, tls).into()
}

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
    fn try_get_text(&self) -> std::result::Result<String, ()>;
}

impl<E: gtk::EntryExt> TryGetText for E {
    fn try_get_text(&self) -> std::result::Result<String, ()> {
        self.get_text().map(|s| s.as_str().to_owned()).ok_or(())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AuthParameters {
    pub instance: Server,
    pub device: DeviceId,
    pub token: AuthToken,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Server(Url);

impl Server {
    pub fn parse(url: String) -> Result<Server> {
        let mut url = url;
        if !url.starts_with("https://") {
            url.insert_str(0, "https://");
        }
        if !url.ends_with('/') {
            url.push('/');
        }
        Ok(Server(Url::parse(&url)?))
    }
}

impl Server {
    #[inline]
    pub fn url(&self) -> &Url { &self.0 }
}

pub async fn start() {
    match token_store::get_stored_token() {
        Some(parameters) => {
            screen::active::start(parameters).await;
        }
        _ => {
            let screen = screen::login::build().await;
            window::set_screen(&screen.main);
        }
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

        if let Ok(icon) = gdk_pixbuf::Pixbuf::new_from_file("res/icon.svg") {
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

type StdError = Box<dyn std::error::Error>;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    InvalidUrl,
    Http(hyper::Error),
    Websocket(tungstenite::Error),
    Timeout,
    ProtocolError(Option<StdError>),
    ErrorResponse(ErrResponse),
    AuthErrorResponse(AuthError),
    UnexpectedMessage,
}

impl From<serde_cbor::Error> for Error {
    fn from(error: serde_cbor::Error) -> Self { Error::ProtocolError(Some(Box::new(error))) }
}

impl From<hyper::Error> for Error {
    fn from(error: hyper::Error) -> Self { Error::Http(error) }
}

impl From<tungstenite::Error> for Error {
    fn from(error: tungstenite::Error) -> Self { Error::Websocket(error) }
}

impl From<hyper::http::uri::InvalidUri> for Error {
    fn from(_: hyper::http::uri::InvalidUri) -> Self { Error::InvalidUrl }
}

impl From<ErrResponse> for Error {
    fn from(error: ErrResponse) -> Self { Error::ErrorResponse(error) }
}

impl From<AuthError> for Error {
    fn from(error: AuthError) -> Self { Error::AuthErrorResponse(error) }
}

impl From<url::ParseError> for Error {
    fn from(_: url::ParseError) -> Self { Error::InvalidUrl }
}
