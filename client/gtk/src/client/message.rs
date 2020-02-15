use uuid::Uuid;

pub use embed::*;
pub use rich::*;
use vertex::*;

use crate::{Client, SharedMut};

use super::ClientUi;

mod rich;
mod embed;

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

    fn push_message(&mut self, author: UserId, content: String) -> Ui::MessageEntryWidget;

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

    async fn push(&self, client: &Client<Ui>, author: UserId, content: String) -> Ui::MessageEntryWidget {
        let mut state = self.state.write().await;
        let list = &mut state.widget;

        let rich = RichMessage::parse(content);
        let widget = list.push_message(author, rich.text.clone());

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

        for (author, content) in messages {
            self.push(&stream.client, author, content).await;
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
    messages: Vec<(UserId, String)>,
}

impl MessageHistory {
    pub fn push(&mut self, author: UserId, content: String) {
        self.messages.push((author, content));
    }

    pub fn read_last(&self, count: usize, buf: &mut Vec<(UserId, String)>) {
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

    pub async fn push(&self, author: UserId, content: String) -> Option<MessageHandle<Ui>> {
        self.history.write().await.push(author, content.clone());

        let list = &self.client.message_list;
        if list.accepts(&self).await {
            let widget = list.push(&self.client, author, content).await;
            Some(MessageHandle { widget })
        } else {
            None
        }
    }

    pub async fn read_last(&self, count: usize, buf: &mut Vec<(UserId, String)>) {
        self.history.read().await.read_last(count, buf);
    }
}
