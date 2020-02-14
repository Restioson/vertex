use gtk::prelude::*;

use crate::{auth, AuthParameters, Error, Result, Server, token_store, TryGetText, window};
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

                let username = screen.username_entry.try_get_text().unwrap_or_default();
                let password = screen.password_entry.try_get_text().unwrap_or_default();

                screen.status_stack.set_visible_child(&screen.spinner);
                screen.error_label.set_text("");

                match login(instance_ip, username, password).await {
                    Ok(parameters) => {
                        screen::active::start(parameters).await;
                    }
                    Err(err) => {
                        println!("Encountered error during login: {:?}", err);
                        screen.error_label.set_text(describe_error(err));
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
    instance: String,
    username: String,
    password: String,
) -> Result<AuthParameters> {
    let instance = Server::parse(instance)?;
    println!("{:?}", instance);

    match token_store::get_stored_token() {
        Some(parameters) if parameters.instance == instance => Ok(parameters),
        _ => {
            let auth = auth::Client::new(instance.clone());
            let token = auth.create_token(
                vertex::UserCredentials::new(username, password),
                vertex::TokenCreationOptions::default(),
            ).await?;

            let parameters = AuthParameters {
                instance,
                device: token.device,
                token: token.token,
            };

            token_store::store_token(&parameters);

            Ok(parameters)
        }
    }
}

fn describe_error(error: Error) -> &'static str {
    match error {
        Error::InvalidUrl => "Invalid instance ip",
        Error::Http(http) => if http.is_connect() {
            "Couldn't connect to instance"
        } else {
            "Network error"
        },
        Error::ProtocolError(_) => "Protocol error: check your server instance?",
        Error::AuthErrorResponse(err) => match err {
            vertex::AuthError::Internal => "Internal server error",
            vertex::AuthError::IncorrectCredentials | vertex::AuthError::InvalidUser
            => "Invalid username or password",
            _ => "Unknown auth error",
        },

        _ => "Unknown error",
    }
}
