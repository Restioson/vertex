use crate::proto;
use crate::proto::DeserializeError;
use crate::types::UserId;
use bitflags::bitflags;
use std::convert::{TryFrom, TryInto};

bitflags! {
    pub struct AdminPermissionFlags: i64 {
        /// All permissions. Could be used for the server owner.
        const ALL = 1;
        /// Ban and unban users.
        const BAN = 1 << 1;
        /// Promote or demote users.
        const PROMOTE = 1 << 2;
        /// Is an admin at all. Allows for searching users and viewing reports. Any other permission
        /// automatically grants this one - it is just used as a placeholder in case of not granting
        /// any other admin permissions.
        const IS_ADMIN = 1 << 3;
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AdminRequest {
    Promote {
        user: UserId,
        permissions: AdminPermissionFlags,
    },
    Demote(UserId),
    Ban(UserId),
    Unban(UserId),
    Unlock(UserId),
    SearchUser {
        name: String,
    },
    ListAllUsers,
    ListAllAdmins,
}

impl From<AdminRequest> for proto::requests::administration::AdminRequest {
    fn from(req: AdminRequest) -> Self {
        use proto::requests::administration as request;
        use proto::requests::administration::admin_request::Request;
        use AdminRequest::*;

        let inner = match req {
            Promote { user, permissions } => Request::PromoteUser(request::Promote {
                user: Some(user.into()),
                permissions_flags: permissions.bits,
            }),
            Demote(user) => Request::DemoteUser(request::Demote {
                user: Some(user.into()),
            }),
            Ban(user) => Request::BanUser(request::Ban {
                user: Some(user.into()),
            }),
            Unban(user) => Request::UnbanUser(request::Unban {
                user: Some(user.into()),
            }),
            Unlock(user) => Request::UnlockUser(request::Unlock {
                user: Some(user.into()),
            }),
            SearchUser { name } => Request::SearchUser(request::SearchUser { name }),
            ListAllUsers => Request::ListAllUsers(proto::types::None {}),
            ListAllAdmins => Request::ListAllAdmins(proto::types::None {}),
        };

        proto::requests::administration::AdminRequest {
            request: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::administration::AdminRequest> for AdminRequest {
    type Error = DeserializeError;

    fn try_from(
        req: proto::requests::administration::AdminRequest,
    ) -> Result<Self, DeserializeError> {
        use proto::requests::administration::admin_request::Request::*;

        let req = match req.request? {
            PromoteUser(promote) => AdminRequest::Promote {
                user: promote.user?.try_into()?,
                permissions: AdminPermissionFlags::from_bits_truncate(promote.permissions_flags),
            },
            DemoteUser(demote) => AdminRequest::Demote(demote.user?.try_into()?),
            BanUser(ban) => AdminRequest::Ban(ban.user?.try_into()?),
            UnbanUser(unban) => AdminRequest::Unban(unban.user?.try_into()?),
            UnlockUser(unlock) => AdminRequest::Unlock(unlock.user?.try_into()?),
            SearchUser(search) => AdminRequest::SearchUser { name: search.name },
            ListAllUsers(_) => AdminRequest::ListAllUsers,
            ListAllAdmins(_) => AdminRequest::ListAllAdmins,
        };

        Ok(req)
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AdminResponse {
    SearchedUsers(Vec<ServerUser>),
    Admins(Vec<Admin>),
}

impl From<AdminResponse> for proto::requests::administration::AdminResponse {
    fn from(res: AdminResponse) -> Self {
        use proto::requests::administration as request;
        use proto::requests::administration::admin_response::Response;
        use AdminResponse::*;

        let inner = match res {
            SearchedUsers(users) => {
                let users = users.into_iter().map(Into::into).collect();
                Response::SearchedUsers(request::SearchedUsers { users })
            }
            Admins(users) => {
                let admins = users.into_iter().map(Into::into).collect();
                Response::Admins(request::Admins { admins })
            }
        };

        proto::requests::administration::AdminResponse {
            response: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::administration::AdminResponse> for AdminResponse {
    type Error = DeserializeError;

    fn try_from(
        res: proto::requests::administration::AdminResponse,
    ) -> Result<Self, DeserializeError> {
        use proto::requests::administration::admin_response::Response::*;

        let res = match res.response? {
            SearchedUsers(results) => {
                let res: Result<_, _> = results.users.into_iter().map(TryInto::try_into).collect();
                let users: Vec<ServerUser> = res?;
                AdminResponse::SearchedUsers(users)
            }
            Admins(results) => {
                let res: Result<_, _> = results.admins.into_iter().map(TryInto::try_into).collect();
                let admins: Vec<Admin> = res?;
                AdminResponse::Admins(admins)
            }
        };

        Ok(res)
    }
}

#[derive(Debug, Clone)]
pub struct ServerUser {
    pub username: String,
    pub display_name: String,
    pub banned: bool,
    pub locked: bool,
    pub compromised: bool,
    pub latest_hash_scheme: bool,
    pub id: UserId,
}

impl From<ServerUser> for proto::requests::administration::ServerUser {
    fn from(user: ServerUser) -> Self {
        proto::requests::administration::ServerUser {
            username: user.username,
            display_name: user.display_name,
            banned: user.banned,
            locked: user.locked,
            compromised: user.compromised,
            latest_hash_scheme: user.latest_hash_scheme,
            id: Some(user.id.into()),
        }
    }
}

impl TryFrom<proto::requests::administration::ServerUser> for ServerUser {
    type Error = DeserializeError;

    fn try_from(
        user: proto::requests::administration::ServerUser
    ) -> Result<Self, DeserializeError> {
        Ok(ServerUser {
            username: user.username,
            display_name: user.display_name,
            banned: user.banned,
            locked: user.locked,
            compromised: user.compromised,
            latest_hash_scheme: user.latest_hash_scheme,
            id: user.id?.try_into()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Admin {
    pub username: String,
    pub id: UserId,
    pub permissions: AdminPermissionFlags,
}

impl From<Admin> for proto::requests::administration::Admin {
    fn from(admin: Admin) -> Self {
        proto::requests::administration::Admin {
            username: admin.username,
            id: Some(admin.id.into()),
            permissions_flags: admin.permissions.bits(),
        }
    }
}

impl TryFrom<proto::requests::administration::Admin> for Admin {
    type Error = DeserializeError;

    fn try_from(
        admin: proto::requests::administration::Admin
    ) -> Result<Self, DeserializeError> {
        Ok(Admin {
            username: admin.username,
            id: admin.id?.try_into()?,
            permissions: AdminPermissionFlags::from_bits_truncate(admin.permissions_flags),
        })
    }
}