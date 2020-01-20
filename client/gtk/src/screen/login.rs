use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen};
use vertex_common::{DeviceId, AuthToken};

const GLADE_SRC: &str = include_str!("glade/login.glade");

pub struct Widgets {
    username_entry: gtk::Entry,
    password_entry: gtk::Entry,
    login_button: gtk::Button,
    register_button: gtk::Button,
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
            password_entry: builder.get_object("password_entry").unwrap(),
            login_button: builder.get_object("login_button").unwrap(),
            register_button: builder.get_object("register_button").unwrap(),
            error_label: builder.get_object("error_label").unwrap(),
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
            .do_async(|screen, (button, event)| async move {
                // TODO: duplication
                fn try_get_text(entry: &gtk::Entry) -> String {
                    entry.get_text()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default()
                }

                let model = screen.model();

                let username = try_get_text(&model.widgets.username_entry);
                let password = try_get_text(&model.widgets.password_entry);

                let client = vertex::Client::new(model.app.net.clone());

                // TODO: error handling
                let (device, token) = match model.app.token_store.get_stored_token() {
                    Some(token) => token,
                    None => client.authenticate(username, password).await.expect("failed to authenticate"),
                };

                let client = client.login(device, token).await.expect("failed to login");
                let client = Rc::new(client);

                let (device, token) = client.token();
                model.app.token_store.store_token(device, token);

                let active = screen::active::build(screen.model().app.clone(), client);
                screen.model().app.set_screen(DynamicScreen::Active(active));
            })
            .build_widget_event()
    );

    widgets.register_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (button, event)| {
                let register = screen::register::build(screen.model().app.clone());
                screen.model().app.set_screen(DynamicScreen::Register(register));
            })
            .build_widget_event()
    );
}
