use super::manager;
use crate::auth::HashSchemeVersion;
use crate::client::session::LogoutThisSession;
use crate::client::{ActiveSession, Session};
use crate::handle_disconnected;
use futures::TryStreamExt;
use std::future::Future;
use vertex::prelude::*;
use xtra::prelude::*;

struct AdminPermissionsChanged(AdminPermissionFlags);

impl xtra::Message for AdminPermissionsChanged {
    type Result = ();
}

impl Handler<AdminPermissionsChanged> for ActiveSession {
    type Responder<'a> = impl Future<Output = ()> + 'a;

    fn handle<'a>(
        &'a mut self,
        message: AdminPermissionsChanged,
        ctx: &'a mut Context<Self>,
    ) -> Self::Responder<'a> {
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
            AdminRequest::Unban(user) => self.unban(user).await,
            AdminRequest::Unlock(user) => self.unlock(user).await,
            AdminRequest::Promote { user, permissions } => self.promote(user, permissions).await,
            AdminRequest::Demote(user) => self.demote(user).await,
            AdminRequest::SearchUser { name } => self.search_user(name).await,
            AdminRequest::ListAllUsers => self.list_all_users().await,
            AdminRequest::ListAllAdmins => self.list_all_admins().await,
            AdminRequest::SearchForReports(criteria) => self.search_reports(criteria).await,
            AdminRequest::SetReportStatus { id, status } => {
                self.set_report_status(id, status).await
            }
            AdminRequest::SetAccountsCompromised(typ) => self.set_accounts_compromised(typ).await,
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

    async fn unban(&mut self, user: UserId) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::BAN)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;

        db.set_banned(user, false)
            .await?
            .map_err(|_| Error::InvalidUser)
            .map(|_| OkResponse::NoData)
    }

    async fn unlock(&mut self, user: UserId) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::BAN)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;

        db.set_locked(user, false)
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
        let their_perms = db
            .get_admin_permissions(user)
            .await
            .map_err(|_| Error::InvalidUser)?; // Error assumes that we are getting own user

        // Don't allow demoting more privileged users
        let all = AdminPermissionFlags::ALL;
        if user != self.user &&
            (their_perms.contains(self.admin_perms()?) || their_perms.contains(all)) {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;

        db.set_admin_permissions(user, perms)
            .await?
            .map_err(|_| Error::InvalidUser)?;

        notify_of_admin_perm_change(user, perms);

        Ok(OkResponse::NoData)
    }

    async fn demote(&mut self, user: UserId) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::PROMOTE)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;
        let no_perms = AdminPermissionFlags::from_bits_truncate(0);

        let their_perms = db
            .get_admin_permissions(user)
            .await
            .map_err(|_| Error::InvalidUser)?; // Error assumes that we are getting own user

        // Don't allow demoting more privileged users but allow demoting self
        let all = AdminPermissionFlags::ALL;
        if user != self.user &&
            (their_perms.contains(self.admin_perms()?) || their_perms.contains(all)) {
            return Err(Error::AccessDenied);
        }

        db.set_admin_permissions(user, no_perms)
            .await?
            .map_err(|_| Error::InvalidUser)?;

        notify_of_admin_perm_change(user, no_perms);

        Ok(OkResponse::NoData)
    }

    async fn search_user(&mut self, name: String) -> Result<OkResponse, Error> {
        let stream = self.global.database.search_user(name).await?;
        let users: Vec<ServerUser> = stream.map_ok(Into::into).try_collect().await?;
        Ok(OkResponse::Admin(AdminResponse::SearchedUsers(users)))
    }

    async fn list_all_users(&mut self) -> Result<OkResponse, Error> {
        let stream = self.global.database.list_all_server_users().await?;
        let users: Vec<ServerUser> = stream.map_ok(Into::into).try_collect().await?;
        Ok(OkResponse::Admin(AdminResponse::SearchedUsers(users)))
    }

    async fn list_all_admins(&mut self) -> Result<OkResponse, Error> {
        let stream = self.global.database.list_all_admins().await?;
        let admins: Vec<Admin> = stream.try_collect().await?;
        Ok(OkResponse::Admin(AdminResponse::Admins(admins)))
    }

    async fn search_reports(&mut self, criteria: SearchCriteria) -> Result<OkResponse, Error> {
        let stream = self.global.database.search_reports(criteria).await?;
        let reports: Vec<Report> = stream.try_collect().await?;
        Ok(OkResponse::Admin(AdminResponse::Reports(reports)))
    }

    async fn set_report_status(
        &mut self,
        id: i32,
        status: ReportStatus,
    ) -> Result<OkResponse, Error> {
        self.global.database.set_report_status(id, status).await?;
        Ok(OkResponse::NoData)
    }

    async fn set_accounts_compromised(
        &mut self,
        typ: SetCompromisedType,
    ) -> Result<OkResponse, Error> {
        if !self.has_admin_perms(AdminPermissionFlags::SET_ACCOUNTS_COMPROMISED)? {
            return Err(Error::AccessDenied);
        }

        let db = &self.global.database;
        let all = typ == SetCompromisedType::All;
        match typ {
            SetCompromisedType::All => db.set_all_accounts_compromised().await?,
            SetCompromisedType::OldHashes => db.set_accounts_with_old_hashes_compromised().await?,
        }

        // Log out logged-in users
        super::manager::USERS.retain(|_, user| {
            if user.hash_scheme_version < HashSchemeVersion::LATEST || all {
                let sessions = &mut user.sessions;
                for (_, session) in sessions {
                    if let Session::Active { actor, .. } = session {
                        let _ = actor
                            .do_send(LogoutThisSession)
                            .map_err(handle_disconnected("ClientSession"));
                    }
                }

                false
            } else {
                true
            }
        });

        Ok(OkResponse::NoData)
    }
}

fn notify_of_admin_perm_change(user: UserId, new: AdminPermissionFlags) {
    let mut active = match manager::get_active_user_mut(user) {
        Ok(user) => user,
        Err(_) => return, // Not logged in
    };

    active.admin_perms = new;

    active
        .sessions
        .values()
        .filter_map(Session::as_active_actor)
        .for_each(|a| {
            let _ = a
                .do_send(AdminPermissionsChanged(new))
                .map_err(handle_disconnected("ClientSession")); // Don't care
        });
}
