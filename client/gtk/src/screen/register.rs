use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{auth, AuthParameters, Error, Result, Server, token_store, TryGetText, window};
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    instance_entry: gtk::Entry,
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
    lazy_static! {
        static ref GLADE: Glade = Glade::open("res/glade/register/register.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        instance_entry: builder.get_object("instance_entry").unwrap(),
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
    screen.login_button.connect_button_release_event(
        screen.connector()
            .do_async(|_screen, (_button, _event)| async move {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
            })
            .build_widget_event()
    );

    screen.register_button.connect_button_release_event(
        screen.connector()
            .do_async(|screen, (_button, _event)| async move {
                let instance_ip = screen.instance_entry.try_get_text().unwrap_or_default();

                let username = screen.username_entry.try_get_text().unwrap_or_default();
                let password_1 = screen.password_entry_1.try_get_text().unwrap_or_default();
                let password_2 = screen.password_entry_2.try_get_text().unwrap_or_default();

                screen.status_stack.set_visible_child(&screen.spinner);
                screen.error_label.set_text("");

                let password = if password_1 == password_2 {
                    Some(password_1)
                } else {
                    screen.error_label.set_text("Passwords do not match");
                    None
                };

                if let Some(password) = password {
                    match register(instance_ip, username, password).await {
                        Ok(parameters) => {
                            screen::active::start(parameters).await;
                        }
                        Err(err) => {
                            println!("Encountered error during register: {:?}", err);
                            screen.error_label.set_text(describe_error(err));
                        }
                    }
                }

                screen.status_stack.set_visible_child(&screen.error_label);
            })
            .build_widget_event()
    );
}

async fn register(
    instance_: String,
    username: String,
    password: String,
) -> Result<AuthParameters> {
    use vertex::prelude::*;

    let instance = Server::parse(instance_)?;
    let credentials = Credentials::new(username, password);

    let auth = auth::Client::new(instance.clone());

    auth.register(credentials.clone(), None).await?;

    let token = auth.create_token(
        credentials,
        TokenCreationOptions::default(),
    ).await?;

    let parameters = AuthParameters {
        instance,
        device: token.device,
        token: token.token,
    };

    token_store::store_token(&parameters);

    Ok(parameters)
}

fn describe_error(error: Error) -> &'static str {
    use vertex::requests::AuthError;

    match error {
        Error::InvalidUrl => "Invalid instance ip",
        Error::Http(http) => if http.is_connect() {
            "Couldn't connect to instance"
        } else {
            "Network error"
        },
        Error::ProtocolError(_) => "Protocol error: check your server instance?",
        Error::AuthErrorResponse(err) => match err {
            AuthError::Internal => "Internal server error",
            AuthError::InvalidUsername => "Invalid username",
            AuthError::InvalidPassword => "Invalid password",
            _ => "Unknown auth error",
        },

        _ => "Unknown error",
    }
}
