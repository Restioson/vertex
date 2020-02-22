use gtk::prelude::*;

use vertex::*;

use crate::client;

use super::*;
use crate::client::{MessageContent, ClientUi};

pub struct ChatWidget {
    pub main: gtk::Box,
    pub room_name: gtk::Label,
    pub message_scroll: gtk::ScrolledWindow,
    pub message_list: gtk::ListBox,
    pub message_entry: gtk::Entry,
    pub front_group: Option<MessageGroupWidget>,
}

impl ChatWidget {
    fn next_group_front(&mut self, author: UserId, profile: UserProfile) -> &MessageGroupWidget {
        match &self.front_group {
            Some(group) if group.author == author => {}
            _ => {
                let group = MessageGroupWidget::build(author, profile);
                self.message_list.insert(&group.widget, -1);
                self.front_group = Some(group);
            }
        }

        self.front_group.as_ref().unwrap()
    }
}

impl client::ChatWidget<Ui> for ChatWidget {
    fn clear(&mut self) {
        for child in self.message_list.get_children() {
            self.message_list.remove(&child);
        }
        self.front_group = None;
    }

    fn add_message_front(&mut self, content: MessageContent) -> MessageEntryWidget {
        let group = self.next_group_front(content.author, content.profile);
        group.push_message(content.text)
    }

    fn add_message_back(&mut self, content: MessageContent) -> MessageEntryWidget {
        // TODO: this isn't correct. we need to group properly
        let group = MessageGroupWidget::build(content.author, content.profile);
        self.message_list.insert(&group.widget, 0);

        group.push_message(content.text)
    }

    // TODO: ..we need to be able to reference by id
    //       how??
    fn remove_message(&mut self, widget: &MessageEntryWidget) {
        widget.remove_from(&self.message_list);
    }
}
