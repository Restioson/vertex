use uuid::Uuid;

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
    fn clear(&mut self);

    fn push_message(&mut self, author: UserId, content: String) -> Ui::MessageEntryWidget;
}

pub trait MessageEntryWidget<Ui: ClientUi> {
    fn set_status(&mut self, status: MessageStatus);
}

pub struct MessageListState<Ui: ClientUi> {
    widget: Ui::MessageListWidget,
    stream: Option<MessageStream<Ui>>,
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
        });
        MessageList { state }
    }

    async fn populate_list(&self, state: &mut MessageListState<Ui>, stream: &MessageStream<Ui>) {
        let mut messages = Vec::with_capacity(25);
        stream.read_last(25, &mut messages).await;

        for (author, content) in messages {
            state.widget.push_message(author, content);
        }
    }

    pub async fn set_stream(&self, stream: MessageStream<Ui>) {
        let mut state = self.state.write().await;

        let stream_changed = match &state.stream {
            Some(last_stream) => last_stream.id != stream.id,
            None => true,
        };

        if stream_changed {
            state.widget.clear();
            self.populate_list(&mut *state, &stream).await;
        }

        state.stream = Some(stream);
    }

    pub async fn detach_stream(&self) {
        let mut state = self.state.write().await;
        state.stream = None;
        state.widget.clear();
    }

    pub async fn accepts(&self, accepts: &MessageStream<Ui>) -> bool {
        let state = self.state.read().await;
        match &state.stream {
            Some(stream) => stream.id == accepts.id,
            None => false,
        }
    }

    pub async fn push(&self, author: UserId, content: String) -> MessageHandle<Ui> {
        let mut state = self.state.write().await;
        let widget = state.widget.push_message(author, content);
        MessageHandle { widget }
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
    list: MessageList<Ui>,
    history: SharedMut<MessageHistory>,
}

impl<Ui: ClientUi> MessageStream<Ui> {
    pub fn new(id: Uuid, list: MessageList<Ui>) -> Self {
        let history = SharedMut::new(MessageHistory {
            messages: vec![]
        });
        MessageStream { id, list, history }
    }

    pub async fn push(&self, author: UserId, content: String) -> Option<MessageHandle<Ui>> {
        self.history.write().await.push(author, content.clone());
        if self.list.accepts(&self).await {
            Some(self.list.push(author, content).await)
        } else {
            None
        }
    }

    pub async fn read_last(&self, count: usize, buf: &mut Vec<(UserId, String)>) {
        self.history.read().await.read_last(count, buf);
    }
}
