use vertex::*;

use crate::SharedMut;

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

pub struct MessageListState<Ui: ClientUi> {
    widget: Ui::MessageListWidget,
}

#[derive(Clone)]
pub struct MessageList<Ui: ClientUi> {
    state: SharedMut<MessageListState<Ui>>,
}

impl<Ui: ClientUi> MessageList<Ui> {
    pub fn new(widget: Ui::MessageListWidget) -> Self {
        MessageList {
            state: SharedMut::new(MessageListState { widget })
        }
    }

    pub async fn push(&self, author: UserId, content: String) -> MessageHandle<Ui> {
        let mut state = self.state.write().await;
        let widget = state.widget.push_message(author, content);
        MessageHandle { widget }
    }
}

#[derive(Clone)]
pub struct MessageStream<Ui: ClientUi> {
    pub list: MessageList<Ui>,
}

impl<Ui: ClientUi> MessageStream<Ui> {
    pub fn new(list: MessageList<Ui>) -> Self {
        MessageStream { list }
    }

    pub async fn push(&self, author: UserId, content: String) -> MessageHandle<Ui> {
        self.list.push(author, content).await
    }
}
