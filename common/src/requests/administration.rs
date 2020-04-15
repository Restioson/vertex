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
        };

        Ok(req)
    }
}