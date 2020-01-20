use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen, TryGetText};

const GLADE_SRC: &str = include_str!("glade/register.glade");

pub struct Widgets {
    username_entry: gtk::Entry,
    password_entry_1: gtk::Entry,
    password_entry_2: gtk::Entry,
    register_button: gtk::Button,
    login_button: gtk::Button,
    error_label: gtk::Label,
}

pub struct Model {
    app: Rc<crate::App>,
    widgets: Widgets,
}

pub fn build(app: Rc<crate::App>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(GLADE_SRC);

    let viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app: app.clone(),
        widgets: Widgets {
            username_entry: builder.get_object("username_entry").unwrap(),
            password_entry_1: builder.get_object("password_entry_1").unwrap(),
            password_entry_2: builder.get_object("password_entry_2").unwrap(),
            register_button: builder.get_object("register_button").unwrap(),
            login_button: builder.get_object("login_button").unwrap(),
            error_label: builder.get_object("error_label").unwrap()
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
            .do_sync(|screen, (button, event)| {
                let login = screen::login::build(screen.model().app.clone());
                screen.model().app.set_screen(DynamicScreen::Login(login));
            })
            .build_widget_event()
    );

    widgets.register_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (button, event)| async move {
                let model = screen.model();

                let username = model.widgets.username_entry.try_get_text().unwrap_or_default();
                let password_1 = model.widgets.password_entry_1.try_get_text().unwrap_or_default();
                let password_2 = model.widgets.password_entry_2.try_get_text().unwrap_or_default();

                model.widgets.error_label.set_text("");

                if password_1 == password_2 {
                    let password = password_1;

                    let client = vertex::Client::new(model.app.net());

                    // TODO: error handling
                    let user = client.register(username.clone(), username.clone(), password.clone()).await
                        .expect("failed to register");

                    let (device, token) = client.authenticate(username, password).await
                        .expect("failed to authenticate");

                    let client = client.login(device, token).await
                        .expect("failed to login");
                    let client = Rc::new(client);

                    let (device, token) = client.token();
                    model.app.token_store.store_token(device, token);

                    let active = screen::active::build(model.app.clone(), client);
                    model.app.set_screen(DynamicScreen::Active(active));
                } else {
                    model.widgets.error_label.set_text("Passwords do not match");
                }
            })
            .build_widget_event()
    );
}
