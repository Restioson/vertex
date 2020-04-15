use super::manager;
use crate::client::{ActiveSession, Session};
use vertex::prelude::*;
use xtra::prelude::*;
use std::future::Future;
use crate::handle_disconnected;

struct AdminPermissionsChanged(AdminPermissionFlags);

impl xtra::Message for AdminPermissionsChanged {
    type Result = ();
}

impl Handler<AdminPermissionsChanged> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(&'a mut self, message: AdminPermissionsChanged, ctx: &'a mut Context<Self>) -> Self::Responder<'a> {
        let msg = ServerMessage::Event(ServerEvent::AdminPermissionsChanged(message.0));
        self.send(msg, ctx)
    }
}

impl ActiveSession {
    pub async fn handle_admin_request(
        &mut self,
        request: AdminRequest,
    ) -> Result<OkResponse, Error> {
        match request {
            AdminRequest::Ban(user) => self.ban(user).await,
            AdminRequest::Promote { user, permissions } => self.promote(user, permissions).await,
            AdminRequest::Demote(user) => self.demote(user).await,
            _ => Err(Error::Unimplemented),
        }
    }

    fn admin_perms(&self) -> Result<AdminPermissionFlags, Error> {
        manager::get_active_user(self.user).map(|u| u.admin_perms)
    }

    fn has_admin_perms(&self, check: AdminPermissionFlags) -> Result<bool, Error> {
        let perms = self.admin_perms()?;
        Ok(perms.contains(AdminPermissionFlags::ALL) || perms.contains(check))
    }

    async fn ban(&mut self, user: UserId) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::BAN)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;
        let their_perms = db
            .get_admin_permissions(user)
            .await
            .map_err(|_| Error::InvalidUser)?; // Error assumes that we are getting own user

        // Don't allow banning more privileged users
        if their_perms.contains(self.admin_perms()?) {
            return Err(Error::AccessDenied);
        }

        db.set_banned(user, true)
            .await?
            .map_err(|_| Error::InvalidUser)
            .map(|_| OkResponse::NoData)
    }

    async fn promote(
        &mut self,
        user: UserId,
        perms: AdminPermissionFlags,
    ) -> Result<OkResponse, Error> {
        // Don't allow promoting above own permissions
        if !self.has_admin_perms(AdminPermissionFlags::PROMOTE | perms)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;

        db.set_admin_permissions(user, perms)
            .await?
            .map_err(|_| Error::InvalidUser)
            .map(|_| OkResponse::NoData)
    }

    async fn demote(&mut self, user: UserId) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::DEMOTE)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;
        let no_perms = AdminPermissionFlags::from_bits_truncate(0);

        db.set_admin_permissions(user, no_perms)
            .await?
            .map_err(|_| Error::InvalidUser)?;

        let active = manager::get_active_user(user).map_err(|_| Error::InvalidUser)?;

        active.sessions
            .values()
            .filter_map(Session::as_active_actor)
            .for_each(|a| {
                let _ = a.do_send(AdminPermissionsChanged(no_perms))
                    .map_err(handle_disconnected("ClientSession")); // Don't care
            });

        Ok(OkResponse::NoData)
    }
}
