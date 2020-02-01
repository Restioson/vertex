use gtk::prelude::*;

use std::rc::Rc;

use crate::net;
use crate::screen::{self, DynamicScreen, Screen};

const SCREEN_SRC: &str = include_str!("glade/settings/settings.glade");

pub struct Widgets {
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<vertex_client::Client<net::Sender>>,
    widgets: Widgets,
}

pub fn build(app: Rc<crate::App>, client: Rc<vertex_client::Client<net::Sender>>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(SCREEN_SRC);

    let viewport: gtk::Viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app,
        client,
        widgets: Widgets {
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

    widgets.category_list.connect_row_selected(
        screen
            .connector()
            .do_async(|screen, (_list, row)| async move {
                if let Some(row) = row {
                    let row: gtk::ListBoxRow = row;
                    let name = row.get_widget_name().map(|s| s.as_str().to_owned()).unwrap_or_default();

                    let model = screen.model();

                    match name.as_str() {
                        "log_out" => {
                            model.app.token_store.forget_token();
                            model.client.revoke_token().await.expect("failed to revoke token");

                            let login = screen::login::build(model.app.clone());
                            model.app.set_screen(DynamicScreen::Login(login));
                        }
                        "close" => {
                            let active = screen::active::build(model.app.clone(), model.client.clone());
                            model.app.set_screen(DynamicScreen::Active(active));
                        }
                        _ => (),
                    }
                }
            })
            .build_widget_and_option_consumer(),
    );
}
