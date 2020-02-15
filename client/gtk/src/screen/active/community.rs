use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::prelude::*;

use crate::client;
use crate::connect::AsConnector;
use crate::window;

use super::*;

#[derive(Clone)]
pub struct CommunityEntryWidget {
    pub expander: CommunityExpander,
    pub room_list: gtk::ListBox,

    menu_button: gtk::Button,
}

impl CommunityEntryWidget {
    pub fn build(name: String) -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/community_entry.glade");

        let community_header: gtk::Box = builder.get_object("community_header").unwrap();

        let community_name: gtk::Label = builder.get_object("community_name").unwrap();
        community_name.set_text(&name);

        let community_motd: gtk::Label = builder.get_object("community_motd").unwrap();
        community_motd.set_text("5 users online");

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
                        let room = community.get_room(room).await;

                        community.client.select_room(room).await;
                    }
                })
                .build_widget_and_option_consumer()
        );
    }

    fn add_room(&self, name: String) -> RoomEntryWidget {
        let widget = RoomEntryWidget::build(name);
        self.room_list.add(&widget.label);

        widget
    }
}

fn build_menu(community_entry: client::CommunityEntry<Ui>) -> gtk::Popover {
    let builder = gtk::Builder::new_from_file("res/glade/active/community_menu.glade");

    let menu: gtk::Popover = builder.get_object("community_menu").unwrap();
    let invite_button: gtk::Button = builder.get_object("invite_button").unwrap();
    let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

    invite_button.connect_button_release_event(
        (menu.clone(), community_entry).connector()
            .do_async(move |(menu, community_entry), (_widget, _event)| async move {
                menu.hide();

                // TODO: error handling
                let invite = community_entry.create_invite(None).await.expect("failed to create invite");

                let builder = gtk::Builder::new_from_file("res/glade/active/dialog/invite_community.glade");
                let main: gtk::Box = builder.get_object("main").unwrap();

                let code_view: gtk::TextView = builder.get_object("code_view").unwrap();
                if let Some(code_view) = code_view.get_buffer() {
                    code_view.set_text(&invite.0);
                }

                code_view.connect_button_release_event(|code_view, _| {
                    if let Some(buf) = code_view.get_buffer() {
                        let (start, end) = (buf.get_start_iter(), buf.get_end_iter());
                        buf.select_range(&start, &end);
                    }
                    gtk::Inhibit(false)
                });

                window::show_dialog(main);
            })
            .build_widget_event()
    );

    menu
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

        event_header.connect_button_release_event(
            expander.connector()
                .do_sync(|expander, (_, _)| {
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
