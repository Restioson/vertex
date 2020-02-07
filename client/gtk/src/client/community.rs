use std::rc::Rc;

use chrono::prelude::*;

use vertex::*;

use crate::{net, UiEntity};

use super::{ClientUi, Error, Result};
use super::message::*;
use super::room::*;
use super::user::*;

pub trait CommunityEntryWidget<Ui: ClientUi> {
    fn bind_events(&self, community: &UiEntity<CommunityEntry<Ui>>);

    fn add_room(&self, name: String) -> Ui::RoomEntryWidget;
}

pub struct CommunityEntry<Ui: ClientUi> {
    request: Rc<net::RequestSender>,
    user: UiEntity<User>,
    message_list: UiEntity<MessageList<Ui>>,

    pub widget: Ui::CommunityEntryWidget,

    pub id: CommunityId,
    name: String,

    rooms: Vec<UiEntity<RoomEntry<Ui>>>,
    selected_room: Option<usize>,
}

impl<Ui: ClientUi> CommunityEntry<Ui> {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        user: UiEntity<User>,
        message_list: UiEntity<MessageList<Ui>>,
        widget: Ui::CommunityEntryWidget,
        id: CommunityId,
        name: String,
    ) -> UiEntity<Self> {
        UiEntity::new(CommunityEntry {
            request,
            user,
            message_list,
            widget,
            id,
            name,
            rooms: Vec::new(),
            selected_room: None,
        })
    }

    pub async fn create_invite(&self, expiration: Option<DateTime<Utc>>) -> Result<InviteCode> {
        let request = ClientRequest::CreateInvite { community: self.id, expiration_date: expiration };
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::Invite { code } => Ok(code),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn create_room(&mut self, name: &str) -> Result<UiEntity<RoomEntry<Ui>>> {
        let request = ClientRequest::CreateRoom { name: name.to_owned(), community: self.id };
        let request = self.request.send(request).await?;

        let response = request.response().await?;

        match response {
            OkResponse::AddRoom { room, .. } => Ok(self.add_room(room)),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub fn select_room(&mut self, index: Option<usize>) {
        self.selected_room = index;
    }

    pub fn selected_room(&self) -> Option<&UiEntity<RoomEntry<Ui>>> {
        self.selected_room.and_then(move |idx| self.get_room(idx))
    }

    #[inline]
    pub fn get_room(&self, index: usize) -> Option<&UiEntity<RoomEntry<Ui>>> {
        self.rooms.get(index)
    }

    pub(super) fn add_room(&mut self, room: RoomStructure) -> UiEntity<RoomEntry<Ui>> {
        let widget = self.widget.add_room(room.name.clone());
        let entry = RoomEntry::new(
            self.request.clone(),
            self.user.clone(),
            self.message_list.clone(),
            widget,
            self.id,
            room.id,
            room.name,
        );

        self.rooms.push(entry.clone());
        self.rooms.last().unwrap().clone()
    }
}
