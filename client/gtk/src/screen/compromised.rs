use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{Result, Server, TryGetText, window, AuthParameters};
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;
use super::login;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    old_password_entry: gtk::Entry,
    new_password_entry: gtk::Entry,
    repeat_password_entry: gtk::Entry,
    change_button: gtk::Button,
    back_to_login_button: gtk::Button,
    status_stack: gtk::Stack,
    error_label: gtk::Label,
    spinner: gtk::Spinner,
}

pub async fn build(params: AuthParameters) -> Screen {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("login/compromised.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        old_password_entry: builder.get_object("old_password_entry").unwrap(),
        new_password_entry: builder.get_object("password_entry").unwrap(),
        repeat_password_entry: builder.get_object("repeat_password_entry").unwrap(),
        change_button: builder.get_object("change_button").unwrap(),
        back_to_login_button: builder.get_object("back_to_login_button").unwrap(),
        status_stack: builder.get_object("status_stack").unwrap(),
        error_label: builder.get_object("error_label").unwrap(),
        spinner: builder.get_object("spinner").unwrap(),
    };

    bind_events(&screen, params).await;

    screen
}

async fn bind_events(screen: &Screen, params: AuthParameters) {
    screen.change_button.connect_clicked(
        (screen.clone(), params).connector()
            .do_async(|(screen, params), _| async move {
                let old = screen.old_password_entry.try_get_text().unwrap_or_default();
                let new = screen.new_password_entry.try_get_text().unwrap_or_default();
                let repeat = screen.repeat_password_entry.try_get_text().unwrap_or_default();

                screen.status_stack.set_visible_child(&screen.spinner);
                screen.error_label.set_text("");

                if new != repeat {
                    screen.error_label.set_text("Passwords do not match");
                    screen.status_stack.set_visible_child(&screen.error_label);
                    return;
                };

                let username = params.username;

                let instance = params.instance;
                let res = change_password(username.clone(), old, new.clone(), instance.clone()).await;
                if let Err(err) = res {
                    log::error!("Encountered error changing password: {:?}", err);
                    screen.error_label.set_text(&login::describe_error(err));
                    screen.status_stack.set_visible_child(&screen.error_label);
                    return;
                }

                match login::login(instance, username, new).await {
                    Ok(parameters) => {
                        screen::active::start(parameters).await;
                    }
                    Err(err) => {
                        log::error!("Encountered error logging in: {:?}", err);
                        screen.error_label.set_text(&login::describe_error(err));
                        screen.status_stack.set_visible_child(&screen.error_label);
                    }
                }
            })
            .build_cloned_consumer()
    );

    screen.back_to_login_button.connect_clicked(
        screen.connector()
            .do_async(|_screen, _| async move {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
            })
            .build_cloned_consumer()
    );
}

async fn change_password(
    username: String,
    old: String,
    new: String,
    instance: Server,
) -> Result<()> {
    let auth = crate::auth::Client::new(instance.clone());

    use vertex::prelude::*;

    auth.change_password(
        Credentials::new(username.clone(), old),
        new,
    ).await?;

    Ok(())
}