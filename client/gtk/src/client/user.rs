use std::rc::Rc;

use vertex::*;

use crate::{net, SharedMut};

use super::Result;

pub struct UserState {
    username: String,
    display_name: String,
}

#[derive(Clone)]
pub struct User {
    request: Rc<net::RequestSender>,
    id: UserId,
    device: DeviceId,
    token: AuthToken,
    state: SharedMut<UserState>,
}

impl User {
    pub(super) fn new(
        request: Rc<net::RequestSender>,
        id: UserId,
        username: String,
        display_name: String,
        device: DeviceId,
        token: AuthToken,
    ) -> Self {
        User {
            request,
            id,
            device,
            token,
            state: SharedMut::new(UserState {
                username,
                display_name,
            }),
        }
    }

    pub async fn change_username(&self, username: String) -> Result<()> {
        let request = ClientRequest::ChangeUsername { new_username: username.clone() };
        let request = self.request.send(request).await?;
        request.response().await?;

        let mut state = self.state.write().await;
        state.username = username;

        Ok(())
    }

    pub async fn change_display_name(&self, display_name: String) -> Result<()> {
        let request = ClientRequest::ChangeDisplayName { new_display_name: display_name.clone() };
        let request = self.request.send(request).await?;
        request.response().await?;

        let mut state = self.state.write().await;
        state.display_name = display_name;

        Ok(())
    }

    pub async fn change_password(&self, old_password: String, new_password: String) -> Result<()> {
        let request = ClientRequest::ChangePassword { old_password, new_password };
        let request = self.request.send(request).await?;
        request.response().await?;
        Ok(())
    }

    #[inline]
    pub fn id(&self) -> UserId { self.id }
}
