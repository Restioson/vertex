use bitflags::bitflags;
use crate::proto;
use crate::types::UserId;
use std::convert::{TryFrom, TryInto};
use crate::proto::DeserializeError;

bitflags! {
    pub struct AdminPermissionFlags: i64 {
        /// All permissions. Could be used for the server owner.
        const ALL = 1;
        /// Ban users.
        const BAN = 1 << 1;
        /// Demote users.
        const DEMOTE = 1 << 2;
        /// Promote users.
        const PROMOTE = 1 << 3;
        /// Is an admin at all. Allows for searching users and viewing reports. Any other permission
        /// automatically grants this one - it is just used as a placeholder in case of not granting
        /// any other admin permissions.
        const IS_ADMIN = 1 << 4;
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
    SearchUser {
        name: String,
    }
}

impl From<AdminRequest> for proto::requests::administration::AdminRequest {
    fn from(req: AdminRequest) -> Self {
        use AdminRequest::*;
        use proto::requests::administration::admin_request::Request;
        use proto::requests::administration as request;

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
            SearchUser { name } => Request::SearchUser(request::SearchUser { name }),
        };

        proto::requests::administration::AdminRequest {
            request: Some(inner),
        }
    }
}

impl TryFrom<proto::requests::administration::AdminRequest> for AdminRequest {
    type Error = DeserializeError;

    fn try_from(
        req: proto::requests::administration::AdminRequest
    ) -> Result<Self, DeserializeError> {
        use proto::requests::administration::admin_request::Request::*;

        let req = match req.request? {
            PromoteUser(promote) => {
                AdminRequest::Promote {
                    user: promote.user?.try_into()?,
                    permissions: AdminPermissionFlags::from_bits_truncate(promote.permissions_flags),
                }
            },
            DemoteUser(demote) => AdminRequest::Demote(demote.user?.try_into()?),
            BanUser(ban) => AdminRequest::Ban(ban.user?.try_into()?),
            SearchUser(search) => AdminRequest::SearchUser { name: search.name }
        };

        Ok(req)
    }
}


#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AdminResponse {
    SearchedUsers(Vec<ServerUser>)
}

impl From<AdminResponse> for proto::requests::administration::AdminResponse {
    fn from(res: AdminResponse) -> Self {
        use AdminResponse::*;
        use proto::requests::administration::admin_response::Response;
        use proto::requests::administration as request;

        let inner = match res {
            SearchedUsers(users) => {
                let users = users.into_iter().map(Into::into).collect();
                Response::SearchedUsers(request::SearchedUsers { users })
            }
        };

        proto::requests::administration::AdminResponse { response: Some(inner) }
    }
}

impl TryFrom<proto::requests::administration::AdminResponse> for AdminResponse {
    type Error = DeserializeError;

    fn try_from(res: proto::requests::administration::AdminResponse) -> Result<Self, DeserializeError> {
        use proto::requests::administration::admin_response::Response::*;

        let res = match res.response? {
            SearchedUsers(results) => {
                let users = results.users.into_iter().map(Into::into).collect();
                AdminResponse::SearchedUsers(users)
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
        }
    }
}

impl From<proto::requests::administration::ServerUser> for ServerUser {
    fn from(user: proto::requests::administration::ServerUser) -> Self {
        ServerUser {
            username: user.username,
            display_name: user.display_name,
            banned: user.banned,
            locked: user.locked,
            compromised: user.compromised,
            latest_hash_scheme: user.latest_hash_scheme,
        }
    }
}
