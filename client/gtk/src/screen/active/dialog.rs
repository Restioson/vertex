use gtk::prelude::*;

use lazy_static::lazy_static;
use vertex::prelude::*;

use crate::{Client, Result, TryGetText};
use crate::connect::AsConnector;
use crate::Glade;
use crate::window;

use super::Ui;

pub fn show_add_community(client: Client<Ui>) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/add_community.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let create_community_button: gtk::Button = builder.get_object("create_community_button").unwrap();
    let join_community_button: gtk::Button = builder.get_object("join_community_button").unwrap();

    let dialog = window::show_dialog(main);

    create_community_button.connect_activate(
        client.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |client, _| {
                    dialog.close();
                    show_create_community(client);
                }
            })
            .build_cloned_consumer()
    );

    join_community_button.connect_activate(
        client.connector()
            .do_sync({
                move |client, _| {
                    dialog.close();
                    show_join_community(client);
                }
            })
            .build_cloned_consumer()
    );
}

pub fn show_create_community(client: Client<Ui>) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/create_community.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let name_entry: gtk::Entry = builder.get_object("name_entry").unwrap();
    let create_button: gtk::Button = builder.get_object("create_button").unwrap();

    let dialog = window::show_dialog(main);

    create_button.connect_activate(
        client.connector()
            .do_async(move |client, _| {
                let name_entry = name_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(name) = name_entry.try_get_text() {
                        dialog.close();

                        async fn create_community(client: Client<Ui>, name: &str) -> Result<()> {
                            let community = client.create_community(name).await?;

                            community.create_room("General").await?;
                            community.create_room("Off Topic").await?;

                            Ok(())
                        }

                        if let Err(err) = create_community(client, &name).await {
                            show_generic_error(&err);
                        }
                    }
                }
            })
            .build_cloned_consumer()
    );
}

pub fn show_join_community(client: Client<Ui>) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/join_community.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let code_entry: gtk::Entry = builder.get_object("invite_code_entry").unwrap();
    let join_button: gtk::Button = builder.get_object("join_button").unwrap();

    let dialog = window::show_dialog(main);

    join_button.connect_activate(
        client.connector()
            .do_async(move |client, _| {
                let code_entry = code_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(code) = code_entry.try_get_text() {
                        dialog.close();

                        let code = InviteCode(code);
                        if let Err(err) = client.join_community(code).await {
                            show_generic_error(&err);
                        }
                    }
                }
            })
            .build_cloned_consumer()
    );
}

pub fn show_generic_error<E: std::fmt::Display>(error: &E) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/error.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let description_label: gtk::Label = builder.get_object("description").unwrap();
    description_label.set_text(&format!("{}", error));

    let ok_button: gtk::Button = builder.get_object("ok_button").unwrap();

    let dialog = window::show_dialog(main);

    ok_button.connect_activate(
        dialog.connector()
            .do_sync(|dialog, _| {
                dialog.close();
            })
            .build_cloned_consumer()
    );
}
