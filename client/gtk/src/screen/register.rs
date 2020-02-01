use gtk::prelude::*;

use std::fmt;
use std::rc::Rc;

use crate::net;
use crate::screen::{self, DynamicScreen, Screen, TryGetText};

const SCREEN_SRC: &str = include_str!("glade/register/register.glade");

pub struct Widgets {
    username_entry: gtk::Entry,
    password_entry_1: gtk::Entry,
    password_entry_2: gtk::Entry,
    register_button: gtk::Button,
    login_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub struct Model {
    app: Rc<crate::App>,
    widgets: Widgets,
}

pub fn build(app: Rc<crate::App>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(SCREEN_SRC);

    let viewport: gtk::Viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app,
        widgets: Widgets {
            username_entry: builder.get_object("username_entry").unwrap(),
            password_entry_1: builder.get_object("password_entry_1").unwrap(),
            password_entry_2: builder.get_object("password_entry_2").unwrap(),
            register_button: builder.get_object("register_button").unwrap(),
            login_button: builder.get_object("login_button").unwrap(),
            status_stack: builder.get_object("status_stack").unwrap(),
            error_label: builder.get_object("error_label").unwrap(),
            spinner: builder.get_object("spinner").unwrap(),
        },
    };

    let screen = Screen::new(viewport, model);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.login_button.connect_button_press_event(
        screen
            .connector()
            .do_sync(|screen, (_button, _event)| {
                let login = screen::login::build(screen.model().app.clone());
                screen.model().app.set_screen(DynamicScreen::Login(login));
            })
            .build_widget_event(),
    );

    widgets.register_button.connect_button_press_event(
        screen
            .connector()
            .do_async(|screen, (_button, _event)| {
                async move {
                    let model = screen.model();

                    let username = model
                        .widgets
                        .username_entry
                        .try_get_text()
                        .unwrap_or_default();
                    let password_1 = model
                        .widgets
                        .password_entry_1
                        .try_get_text()
                        .unwrap_or_default();
                    let password_2 = model
                        .widgets
                        .password_entry_2
                        .try_get_text()
                        .unwrap_or_default();

                    model
                        .widgets
                        .status_stack
                        .set_visible_child(&model.widgets.spinner);
                    model.widgets.error_label.set_text("");

                    match register(&screen.model().app, username, password_1, password_2).await {
                        Ok(client) => {
                            let (device, token) = client.token();
                            model.app.token_store.store_token(device, token);

                            let client = Rc::new(client);

                            let active = screen::active::build(screen.model().app.clone(), client);
                            screen.model().app.set_screen(DynamicScreen::Active(active));
                        }
                        Err(err) => model.widgets.error_label.set_text(&format!("{}", err)),
                    }

                    model
                        .widgets
                        .status_stack
                        .set_visible_child(&model.widgets.error_label);
                }
            })
            .build_widget_event(),
    );
}

async fn register(
    app: &crate::App,
    username: String,
    password_1: String,
    password_2: String,
) -> Result<vertex_client::Client<net::Sender>, RegisterError> {
    if password_1 != password_2 {
        return Err(RegisterError::PasswordsDoNotMatch);
    }

    let password = password_1;

    let client = vertex_client::auth::Client::new(app.request_sender());
    let credentials = vertex::UserCredentials::new(username.clone(), password);

    let _ = client
        .register(credentials.clone(), username.clone())
        .await?;
    let (device, token) = client.authenticate(credentials).await?;

    Ok(client.login(device, token).await?)
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

impl From<vertex_client::Error> for RegisterError {
    fn from(err: vertex_client::Error) -> Self {
        match err {
            vertex_client::Error::Net(_) => RegisterError::NetworkError,
            vertex_client::Error::Response(err) => err.into(),
            _ => RegisterError::UnknownError,
        }
    }
}

impl From<vertex::ErrResponse> for RegisterError {
    fn from(err: vertex::ErrResponse) -> Self {
        match err {
            vertex::ErrResponse::Internal => RegisterError::InternalServerError,
            vertex::ErrResponse::InvalidUsername => RegisterError::InvalidUsername,
            vertex::ErrResponse::InvalidPassword => RegisterError::InvalidPassword,
            vertex::ErrResponse::UsernameAlreadyExists => RegisterError::UsernameAlreadyExists,
            _ => RegisterError::UnknownError,
        }
    }
}
