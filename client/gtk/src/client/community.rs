use chrono::prelude::*;

use vertex::prelude::*;

use crate::{Client, SharedMut};

use super::{Error, Result};
use super::room::*;

use crate::screen::active::CommunityEntryWidget;

pub struct CommunityState {
    pub name: String,
    rooms: Vec<RoomEntry>,
}

#[derive(Clone)]
pub struct CommunityEntry {
    pub client: Client,

    pub widget: CommunityEntryWidget,

    pub id: CommunityId,
    pub state: SharedMut<CommunityState>,
}

impl CommunityEntry {
    pub(super) fn new(
        client: Client,
        widget: CommunityEntryWidget,
        id: CommunityId,
        name: String,
    ) -> Self {
        let state = SharedMut::new(CommunityState {
            name,
            rooms: Vec::new(),
        });
        CommunityEntry { client, widget, id, state }
    }

    pub async fn create_invite(
        &self,
        expiration_datetime: Option<DateTime<Utc>>
    ) -> Result<InviteCode> {
        let request = ClientRequest::CreateInvite { community: self.id, expiration_datetime };
        let request = self.client.request.send(request).await;

        match request.response().await? {
            OkResponse::NewInvite(code) => Ok(code),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn create_room(&self, name: &str) -> Result<RoomEntry> {
        let request = ClientRequest::CreateRoom { name: name.to_owned(), community: self.id };
        let request = self.client.request.send(request).await;

        let response = request.response().await?;

        match response {
            OkResponse::AddRoom { room, .. } => Ok(self.add_room(room).await),
            _ => Err(Error::UnexpectedMessage),
        }
    }

    pub async fn room_by_id(&self, id: RoomId) -> Option<RoomEntry> {
        self.state.read().await.rooms.iter()
            .find(|&room| room.id == id)
            .cloned()
    }

    #[inline]
    pub async fn get_room(&self, index: usize) -> Option<RoomEntry> {
        self.state.read().await.rooms.get(index).cloned()
    }

    pub(super) async fn add_room(&self, room: RoomStructure) -> RoomEntry {
        let widget = self.widget.add_room(room.name.clone());
        let entry = RoomEntry::new(
            self.client.clone(),
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
