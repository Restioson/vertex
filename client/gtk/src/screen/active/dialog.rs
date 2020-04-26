use gtk::prelude::*;

use vertex::prelude::*;

use crate::{Client, Result, TryGetText, client};
use crate::connect::AsConnector;
use crate::window;

use super::Ui;
use gtk::{DialogFlags, ResponseType, Label, EntryBuilder, WidgetExt};
use atk::{RelationType, AtkObjectExt, RelationSetExt};

pub fn show_add_community(client: Client<Ui>) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("Create", ResponseType::Other(0)),
                ("Join", ResponseType::Other(1))
            ],
        );

        let label = Label::new(Some("Add a Community"));
        label.get_style_context().add_class("title");
        dialog.get_content_area().add(&label);

        let client = client.clone();
        dialog.connect_response(move |dialog, response_ty| {
            match response_ty {
                ResponseType::Other(x) => {
                    match x {
                        0 => show_create_community(client.clone()),
                        1 => show_join_community(client.clone()),
                        _ => {}
                    }
                    dialog.emit_close();
                }
                _ => {},
            }
        });

        dialog
    });
}

async fn create_community(client: Client<Ui>, name: &str) -> Result<()> {
    let community = client.create_community(name).await?;
    community.create_room("General").await?;
    community.create_room("Off Topic").await?;
    Ok(())
}

pub fn show_create_community(client: Client<Ui>) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[("Create", ResponseType::Apply)],
        );

        let label = Label::new(Some("Create A Community"));
        label.get_style_context().add_class("title");
        let entry = EntryBuilder::new()
            .placeholder_text("Community name...")
            .build();

        entry.clone().connect_activate(
            dialog.connector()
                .do_sync(|dialog, _| dialog.response(ResponseType::Apply))
                .build_cloned_consumer()
        );

        let content = dialog.get_content_area();
        content.add(&label);
        content.add(&entry);

        let client = client.clone();
        dialog.connect_response(
            client.connector()
                .do_async(move |client, (dialog, response_type): (gtk::Dialog, ResponseType)| {
                    let entry = entry.clone();
                    async move {
                        if response_type != ResponseType::Apply {
                            return;
                        }

                        if let Ok(name) = entry.try_get_text() {
                            if let Err(err) = create_community(client, &name).await {
                                show_generic_error(&err);
                            }
                        }

                        dialog.emit_close();
                    }
                })
                .build_widget_and_owned_listener()
        );

        dialog
    });
}

pub fn show_join_community(client: Client<Ui>) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[("Join", ResponseType::Apply)],
        );

        let label = Label::new(Some("Join A Community"));
        label.get_style_context().add_class("title");
        let entry = EntryBuilder::new()
            .placeholder_text("Invite code...")
            .build();

        entry.clone().connect_activate(
            dialog.connector()
                .do_sync(|dialog, _| dialog.response(ResponseType::Apply))
                .build_cloned_consumer()
        );

        let content = dialog.get_content_area();
        content.add(&label);
        content.add(&entry);

        let client = client.clone();
        dialog.connect_response(
            client.connector()
                .do_async(move |client, (dialog, response_type): (gtk::Dialog, ResponseType)| {
                    let entry = entry.clone();
                    async move {
                        if response_type != ResponseType::Apply {
                            return;
                        }

                        let code_entry = entry.clone();
                        if let Ok(code) = code_entry.try_get_text() {
                            let code = InviteCode(code);
                            if let Err(err) = client.join_community(code).await {
                                show_generic_error(&err);
                            }
                        }
                        dialog.emit_close();
                    }
                })
                .build_widget_and_owned_listener()
        );

        dialog
    });
}

pub fn show_invite_dialog(invite: InviteCode) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[("Ok", ResponseType::Ok)],
        );

        let label = Label::new(Some("Invite Code"));
        label.get_style_context().add_class("title");

        let code_view: gtk::TextView = gtk::TextViewBuilder::new()
            .editable(false)
            .name("Invite code")
            .buffer(&gtk::TextBufferBuilder::new().text(&invite.0).build())
            .build();

        let objs = (code_view.get_accessible(), label.get_accessible());
        if let (Some(code_view), Some(label)) = objs {
            let relations = code_view.ref_relation_set().expect("Error getting relations set");
            relations.add_relation_by_type(RelationType::LabelledBy, &label);
        }

        code_view.get_style_context().add_class("invite_code_text");

        let content = dialog.get_content_area();
        content.add(&label);
        content.add(&code_view);

        code_view.connect_button_release_event(|code_view, _| {
            if let Some(buf) = code_view.get_buffer() {
                let (start, end) = (buf.get_start_iter(), buf.get_end_iter());
                buf.select_range(&start, &end);
            }
            gtk::Inhibit(false)
        });

        dialog.connect_response(|dialog, _| dialog.emit_close());
        dialog
    });
}

pub fn show_create_room(community: client::CommunityEntry<Ui>) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[("Create", ResponseType::Apply)],
        );

        let label = Label::new(Some("Create A Channel"));
        label.get_style_context().add_class("title");
        let entry = EntryBuilder::new()
            .placeholder_text("Channel name...")
            .build();

        entry.clone().connect_activate(
            dialog.connector()
                .do_sync(|dialog, _| dialog.response(ResponseType::Apply))
                .build_cloned_consumer()
        );

        let content = dialog.get_content_area();
        content.add(&label);
        content.add(&entry);

        dialog.connect_response(
            community.connector()
                .do_async(move |community, (dialog, response_type): (gtk::Dialog, ResponseType)| {
                    let entry = entry.clone();
                    async move {
                        if response_type != ResponseType::Apply {
                            return;
                        }

                        if let Ok(name) = entry.try_get_text() {
                            if let Err(err) = community.create_room(&name).await {
                                show_generic_error(&err);
                            }
                        }

                        dialog.emit_close();
                    }
                })
                .build_widget_and_owned_listener()
        );

        dialog
    });
}

pub fn show_generic_error<E: std::fmt::Display>(error: &E) {
    window::show_dialog(|window| {
        let dialog = gtk::Dialog::new_with_buttons(
            None,
            Some(&window.window),
            DialogFlags::MODAL | DialogFlags::DESTROY_WITH_PARENT,
            &[("Ok", ResponseType::Ok)],
        );

        let heading = Label::new(Some("Error"));
        heading.get_style_context().add_class("title");
        heading.set_widget_name("error_title");

        let description: gtk::Label = gtk::Label::new(Some(&format!("{}", error)));
        description.get_style_context().add_class("error_description");

        let content = dialog.get_content_area();
        content.add(&heading);
        content.add(&description);

        dialog.connect_response(|dialog, _| dialog.emit_close());
        dialog
    });
}
