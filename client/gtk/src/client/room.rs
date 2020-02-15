use vertex::*;

use crate::{Client, SharedMut};

use super::{ClientUi, Result};
use super::message::*;
use crate::client::community::CommunityEntry;

pub trait RoomEntryWidget<Ui: ClientUi>: Clone {
    fn bind_events(&self, room: &RoomEntry<Ui>);
}

pub struct RoomState {
    name: String,
}

#[derive(Clone)]
pub struct RoomEntry<Ui: ClientUi> {
    pub client: Client<Ui>,

    pub widget: Ui::RoomEntryWidget,

    pub message_stream: MessageStream<Ui>,

    pub community: CommunityEntry<Ui>,
    pub id: RoomId,

    state: SharedMut<RoomState>,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        client: Client<Ui>,
        message_list: MessageList<Ui>,
        widget: Ui::RoomEntryWidget,
        community: CommunityEntry<Ui>,
        id: RoomId,
        name: String,
    ) -> Self {
        let message_stream = MessageStream::new(id.0, message_list);
        let state = RoomState { name };
        let state = SharedMut::new(state);

        RoomEntry { client, widget, message_stream, community, id, state }
    }

    pub async fn add_message(&self, author: UserId, content: String) {
        if let Some(mut message) = self.message_stream.push(author, content).await {
            message.set_status(MessageStatus::Ok);
        }
    }

    pub async fn send_message(&self, content: String) -> Result<()> {
        match self.message_stream.push(self.client.user.id(), content.clone()).await {
            Some(mut message) => {
                message.set_status(MessageStatus::Pending);

                let result = self.send_message_request(content).await;
                match result {
                    Ok(_) => message.set_status(MessageStatus::Ok),
                    Err(_) => message.set_status(MessageStatus::Err),
                }

                result
            }
            None => self.send_message_request(content).await
        }
    }

    async fn send_message_request(&self, content: String) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community: self.community.id,
            to_room: self.id,
            content,
        });

        let request = self.client.request.send(request).await?;
        request.response().await?;

        Ok(())
    }
}

impl<Ui: ClientUi> PartialEq<RoomEntry<Ui>> for RoomEntry<Ui> {
    fn eq(&self, other: &RoomEntry<Ui>) -> bool {
        self.id == other.id && self.community.id == other.community.id
    }
}

impl<Ui: ClientUi> Eq for RoomEntry<Ui> {}
