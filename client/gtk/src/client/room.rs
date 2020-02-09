use std::rc::Rc;

use vertex::*;

use crate::{net, SharedMut};

use super::{ClientUi, Result};
use super::message::*;
use super::user::*;

pub trait RoomEntryWidget<Ui: ClientUi>: Clone {
    fn bind_events(&self, room: &RoomEntry<Ui>);
}

pub struct RoomState {
    name: String,
}

#[derive(Clone)]
pub struct RoomEntry<Ui: ClientUi> {
    request: Rc<net::RequestSender>,
    user: User,

    pub widget: Ui::RoomEntryWidget,

    pub message_stream: MessageStream<Ui>,

    pub community: CommunityId,
    pub id: RoomId,

    state: SharedMut<RoomState>,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        user: User,
        message_list: MessageList<Ui>,
        widget: Ui::RoomEntryWidget,
        community: CommunityId,
        id: RoomId,
        name: String,
    ) -> Self {
        RoomEntry {
            request,
            user,
            widget,
            message_stream: MessageStream::new(message_list),
            community,
            id,
            state: SharedMut::new(RoomState {
                name
            }),
        }
    }

    pub async fn send_message(&self, content: String) -> Result<()> {
        let mut message = self.message_stream.push(self.user.id(), content.clone()).await;
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
