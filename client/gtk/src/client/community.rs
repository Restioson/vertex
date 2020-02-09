use std::rc::Rc;

use chrono::prelude::*;

use vertex::*;

use crate::{net, SharedMut};

use super::{ClientUi, Error, Result};
use super::message::*;
use super::room::*;
use super::user::*;

pub trait CommunityEntryWidget<Ui: ClientUi>: Clone {
    fn bind_events(&self, community: &CommunityEntry<Ui>);

    fn add_room(&self, name: String) -> Ui::RoomEntryWidget;
}

pub struct CommunityState<Ui: ClientUi> {
    name: String,
    rooms: Vec<RoomEntry<Ui>>,
}

#[derive(Clone)]
pub struct CommunityEntry<Ui: ClientUi> {
    request: Rc<net::RequestSender>,
    user: User,
    message_list: MessageList<Ui>,

    pub widget: Ui::CommunityEntryWidget,

    pub id: CommunityId,

    state: SharedMut<CommunityState<Ui>>,
}

impl<Ui: ClientUi> CommunityEntry<Ui> {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        user: User,
        message_list: MessageList<Ui>,
        widget: Ui::CommunityEntryWidget,
        id: CommunityId,
        name: String,
    ) -> Self {
        CommunityEntry {
            request,
            user,
            message_list,
            widget,
            id,
            state: SharedMut::new(CommunityState {
                name,
                rooms: Vec::new(),
            }),
        }
    }

    pub async fn create_invite(&self, expiration: Option<DateTime<Utc>>) -> Result<InviteCode> {
        let request = ClientRequest::CreateInvite { community: self.id, expiration_date: expiration };
        let request = self.request.send(request).await?;

        match request.response().await? {
            OkResponse::Invite { code } => Ok(code),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn create_room(&self, name: &str) -> Result<RoomEntry<Ui>> {
        let request = ClientRequest::CreateRoom { name: name.to_owned(), community: self.id };
        let request = self.request.send(request).await?;

        let response = request.response().await?;

        match response {
            OkResponse::AddRoom { room, .. } => Ok(self.add_room(room).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    #[inline]
    pub async fn get_room(&self, index: usize) -> Option<RoomEntry<Ui>> {
        self.state.read().await.rooms.get(index).cloned()
    }

    pub(super) async fn add_room(&self, room: RoomStructure) -> RoomEntry<Ui> {
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

        let mut state = self.state.write().await;
        state.rooms.push(entry);
        state.rooms.last().unwrap().clone()
    }
}
