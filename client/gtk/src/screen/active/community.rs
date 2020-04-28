use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::client;
use crate::connect::AsConnector;
use crate::Glade;

use super::*;
use atk::{AtkObjectExt, RelationType, RelationSetExt};

#[derive(Clone)]
pub struct CommunityEntryWidget {
    pub widget: gtk::Box,
    pub room_list: gtk::ListBox,

    menu_button: gtk::Button,
}

impl CommunityEntryWidget {
    pub fn build(name: String, description: String) -> Self {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/community_entry.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();

        let community_entry: gtk::Box = builder.get_object("community_entry").unwrap();

        let community_expander: gtk::Expander = builder.get_object("community_expander").unwrap();

        let community_name: gtk::Label = builder.get_object("community_name").unwrap();
        community_name.set_text(&name);
        community_name.set_xalign(0.0);

        let community_description: gtk::Label = builder.get_object("community_description").unwrap();
        community_description.set_text(&description);
        community_description.set_xalign(0.0);

        let room_list: gtk::ListBox = builder.get_object("room_list").unwrap();

        let objs = (community_expander.get_accessible(), community_name.get_accessible(), community_description.get_accessible());
        if let (Some(exp), Some(name), Some(desc)) = objs {
            let relations = exp.ref_relation_set().expect("Error getting relations set");
            relations.add_relation_by_type(RelationType::LabelledBy, &name);
            relations.add_relation_by_type(RelationType::LabelledBy, &desc);
        }

        CommunityEntryWidget {
            widget: community_entry,
            room_list,
            menu_button: builder.get_object("menu_button").unwrap(),
        }
    }
}

impl client::CommunityEntryWidget<Ui> for CommunityEntryWidget {
    fn bind_events(&self, community_entry: &client::CommunityEntry<Ui>) {
        self.menu_button.connect_clicked(
            community_entry.connector()
                .do_sync(|community, button| {
                    let menu = build_menu(community);
                    menu.set_relative_to(Some(&button));
                    menu.show();

                    menu.connect_hide(|popover| {
                        // weird gtk behavior: if we don't do this, it messes with dialog rendering order
                        popover.set_relative_to::<gtk::Widget>(None);
                    });
                })
                .inhibit(true)
                .build_cloned_consumer()
        );
        self.room_list.connect_row_selected(
            community_entry.connector()
                .do_async(|community, (_, room): (gtk::ListBox, Option<gtk::ListBoxRow>)| async move {
                    if let Some(room) = room {
                        if let Some(selected_community) = community.client.selected_community().await {
                            if community.id != selected_community.id {
                                selected_community.widget.room_list.unselect_all();
                            }
                        }

                        let room = room.get_index() as usize;
                        match community.get_room(room).await {
                            Some(room) => community.client.select_room(room).await,
                            None => community.client.deselect_room().await,
                        }
                    }
                })
                .build_widget_and_option_consumer()
        );
    }

    fn add_room(&self, name: String) -> RoomEntryWidget {
        let widget = RoomEntryWidget::build(name);
        self.room_list.add(&widget.container);
        self.room_list.show_all();

        widget
    }
}

fn build_menu(community_entry: client::CommunityEntry<Ui>) -> gtk::Popover {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/community_menu.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();

    let menu: gtk::Popover = builder.get_object("community_menu").unwrap();
    let invite_button: gtk::Button = builder.get_object("invite_button").unwrap();
    let create_channel_button: gtk::Button = builder.get_object("create_channel_button").unwrap();
    let _settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

    invite_button.connect_clicked(
        (menu.clone(), community_entry.clone()).connector()
            .do_async(move |(menu, community_entry), _| async move {
                menu.hide();

                match community_entry.create_invite(None).await {
                    Ok(invite) => dialog::show_invite_dialog(invite),
                    Err(err) => dialog::show_generic_error(&err),
                }
            })
            .build_cloned_consumer()
    );

    create_channel_button.connect_clicked(
        (menu.clone(), community_entry).connector()
            .do_sync(move |(menu, community_entry), _| {
                menu.hide();
                dialog::show_create_room(community_entry);
            })
            .build_cloned_consumer()
    );

    menu
}

