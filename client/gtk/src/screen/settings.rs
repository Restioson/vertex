use gtk::prelude::*;

use lazy_static::lazy_static;

mod administration;

use crate::{Client, token_store, window, SharedMut};
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;
use administration::*;
use gtk::{Align, Orientation};

#[derive(Clone)]
pub struct Screen {
    pub main: gtk::Viewport,
    client: Client<screen::active::Ui>,
    category_list: gtk::ListBox,
    settings_viewport: gtk::Viewport,
    current_settings: SharedMut<gtk::Widget>,
    close: gtk::Button,
    log_out: gtk::Button,
}

pub async fn build(client: Client<screen::active::Ui>) -> Screen {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("settings/settings.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let close: gtk::Button = builder.get_object("close_button").unwrap();
    let log_out: gtk::Button = builder.get_object("log_out_button").unwrap();
    let category_list: gtk::ListBox = builder.get_object("category_list").unwrap();

    let perms = client.state.upgrade().unwrap().read().await.admin_perms;
    if !perms.is_empty() {
        let label = gtk::LabelBuilder::new()
            .label("Administration")
            .halign(Align::Start)
            .build();

        let pos = category_list.get_children().len() as i32;
        category_list.insert(&label, pos);

        let row = category_list.get_row_at_index(pos).unwrap();
        row.set_widget_name("admin");
        category_list.show_all();
    }

    let settings_viewport: gtk::Viewport = builder.get_object("settings_viewport").unwrap();
    let empty = gtk::Box::new(Orientation::Vertical, 0).upcast();
    settings_viewport.add(&empty);

    let screen = Screen {
        main: builder.get_object("viewport").unwrap(),
        client,
        category_list,
        settings_viewport,
        current_settings: SharedMut::new(empty),
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

    screen.category_list.connect_row_selected(
        screen.connector()
            .do_async(|screen, (_list, row)| async move {
                if let Some(row) = row {
                    let row: gtk::ListBoxRow = row;
                    let name = row.get_widget_name()
                        .map(|s| s.as_str().to_owned())
                        .unwrap_or_default();

                    let widget = match name.as_str() {
                        "admin" => Some(build_administration(screen.client)),
                        _ => None,
                    };

                    let mut cur = screen.current_settings.write().await;
                    screen.settings_viewport.remove(&*cur);
                    let widget = widget.unwrap_or_else(|| {
                        gtk::Box::new(Orientation::Vertical, 0).upcast()
                    });

                    screen.settings_viewport.add(&widget);
                    *cur = widget;
                }
            })
            .build_widget_and_option_consumer()
    );
}
