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
    password_entry: gtk::Entry,
    login_button: gtk::Button,
    register_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub async fn build() -> Screen {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("login/login.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();

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
    screen.login_button.connect_activate(
        screen.connector()
            .do_async(|screen, _| async move {
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
                        screen.error_label.set_text(&describe_error(err));
                    }
                }

                screen.status_stack.set_visible_child(&screen.error_label);
            })
            .build_cloned_consumer()
    );

    screen.register_button.connect_activate(
        screen.connector()
            .do_async(|_screen, _| async move {
                let screen = screen::register::build().await;
                window::set_screen(&screen.main);
            })
            .build_cloned_consumer()
    );
}

async fn login(
    instance: String,
    username: String,
    password: String,
) -> Result<AuthParameters> {
    let instance = Server::parse(instance)?;
    let auth = auth::Client::new(instance.clone());

    use vertex::prelude::*;
    let token = auth.create_token(
        Credentials::new(username, password),
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

fn describe_error(error: Error) -> String {
    match error {
        Error::InvalidUrl => "Invalid instance ip".to_owned(),
        Error::ProtocolError(_) => "Protocol error: check your server instance?".to_owned(),
        error => format!("{}", error),
    }
}
