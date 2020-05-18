use std::collections::LinkedList;

use chrono::{DateTime, Utc};
use gtk::prelude::*;

use vertex::prelude::*;

use crate::client;
use crate::client::{ChatSide, MessageContent};

use super::*;

pub struct ChatWidget {
    pub main: gtk::Box,
    pub room_name: gtk::Label,
    pub message_scroll: gtk::ScrolledWindow,
    pub message_list: gtk::ListBox,
    pub message_entry: gtk::TextView,
    pub groups: LinkedList<MessageGroupWidget>,
}

impl ChatWidget {
    fn add_group(&mut self, author: UserId, profile: Profile, time: DateTime<Utc>, side: ChatSide) {
        let group = MessageGroupWidget::build(author, profile, time);
        group.widget.hide();

        match side {
            ChatSide::Front => {
                self.message_list.add(&group.widget);
                self.groups.push_front(group);
            }
            ChatSide::Back => {
                self.message_list.insert(&group.widget, 0);
                self.groups.push_back(group);
            }
        }
    }

    fn next_group(&mut self, author: UserId, profile: Profile, time: DateTime<Utc>, side: ChatSide) -> &MessageGroupWidget {
        match self.group_for(side) {
            Some(group) if group.can_combine(author, time) => {}
            _ => self.add_group(author, profile, time, side),
        }

        self.group_for(side).as_ref().unwrap()
    }

    fn group_for(&self, side: ChatSide) -> Option<&MessageGroupWidget> {
        match side {
            ChatSide::Front => self.groups.front(),
            ChatSide::Back => self.groups.back(),
        }
    }

    // TODO: not a great solution
    fn remove_group(&mut self, group: &MessageGroupWidget) {
        let mut cursor = self.groups.cursor_front_mut();

        while let Some(current) = cursor.current() {
            if current == group {
                cursor.remove_current();
                group.remove_from(&self.message_list);

                return;
            }
            cursor.move_next();
        }
    }
}

impl client::ChatWidget<Ui> for ChatWidget {
    fn clear(&mut self) {
        for child in self.message_list.get_children() {
            self.message_list.remove(&child);
        }
        self.groups.clear();
    }

    fn add_message(
        &mut self,
        content: MessageContent,
        side: ChatSide,
        client: Client<Ui>,
        id: MessageId,
    ) -> MessageEntryWidget {
        let group = self.next_group(content.author, content.profile, content.time, side);
        group.add_message(content.text, id, side, client)
    }

    fn remove_message(&mut self, widget: &MessageEntryWidget) {
        if let Some(group) = widget.remove() {
            self.remove_group(group);
        }
    }

    fn flush(&mut self) {
        self.message_list.show_all();
    }
}
