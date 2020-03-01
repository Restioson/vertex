use std::collections::HashMap;
use std::rc::Rc;

use vertex::prelude::*;

use crate::{client, Error, net, Result, SharedMut};

fn create_default_profile(user: UserId) -> Profile {
    let name = format!("{}", user.0);
    Profile {
        version: ProfileVersion(0),
        username: name.clone(),
        display_name: name,
    }
}

// TODO: invalidate old records
#[derive(Clone)]
pub struct ProfileCache {
    request: Rc<net::RequestSender>,
    user: client::User,
    cache: SharedMut<HashMap<UserId, Profile>>,
}

impl ProfileCache {
    pub fn new(request: Rc<net::RequestSender>, user: client::User) -> ProfileCache {
        ProfileCache {
            request,
            user,
            cache: SharedMut::new(HashMap::new()),
        }
    }

    pub async fn get_or_default(&self, id: UserId, version: ProfileVersion) -> Profile {
        match self.get(id, version).await {
            Ok(profile) => profile,
            Err(err) => {
                println!("failed to get profile for {:?}: {:?}", id, err);
                let existing = self.get_existing(id, None).await;
                existing.unwrap_or_else(|| create_default_profile(id))
            }
        }
    }

    pub async fn get(&self, id: UserId, version: ProfileVersion) -> Result<Profile> {
        if id == self.user.id {
            return Ok(self.user.profile().await);
        }

        if let Some(existing) = self.get_existing(id, Some(version)).await {
            return Ok(existing);
        }

        let profile = self.request(id).await?;

        Ok(profile)
    }

    pub async fn get_existing(&self, id: UserId, version: Option<ProfileVersion>) -> Option<Profile> {
        let cache = self.cache.read().await;
        cache.get(&id).and_then(|profile| {
            match version {
                Some(version) if profile.version != version => None,
                _ => Some(profile.clone()),
            }
        })
    }

    async fn request(&self, id: UserId) -> Result<Profile> {
        let request = ClientRequest::GetProfile(id);
        let request = self.request.send(request).await;

        match request.response().await? {
            OkResponse::Profile(profile) => {
                let mut cache = self.cache.write().await;
                cache.insert(id, profile.clone());
                Ok(profile)
            }
            _ => Err(Error::UnexpectedMessage),
        }
    }
}
