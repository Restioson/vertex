#![feature(type_alias_impl_trait, linked_list_cursors, type_ascription)]
#![windows_subsystem = "windows"]

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::fmt;

use gio::prelude::*;
use gtk::prelude::*;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use url::Url;
use serde::{Serialize, Deserialize};

use vertex::prelude::*;
use vertex::proto::DeserializeError;

pub use crate::client::Client;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod client;
pub mod connect;
pub mod net;
pub mod screen;
pub mod token_store;
pub mod window;
pub mod scheduler;

#[derive(Clone)]
pub struct Glade(Arc<String>);

impl Glade {
    pub fn open<P: AsRef<Path>>(glade_path: P) -> io::Result<Glade> {
        let mut path = PathBuf::from(resource("glade"));
        path.push(glade_path);

        let mut file = File::open(path)?;
        let mut source = String::new();
        file.read_to_string(&mut source)?;

        #[allow(unused_mut)]
        let mut glade_string = source;

        // Replace res/* with relative link
        #[cfg(feature = "deploy")]
        {
            let res_path = resources_path().into_os_string().into_string().unwrap();
            let res_path = format!("{}/", res_path);
            glade_string = glade_string.replace("res/", &res_path);
        }

        Ok(Glade(Arc::new(glade_string)))
    }

    #[inline]
    pub fn builder(&self) -> gtk::Builder {
        gtk::Builder::new_from_string(&self.0)
    }
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
    pub async fn read(&self) -> RwLockReadGuard<'_, T> { self.0.read().await }

    #[inline]
    pub async fn write(&self) -> RwLockWriteGuard<'_, T> { self.0.write().await }

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
        self.0.upgrade().map(SharedMut)
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

#[derive(Clone, Serialize, Deserialize)]
pub struct AuthParameters {
    pub instance: Server,
    pub device: DeviceId,
    pub token: AuthToken,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Server(Url);

impl Server {
    pub fn parse(url: String) -> Result<Server> {
        let mut url = url;
        if !url.starts_with("https://") && !url.starts_with("http://") {
            url.insert_str(0, "https://");
        }
        if !url.ends_with('/') {
            url.push('/');
        }

        url.push_str("vertex/client/");

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

fn resources_path() -> PathBuf {
    let mut path;

    #[cfg(not(feature = "deploy"))]
    {
        path = PathBuf::new();
    }

    #[cfg(feature = "deploy")]
    {
        path = std::env::current_exe().unwrap();
        path.pop();
    }

    path.push("res");
    path
}

fn resource<P: AsRef<Path>>(rest: P) -> String {
    let mut path = resources_path();
    path.push(rest);
    path.into_os_string().into_string().expect("tmp path is invalid utf-8!")
}

fn setup_gtk_style() {
    let screen = gdk::Screen::get_default().expect("unable to get screen");
    let css_provider = gtk::CssProvider::new();
    css_provider.load_from_path(&resource("style.css")).expect("unable to load css");

    gtk::StyleContext::add_provider_for_screen(&screen, &css_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

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

        if let Ok(icon) = gdk_pixbuf::Pixbuf::new_from_file(resource("icon.svg")) {
            window = window.icon(&icon);
        }

        window::init(window.build());

        scheduler::spawn(async move {
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
    ErrorResponse(vertex::responses::Error),
    AuthErrorResponse(AuthError),
    UnexpectedMessage,
    DeserializeError(DeserializeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;
        match self {
            InvalidUrl => write!(f, "Invalid url"),
            Http(http) => if http.is_connect() {
                write!(f, "Couldn't connect to instance")
            } else {
                write!(f, "Network error")
            },
            Websocket(ws) => write!(f, "{}", ws),
            Timeout => write!(f, "Connection timed out"),
            ProtocolError(err) => match err {
                Some(err) => write!(f, "Protocol error: {}", err),
                None => write!(f, "Protocol error"),
            },
            ErrorResponse(err) => write!(f, "{}", err),
            AuthErrorResponse(err) => write!(f, "{}", err),
            UnexpectedMessage => write!(f, "Received unexpected message"),
            DeserializeError(_) => write!(f, "Failed to deserialize message"),
        }
    }
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

impl From<AuthError> for Error {
    fn from(error: AuthError) -> Self { Error::AuthErrorResponse(error) }
}

impl From<url::ParseError> for Error {
    fn from(_: url::ParseError) -> Self { Error::InvalidUrl }
}

impl From<DeserializeError> for Error {
    fn from(err: DeserializeError) -> Self {
        Error::DeserializeError(err)
    }
}
