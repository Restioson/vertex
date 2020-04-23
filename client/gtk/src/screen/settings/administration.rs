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
    create_and_setup_view(&users_list_view);
    let users_list = create_treeview(&users_list_view);

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

fn create_treeview(view: &gtk::TreeView) -> gtk::ListStore {
    let types: Vec<glib::Type> = Some(bool::static_type())
        .into_iter()
        .chain(iter::repeat(String::static_type()))
        .take(7)
        .collect();
    let users_list = gtk::ListStore::new(&types);

    view.set_model(Some(&users_list));
    users_list
}

fn append_text_column(tree: &gtk::TreeView, id: i32) {
    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererText::new();

    column.pack_start(&cell, true);
    // Association of the view's column with the model's `id` column.
    column.add_attribute(&cell, "text", id);
    tree.append_column(&column);
}

fn create_and_setup_view(tree: &gtk::TreeView) {
    tree.set_headers_visible(false);

    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererToggle::new();
    column.pack_start(&cell, true);
    column.add_attribute(&cell, "", 0);
    tree.append_column(&column);

    for i in 1..7 {
        append_text_column(tree, i);
    }
}

fn insert_users(
    list: &gtk::ListStore,
    view: &gtk::TreeView,
    users: Vec<ServerUser>
) {
    // Clear all rows
    list.clear();

    for (y, user) in users.into_iter().enumerate() {
        insert_user(list, user, y as i32 + 1);
    }

    view.set_model(Some(list));
    view.show_all();
}

fn insert_user(list: &gtk::ListStore, user: ServerUser, y: i32) {
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

fn label_for(text: &str) -> &str {
    // gtk::LabelBuilder::new()
    //     .label(text)
    //     .selectable(true)
    //     .build()
    text
}

fn label_for_bool(b: bool) -> &'static str {
    label_for(if b { "Yes" } else { "No" })
}
