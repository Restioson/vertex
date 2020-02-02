use std::fmt;
use std::rc::Rc;

use gtk::prelude::*;

use crate::auth;
use crate::screen::{self, Screen, TryGetText};

const SCREEN_SRC: &str = include_str!("glade/login/login.glade");

pub struct Widgets {
    username_entry: gtk::Entry,
    password_entry: gtk::Entry,
    login_button: gtk::Button,
    register_button: gtk::Button,
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
        app: app.clone(),
        widgets: Widgets {
            username_entry: builder.get_object("username_entry").unwrap(),
            password_entry: builder.get_object("password_entry").unwrap(),
            login_button: builder.get_object("login_button").unwrap(),
            register_button: builder.get_object("register_button").unwrap(),
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
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let model = screen.model();

                let username = model.widgets.username_entry.try_get_text().unwrap_or_default();
                let password = model.widgets.password_entry.try_get_text().unwrap_or_default();

                model.widgets.status_stack.set_visible_child(&model.widgets.spinner);
                model.widgets.error_label.set_text("");

                match login(&screen.model().app, username, password).await {
                    Ok(ws) => {
                        let app = &screen.model().app;
                        app.set_screen(screen::active::build(app.clone(), ws));
                    }
                    Err(err) => model.widgets.error_label.set_text(&format!("{}", err)),
                }

                model.widgets.status_stack.set_visible_child(&model.widgets.error_label);
            })
            .build_widget_event()
    );

    widgets.register_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (_button, _event)| {
                let app = &screen.model().app;
                app.set_screen(screen::register::build(app.clone()));
            })
            .build_widget_event()
    );
}

async fn login(
    app: &crate::App,
    username: String,
    password: String,
) -> Result<auth::AuthenticatedWs, LoginError> {
    let auth = auth::Client::new(app.server());

    let (device, token) = match app.token_store.get_stored_token() {
        Some(token) => token,
        None => {
            let token = auth.create_token(
                vertex::UserCredentials::new(username, password),
                vertex::TokenCreationOptions::default(),
            ).await?;

            app.token_store.store_token(token.device, token.token.clone());
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
