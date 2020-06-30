use chrono::{DateTime, Utc};

pub use embed::*;
pub use rich::*;
use vertex::prelude::*;



mod rich;
mod embed;

pub const MESSAGE_PAGE_SIZE: usize = 50;
pub const RECENT_HISTORY_SIZE: u64 = MESSAGE_PAGE_SIZE as u64;

#[derive(Debug, Copy, Clone)]
pub enum MessageStatus {
    Pending,
    Ok,
    Err,
}

#[derive(Debug, Clone)]
pub struct MessageContent {
    pub author: UserId,
    pub profile: Profile,
    pub text: Option<String>, // TODO properly handle deletion
    pub time: DateTime<Utc>,
}

pub struct MessageRingBuffer {
    buffer: Vec<Message>,
    write_index: usize,
    newest_index: Option<usize>,
}

impl MessageRingBuffer {
    pub fn new(capacity: usize) -> Self {
        MessageRingBuffer {
            buffer: Vec::with_capacity(capacity),
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
    pub fn push(&mut self, message: Message) {
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
    pub fn last(&self) -> Option<MessageId> {
        self.newest_index.and_then(|index| self.buffer.get(index)).map(|m| m.id)
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a Message> + 'a {
        let (new, old) = self.buffer.split_at(self.write_index);
        old.iter().chain(new.iter())
    }

    pub fn collect(mut self) -> Vec<Message> {
        let mut old = self.buffer.split_off(self.write_index);
        old.extend(self.buffer);
        old
    }
}
