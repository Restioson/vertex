use std::rc::Rc;

use vertex::*;

use crate::net;

use super::Result;

pub struct User {
    net: Rc<net::RequestSender>,
    username: String,
    display_name: String,
    device: DeviceId,
    token: AuthToken,
}

impl User {
    pub(super) fn new(
        net: Rc<net::RequestSender>,
        username: String,
        display_name: String,
        device: DeviceId,
        token: AuthToken,
    ) -> Self {
        User { net, username, display_name, device, token }
    }

    pub async fn change_username(&mut self, username: String) -> Result<()> {
        let request = ClientRequest::ChangeUsername { new_username: username.clone() };
        let request = self.net.request(request).await?;
        request.response().await?;

        self.username = username;

        Ok(())
    }

    pub async fn change_display_name(&mut self, display_name: String) -> Result<()> {
        let request = ClientRequest::ChangeDisplayName { new_display_name: display_name.clone() };
        let request = self.net.request(request).await?;
        request.response().await?;

        self.display_name = display_name.clone();

        Ok(())
    }

    pub async fn change_password(&mut self, old_password: String, new_password: String) -> Result<()> {
        let request = ClientRequest::ChangePassword { old_password, new_password };
        let request = self.net.request(request).await?;
        request.response().await?;
        Ok(())
    }
}
