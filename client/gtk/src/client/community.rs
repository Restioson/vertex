use std::rc::Rc;

use chrono::prelude::*;

use vertex::*;

use crate::{net, UiShared};

use super::{ClientUi, Error, Result};
use super::room::*;

pub trait CommunityEntryWidget<Ui: ClientUi> {
    fn bind_events(&self, community: &UiShared<CommunityEntry<Ui>>);

    fn add_room(&self, name: String) -> Ui::RoomEntryWidget;
}

pub struct CommunityEntry<Ui: ClientUi> {
    net: Rc<net::RequestSender>,
    pub widget: Ui::CommunityEntryWidget,

    pub id: CommunityId,
    name: String,

    rooms: Vec<UiShared<RoomEntry<Ui>>>,
    selected_room: Option<usize>,
}

impl<Ui: ClientUi> CommunityEntry<Ui> {
    pub(super) fn new(
        net: Rc<net::RequestSender>,
        widget: Ui::CommunityEntryWidget,
        id: CommunityId,
        name: String,
    ) -> UiShared<Self> {
        UiShared::new(CommunityEntry {
            net,
            widget,
            id,
            name,
            rooms: Vec::new(),
            selected_room: None,
        })
    }

    pub async fn create_invite(&self, expiration: Option<DateTime<Utc>>) -> Result<InviteCode> {
        let request = ClientRequest::CreateInvite { community: self.id, expiration_date: expiration };
        let request = self.net.request(request).await?;

        match request.response().await? {
            OkResponse::Invite { code } => Ok(code),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub async fn create_room(&mut self, name: &str) -> Result<&UiShared<RoomEntry<Ui>>> {
        let request = ClientRequest::CreateRoom { name: name.to_owned(), community: self.id };
        let request = self.net.request(request).await?;
        let response = request.response().await?;

        match response {
            OkResponse::AddRoom { room, .. } => Ok(self.add_room(room)),
            _ => Err(Error::UnexpectedResponse),
        }
    }

    pub fn select_room(&mut self, index: Option<usize>) {
        self.selected_room = index;
    }

    pub fn selected_room(&self) -> Option<&UiShared<RoomEntry<Ui>>> {
        self.selected_room.and_then(move |idx| self.get_room(idx))
    }

    #[inline]
    pub fn get_room(&self, index: usize) -> Option<&UiShared<RoomEntry<Ui>>> {
        self.rooms.get(index)
    }

    pub(super) fn add_room(&mut self, room: RoomStructure) -> &UiShared<RoomEntry<Ui>> {
        let widget = self.widget.add_room(room.name.clone());
        let entry = RoomEntry::new(
            self.net.clone(),
            widget,
            self.id,
            room.id,
            room.name,
        );

        self.rooms.push(entry.clone());
        self.rooms.last().unwrap()
    }
}
