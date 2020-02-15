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

pub trait MessageEntryWidget<Ui: ClientUi>: Clone {
    fn set_status(&mut self, status: MessageStatus);

    fn push_embed(&mut self, client: &Client<Ui>, embed: MessageEmbed);
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
    pub id: Uuid,
    pub client: Client<Ui>,
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

        let chat = &self.client.chat;
        if chat.accepts(&self).await {
            let widget = chat.push(&self.client, message).await;
            Some(MessageHandle { widget })
        } else {
            None
        }
    }

    pub async fn read_last(&self, count: usize, buf: &mut Vec<MessageSource>) {
        self.history.read().await.read_last(count, buf);
    }
}
