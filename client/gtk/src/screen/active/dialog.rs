use gtk::prelude::*;

use vertex::*;

use crate::{Client, TryGetText};
use crate::connect::AsConnector;
use crate::window;

use super::Ui;

pub fn show_add_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/add_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let create_community_button: gtk::Button = builder.get_object("create_community_button").unwrap();
    let join_community_button: gtk::Button = builder.get_object("join_community_button").unwrap();

    let dialog = window::show_dialog(main);

    create_community_button.connect_button_release_event(
        client.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |client, _| {
                    dialog.close();
                    show_create_community(client);
                }
            })
            .build_widget_event()
    );

    join_community_button.connect_button_release_event(
        client.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |client, _| {
                    dialog.close();
                    show_join_community(client);
                }
            })
            .build_widget_event()
    );
}

pub fn show_create_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/create_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let name_entry: gtk::Entry = builder.get_object("name_entry").unwrap();
    let create_button: gtk::Button = builder.get_object("create_button").unwrap();

    let dialog = window::show_dialog(main);

    create_button.connect_button_release_event(
        client.connector()
            .do_async(move |client, _| {
                let name_entry = name_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(name) = name_entry.try_get_text() {
                        dialog.close();

                        // TODO: error handling
                        let community = client.create_community(&name).await.unwrap();

                        community.create_room("General").await.unwrap();
                        community.create_room("Off Topic").await.unwrap();
                    }
                }
            })
            .build_widget_event()
    );
}

pub fn show_join_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/join_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let code_entry: gtk::Entry = builder.get_object("invite_code_entry").unwrap();
    let join_button: gtk::Button = builder.get_object("join_button").unwrap();

    let dialog = window::show_dialog(main);

    join_button.connect_button_release_event(
        client.connector()
            .do_async(move |client, _| {
                let code_entry = code_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(code) = code_entry.try_get_text() {
                        dialog.close();

                        let code = InviteCode(code);
                        // TODO: error handling
                        client.join_community(code).await.unwrap();
                    }
                }
            })
            .build_widget_event()
    );
}
