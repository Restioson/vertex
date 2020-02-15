use uuid::Uuid;

pub use embed::*;
pub use rich::*;
use vertex::*;

use crate::{Client, SharedMut};

use super::ClientUi;

mod rich;
mod embed;

#[derive(Debug, Clone)]
pub struct MessageSource {
    pub author: UserId,
    pub author_profile_version: Option<ProfileVersion>,
    pub content: String,
}

#[derive(Debug, Copy, Clone)]
pub enum MessageStatus {
    Pending,
    Ok,
    Err,
}

#[derive(Debug, Clone)]
pub struct MessageHandle<Ui: ClientUi> {
    widget: Ui::MessageEntryWidget,
}

impl<Ui: ClientUi> MessageHandle<Ui> {
    #[inline]
    pub fn set_status(&mut self, status: MessageStatus) {
        self.widget.set_status(status);
    }
}

pub trait MessageListWidget<Ui: ClientUi> {
    fn clear(&mut self);

    fn push_message(&mut self, author: UserId, author_profile: UserProfile, content: String) -> Ui::MessageEntryWidget;

    fn bind_events(&self, list: &MessageList<Ui>);
}

pub trait MessageEntryWidget<Ui: ClientUi>: Clone {
    fn set_status(&mut self, status: MessageStatus);

    fn push_embed(&mut self, client: &Client<Ui>, embed: MessageEmbed);
}

pub struct MessageListState<Ui: ClientUi> {
    widget: Ui::MessageListWidget,
    stream: Option<MessageStream<Ui>>,
    reading_new: bool,
}

#[derive(Clone)]
pub struct MessageList<Ui: ClientUi> {
    state: SharedMut<MessageListState<Ui>>,
}

impl<Ui: ClientUi> MessageList<Ui> {
    pub fn new(widget: Ui::MessageListWidget) -> Self {
        let state = SharedMut::new(MessageListState {
            widget,
            stream: None,
            reading_new: true,
        });
        MessageList { state }
    }

    pub async fn bind_events(&self) {
        let state = self.state.read().await;
        state.widget.bind_events(&self);
    }

    async fn push(&self, client: &Client<Ui>, message: MessageSource) -> Ui::MessageEntryWidget {
        let profile = client.profiles.get(message.author, message.author_profile_version).await.unwrap(); // TODO

        let mut state = self.state.write().await;
        let list = &mut state.widget;

        let rich = RichMessage::parse(message.content);
        let widget = list.push_message(message.author, profile, rich.text.clone());

        if rich.has_embeds() {
            glib::MainContext::ref_thread_default().spawn_local({
                let client = client.clone();
                let mut widget = widget.clone();
                async move {
                    for embed in rich.load_embeds().await {
                        widget.push_embed(&client, embed);
                    }
                }
            });
        }

        widget
    }

    async fn populate_list(&self, stream: &MessageStream<Ui>) {
        let mut messages = Vec::with_capacity(50);
        stream.read_last(50, &mut messages).await;

        for message in messages {
            self.push(&stream.client, message).await;
        }
    }

    async fn accepts(&self, accepts: &MessageStream<Ui>) -> bool {
        let state = self.state.read().await;
        match &state.stream {
            Some(stream) => stream.id == accepts.id,
            None => false,
        }
    }

    pub async fn set_stream(&self, stream: &MessageStream<Ui>) {
        {
            let mut state = self.state.write().await;
            state.stream = Some(stream.clone());
            state.widget.clear();
        }

        self.populate_list(&stream).await;
    }

    pub async fn detach_stream(&self) {
        let mut state = self.state.write().await;
        state.stream = None;
        state.widget.clear();
    }

    pub async fn set_reading_new(&self, reading_new: bool) {
        self.state.write().await.reading_new = reading_new;
    }

    pub async fn reading_new(&self) -> bool {
        self.state.read().await.reading_new
    }
}

// TODO: Very naive implementation - to be replaced with sqlite database backend
pub struct MessageHistory {
    messages: Vec<MessageSource>,
}

impl MessageHistory {
    pub fn push(&mut self, message: MessageSource) {
        self.messages.push(message);
    }

    pub fn read_last(&self, count: usize, buf: &mut Vec<MessageSource>) {
        let min = self.messages.len().checked_sub(count + 1).unwrap_or(0);
        for i in min..self.messages.len() {
            buf.push(self.messages[i].clone());
        }
    }
}

#[derive(Clone)]
pub struct MessageStream<Ui: ClientUi> {
    id: Uuid,
    client: Client<Ui>,
    history: SharedMut<MessageHistory>,
}

impl<Ui: ClientUi> MessageStream<Ui> {
    pub fn new(id: Uuid, client: Client<Ui>) -> Self {
        let history = MessageHistory { messages: Vec::new() };
        let history = SharedMut::new(history);
        MessageStream { id, client, history }
    }

    pub async fn push(&self, message: MessageSource) -> Option<MessageHandle<Ui>> {
        self.history.write().await.push(message.clone());

        let list = &self.client.message_list;
        if list.accepts(&self).await {
            let widget = list.push(&self.client, message).await;
            Some(MessageHandle { widget })
        } else {
            None
        }
    }

    pub async fn read_last(&self, count: usize, buf: &mut Vec<MessageSource>) {
        self.history.read().await.read_last(count, buf);
    }
}
