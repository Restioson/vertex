use crate::net::{Network, SendRequest};
use crate::{Error, Result};
use std::collections::HashMap;
use vertex::prelude::*;
use xtra::prelude::*;

// TODO: invalidate old records
#[derive(Clone)]
pub struct ProfileCache {
    cache: HashMap<UserId, Profile>,
}

pub enum ProfileResult {
    /// Up-to-date profile successfully retrieved
    UpToDate(Profile),
    /// Up-to-date profile could not be retrieved, cached available.
    Cached(Profile, Error),
    /// No profile could be retrieved
    None(Error),
}

// TODO from
impl Into<Result<Profile>> for ProfileResult {
    fn into(self) -> Result<Profile> {
        match self {
            ProfileResult::UpToDate(p) => Ok(p),
            ProfileResult::Cached(p, _) => Ok(p),
            ProfileResult::None(e) => Err(e),
        }
    }
}

impl ProfileCache {
    pub fn new() -> ProfileCache {
        ProfileCache {
            cache: HashMap::new(),
        }
    }

    pub async fn get(
        &mut self,
        sender: &Address<Network>,
        id: UserId,
        version: ProfileVersion,
    ) -> ProfileResult {
        if !self
            .cache
            .get(&id)
            .map(|p| p.version == version)
            .unwrap_or(false)
        {
            match (self.load(id, sender).await, self.cache.contains_key(&id)) {
                (Ok(profile), _) => {
                    self.cache.insert(id, profile);
                    ProfileResult::UpToDate(self.cache.get(&id).unwrap().clone())
                }
                (Err(err), true) => {
                    ProfileResult::Cached(self.cache.get(&id).unwrap().clone(), err)
                }
                (Err(err), false) => ProfileResult::None(err),
            }
        } else {
            ProfileResult::UpToDate(self.cache.get(&id).unwrap().clone())
        }
    }

    async fn load(&self, id: UserId, sender: &Address<Network>) -> Result<Profile> {
        let request = ClientRequest::GetProfile(id);
        let request = sender.send(SendRequest(request)).await.unwrap()?;

        match request.response().await? {
            OkResponse::Profile(profile) => Ok(profile),
            other => Err(Error::UnexpectedMessage {
                expected: "OkResponse::Profile",
                got: Box::new(other),
            }),
        }
    }
}
