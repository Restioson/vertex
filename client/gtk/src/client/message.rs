use vertex::*;

use crate::UiEntity;

use super::ClientUi;

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
    fn push_message(&mut self, author: UserId, content: String) -> Ui::MessageEntryWidget;
}

pub trait MessageEntryWidget<Ui: ClientUi> {
    fn set_status(&mut self, status: MessageStatus);
}

pub struct MessageList<Ui: ClientUi> {
    pub widget: Ui::MessageListWidget,
}

impl<Ui: ClientUi> MessageList<Ui> {
    pub fn new(widget: Ui::MessageListWidget) -> UiEntity<Self> {
        UiEntity::new(MessageList { widget })
    }

    pub fn push(&mut self, author: UserId, content: String) -> MessageHandle<Ui> {
        let widget = self.widget.push_message(author, content);
        MessageHandle { widget }
    }
}

pub struct MessageStream<Ui: ClientUi> {
    pub list: UiEntity<MessageList<Ui>>,
}

impl<Ui: ClientUi> MessageStream<Ui> {
    pub fn new(list: UiEntity<MessageList<Ui>>) -> Self {
        MessageStream { list }
    }

    pub async fn push(&mut self, author: UserId, content: String) -> MessageHandle<Ui> {
        self.list.write().await.push(author, content)
    }
}
