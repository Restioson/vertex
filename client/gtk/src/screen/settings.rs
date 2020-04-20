use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{Client, token_store, window};
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    client: Client<screen::active::Ui>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
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
    // screen.category_list.connect_row_selected(
    //     screen.connector()
    //         .do_async(|screen, (_list, row)| async move {
    //             if let Some(row) = row {
    //                 let row: gtk::ListBoxRow = row;
    //                 let name = row.get_widget_name()
    //                     .map(|s| s.as_str().to_owned())
    //                     .unwrap_or_default();
    //
    //                 match name.as_str() {
    //                     _ => ()
    //                 }
    //             }
    //         })
    //         .build_widget_and_option_consumer()
    // );
}
