use std::collections::LinkedList;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use vertex::*;

use crate::{Client, scheduler, SharedMut};
use crate::client::RoomEntry;

use super::ClientUi;
use super::message::*;

pub const MESSAGE_DROP_THRESHOLD: usize = MESSAGE_PAGE_SIZE * 4;
pub const MESSAGE_DROP_COUNT: usize = MESSAGE_PAGE_SIZE * 2;

pub trait ChatWidget<Ui: ClientUi> {
    fn clear(&mut self);

    fn add_message_front(&mut self, content: MessageContent) -> Ui::MessageEntryWidget;

    fn add_message_back(&mut self, content: MessageContent) -> Ui::MessageEntryWidget;

    fn remove_message(&mut self, widget: &Ui::MessageEntryWidget);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum InsertSide {
    Front,
    Back,
}

struct ChatEntry<Ui: ClientUi> {
    id: Option<MessageId>,
    widget: Ui::MessageEntryWidget,
}

struct ChatState<Ui: ClientUi> {
    client: Client<Ui>,
    widget: Ui::ChatWidget,
    entries: LinkedList<ChatEntry<Ui>>,
}

impl<Ui: ClientUi> ChatState<Ui> {
    fn new(client: Client<Ui>, widget: Ui::ChatWidget) -> Self {
        ChatState {
            client,
            widget,
            entries: LinkedList::new(),
        }
    }

    fn push_widget(&mut self, content: MessageContent, side: InsertSide) -> Ui::MessageEntryWidget {
        let rich = RichMessage::parse(content.text.clone());
        let widget = match side {
            InsertSide::Back => self.widget.add_message_back(content),
            InsertSide::Front => self.widget.add_message_front(content),
        };

        if rich.has_embeds() {
            let client = self.client.clone();
            let widget = widget.clone();

            scheduler::spawn(async move {
                let embeds = rich.load_embeds().await;
                for embed in embeds {
                    widget.push_embed(&client, embed);
                }
            });
        }

        widget
    }

    fn push(&mut self, id: Option<MessageId>, content: MessageContent, side: InsertSide) -> Ui::MessageEntryWidget {
        let widget = self.push_widget(content, side);
        let entry = ChatEntry { id, widget: widget.clone() };

        match side {
            InsertSide::Front => self.entries.push_back(entry),
            InsertSide::Back => self.entries.push_front(entry),
        }

        if self.entries.len() > MESSAGE_DROP_THRESHOLD {
            match side {
                InsertSide::Front => self.drop_back(),
                InsertSide::Back => self.drop_front(),
            }
        }

        widget
    }

    fn drop_front(&mut self) {
        if self.entries.len() <= MESSAGE_DROP_COUNT {
            return;
        }

        let dropped = self.entries.split_off(self.entries.len() - MESSAGE_DROP_COUNT);
        for dropped in dropped {
            self.widget.remove_message(&dropped.widget);
        }
    }

    fn drop_back(&mut self) {
        if self.entries.len() <= MESSAGE_DROP_COUNT {
            return;
        }

        let result = self.entries.split_off(MESSAGE_DROP_COUNT);
        for dropped in &self.entries {
            self.widget.remove_message(&dropped.widget);
        }

        self.entries = result;
    }

    fn clear(&mut self) {
        self.widget.clear();
        self.entries.clear();
    }

    fn oldest_message(&self) -> Option<MessageId> {
        self.entries.iter().flat_map(|entry| entry.id).next()
    }
}

#[derive(Clone)]
pub struct Chat<Ui: ClientUi> {
    client: Client<Ui>,
    room: RoomEntry<Ui>,
    state: SharedMut<ChatState<Ui>>,
    reading_new: Rc<AtomicBool>,
}

impl<Ui: ClientUi> Chat<Ui> {
    pub fn new(client: Client<Ui>, widget: Ui::ChatWidget, room: RoomEntry<Ui>) -> Self {
        Chat {
            client: client.clone(),
            room,
            state: SharedMut::new(ChatState::new(client, widget)),
            reading_new: Rc::new(AtomicBool::new(true)),
        }
    }

    async fn build_content(&self, message: &Message) -> MessageContent {
        MessageContent {
            author: message.author,
            profile: self.client.profiles.get_or_default(message.author, message.author_profile_version).await,
            text: message.content.clone(),
        }
    }

    pub async fn push(&self, message: Message) -> Ui::MessageEntryWidget {
        let content = self.build_content(&message).await;

        let mut state = self.state.write().await;
        state.push(Some(message.id), content, InsertSide::Front)
    }

    pub async fn push_raw(&self, content: MessageContent) -> Ui::MessageEntryWidget {
        let mut state = self.state.write().await;
        state.push(None, content, InsertSide::Front)
    }

    pub fn accepts(&self, room: RoomId) -> bool {
        self.room.id == room
    }

    pub async fn clear(&self) {
        self.state.write().await.clear();
    }

    pub async fn update(&self, update: RoomUpdate) {
        self.room.update(&update).await;

        if !update.continuous {
            self.clear().await;
        } else {
            let state = self.state.read().await;
            if state.entries.is_empty() {
                let history = self.room.collect_recent_history().await;
                self.extend(history, InsertSide::Front).await;
            }
        }

        self.extend(update.new_messages.messages, InsertSide::Front).await;
    }

    async fn extend(&self, messages: Vec<Message>, side: InsertSide) {
        let mut state = self.state.write().await;

        let mut messages = messages;
        if side == InsertSide::Back {
            messages.reverse();
        }

        for message in messages {
            let content = self.build_content(&message).await;
            state.push(Some(message.id), content, side);
        }
    }

    pub async fn extend_older(&self) {
        let oldest_message = self.state.read().await.oldest_message();
        if let Some(oldest_message) = oldest_message {
            let selector = MessageSelector::Before(Bound::Exclusive(oldest_message));

            // TODO: error handling
            let history = self.room.request_messages(selector, MESSAGE_PAGE_SIZE).await.unwrap();
            self.extend(history.messages, InsertSide::Back).await;
        }
    }

    pub async fn extend_newer(&self) {
        // TODO
    }

    pub async fn set_reading_new(&self, reading_new: bool) {
        let prev_reading_new = self.reading_new.swap(reading_new, Ordering::SeqCst);
        if reading_new && !prev_reading_new {
            self.room.mark_as_read().await;
        }
    }

    pub fn reading_new(&self) -> bool {
        self.reading_new.load(Ordering::SeqCst)
    }
}
