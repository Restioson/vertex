use vertex::*;

use crate::Client;

use super::{ClientUi, Result};
use super::community::*;
use super::message::*;

pub trait RoomEntryWidget<Ui: ClientUi>: Clone {
    fn bind_events(&self, room: &RoomEntry<Ui>);
}

#[derive(Clone)]
pub struct RoomEntry<Ui: ClientUi> {
    pub client: Client<Ui>,

    pub widget: Ui::RoomEntryWidget,

    pub message_stream: MessageStream<Ui>,

    pub community: CommunityEntry<Ui>,
    pub id: RoomId,

    pub name: String,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        client: Client<Ui>,
        widget: Ui::RoomEntryWidget,
        community: CommunityEntry<Ui>,
        id: RoomId,
        name: String,
    ) -> Self {
        let message_stream = MessageStream::new(id.0, client.clone());
        RoomEntry { client, widget, message_stream, community, id, name }
    }

    pub async fn add_message(&self, message: MessageSource) {
        if let Some(mut message) = self.message_stream.push(message).await {
            message.set_status(MessageStatus::Ok);
        }
    }

    pub async fn send_message(&self, content: String) -> Result<()> {
        let message = MessageSource {
            author: self.client.user.id,
            author_profile_version: None,
            content: content.clone(),
        };

        match self.message_stream.push(message).await {
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
