use std::fmt;
use std::rc::Rc;

use gtk::prelude::*;

use crate::auth;
use crate::screen::{self, Screen, TryGetText};

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
    let builder = gtk::Builder::new_from_file("res/glade/register/register.glade");

    let viewport: gtk::Viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app: app.clone(),
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
        screen.connector()
            .do_sync(|screen, (_button, _event)| {
                let app = &screen.model().app;
                app.set_screen(screen::login::build(app.clone()));
            })
            .build_widget_event()
    );

    widgets.register_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let model = screen.model();

                let username = model.widgets.username_entry.try_get_text().unwrap_or_default();
                let password_1 = model.widgets.password_entry_1.try_get_text().unwrap_or_default();
                let password_2 = model.widgets.password_entry_2.try_get_text().unwrap_or_default();

                model.widgets.status_stack.set_visible_child(&model.widgets.spinner);
                model.widgets.error_label.set_text("");

                match register(&screen.model().app, username, password_1, password_2).await {
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
}

async fn register(
    app: &crate::App,
    username: String,
    password_1: String,
    password_2: String,
) -> Result<auth::AuthenticatedWs, RegisterError> {
    if password_1 != password_2 {
        return Err(RegisterError::PasswordsDoNotMatch);
    }

    let password = password_1;
    let credentials = vertex::UserCredentials::new(username, password);

    let auth = auth::Client::new(app.server());

    let _ = auth.register(credentials.clone(), None).await?;

    let token = auth.create_token(
        credentials,
        vertex::TokenCreationOptions::default(),
    ).await?;

    app.token_store.store_token(token.device, token.token.clone());

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
