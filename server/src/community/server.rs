use crate::community::CommunityActor;
use vertex_common::{CommunityId, UserId, DeviceId, ServerError};
use std::collections::HashMap;
use actix::{Addr, Context, Actor, Message, ResponseFuture, MailboxError};
use std::fmt::Debug;
use actix::prelude::fut;
use actix::fut::Either;

struct CommunityServer {
    communities: HashMap<CommunityId, Addr<CommunityActor>>,
}

pub struct GetCommunityActor(pub CommunityId);

#[derive(Debug)]
pub struct AuthenticatedMessage<T: Message + Debug> {
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub msg: T,
}

impl<T: Message + Debug> Message for AuthenticatedMessage<T> {
    type Result = T::Result;
}

impl Handle<AuthenticatedMessage<GetCommunityActor>> for CommunityServer {
    type Result = ResponseFuture<Option<CommunityActor>, ServerError>;

    fn handle(&mut self, msg: AuthenticatedMessage<GetCommunityActor>, _: &mut Context<Self>) -> Self::Result {
        match self.communities.get(&msg.msg.0) {
            Some(addr) => {
                addr.send()
                    .
            }
            None => Either::B(fut::ok(None)),
        }
    }
}

impl Actor for CommunityServer {
    type Context = Context<Self>;
}
