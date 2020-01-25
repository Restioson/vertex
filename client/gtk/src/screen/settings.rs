use gtk::prelude::*;

use std::rc::Rc;

use vertex_common::*;
use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen, TryGetText};

use std::fmt;

const GLADE_SRC: &str = include_str!("glade/settings.glade");

pub struct Widgets {
    apply_button: gtk::Button,
    close_button: gtk::Button,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<vertex::Client>,
    widgets: Widgets,
}

pub fn build(app: Rc<crate::App>, client: Rc<vertex::Client>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(GLADE_SRC);

    let viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app,
        client,
        widgets: Widgets {
            apply_button: builder.get_object("apply_button").unwrap(),
            close_button: builder.get_object("close_button").unwrap(),
            category_list: builder.get_object("category_list").unwrap(),
            settings_viewport: builder.get_object("settings_viewport").unwrap(),
        },
    };

    let screen = Screen::new(viewport, model);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.close_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (button, event)| return_to_active(&screen.model()))
            .build_widget_event()
    );

    widgets.apply_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (button, event)| return_to_active(&screen.model()))
            .build_widget_event()
    );

    widgets.category_list.connect_row_selected(
        screen.connector()
            .do_async(|screen, (list, row)| async move {
                let model = screen.model();
                model.client.revoke_current_token().await.expect("failed to revoke token");
                model.app.token_store.forget_token();

                let login = screen::login::build(model.app.clone());
                model.app.set_screen(DynamicScreen::Login(login));
            })
            .build_widget_and_option_consumer()
    );
}

fn return_to_active(model: &Model) {
    let active = screen::active::build(model.app.clone(), model.client.clone());
    model.app.set_screen(DynamicScreen::Active(active));
}
