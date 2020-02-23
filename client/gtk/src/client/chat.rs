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

    fn add_message(&mut self, content: MessageContent, side: ChatSide) -> Ui::MessageEntryWidget;

    fn remove_message(&mut self, widget: &Ui::MessageEntryWidget);

    fn flush(&mut self);
}

pub struct PendingMessageHandle<'a, Ui: ClientUi> {
    chat: &'a Chat<Ui>,
    widget: Ui::MessageEntryWidget,
}

impl<'a, Ui: ClientUi> PendingMessageHandle<'a, Ui> {
    pub async fn upgrade(self, message: Message) {
        let mut state = self.chat.state.write().await;
        state.widget.remove_message(&self.widget);
        drop(state);

        self.chat.push(message).await;
    }

    pub fn set_error(self) {
        self.widget.set_status(MessageStatus::Err);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ChatSide {
    Front,
    Back,
}

struct ChatEntry<Ui: ClientUi> {
    id: MessageId,
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

    fn push_widget(&mut self, content: MessageContent, side: ChatSide) -> Ui::MessageEntryWidget {
        let rich = RichMessage::parse(content.text.clone());
        let widget = self.widget.add_message(content, side);

        if rich.has_embeds() {
            let client = self.client.clone();
            let widget = widget.clone();

            scheduler::spawn(async move {
                let embeds = rich.load_embeds(&client.embeds).await;
                for embed in embeds {
                    widget.push_embed(&client, embed);
                }
            });
        }

        widget
    }

    fn push(&mut self, id: MessageId, content: MessageContent, side: ChatSide) -> Ui::MessageEntryWidget {
        let widget = self.push_widget(content, side);
        let entry = ChatEntry { id, widget: widget.clone() };

        match side {
            ChatSide::Front => self.entries.push_front(entry),
            ChatSide::Back => self.entries.push_back(entry),
        }

        if self.entries.len() > MESSAGE_DROP_THRESHOLD {
            self.drop_side(side);
        }

        widget
    }

    fn drop_side(&mut self, side: ChatSide) {
        if self.entries.len() <= MESSAGE_DROP_COUNT {
            return;
        }

        let dropped = match side {
            ChatSide::Front => {
                self.entries.split_off(self.entries.len() - MESSAGE_DROP_COUNT)
            }
            ChatSide::Back => {
                let result = self.entries.split_off(MESSAGE_DROP_COUNT);
                std::mem::replace(&mut self.entries, result)
            }
        };

        for dropped in dropped {
            self.widget.remove_message(&dropped.widget);
        }
    }

    fn clear(&mut self) {
        self.widget.clear();
        self.entries.clear();
    }

    fn flush(&mut self) {
        self.widget.flush();
    }

    #[inline]
    fn oldest_message(&self) -> Option<MessageId> {
        self.entries.back().map(|entry| entry.id)
    }

    #[inline]
    fn newest_message(&self) -> Option<MessageId> {
        self.entries.front().map(|entry| entry.id)
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
    pub async fn new(client: Client<Ui>, widget: Ui::ChatWidget, room: RoomEntry<Ui>) -> Self {
        let history = room.collect_recent_history().await;

        let chat = Chat {
            client: client.clone(),
            room,
            state: SharedMut::new(ChatState::new(client, widget)),
            reading_new: Rc::new(AtomicBool::new(true)),
        };
        chat.extend(history, ChatSide::Front).await;

        chat
    }

    async fn build_content(&self, message: &Message) -> MessageContent {
        MessageContent {
            author: message.author,
            profile: self.client.profiles.get_or_default(message.author, message.author_profile_version).await,
            text: message.content.clone(),
            time: message.sent.clone(),
        }
    }

    pub async fn push(&self, message: Message) -> Ui::MessageEntryWidget {
        let content = self.build_content(&message).await;

        let mut state = self.state.write().await;
        let widget = state.push(message.id, content, ChatSide::Front);

        state.flush();

        widget
    }

    pub async fn push_pending<'a>(&'a self, content: MessageContent) -> PendingMessageHandle<'a, Ui> {
        let mut state = self.state.write().await;

        let widget = state.push_widget(content, ChatSide::Front);
        state.flush();

        widget.set_status(MessageStatus::Pending);

        PendingMessageHandle {
            chat: self,
            widget,
        }
    }

    #[inline]
    pub fn accepts(&self, room: RoomId) -> bool {
        self.room.id == room
    }

    #[inline]
    pub async fn clear(&self) {
        self.state.write().await.clear();
    }

    pub async fn update(&self, update: RoomUpdate) {
        if !update.continuous {
            self.clear().await;
        }

        self.room.update(&update).await;

        self.extend(update.new_messages.buffer, ChatSide::Front).await;
    }

    async fn extend(&self, messages: Vec<Message>, side: ChatSide) {
        let mut messages = messages;
        if side == ChatSide::Back {
            messages.reverse();
        }

        let mut state = self.state.write().await;
        for message in messages {
            let content = self.build_content(&message).await;
            state.push(message.id, content, side);
        }

        state.flush();
    }

    pub async fn extend_older(&self) {
        let oldest_message = self.state.read().await.oldest_message();
        if let Some(oldest_message) = oldest_message {
            let selector = MessageSelector::Before(Bound::Exclusive(oldest_message));

            // TODO: error handling
            let history = self.room.request_messages(selector, MESSAGE_PAGE_SIZE).await.unwrap();
            self.extend(history.buffer, ChatSide::Back).await;
        }
    }

    pub async fn extend_newer(&self) {
        let newest_message = self.state.read().await.newest_message();
        if newest_message == self.room.newest_message().await {
            return;
        }

        if let Some(newest_message) = newest_message {
            let selector = MessageSelector::After(Bound::Exclusive(newest_message));

            // TODO: error handling
            let history = self.room.request_messages(selector, MESSAGE_PAGE_SIZE).await.unwrap();
            self.extend(history.buffer, ChatSide::Front).await;
        }
    }

    // TODO: we can only be reading_new when we are at the bottom of the message history!
    // TODO: marking as read does not handle when new messages are added or when you're scrolling down
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
