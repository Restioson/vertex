use gtk::prelude::*;

use crate::{Client, token_store, UiEntity, window};
use crate::connect::AsConnector;
use crate::screen;

pub struct Model {
    pub main: gtk::Viewport,
    client: UiEntity<Client<screen::active::Ui>>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
}

pub fn build(client: UiEntity<Client<screen::active::Ui>>) -> UiEntity<Model> {
    let builder = gtk::Builder::new_from_file("res/glade/settings/settings.glade");

    let screen = UiEntity::new(Model {
        main: builder.get_object("viewport").unwrap(),
        client,
        category_list: builder.get_object("category_list").unwrap(),
        settings_viewport: builder.get_object("settings_viewport").unwrap(),
    });

    bind_events(&screen);

    screen
}

fn bind_events(screen: &UiEntity<Model>) {
    let model = screen.borrow();

    model.category_list.connect_row_selected(
        screen.connector()
            .do_async(|screen, (_list, row)| async move {
                if let Some(row) = row {
                    let row: gtk::ListBoxRow = row;
                    let name = row.get_widget_name()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();

                    let model = screen.borrow();

                    match name.as_str() {
                        "log_out" => {
                            token_store::forget_token();
                            model.client.borrow_mut().log_out().await.expect("failed to revoke token");

                            let screen = screen::login::build();
                            window::set_screen(&screen.borrow().main);
                        }
                        "close" => {
                            window::set_screen(&model.client.borrow().ui.main);
                        }
                        _ => ()
                    }
                }
            })
            .build_widget_and_option_consumer()
    );
}
