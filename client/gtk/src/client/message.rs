use std::iter;

pub use embed::*;
pub use rich::*;
use vertex::*;

use crate::{Client, Error, Result, SharedMut};

use super::ClientUi;

mod rich;
mod embed;

const RECENT_HISTORY_CAPACITY: usize = 50;

#[derive(Debug, Copy, Clone)]
pub enum MessageStatus {
    Pending,
    Ok,
    Err,
}

pub trait MessageEntryWidget<Ui: ClientUi>: Clone {
    fn set_status(&mut self, status: MessageStatus);

    fn push_embed(&mut self, client: &Client<Ui>, embed: MessageEmbed);
}

struct RecentHistory {
    buffer: Vec<HistoricMessage>,
    write_index: usize,
    newest_index: Option<usize>,
}

impl RecentHistory {
    pub fn new() -> Self {
        RecentHistory {
            buffer: Vec::with_capacity(RECENT_HISTORY_CAPACITY),
            write_index: 0,
            newest_index: None,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.write_index = 0;
        self.newest_index = None;
    }

    #[inline]
    pub fn push(&mut self, message: HistoricMessage) {
        if self.buffer.len() < self.buffer.capacity() {
            self.buffer.push(message);
        } else {
            self.buffer[self.write_index] = message;
        }
        self.newest_index = Some(self.write_index);
        self.write_index = (self.write_index + 1) % self.buffer.capacity();
    }

    #[inline]
    pub fn contains(&self, id: MessageId) -> bool {
        self.buffer.iter().any(|m| m.id == id)
    }

    #[inline]
    pub fn newest(&self) -> Option<&HistoricMessage> {
        self.newest_index.and_then(|index| self.buffer.get(index))
    }

    pub fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a HistoricMessage> + 'a> {
        match self.newest_index {
            Some(newest_index) => {
                if self.buffer.len() < self.buffer.capacity() {
                    Box::new(self.buffer.iter())
                } else {
                    Box::new(
                        self.buffer[newest_index + 1..self.buffer.len()].iter()
                            .chain(self.buffer[0..=newest_index].iter())
                    )
                }
            }
            None => Box::new(iter::empty()),
        }
    }
}

struct StreamState {
    recent_history: RecentHistory,
    last_read: Option<MessageId>,
}

impl StreamState {}

#[derive(Clone)]
pub struct MessageStream<Ui: ClientUi> {
    pub community: CommunityId,
    pub room: RoomId,

    pub client: Client<Ui>,

    state: SharedMut<StreamState>,
}

impl<Ui: ClientUi> MessageStream<Ui> {
    pub fn new(community: CommunityId, room: RoomId, client: Client<Ui>) -> Self {
        let state = SharedMut::new(StreamState {
            recent_history: RecentHistory::new(),
            last_read: None,
        });
        MessageStream { community, room, client, state }
    }

    pub async fn update(&self, state: RoomState) -> Result<()> {
        let RoomState { newest_message, last_read } = state;

        self.state.write().await.last_read = last_read;

        if let Some(newest_message) = newest_message {
            self.catch_up_message_history(newest_message).await?;
        }

        let chat = &self.client.chat;
        if chat.accepts(&self).await {
            let state = self.state.read().await;
            for message in state.recent_history.iter() {
                chat.push_historic(&self.client, message.clone()).await;
            }
        }

        Ok(())
    }

    async fn catch_up_message_history(&self, newest_message: MessageId) -> Result<()> {
        let mut state = self.state.write().await;
        if state.recent_history.contains(newest_message) {
            // we're already caught up
            return Ok(());
        }

        let selector = match state.last_read {
            Some(last_read) => MessageSelector::UpTo {
                from: newest_message,
                up_to: last_read,
                count: RECENT_HISTORY_CAPACITY,
            },
            None => MessageSelector::Before {
                message: newest_message,
                count: RECENT_HISTORY_CAPACITY,
            },
        };

        let messages = self.request_messages(selector).await?;

        state.recent_history.clear();
        for message in messages.into_iter().rev() {
            state.recent_history.push(message);
        }

        Ok(())
    }

    pub async fn mark_as_read(&self) {
        // only mark as read if we had unread messages
        if !self.has_unread_messages().await {
            return;
        }

        let mut state = self.state.write().await;

        state.last_read = state.recent_history.newest()
            .map(|m| m.id);

        self.client.request.send(ClientRequest::SetAsRead {
            community: self.community,
            room: self.room,
        }).await;
    }

    pub async fn has_unread_messages(&self) -> bool {
        let state = self.state.read().await;
        match state.recent_history.newest() {
            Some(newest) => Some(newest.id) != state.last_read,
            None => false,
        }
    }

    pub async fn push(&self, message: HistoricMessage) -> Option<Ui::MessageEntryWidget> {
        self.state.write().await.recent_history.push(message.clone());

        let chat = &self.client.chat;
        if chat.accepts(&self).await {
            Some(chat.push_historic(&self.client, message).await)
        } else {
            None
        }
    }

    async fn request_messages(&self, selector: MessageSelector) -> Result<Vec<HistoricMessage>> {
        let request = ClientRequest::ReadMessages {
            community: self.community,
            room: self.room,
            selector,
        };
        let request = self.client.request.send(request).await;

        match request.response().await? {
            OkResponse::MessageHistory(messages) => Ok(messages),
            _ => Err(Error::UnexpectedMessage),
        }
    }
}
