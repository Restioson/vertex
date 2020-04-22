use gtk::prelude::*;
use lazy_static::lazy_static;
use crate::{Glade, Client, screen, TryGetText};
use crate::connect::AsConnector;
use crate::screen::active::dialog;
use vertex::requests::ServerUser;
use std::rc::Rc;
use std::cell::Cell;

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
    let users_list: gtk::Grid = builder.get_object("users_search_grid").unwrap();
    let len = Rc::new(Cell::new(0));

    users_search.connect_activate(
        (client.clone(), users_list.clone(), len.clone()).connector()
            .do_async(|(client, list, len), entry: gtk::SearchEntry| {
                async move {
                    let txt = entry.try_get_text().unwrap_or_else(|_| String::new());
                    match client.search_users(txt).await {
                        Ok(users) => insert_users(&len, &list, users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                }
            })
            .build_cloned_consumer()
    );

    list_all_button.connect_clicked(
        (client, users_list, len).connector()
            .do_async(|(client, list, len), _| {
                async move {
                    match client.list_all_server_users().await {
                        Ok(users) => insert_users(&len, &list, users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                }
            })
            .build_cloned_consumer()
    );

    main.upcast()
}

fn insert_users(len: &Cell<usize>, grid: &gtk::Grid, users: Vec<ServerUser>) {
    // Clear all rows
    for _ in 0..len.get() {
        // Row number changes down as is removed
        grid.remove_row(1 as i32);
    }

    // Add new rows
    len.set(users.len());
    for (y, user) in users.into_iter().enumerate() {
        insert_user(grid, user, y as i32 + 1);
        grid.show_all();
    }
}

fn insert_user(grid: &gtk::Grid, user: ServerUser, y: i32) {
    // +---------+----------+--------------+--------+-------------+--------+------------+
    // | Checked | Username | Display name | Banned | Compromised | Locked | Latest HSV |
    // +---------+----------+--------------+--------+-------------+--------+------------+

    let check = gtk::CheckButton::new().upcast();
    let name = gtk::Label::new(Some(&user.username)).upcast();
    let display_name = gtk::Label::new(Some(&user.display_name)).upcast();
    let banned = label_for_bool(user.banned);
    let compromised = label_for_bool(user.compromised);
    let locked = label_for_bool(user.locked);
    let latest_hsv = label_for_bool(user.latest_hash_scheme);
    let arr = [check, name, display_name, banned, compromised, locked, latest_hsv];

    for (x, widget) in arr.iter().enumerate() {
        widget.get_style_context().add_class("user_property_item");
        grid.attach(widget, x as i32, y, 1, 1);
    }
}

fn label_for_bool(b: bool) -> gtk::Widget {
    gtk::Label::new(Some(if b { "Yes" } else { "No" })).upcast()
}
