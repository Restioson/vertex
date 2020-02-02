use std::rc::Rc;

use vertex::*;

use crate::{net, UiShared};

use super::{ClientUi, Result};

pub trait RoomEntryWidget<Ui: ClientUi> {
    fn bind_events(&self, room: &UiShared<RoomEntry<Ui>>);
}

pub struct RoomEntry<Ui: ClientUi> {
    net: Rc<net::RequestSender>,
    pub widget: Ui::RoomEntryWidget,

    pub community: CommunityId,
    pub id: RoomId,
    name: String,
}

impl<Ui: ClientUi> RoomEntry<Ui> {
    pub(super) fn new(
        net: Rc<net::RequestSender>,
        widget: Ui::RoomEntryWidget,
        community: CommunityId,
        id: RoomId,
        name: String,
    ) -> UiShared<Self> {
        UiShared::new(RoomEntry {
            net,
            widget,
            community,
            id,
            name,
        })
    }

    pub async fn send_message(&mut self, content: String) -> Result<()> {
        let request = ClientRequest::SendMessage(ClientSentMessage {
            to_community: self.community,
            to_room: self.id,
            content,
        });

        let request = self.net.request(request).await?;
        request.response().await?;

        Ok(())
    }
}
