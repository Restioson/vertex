use std::rc::Rc;

use vertex::prelude::*;

use crate::{net, SharedMut};

use super::Result;

pub struct UserState {
    profile: Profile,
}

#[derive(Clone)]
pub struct User {
    request: Rc<net::RequestSender>,
    pub id: UserId,
    device: DeviceId,
    token: AuthToken,
    state: SharedMut<UserState>,
}

impl User {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        id: UserId,
        profile: Profile,
        device: DeviceId,
        token: AuthToken,
    ) -> Self {
        User {
            request,
            id,
            device,
            token,
            state: SharedMut::new(UserState {
                profile
            }),
        }
    }

    pub async fn change_username(&self, username: String) -> Result<()> {
        let request = ClientRequest::ChangeUsername { new_username: username.clone() };
        let request = self.request.send(request).await;
        request.response().await?;

        let mut state = self.state.write().await;
        state.profile.username = username;

        Ok(())
    }

    pub async fn change_display_name(&self, display_name: String) -> Result<()> {
        let request = ClientRequest::ChangeDisplayName { new_display_name: display_name.clone() };
        let request = self.request.send(request).await;
        request.response().await?;

        let mut state = self.state.write().await;
        state.profile.display_name = display_name;

        Ok(())
    }

    pub async fn change_password(&self, old_password: String, new_password: String) -> Result<()> {
        let request = ClientRequest::ChangePassword { old_password, new_password };
        let request = self.request.send(request).await;
        request.response().await?;
        Ok(())
    }

    pub async fn profile(&self) -> Profile {
        self.state.read().await.profile.clone()
    }
}
