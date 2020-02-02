use std::fmt;

use gtk::prelude::*;

use crate::{auth, local_server, token_store, TryGetText, window};
use crate::connect::AsConnector;
use crate::screen;
use crate::UiShared;

pub struct Screen {
    pub main: gtk::Viewport,
    username_entry: gtk::Entry,
    password_entry: gtk::Entry,
    login_button: gtk::Button,
    register_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub fn build() -> UiShared<Screen> {
    let builder = gtk::Builder::new_from_file("res/glade/login/login.glade");

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        username_entry: builder.get_object("username_entry").unwrap(),
        password_entry: builder.get_object("password_entry").unwrap(),
        login_button: builder.get_object("login_button").unwrap(),
        register_button: builder.get_object("register_button").unwrap(),
        status_stack: builder.get_object("status_stack").unwrap(),
        error_label: builder.get_object("error_label").unwrap(),
        spinner: builder.get_object("spinner").unwrap(),
    };

    let screen = UiShared::new(screen);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &UiShared<Screen>) {
    let widgets = screen.borrow();

    widgets.login_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let widgets = screen.borrow();

                let username = widgets.username_entry.try_get_text().unwrap_or_default();
                let password = widgets.password_entry.try_get_text().unwrap_or_default();

                widgets.status_stack.set_visible_child(&widgets.spinner);
                widgets.error_label.set_text("");

                match login(username, password).await {
                    Ok(ws) => {
                        let screen = screen::active::build(ws);
                        window::set_screen(&screen.borrow().ui.main);
                    }
                    Err(err) => widgets.error_label.set_text(&format!("{}", err)),
                }

                widgets.status_stack.set_visible_child(&widgets.error_label);
            })
            .build_widget_event()
    );

    widgets.register_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (_button, _event)| {
                let screen = screen::register::build();
                window::set_screen(&screen.borrow().main);
            })
            .build_widget_event()
    );
}

async fn login(
    username: String,
    password: String,
) -> Result<auth::AuthenticatedWs, LoginError> {
    let server = local_server();
    let auth = auth::Client::new(server.clone());

    let (device, token) = match token_store::get_stored_token() {
        Some(token) => token,
        None => {
            let token = auth.create_token(
                vertex::UserCredentials::new(username, password),
                vertex::TokenCreationOptions::default(),
            ).await?;

            token_store::store_token(server, token.device, token.token.clone());
            (token.device, token.token)
        }
    };

    Ok(auth.authenticate(device, token).await?)
}

#[derive(Copy, Clone, Debug)]
enum LoginError {
    InvalidUsernameOrPassword,
    InternalServerError,
    NetworkError,
    UnknownError,
}

impl fmt::Display for LoginError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LoginError::InvalidUsernameOrPassword => write!(f, "Invalid username or password"),
            LoginError::InternalServerError => write!(f, "Internal server error"),
            LoginError::NetworkError => write!(f, "Network error"),
            LoginError::UnknownError => write!(f, "Unknown error"),
        }
    }
}

impl From<auth::Error> for LoginError {
    fn from(err: auth::Error) -> Self {
        match err {
            auth::Error::Net(_) => LoginError::NetworkError,
            auth::Error::Server(err) => err.into(),
            _ => LoginError::UnknownError,
        }
    }
}

impl From<vertex::AuthError> for LoginError {
    fn from(err: vertex::AuthError) -> Self {
        match err {
            vertex::AuthError::Internal => LoginError::InternalServerError,
            vertex::AuthError::IncorrectCredentials | vertex::AuthError::InvalidUser
            => LoginError::InvalidUsernameOrPassword,

            _ => LoginError::UnknownError,
        }
    }
}
