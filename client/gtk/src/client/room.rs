use std::rc::Rc;

use vertex::*;

use crate::{net, UiEntity};

use super::{ClientUi, Result};
use super::message::*;
use super::user::*;

pub trait RoomEntryWidget<Ui: ClientUi> {
    fn bind_events(&self, room: &UiEntity<RoomEntry<Ui>>);
}

pub struct RoomEntry<Ui: ClientUi> {
    request: Rc<net::RequestSender>,
    user: UiEntity<User>,

    pub widget: Ui::RoomEntryWidget,

    pub message_stream: MessageStream<Ui>,

    pub community: CommunityId,
    pub id: RoomId,
    name: String,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        user: UiEntity<User>,
        message_list: UiEntity<MessageList<Ui>>,
        widget: Ui::RoomEntryWidget,
        community: CommunityId,
        id: RoomId,
        name: String,
    ) -> UiEntity<Self> {
        UiEntity::new(RoomEntry {
            request,
            user,
            widget,
            message_stream: MessageStream::new(message_list),
            community,
            id,
            name,
        })
    }

    pub async fn send_message(&mut self, content: String) -> Result<()> {
        let mut message = self.message_stream.push(self.user.read().await.id(), content.clone()).await;
        message.set_status(MessageStatus::Pending);

        let result = self.send_message_request(content).await;
        match result {
            Ok(_) => message.set_status(MessageStatus::Ok),
            Err(_) => message.set_status(MessageStatus::Err),
        }

        result
    }

    async fn send_message_request(&self, content: String) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community: self.community,
            to_room: self.id,
            content,
        });

        let request = self.request.send(request).await?;
        request.response().await?;

        Ok(())
    }
}
