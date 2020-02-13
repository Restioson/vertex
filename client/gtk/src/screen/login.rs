use std::fmt;

use gtk::prelude::*;

use crate::{auth, Server, token_store, TryGetText, window};
use crate::connect::AsConnector;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    instance_entry: gtk::Entry,
    username_entry: gtk::Entry,
    password_entry: gtk::Entry,
    login_button: gtk::Button,
    register_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub async fn build() -> Screen {
    let builder = gtk::Builder::new_from_file("res/glade/login/login.glade");

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        instance_entry: builder.get_object("instance_entry").unwrap(),
        username_entry: builder.get_object("username_entry").unwrap(),
        password_entry: builder.get_object("password_entry").unwrap(),
        login_button: builder.get_object("login_button").unwrap(),
        register_button: builder.get_object("register_button").unwrap(),
        status_stack: builder.get_object("status_stack").unwrap(),
        error_label: builder.get_object("error_label").unwrap(),
        spinner: builder.get_object("spinner").unwrap(),
    };

    bind_events(&screen).await;

    screen
}

async fn bind_events(screen: &Screen) {
    screen.login_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let instance_ip = screen.instance_entry.try_get_text().unwrap_or_default();
                let instance = Server::parse(instance_ip);

                let username = screen.username_entry.try_get_text().unwrap_or_default();
                let password = screen.password_entry.try_get_text().unwrap_or_default();

                screen.status_stack.set_visible_child(&screen.spinner);
                screen.error_label.set_text("");

                match login(instance, username, password).await {
                    Ok(ws) => {
                        let loading = screen::loading::build();
                        window::set_screen(&loading);

                        let client = screen::active::start(ws).await;
                        window::set_screen(&client.ui.main);
                    }
                    Err(err) => {
                        println!("Encountered error during login: {:?}", err);
                        screen.error_label.set_text(&format!("{}", err));
                    }
                }

                screen.status_stack.set_visible_child(&screen.error_label);
            })
            .build_widget_event()
    );

    screen.register_button.connect_button_press_event(
        screen.connector()
            .do_async(|_screen, (_button, _event)| async move {
                let screen = screen::register::build().await;
                window::set_screen(&screen.main);
            })
            .build_widget_event()
    );
}

async fn login(
    instance: Server,
    username: String,
    password: String,
) -> Result<auth::AuthenticatedWs, LoginError> {
    let auth = auth::Client::new(instance.clone());

    let (device, token) = match token_store::get_stored_token() {
        Some(token) if token.instance == instance => (token.device, token.token),
        _ => {
            let token = auth.create_token(
                vertex::UserCredentials::new(username, password),
                vertex::TokenCreationOptions::default(),
            ).await?;

            token_store::store_token(instance, token.device, token.token.clone());
            (token.device, token.token)
        }
    };

    Ok(auth.authenticate(device, token).await?)
}

type StdError = Box<dyn std::error::Error>;

#[derive(Debug)]
enum LoginError {
    InvalidInstanceIp,
    InvalidUsernameOrPassword,
    InternalServerError,
    ConnectError(hyper::Error),
    NetworkError(hyper::Error),
    ProtocolError(StdError),
    UnknownError,
}

impl fmt::Display for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LoginError::InvalidInstanceIp => write!(f, "Invalid instance ip"),
            LoginError::InvalidUsernameOrPassword => write!(f, "Invalid username or password"),
            LoginError::InternalServerError => write!(f, "Internal server error"),
            LoginError::ConnectError(_) => write!(f, "Couldn't connect to instance"),
            LoginError::NetworkError(_) => write!(f, "Network error"),
            LoginError::ProtocolError(_) => write!(f, "Protocol error: check your server instance?"),
            LoginError::UnknownError => write!(f, "Unknown error"),
        }
    }
}

impl From<auth::Error> for LoginError {
    fn from(err: auth::Error) -> Self {
        match err {
            auth::Error::Net(err) => if err.is_connect() {
                LoginError::ConnectError(err)
            } else {
                LoginError::NetworkError(err)
            },

            auth::Error::InvalidUri(_) => LoginError::InvalidInstanceIp,
            auth::Error::SerdeUrlEncoded(err) => LoginError::ProtocolError(Box::new(err)),
            auth::Error::SerdeCbor(err) => LoginError::ProtocolError(Box::new(err)),

            auth::Error::Server(err) => match err {
                vertex::AuthError::Internal => LoginError::InternalServerError,
                vertex::AuthError::IncorrectCredentials | vertex::AuthError::InvalidUser
                => LoginError::InvalidUsernameOrPassword,

                _ => LoginError::UnknownError,
            },

            _ => LoginError::UnknownError,
        }
    }
}
