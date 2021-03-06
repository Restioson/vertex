use std::sync::Mutex;
use std::rc::Rc;
use std::fmt;
use atk::prelude::*;
use gtk::prelude::*;
use lazy_static::lazy_static;
use bimap::BiMap;
use vertex::prelude::*;
use crate::{Glade, Client};
use crate::screen::active::dialog;
use users_search::UsersSearch;
use admins_list::AdminsList;
use reports_list::ReportsList;
use crate::connect::AsConnector;

mod users_search;
mod admins_list;
mod parse_search;
mod reports_list;

lazy_static! {
    static ref GLADE: Glade = Glade::open("settings/administration.glade").unwrap();
}

pub fn build_administration(
    client: Client,
    perms: AdminPermissionFlags,
) -> gtk::Widget {
    use vertex::requests::AdminPermissionFlags as Perms;

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();
    UsersSearch::build(builder.clone(), client.clone());
    AdminsList::build(builder.clone(), client.clone());
    ReportsList::build(builder.clone() , client.clone());

    if perms.contains(Perms::SET_ACCOUNTS_COMPROMISED) || perms.contains(Perms::ALL) {
        let buttons: gtk::Box = builder.get_object("set_compromised_buttons").unwrap();
        let set_all_compromised = gtk::Button::new_with_label("Set all accounts compromised");
        set_all_compromised.connect_clicked(
            client.connector()
                .do_sync(|client, _| {
                    dialog::show_confirm(
                        "Confirm Action",
                        "Are you sure you want to mark all\naccounts as compromised? \
                        This will\nlog out all users and require them\nto change their passwords.",
                        client,
                        |client| async move {
                            if let Err(e) = client.set_compromised(SetCompromisedType::All).await {
                                dialog::show_generic_error(&e)
                            }
                        },
                    );

                })
                .build_cloned_consumer()
        );

        buttons.add(&set_all_compromised);
        buttons.show_all();
    }

    main.upcast()
}

fn append_text_column(header: &str, tree: &gtk::TreeView, id: i32) {
    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererText::new();
    column.pack_start(&cell, true);
    column.add_attribute(&cell, "text", id);
    column.set_title(header);
    tree.append_column(&column);
}

fn append_checkbutton_column(header: &str, list: &gtk::ListStore, tree: &gtk::TreeView, id: i32) {
    let column = gtk::TreeViewColumn::new();
    let cell = gtk::CellRendererToggle::new();
    cell.set_activatable(true);

    cell.connect_toggled(
        list.connector()
            .do_sync(move |store, (_cell, path): (gtk::CellRendererToggle, gtk::TreePath)| {
                let row = store.get_iter(&path).unwrap();
                let toggled = store.get_value(&row, id).get::<bool>().unwrap().unwrap();
                let toggled = !toggled;
                store.set_value(&row, id as u32, &toggled.to_value())
            })
            .build_widget_and_owned_listener()
    );

    column.pack_start(&cell, true);
    column.add_attribute(&cell, "active", id);
    column.set_title(header);
    tree.append_column(&column);
}


enum Action {
    Ban,
    Unban,
    Unlock,
    Demote,
    Promote { permissions: AdminPermissionFlags }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let gerund = match self {
            Action::Ban => "banning",
            Action::Unban => "unbanning",
            Action::Unlock => "unlocking",
            Action::Demote => "demoting",
            Action::Promote { .. } => "promoting",
        };

        f.write_str(gerund)
    }
}

async fn perform_action(
    action: Action,
    list: gtk::ListStore,
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>,
    client: &Client,
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
        Action::Promote { permissions } => client.promote_users(selected, permissions).await,
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
