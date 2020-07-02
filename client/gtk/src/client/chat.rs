use std::collections::LinkedList;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use vertex::prelude::*;

use crate::{Client, SharedMut, Result, scheduler};
use crate::client::RoomEntry;
use crate::screen::active::message::MessageEntryWidget;
use crate::screen::active::ChatWidget;

use super::message::*;
use uuid::Uuid;

pub const MESSAGE_DROP_THRESHOLD: usize = MESSAGE_PAGE_SIZE * 4;
pub const MESSAGE_DROP_COUNT: usize = MESSAGE_PAGE_SIZE * 2;

pub struct PendingMessageHandle<'a> {
    chat: &'a Chat,
    widget: MessageEntryWidget,
    fake_id: MessageId,
}

impl<'a> PendingMessageHandle<'a> {
    pub async fn upgrade(self, message: Message) {
        let mut state = self.chat.state.write().await;
        state.widget.remove_message(self.fake_id);
        drop(state);

        self.chat.push(message).await;
    }

    pub fn set_error(self) {
        self.widget.set_status(MessageStatus::Err);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ChatSide {
    /// Bottom of the screen
    Front,

    /// Top of the screen
    Back,
}

struct ChatEntry {
    id: MessageId,
}

pub struct ChatState {
    client: Client,
    widget: ChatWidget,
    entries: LinkedList<ChatEntry>,
}

impl ChatState {
    fn new(client: Client, widget: ChatWidget) -> Self {
        ChatState {
            client,
            widget,
            entries: LinkedList::new(),
        }
    }

    fn push_widget(
        &mut self,
        content: MessageContent,
        side: ChatSide,
        id: MessageId,
    ) -> MessageEntryWidget {
        let rich = RichMessage::parse(content.text.clone());
        let widget = self.widget.add_message(content, side, self.client.clone(), id);

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

    fn push(&mut self, id: MessageId, content: MessageContent, side: ChatSide) -> MessageEntryWidget {
        let widget = self.push_widget(content, side, id);
        let entry = ChatEntry { id };

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

        let mut dropped = match side {
            ChatSide::Front => {
                self.entries.split_off(self.entries.len() - MESSAGE_DROP_COUNT)
            }
            ChatSide::Back => {
                let result = self.entries.split_off(MESSAGE_DROP_COUNT);
                std::mem::replace(&mut self.entries, result)
            }
        };

        for dropped in dropped.iter_mut() {
            self.widget.remove_message(dropped.id);
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
pub struct Chat {
    client: Client,
    room: RoomEntry,
    pub state: SharedMut<ChatState>,
    reading_new: Rc<AtomicBool>,
}

impl Chat {
    pub async fn new(client: Client, widget: ChatWidget, room: RoomEntry) -> Self {
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
            time: message.time_sent,
        }
    }

    pub async fn push(&self, message: Message) -> MessageEntryWidget {
        let content = self.build_content(&message).await;

        let mut state = self.state.write().await;
        let widget = state.push(message.id, content, ChatSide::Front);

        state.flush();

        widget
    }

    pub async fn push_pending(&self, content: MessageContent) -> PendingMessageHandle<'_> {
        let mut state = self.state.write().await;

        let fake_id = MessageId(Uuid::new_v4()); // Chance of collision is too small
        let widget = state.push_widget(content, ChatSide::Front, fake_id);
        state.flush();

        widget.set_status(MessageStatus::Pending);

        PendingMessageHandle {
            chat: self,
            widget,
            fake_id,
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

    pub async fn extend_older(&self) -> Result<()> {
        let oldest_message = self.state.read().await.oldest_message();
        if let Some(oldest_message) = oldest_message {
            let selector = MessageSelector::Before(Bound::Exclusive(oldest_message));

            let history = self.room.request_messages(selector, MESSAGE_PAGE_SIZE).await?;
            self.extend(history.buffer, ChatSide::Back).await;
        }

        Ok(())
    }

    pub async fn extend_newer(&self) -> Result<()> {
        let newest_message = self.state.read().await.newest_message();
        if newest_message == self.room.newest_message().await {
            return Ok(());
        }

        if let Some(newest_message) = newest_message {
            let selector = MessageSelector::After(Bound::Exclusive(newest_message));

            let history = self.room.request_messages(selector, MESSAGE_PAGE_SIZE).await?;
            self.extend(history.buffer, ChatSide::Front).await;
        }

        Ok(())
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
