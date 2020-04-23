use atk::prelude::*;
use gtk::prelude::*;
use lazy_static::lazy_static;
use crate::{Glade, Client, screen, TryGetText};
use crate::connect::AsConnector;
use crate::screen::active::dialog;
use vertex::requests::ServerUser;
use std::iter;

pub fn build_administration(
    client: Client<screen::active::Ui>
) -> gtk::Widget {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("settings/administration.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let users_search: gtk::SearchEntry = builder.get_object("users_search_entry").unwrap();
    let list_all_button: gtk::Button = builder.get_object("list_users_button").unwrap();
    let users_list_view: gtk::TreeView = builder.get_object("users_search_list").unwrap();
    let users_list = create_model();
    create_and_setup_view(&users_list,&users_list_view);

    users_search.connect_activate(
        (client.clone(), users_list.clone(), users_list_view.clone()).connector()
            .do_async(|(client, list, view), entry: gtk::SearchEntry| {
                async move {
                    let txt = entry.try_get_text().unwrap_or_else(|_| String::new());
                    match client.search_users(txt).await {
                        Ok(users) => insert_users(&list, &view, users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                }
            })
            .build_cloned_consumer()
    );

    list_all_button.connect_clicked(
        (client, users_list, users_list_view).connector()
            .do_async(|(client, list, view), _| {
                async move {
                    match client.list_all_server_users().await {
                        Ok(users) => insert_users(&list, &view, users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                }
            })
            .build_cloned_consumer()
    );

    main.upcast()
}

fn create_model() -> gtk::ListStore {
    let types: Vec<glib::Type> = Some(bool::static_type())
        .into_iter()
        .chain(iter::repeat(String::static_type()))
        .take(7)
        .collect();
    let users_list = gtk::ListStore::new(&types);

    users_list
}

fn append_text_column(header: &str, tree: &gtk::TreeView, id: i32) {
    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererText::new();
    column.pack_start(&cell, true);
    column.add_attribute(&cell, "text", id);
    column.set_title(header);
    tree.append_column(&column);
}

fn create_and_setup_view(store: &gtk::ListStore, tree: &gtk::TreeView) {
    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererToggle::new();
    cell.set_activatable(true);

    cell.connect_toggled(
        store.connector()
            .do_sync(|store, (_cell, path): (gtk::CellRendererToggle, gtk::TreePath)| {
                let row = store.get_iter(&path).unwrap();
                let toggled = store.get_value(&row, 0).downcast::<bool>().unwrap();
                store.set_value(&row, 0, &(!toggled.get_some()).to_value())
            })
            .build_widget_and_owned_listener()
    );

    column.pack_start(&cell, true);
    column.add_attribute(&cell, "active", 0);
    column.set_title("Selected");
    tree.append_column(&column);

    let headers = [
        "Username",
        "Display name",
        "Banned",
        "Compromised",
        "Locked",
        "Latest hash scheme"
    ];

    for (i, header) in headers.iter().enumerate() {
        append_text_column(header, tree, i as i32 + 1);
    }

    tree.set_model(Some(store));
}

fn insert_users(
    list: &gtk::ListStore,
    view: &gtk::TreeView,
    users: Vec<ServerUser>
) {
    list.clear();

    for user in users {
        insert_user(list, user);
    }

    view.set_model(Some(list));
    view.show_all();
}

fn insert_user(list: &gtk::ListStore, user: ServerUser) {
    // +---------+----------+--------------+--------+-------------+--------+------------+
    // | Checked | Username | Display name | Banned | Compromised | Locked | Latest HSV |
    // +---------+----------+--------------+--------+-------------+--------+------------+

    let arr: &[&dyn glib::ToValue] = &[
        &false,
        &user.username,
        &user.display_name,
        &label_for_bool(user.banned),
        &label_for_bool(user.compromised),
        &label_for_bool(user.locked),
        &label_for_bool(user.latest_hash_scheme),
    ];

    let cols: Vec<_> = (0..7).collect();
    list.insert_with_values(None, &cols, arr);
}

fn label_for_bool(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}
