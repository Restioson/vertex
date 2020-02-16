use gtk::prelude::*;

use vertex::*;

use crate::client;

use super::*;

pub struct ChatWidget {
    pub main: gtk::Box,
    pub room_name: gtk::Label,
    pub message_scroll: gtk::ScrolledWindow,
    pub message_list: gtk::ListBox,
    pub message_entry: gtk::Entry,
    pub last_group: Option<GroupedMessageWidget>,
}

impl ChatWidget {
    pub fn build() -> Self {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("res/glade/active/chat.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();

        let main: gtk::Box = builder.get_object("chat").unwrap();

        ChatWidget {
            main,
            room_name: builder.get_object("room_name").unwrap(),
            message_scroll: builder.get_object("message_scroll").unwrap(),
            message_list: builder.get_object("message_list").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            last_group: None,
        }
    }

    fn next_group(&mut self, author: UserId, profile: UserProfile) -> &GroupedMessageWidget {
        match &self.last_group {
            Some(group) if group.author == author => {}
            _ => {
                let group = GroupedMessageWidget::build(author, profile);
                self.message_list.insert(&group.widget, -1);
                self.last_group = Some(group);
            }
        }

        self.last_group.as_ref().unwrap()
    }
}

impl client::ChatWidget<Ui> for ChatWidget {
    fn set_room(&mut self, room: Option<&client::RoomEntry<Ui>>) {
        let enabled = room.is_some();
        self.message_entry.set_can_focus(enabled);
        self.message_entry.set_editable(enabled);

        for child in self.message_list.get_children() {
            self.message_list.remove(&child);
        }
        self.last_group = None;

        match room {
            Some(room) => {
                self.message_entry.set_placeholder_text(Some("Send message..."));
                self.message_entry.get_style_context().remove_class("disabled");

                self.room_name.set_text(&room.name);
            },
            None => {
                self.message_entry.set_placeholder_text(Some("Select a room to send messages..."));
                self.message_entry.get_style_context().add_class("disabled");

                self.room_name.set_text("");
            }
        }
    }

    fn push_message(&mut self, author: UserId, author_profile: UserProfile, content: String) -> MessageEntryWidget {
        let group = self.next_group(author, author_profile);
        group.push_message(content)
    }

    fn bind_events(&self, client: &Client<Ui>, chat: &client::Chat<Ui>) {
        self.message_entry.connect_activate(
            client.connector()
                .do_async(|client, entry: gtk::Entry| async move {
                    if let Some(selected_room) = client.selected_room().await {
                        let content = entry.try_get_text().unwrap_or_default();
                        if !content.trim().is_empty() {
                            entry.set_text("");
                            selected_room.send_message(content).await;
                        }
                    }
                })
                .build_cloned_consumer()
        );

        let adjustment = self.message_scroll.get_vadjustment().unwrap();

        adjustment.connect_value_changed(
            chat.connector()
                .do_async(|chat, adjustment: gtk::Adjustment| async move {
                    let upper = adjustment.get_upper() - adjustment.get_page_size();
                    let reading_new = adjustment.get_value() + 10.0 >= upper;
                    chat.set_reading_new(reading_new).await;
                })
                .build_cloned_consumer()
        );

        self.message_list.connect_size_allocate(
            (chat.clone(), adjustment).connector()
                .do_sync(|(list, adjustment), (_, _)| {
                    if list.reading_new() {
                        adjustment.set_value(adjustment.get_upper() - adjustment.get_page_size());
                    }
                })
                .build_widget_listener()
        );
    }
}
