use gtk::prelude::*;

use crate::{Client, token_store, window};
use crate::connect::AsConnector;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    client: Client<screen::active::Ui>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
}

pub fn build(client: Client<screen::active::Ui>) -> Screen {
    let builder = gtk::Builder::new_from_file("res/glade/settings/settings.glade");

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        client,
        category_list: builder.get_object("category_list").unwrap(),
        settings_viewport: builder.get_object("settings_viewport").unwrap(),
    };

    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen) {
    screen.category_list.connect_row_selected(
        screen.connector()
            .do_async(|screen, (_list, row)| async move {
                if let Some(row) = row {
                    let row: gtk::ListBoxRow = row;
                    let name = row.get_widget_name()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();

                    match name.as_str() {
                        "log_out" => {
                            token_store::forget_token();
                            screen.client.log_out().await;
                        }
                        "close" => {
                            window::set_screen(&screen.client.ui.main);
                        }
                        _ => ()
                    }
                }
            })
            .build_widget_and_option_consumer()
    );
}
