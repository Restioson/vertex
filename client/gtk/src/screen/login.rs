use gtk::prelude::*;

use std::rc::Rc;

use crate::net;
use crate::screen::{self, DynamicScreen, Screen, TryGetText};

use std::fmt;

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
        app,
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
        screen
            .connector()
            .do_async(|screen, (_button, _event)| {
                async move {
                    let model = screen.model();
                    let widgets = &model.widgets;
                    let username = widgets.username_entry.try_get_text().unwrap_or_default();
                    let password = widgets.password_entry.try_get_text().unwrap_or_default();

                    widgets.status_stack.set_visible_child(&widgets.spinner);
                    widgets.error_label.set_text("");

                    match login(&screen.model().app, username, password).await {
                        Ok(client) => {
                            let (device, token) = client.token();
                            model.app.token_store.store_token(device, token);

                            let client = Rc::new(client);

                            let active = screen::active::build(screen.model().app.clone(), client);
                            screen.model().app.set_screen(DynamicScreen::Active(active));
                        }
                        Err(err) => model.widgets.error_label.set_text(&format!("{}", err)),
                    }

                    widgets.status_stack.set_visible_child(&widgets.error_label);
                }
            })
            .build_widget_event(),
    );

    widgets.register_button.connect_button_press_event(
        screen
            .connector()
            .do_sync(|screen, (_button, _event)| {
                let register = screen::register::build(screen.model().app.clone());
                screen
                    .model()
                    .app
                    .set_screen(DynamicScreen::Register(register));
            })
            .build_widget_event(),
    );
}

async fn login(
    app: &crate::App,
    username: String,
    password: String,
) -> Result<vertex_client::Client<net::Sender>, LoginError> {
    let client = vertex_client::auth::Client::new(app.request_sender());

    let (device, token) = match app.token_store.get_stored_token() {
        Some(token) => token,
        None => {
            client
                .authenticate(vertex::UserCredentials::new(username, password))
                .await?
        }
    };

    Ok(client.login(device, token).await?)
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

impl From<vertex_client::Error> for LoginError {
    fn from(err: vertex_client::Error) -> Self {
        match err {
            vertex_client::Error::Net(_) => LoginError::NetworkError,
            vertex_client::Error::Response(err) => err.into(),
            _ => LoginError::UnknownError,
        }
    }
}

impl From<vertex::ErrResponse> for LoginError {
    fn from(err: vertex::ErrResponse) -> Self {
        match err {
            vertex::ErrResponse::Internal => LoginError::InternalServerError,
            vertex::ErrResponse::IncorrectUsernameOrPassword | vertex::ErrResponse::InvalidUser => {
                LoginError::InvalidUsernameOrPassword
            }
            _ => LoginError::UnknownError,
        }
    }
}
