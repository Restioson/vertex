use atk::prelude::*;
use gtk::prelude::*;
use lazy_static::lazy_static;
use crate::{Glade, Client, screen, TryGetText};
use crate::connect::AsConnector;
use crate::screen::active::dialog;
use vertex::requests::ServerUser;
use std::iter;
use std::sync::Mutex;
use bimap::BiMap;
use vertex::types::UserId;
use std::rc::Rc;
use std::fmt;

lazy_static! {
    static ref GLADE: Glade = Glade::open("settings/administration.glade").unwrap();
}

pub fn build_administration(client: Client<screen::active::Ui>) -> gtk::Widget {
    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let users_search: gtk::SearchEntry = builder.get_object("users_search_entry").unwrap();
    let list_all_button: gtk::Button = builder.get_object("list_users_button").unwrap();
    let users_list_view: gtk::TreeView = builder.get_object("users_search_list").unwrap();
    let ban_button: gtk::Button = builder.get_object("ban_button").unwrap();
    let unban_button: gtk::Button = builder.get_object("unban_button").unwrap();
    let unlock_button: gtk::Button = builder.get_object("unlock_button").unwrap();
    let demote_button: gtk::Button = builder.get_object("demote_button").unwrap();
    let users_list = create_model();
    let username_to_id: Rc<Mutex<BiMap<String, UserId>>> = Rc::new(Mutex::new(BiMap::new()));
    create_and_setup_view(&users_list,&users_list_view);

    users_search.connect_activate(
        (client.clone(), users_list.clone(), users_list_view.clone(), username_to_id.clone()).connector()
            .do_async(|(client, list, view, map), entry: gtk::SearchEntry| async move {
                let txt = entry.try_get_text().unwrap_or_else(|_| String::new());
                match client.search_users(txt).await {
                    Ok(users) => insert_users(&list, &view, map, users),
                    Err(err) => dialog::show_generic_error(&err),
                }
            })
            .build_cloned_consumer()
    );

    list_all_button.connect_clicked(
        (client.clone(), users_list.clone(), users_list_view, username_to_id.clone()).connector()
            .do_async(|(client, list, view, map), _| async move {
                match client.list_all_server_users().await {
                    Ok(users) => insert_users(&list, &view, map, users),
                    Err(err) => dialog::show_generic_error(&err),
                }
            })
            .build_cloned_consumer()
    );

    ban_button.connect_clicked(
        (client.clone(), users_list.clone(), username_to_id.clone()).connector()
            .do_async(|(client, list, map), _| {
                perform_action(Action::Ban, client, list, map)
            })
            .build_cloned_consumer()
    );

    unban_button.connect_clicked(
        (client.clone(), users_list.clone(), username_to_id.clone()).connector()
            .do_async(|(client, list, map), _| {
                perform_action(Action::Unban, client, list, map)
            })
            .build_cloned_consumer()
    );

    unlock_button.connect_clicked(
        (client.clone(), users_list.clone(), username_to_id.clone()).connector()
            .do_async(|(client, list, map), _| {
                perform_action(Action::Unlock, client, list, map)
            })
            .build_cloned_consumer()
    );

    demote_button.connect_clicked(
        (client, users_list, username_to_id).connector()
            .do_async(|(client, list, map), _| {
                perform_action(Action::Demote, client, list, map)
            })
            .build_cloned_consumer()
    );

    main.upcast()
}

// TODO(admin) v v v
// pub fn show_promote(client: Client<screen::active::Ui>, builder: gtk::Builder) {
//     let main: gtk::Box = builder.get_object("promote_dialog").unwrap();
//
//     let all: gtk::CheckButton = builder.get_object("all_checkbutton").unwrap();
//     let ban: gtk::CheckButton = builder.get_object("ban_checkbutton").unwrap();
//     let basic: gtk::CheckButton = builder.get_object("basic_checkbutton").unwrap();
//     let all: gtk::CheckButton = builder.get_object("all_checkbutton").unwrap();
//     let demote: gtk::CheckButton = builder.get_object("demote_checkbutton").unwrap();
//     let promote: gtk::CheckButton = builder.get_object("promote_checkbutton").unwrap();
//
//     let dialog = window::show_dialog(main);
//
//     join_button.connect_button_release_event(
//         client.connector()
//             .do_async(move |client, _| {
//                 let permissions = AdminPermissionFlags::from_bits_truncate(0);
//             })
//             .build_widget_event()
//     );
// }

enum Action {
    Ban,
    Unban,
    Unlock,
    Demote,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let gerund = match self {
            Action::Ban => "banning",
            Action::Unban => "unbanning",
            Action::Unlock => "unlocking",
            Action::Demote => "demoting",
        };

        f.write_str(gerund)
    }
}

async fn perform_action(
    action: Action,
    client: Client<screen::active::Ui>,
    list: gtk::ListStore,
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>
) {
    let mut selected = Vec::new();
    let map = username_to_id.lock().unwrap();
    list.foreach(|_, _, iter| {
        let toggled = list.get_value(iter, 0).get::<bool>().unwrap().unwrap();
        if toggled {
            let name = list.get_value(iter, 1).get::<String>().unwrap();
            selected.push(*map.get_by_left(&name.unwrap()).unwrap());
            list.set_value(iter, 0, &false.to_value());
        }

        false
    });
    drop(map); // Drop lock

    let res = match action {
        Action::Ban => client.ban_users(selected).await,
        Action::Unban => client.unban_users(selected).await,
        Action::Unlock => client.unlock_users(selected).await,
        Action::Demote => client.demote_users(selected).await,
    };

    match res {
        Ok(errors) if !errors.is_empty() => {
            let map = username_to_id.lock().unwrap();
            let mut msg = format!("Error {} the following {} users:", action, errors.len());
            for (id, error) in errors {
                let name = map.get_by_right(&id).unwrap();
                msg.push_str(&format!("\n  - {} ({})", name, error));
            }
            dialog::show_generic_error(&msg)
        }
        Err(err) => dialog::show_generic_error(&err),
        _ => {}
    }
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
                let toggled = store.get_value(&row, 0).get::<bool>().unwrap().unwrap();
                store.set_value(&row, 0, &(!toggled).to_value())
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
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>,
    users: Vec<ServerUser>
) {
    let mut map = username_to_id.lock().unwrap();
    list.clear();
    map.clear();

    for user in users {
        map.insert(user.username.clone(), user.id);
        insert_user(list, user);
    }

    view.set_model(Some(list));
    view.show_all();
}

fn insert_user(list: &gtk::ListStore, user: ServerUser) {
    // +----------+----------+--------------+--------+-------------+--------+-------------+
    // | Selected | Username | Display name | Banned | Compromised | Locked | Latest HSV  |
    // +----------+----------+--------------+--------+-------------+--------+-------------+

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
