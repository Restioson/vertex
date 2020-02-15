use gtk::prelude::*;

use crate::client;
use crate::connect::AsConnector;
use crate::window;

use super::*;

#[derive(Clone)]
pub struct CommunityEntryWidget {
    pub expander: gtk::Expander,
    pub room_list: gtk::ListBox,
    pub invite_button: gtk::Button,
    pub settings_button: gtk::Button,
}

impl CommunityEntryWidget {
    pub fn build(name: String) -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/community_entry.glade");

        let expander: gtk::Expander = builder.get_object("community_expander").unwrap();

        let community_name: gtk::Label = builder.get_object("community_name").unwrap();
        let community_motd: gtk::Label = builder.get_object("community_motd").unwrap();

        let invite_button: gtk::Button = builder.get_object("invite_button").unwrap();
        let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

        let room_list: gtk::ListBox = builder.get_object("room_list").unwrap();

        community_name.set_text(&name);
        community_motd.set_text("5 users online");

        let settings_image = settings_button.get_child()
            .and_then(|img| img.downcast::<gtk::Image>().ok())
            .unwrap();

        settings_image.set_from_pixbuf(Some(
            &gdk_pixbuf::Pixbuf::new_from_file_at_size(
                "res/feather/settings.svg",
                20, 20,
            ).unwrap()
        ));

        let invite_image = invite_button.get_child()
            .and_then(|img| img.downcast::<gtk::Image>().ok())
            .unwrap();

        invite_image.set_from_pixbuf(Some(
            &gdk_pixbuf::Pixbuf::new_from_file_at_size(
                "res/feather/user-plus.svg",
                20, 20,
            ).unwrap()
        ));

        expander.set_expanded(false);

        CommunityEntryWidget {
            expander,
            room_list,
            invite_button,
            settings_button,
        }
    }
}

impl client::CommunityEntryWidget<Ui> for CommunityEntryWidget {
    fn bind_events(&self, community_entry: &client::CommunityEntry<Ui>) {
        self.room_list.connect_row_selected(
            community_entry.connector()
                .do_async(|community, (_, room): (gtk::ListBox, Option<gtk::ListBoxRow>)| async move {
                    if let Some(room) = room {
                        if let Some(room) = community.client.selected_room().await {
                            if room.community.id != community.id {
                                room.community.widget.room_list.unselect_all();
                            }
                        }
                        let room = room.get_index() as usize;
                        let room = community.get_room(room).await;
                        community.client.select_room(room).await;
                    }
                })
                .build_widget_and_option_consumer()
        );

        self.invite_button.connect_button_press_event(
            community_entry.connector()
                .do_async(move |community_entry, (_widget, _event)| async move {
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
    }

    fn add_room(&self, name: String) -> RoomEntryWidget {
        let widget = RoomEntryWidget::build(name);

        self.room_list.add(&widget.label);
        widget.label.show_all();

        widget
    }
}
