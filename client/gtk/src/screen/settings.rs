use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{Client, SharedMut, token_store, window};
use crate::config;
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    client: Client<screen::active::Ui>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
    current_settings: SharedMut<Option<gtk::Widget>>,
    close: gtk::Button,
    log_out: gtk::Button,
}

pub fn build(client: Client<screen::active::Ui>) -> Screen {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("settings/settings.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let close: gtk::Button = builder.get_object("close_button").unwrap();
    let log_out: gtk::Button = builder.get_object("log_out_button").unwrap();

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        client,
        category_list: builder.get_object("category_list").unwrap(),
        settings_viewport: builder.get_object("settings_viewport").unwrap(),
        current_settings: SharedMut::new(None),
        close,
        log_out,
    };

    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen) {
    screen.close.connect_clicked(
        screen.connector()
            .do_sync(|screen, _| window::set_screen(&screen.client.ui.main))
            .build_cloned_consumer()
    );

    screen.log_out.connect_clicked(
        screen.connector()
            .do_async(|screen, _| async move {
                token_store::forget_token();
                screen.client.log_out().await;
            })
            .build_cloned_consumer()
    );

    // Template v v v
    screen.category_list.connect_row_selected(
        screen.connector()
            .do_async(|screen, (_list, row)| async move {
                if let Some(row) = row {
                    let row: gtk::ListBoxRow = row;
                    let name = row.get_widget_name()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();

                    let widget = match name.as_str() {
                        "a11y" => Some(build_accessibility()),
                        _ => None,
                    };

                    if let Some(widget) = widget {
                        let mut cur = screen.current_settings.write().await;
                        if let Some(cur) = cur.take() {
                            screen.settings_viewport.remove(&cur);
                        }

                        screen.settings_viewport.add(&widget);
                        widget.show_all();
                        screen.settings_viewport.show_all();

                        *cur = Some(widget);
                    }
                }
            })
            .build_widget_and_option_consumer()
    );
}

fn build_accessibility() -> gtk::Widget {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("settings/a11y.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let viewport: gtk::Box = builder.get_object("main").unwrap();

    let narrate_new: gtk::Switch = builder.get_object("narrate_new").unwrap();

    let config = config::get();
    narrate_new.set_state(config.narrate_new_messages);

    narrate_new.connect_state_set(|_switch, state| {
        config::modify(|config| config.narrate_new_messages = state);
        gtk::Inhibit(false)
    });

    viewport.upcast()
}
