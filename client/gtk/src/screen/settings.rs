use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{Client, token_store, window};
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;
use gtk::Align;

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    client: Client<screen::active::Ui>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
}

pub async fn build(client: Client<screen::active::Ui>) -> Screen {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("settings/settings.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let category_list: gtk::ListBox = builder.get_object("category_list").unwrap();

    let perms = client.state.upgrade().unwrap().read().await.admin_perms;
    if perms.bits() > 0 { // If has any flags
        let label = gtk::LabelBuilder::new()
            .label("Administration")
            .halign(Align::Start)
            .build();

        let pos = (category_list.get_children().len() - 2) as i32; // 2 for divider & close
        category_list.insert(&label, pos);

        let row = category_list.get_row_at_index(pos).unwrap();
        row.set_widget_name("administration");
        category_list.show_all();
    }

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        client,
        category_list,
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
                            println!("x");
                            token_store::forget_token();
                            println!("y");
                            screen.client.log_out().await;
                            println!("z");
                        }
                        "close" => {
                            window::set_screen(&screen.client.ui.main);
                        }
                        x => {}
                    }
                }
            })
            .build_widget_and_option_consumer()
    );
}
