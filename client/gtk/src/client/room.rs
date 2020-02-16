use vertex::*;

use crate::Client;

use super::{ClientUi, Result};
use super::message::*;

pub trait RoomEntryWidget<Ui: ClientUi>: Clone {
    fn bind_events(&self, room: &RoomEntry<Ui>);
}

#[derive(Clone)]
pub struct RoomEntry<Ui: ClientUi> {
    pub client: Client<Ui>,

    pub widget: Ui::RoomEntryWidget,

    pub message_stream: MessageStream<Ui>,

    pub community: CommunityId,
    pub id: RoomId,

    pub name: String,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        client: Client<Ui>,
        widget: Ui::RoomEntryWidget,
        community: CommunityId,
        id: RoomId,
        name: String,
    ) -> Self {
        let message_stream = MessageStream::new(community, id, client.clone());
        RoomEntry { client, widget, message_stream, community, id, name }
    }

    pub async fn update(&self, state: RoomState) -> Result<()> {
        self.message_stream.update(state).await
    }

    pub async fn send_message(&self, content: String) {
        let user = self.client.user.id;
        let profile = self.client.user.profile().await;

        let mut message = self.client.chat.push(
            &self.client,
            user, profile,
            content.clone(),
        ).await;

        message.set_status(MessageStatus::Pending);

        let result = self.send_message_request(content).await;
        match result {
            Ok(_) => message.set_status(MessageStatus::Ok),
            Err(_) => message.set_status(MessageStatus::Err),
        }
    }

    async fn send_message_request(&self, content: String) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community: self.community,
            to_room: self.id,
            content,
        });

        let request = self.client.request.send(request).await;
        request.response().await?;

        Ok(())
    }
}

impl<Ui: ClientUi> PartialEq<RoomEntry<Ui>> for RoomEntry<Ui> {
    fn eq(&self, other: &RoomEntry<Ui>) -> bool {
        self.id == other.id && self.community == other.community
    }
}

impl<Ui: ClientUi> Eq for RoomEntry<Ui> {}
