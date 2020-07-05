use crate::{Client, Error, Result};
use async_trait::async_trait;
use vertex::prelude::{Message, *};
use xtra::prelude::*;

#[async_trait]
#[allow(unused_variables)]
pub trait EventHandler {
    async fn ready(&mut self, client: &mut Client) {}
    /// In the event of a timeout, client will not be available.
    async fn error(&mut self, error: Error, client: Option<&mut Client>) {}
    async fn internal_error(&mut self, client: &mut Client) {}
    async fn add_message(
        &mut self,
        community: CommunityId,
        room: RoomId,
        message: Message,
        client: &mut Client,
    ) {
    }
    async fn message_ready(&mut self, community: CommunityId, room: RoomId, client: &mut Client) {}
    async fn edit_message(&mut self, edit: Edit, client: &mut Client) {}
    async fn delete_message(&mut self, delete: Delete, client: &mut Client) {}
    async fn logged_out(&mut self) {}
    async fn add_room(&mut self, community: CommunityId, room: RoomStructure, client: &mut Client) {
    }
    async fn add_community(&mut self, community: CommunityStructure, client: &mut Client) {}
    async fn remove_community(
        &mut self,
        id: CommunityId,
        reason: RemoveCommunityReason,
        client: &mut Client,
    ) {
    }
    async fn admin_permissions_changed(&mut self, new: AdminPermissionFlags, client: &mut Client) {}
}

pub struct EventHandlerActor<H>
    where H: EventHandler + Send + 'static
{
    handler: H,
    client: Option<Client>,
}

impl<H> EventHandlerActor<H>
where
    H: EventHandler + Send + 'static,
{
    pub fn new(handler: H) -> Self {
        EventHandlerActor {
            handler,
            client: None,
        }
    }
}

impl<H> Actor for EventHandlerActor<H> where H: EventHandler + Send + 'static {}

pub enum HandlerMessage {
    Event(Result<ServerEvent>),
    Ready(Client),
}

impl xtra::Message for HandlerMessage {
    type Result = ();
}

#[async_trait]
impl<H> Handler<HandlerMessage> for EventHandlerActor<H>
    where H: EventHandler + Send + 'static
{
    async fn handle(&mut self, msg: HandlerMessage, ctx: &mut Context<Self>) {
        use ServerEvent::*;

        let client = self.client.as_mut();
        let (event, client) = match msg {
            HandlerMessage::Event(Ok(event)) => (event, client.unwrap()),
            HandlerMessage::Event(Err(err)) => {
                if let Error::Websocket(_) = err {
                    ctx.stop();
                }

                return self.handler.error(err, client).await
            }
            HandlerMessage::Ready(mut client) => {
                self.handler.ready(&mut client).await;
                self.client = Some(client);
                return;
            }
        };

        match event {
            ClientReady(ready) => log::error!("Client sent ready at wrong time: {:#?}", ready),
            AddMessage {
                community,
                room,
                message,
            } => {
                self.handler
                    .add_message(community, room, message, client)
                    .await
            }
            InternalError => self.handler.internal_error(client).await,
            NotifyMessageReady { community, room } => {
                self.handler.message_ready(community, room, client).await
            }
            Edit(edit) => self.handler.edit_message(edit, client).await,
            Delete(delete) => self.handler.delete_message(delete, client).await,
            SessionLoggedOut => {
                self.handler.logged_out().await;
                ctx.stop();
            }
            AddRoom {
                community,
                structure,
            } => self.handler.add_room(community, structure, client).await,
            AddCommunity(community) => self.handler.add_community(community, client).await,
            RemoveCommunity { id, reason } => {
                self.handler.remove_community(id, reason, client).await
            }
            AdminPermissionsChanged(new) => {
                self.handler.admin_permissions_changed(new, client).await
            }
            other => log::error!("Unimplemented server event {:#?}", other),
        };
    }
}
