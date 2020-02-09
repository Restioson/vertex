use std::fmt;

use gtk::prelude::*;

use crate::{auth, local_server, token_store, TryGetText, window};
use crate::connect::AsConnector;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    username_entry: gtk::Entry,
    password_entry_1: gtk::Entry,
    password_entry_2: gtk::Entry,
    register_button: gtk::Button,
    login_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub async fn build() -> Screen {
    let builder = gtk::Builder::new_from_file("res/glade/register/register.glade");

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        username_entry: builder.get_object("username_entry").unwrap(),
        password_entry_1: builder.get_object("password_entry_1").unwrap(),
        password_entry_2: builder.get_object("password_entry_2").unwrap(),
        register_button: builder.get_object("register_button").unwrap(),
        login_button: builder.get_object("login_button").unwrap(),
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
            .do_async(|_screen, (_button, _event)| async move {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
            })
            .build_widget_event()
    );

    screen.register_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let username = screen.username_entry.try_get_text().unwrap_or_default();
                let password_1 = screen.password_entry_1.try_get_text().unwrap_or_default();
                let password_2 = screen.password_entry_2.try_get_text().unwrap_or_default();

                screen.status_stack.set_visible_child(&screen.spinner);
                screen.error_label.set_text("");

                match register(username, password_1, password_2).await {
                    Ok(ws) => {
                        let screen = screen::loading::build();
                        window::set_screen(&screen);

                        let client = screen::active::start(ws).await;
                        window::set_screen(&client.ui.main);
                    }
                    Err(err) => screen.error_label.set_text(&format!("{}", err)),
                }

                screen.status_stack.set_visible_child(&screen.error_label);
            })
            .build_widget_event()
    );
}

async fn register(
    username: String,
    password_1: String,
    password_2: String,
) -> Result<auth::AuthenticatedWs, RegisterError> {
    if password_1 != password_2 {
        return Err(RegisterError::PasswordsDoNotMatch);
    }

    let password = password_1;
    let credentials = vertex::UserCredentials::new(username, password);

    let server = local_server();
    let auth = auth::Client::new(server.clone());

    let _ = auth.register(credentials.clone(), None).await?;

    let token = auth.create_token(
        credentials,
        vertex::TokenCreationOptions::default(),
    ).await?;

    token_store::store_token(server, token.device, token.token.clone());

    Ok(auth.authenticate(token.device, token.token).await?)
}

#[derive(Copy, Clone, Debug)]
enum RegisterError {
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidPassword,
    PasswordsDoNotMatch,
    InternalServerError,
    NetworkError,
    UnknownError,
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RegisterError::UsernameAlreadyExists => write!(f, "Username already exists"),
            RegisterError::InvalidUsername => write!(f, "Invalid username"),
            RegisterError::InvalidPassword => write!(f, "Invalid password"),
            RegisterError::PasswordsDoNotMatch => write!(f, "Passwords do not match"),
            RegisterError::InternalServerError => write!(f, "Internal server error"),
            RegisterError::NetworkError => write!(f, "Network error"),
            RegisterError::UnknownError => write!(f, "Unknown error"),
        }
    }
}

impl From<auth::Error> for RegisterError {
    fn from(err: auth::Error) -> Self {
        match err {
            auth::Error::Net(_) => RegisterError::NetworkError,
            auth::Error::Server(err) => err.into(),
            _ => RegisterError::UnknownError,
        }
    }
}

impl From<vertex::AuthError> for RegisterError {
    fn from(err: vertex::AuthError) -> Self {
        match err {
            vertex::AuthError::Internal => RegisterError::InternalServerError,
            vertex::AuthError::InvalidUsername => RegisterError::InvalidUsername,
            vertex::AuthError::InvalidPassword => RegisterError::InvalidPassword,
            vertex::AuthError::UsernameAlreadyExists => RegisterError::UsernameAlreadyExists,
            _ => RegisterError::UnknownError,
        }
    }
}
