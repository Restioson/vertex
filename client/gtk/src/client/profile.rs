use std::collections::HashMap;
use std::rc::Rc;

use vertex::*;

use crate::{client, Error, net, Result, SharedMut};

// TODO: invalidate old records
#[derive(Clone)]
pub struct ProfileCache {
    request: Rc<net::RequestSender>,
    user: client::User,
    cache: SharedMut<HashMap<UserId, UserProfile>>,
}

impl ProfileCache {
    pub fn new(request: Rc<net::RequestSender>, user: client::User) -> ProfileCache {
        ProfileCache {
            request,
            user,
            cache: SharedMut::new(HashMap::new()),
        }
    }

    pub async fn get(&self, id: UserId, version: Option<ProfileVersion>) -> Result<UserProfile> {
        if id == self.user.id {
            return Ok(self.user.profile().await);
        }

        if let Some(existing) = self.get_existing(id, version).await {
            return Ok(existing);
        }

        let profile = self.request(id).await?;

        let mut cache = self.cache.write().await;
        cache.insert(id, profile.clone());

        Ok(profile)
    }

    pub async fn get_existing(&self, id: UserId, version: Option<ProfileVersion>) -> Option<UserProfile> {
        let cache = self.cache.read().await;
        cache.get(&id).and_then(|profile| {
            match version {
                Some(version) if profile.version != version => None,
                _ => Some(profile.clone()),
            }
        })
    }

    async fn request(&self, id: UserId) -> Result<UserProfile> {
        let request = ClientRequest::GetUserProfile(id);
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::UserProfile(profile) => Ok(profile),
            _ => Err(Error::UnexpectedMessage),
        }
    }
}
