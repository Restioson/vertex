use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::prelude::*;

use lazy_static::lazy_static;
use vertex::types::InviteCode;

use crate::{client, window};
use crate::connect::AsConnector;
use crate::{Glade, TryGetText};

use super::*;

#[derive(Clone)]
pub struct CommunityEntryWidget {
    pub expander: CommunityExpander,
    pub room_list: gtk::ListBox,

    menu_button: gtk::Button,
}

impl CommunityEntryWidget {
    pub fn build(name: String, description: String) -> Self {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/community_entry.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();

        let community_header: gtk::Box = builder.get_object("community_header").unwrap();

        let community_name: gtk::Label = builder.get_object("community_name").unwrap();
        community_name.set_text(&name);

        let community_description: gtk::Label = builder.get_object("community_description").unwrap();
        community_description.set_text(&description);
        // TODO do something with the motd

        let room_list: gtk::ListBox = builder.get_object("room_list").unwrap();

        let expander = CommunityExpander::new(
            community_header.upcast(),
            room_list.clone().upcast(),
        );

        CommunityEntryWidget {
            expander,
            room_list,
            menu_button: builder.get_object("menu_button").unwrap(),
        }
    }
}

impl client::CommunityEntryWidget<Ui> for CommunityEntryWidget {
    fn bind_events(&self, community_entry: &client::CommunityEntry<Ui>) {
        // TODO(a11y)
        self.menu_button.connect_button_release_event(
            community_entry.connector()
                .do_sync(|community, (button, _)| {
                    let menu = build_menu(community);
                    menu.set_relative_to(Some(&button));
                    menu.show();

                    menu.connect_hide(|popover| {
                        // weird gtk behavior: if we don't do this, it messes with dialog rendering order
                        popover.set_relative_to::<gtk::Widget>(None);
                    });
                })
                .inhibit(true)
                .build_widget_event()
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

    invite_button.connect_activate(
        (menu.clone(), community_entry.clone()).connector()
            .do_async(move |(menu, community_entry), _| async move {
                menu.hide();

                match community_entry.create_invite(None).await {
                    Ok(invite) => show_invite_dialog(invite),
                    Err(err) => dialog::show_generic_error(&err),
                }
            })
            .build_cloned_consumer()
    );

    create_channel_button.connect_activate(
        (menu.clone(), community_entry).connector()
            .do_sync(move |(menu, community_entry), _| {
                menu.hide();
                show_create_room(community_entry);
            })
            .build_cloned_consumer()
    );

    menu
}

fn show_invite_dialog(invite: InviteCode) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/invite_community.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let code_view: gtk::TextView = builder.get_object("code_view").unwrap();
    if let Some(code_view) = code_view.get_buffer() {
        code_view.set_text(&invite.0);
    }

    // TODO(a11y)
    code_view.connect_button_release_event(|code_view, _| {
        if let Some(buf) = code_view.get_buffer() {
            let (start, end) = (buf.get_start_iter(), buf.get_end_iter());
            buf.select_range(&start, &end);
        }
        gtk::Inhibit(false)
    });

    window::show_dialog(main);
}

fn show_create_room(community: client::CommunityEntry<Ui>) {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/dialog/create_room.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let main: gtk::Box = builder.get_object("main").unwrap();

    let name_entry: gtk::Entry = builder.get_object("name_entry").unwrap();
    let create_button: gtk::Button = builder.get_object("create_button").unwrap();

    let dialog = window::show_dialog(main);

    create_button.connect_activate(
        community.connector()
            .do_async(move |community, _| {
                let name_entry = name_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(name) = name_entry.try_get_text() {
                        dialog.close();

                        if let Err(err) = community.create_room(&name).await {
                            show_generic_error(&err);
                        }
                    }
                }
            })
            .build_cloned_consumer()
    );
}

#[derive(Clone)]
pub struct CommunityExpander {
    pub widget: gtk::Box,
    content: gtk::Widget,
    expanded: Rc<AtomicBool>,
}

impl CommunityExpander {
    fn new(header: gtk::Widget, content: gtk::Widget) -> Self {
        let widget = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Vertical)
            .build();

        let event_header = gtk::EventBoxBuilder::new()
            .above_child(false)
            .build();
        event_header.add(&header);

        widget.add(&event_header);

        let expander = CommunityExpander {
            widget,
            content,
            expanded: Rc::new(AtomicBool::new(false)),
        };

        // TODO(a11y)
        event_header.connect_button_release_event(
            expander.connector()
                .do_sync(|expander, _| {
                    let expanded = expander.expanded.load(Ordering::SeqCst);
                    if expanded {
                        expander.widget.remove(&expander.content);
                    } else {
                        expander.widget.add(&expander.content);
                        expander.content.show_all();
                    }
                    expander.expanded.store(!expanded, Ordering::SeqCst);
                })
                .build_widget_event()
        );

        expander
    }
}
