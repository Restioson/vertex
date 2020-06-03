use crate::proto;
use crate::proto::DeserializeError;
use crate::types::*;
use bitflags::bitflags;
use std::convert::{TryFrom, TryInto};
use chrono::{DateTime, Utc, NaiveDateTime, TimeZone};
use std::fmt;

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
    SearchForReports(SearchCriteria),
    SetReportStatus {
        id: i32,
        status: ReportStatus,
    },
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
            SearchForReports(criteria) => Request::SearchForReports(criteria.into()),
            SetReportStatus { id, status } => Request::SetReportStatus(request::SetReportStatus {
                id,
                status: status as i8 as u32,
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
            SearchForReports(criteria) => AdminRequest::SearchForReports(criteria.try_into()?),
            SetReportStatus(set) => AdminRequest::SetReportStatus {
                id: set.id,
                status: i8::try_from(set.status)?.try_into()?,
            }
        };

        Ok(req)
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AdminResponse {
    SearchedUsers(Vec<ServerUser>),
    Admins(Vec<Admin>),
    Reports(Vec<Report>),
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
            Reports(reports) => {
                let reports = reports.into_iter().map(Into::into).collect();
                Response::Reports(request::Reports { reports })
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
            Reports(reports) => {
                let res: Result<_, _> = reports.reports.into_iter().map(TryInto::try_into).collect();
                let admins: Vec<Report> = res?;
                AdminResponse::Reports(admins)
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

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(i8)]
pub enum ReportStatus {
    Opened = 0,
    Accepted = 1,
    Denied = 2,
}

pub struct InvalidReportStatus;

impl From<InvalidReportStatus> for DeserializeError {
    fn from(_: InvalidReportStatus) -> DeserializeError {
        DeserializeError::InvalidEnumVariant
    }
}

impl TryFrom<i8> for ReportStatus {
    type Error = InvalidReportStatus;

    fn try_from(c: i8) -> Result<Self, Self::Error> {
        match c {
            0 => Ok(ReportStatus::Opened),
            1 => Ok(ReportStatus::Accepted),
            2 => Ok(ReportStatus::Denied),
            _ => Err(InvalidReportStatus),
        }
    }
}

impl fmt::Display for ReportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let desc = match self {
            ReportStatus::Opened => "open",
            ReportStatus::Accepted => "accepted",
            ReportStatus::Denied => "denied",
        };

        f.write_str(desc)
    }
}

#[derive(Debug, Clone)]
pub struct ReportUser {
    pub id: UserId,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct ReportMessage {
    pub id: Option<MessageId>,
    pub sent_at: DateTime<Utc>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ReportRoom {
    pub id: RoomId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct ReportCommunity {
    pub id: CommunityId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub id: i32,
    pub reporter: Option<ReportUser>,
    pub reported: ReportUser,
    pub message: ReportMessage,
    pub room: Option<ReportRoom>,
    pub community: Option<ReportCommunity>,
    pub datetime: DateTime<Utc>,
    pub short_desc: String,
    pub extended_desc: String,
    pub status: ReportStatus,
}

impl PartialEq<Report> for Report {
    fn eq(&self, other: &Report) -> bool {
        self.id == other.id
    }
}

impl From<Report> for proto::requests::administration::Report {
    fn from(report: Report) -> Self {
        use proto::requests::administration as proto;
        proto::Report {
            id: report.id,
            reporter: report.reporter.map(|reporter| {
                proto::ReportUser {
                    id: Some(reporter.id.into()),
                    username: reporter.username,
                }
            }),
            reported: Some(proto::ReportUser {
                id: Some(report.reported.id.into()),
                username: report.reported.username,
            }),
            message: Some(proto::ReportMessage {
                id: report.message.id.map(Into::into),
                text: report.message.text,
                sent_at: report.message.sent_at.timestamp(),
            }),
            room: report.room.map(|room| {
                proto::ReportRoom {
                    id: Some(room.id.into()),
                    name: room.name,
                }
            }),
            community: report.community.map(|community| {
                proto::ReportCommunity {
                    id: Some(community.id.into()),
                    name: community.name,
                }
            }),
            datetime: report.datetime.timestamp(),
            short_desc: report.short_desc,
            extended_desc: report.extended_desc,
            status: report.status as i8 as u32,
        }
    }
}

impl TryFrom<proto::requests::administration::Report> for Report {
    type Error = DeserializeError;

    fn try_from(
        report: proto::requests::administration::Report
    ) -> Result<Self, DeserializeError> {
        let dt = &NaiveDateTime::from_timestamp(report.datetime, 0);
        let sent_at = &NaiveDateTime::from_timestamp(report.message.clone()?.sent_at, 0);
        Ok(Report {
            id: report.id,
            reporter: report.reporter.map::<Result<_, DeserializeError>, _>(|reporter| {
                Ok(ReportUser {
                    id: reporter.id?.try_into()?,
                    username: reporter.username,
                })
            }).transpose()?,
            reported: ReportUser {
                id: report.reported.clone()?.id?.try_into()?,
                username: report.reported?.username,
            },
            message: ReportMessage {
                id: report.message.clone()?.id.map(TryInto::try_into).transpose()?,
                text: report.message?.text,
                sent_at: Utc.from_utc_datetime(&sent_at),
            },
            room: report.room.map::<Result<_, DeserializeError>, _>(|room| {
                Ok(ReportRoom {
                    id: room.id?.try_into()?,
                    name: room.name,
                })
            }).transpose()?,
            community: report.community.map::<Result<_, DeserializeError>, _>(|community| {
                Ok(ReportCommunity {
                    id: community.id?.try_into()?,
                    name: community.name,
                })
            }).transpose()?,
            datetime: Utc.from_utc_datetime(&dt),
            short_desc: report.short_desc,
            extended_desc: report.extended_desc,
            status: ReportStatus::try_from(i8::try_from(report.status)?)?,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchCriteria {
    pub words: String,
    pub of_user: Option<String>,
    pub by_user: Option<String>,
    pub before_date: Option<DateTime<Utc>>,
    pub after_date: Option<DateTime<Utc>>,
    pub in_community: Option<String>,
    pub in_room: Option<String>,
    pub status: Option<ReportStatus>,
}

impl TryFrom<proto::requests::administration::SearchCriteria> for SearchCriteria {
    type Error = DeserializeError;

    fn try_from(
        c: proto::requests::administration::SearchCriteria
    ) -> Result<SearchCriteria, DeserializeError> {
        use proto::requests::administration::search_criteria::{
            OfUser, ByUser, BeforeDate, AfterDate, InCommunity, InRoom, Status
        };

        Ok(SearchCriteria {
            words: c.words,
            of_user: c.of_user.map(|OfUser::OfUserPresent(x)| x),
            by_user: c.by_user.map(|ByUser::ByUserPresent(x)| x),
            before_date: c.before_date.map(|BeforeDate::BeforeTimestamp(x)| {
                let dt = &NaiveDateTime::from_timestamp(x, 0);
                Utc.from_utc_datetime(dt)
            }),
            after_date: c.after_date.map(|AfterDate::AfterTimestamp(x)| {
                let dt = &NaiveDateTime::from_timestamp(x, 0);
                Utc.from_utc_datetime(dt)
            }),
            in_community: c.in_community.map(|InCommunity::InCommunityPresent(x)| x),
            in_room: c.in_room.map(|InRoom::InRoomPresent(x)| x),
            status: c.status.map::<Result<_, DeserializeError>, _>(|Status::StatusCode(x)| {
                Ok(ReportStatus::try_from(i8::try_from(x)?)?)
            }).transpose()?
        })
    }
}

impl From<SearchCriteria> for proto::requests::administration::SearchCriteria {
    fn from(c: SearchCriteria) -> Self {
        use proto::requests::administration::search_criteria::{
            OfUser, ByUser, BeforeDate, AfterDate, InCommunity, InRoom, Status
        };

        proto::requests::administration::SearchCriteria {
            words: c.words,
            of_user: c.of_user.map(OfUser::OfUserPresent),
            by_user: c.by_user.map(ByUser::ByUserPresent),
            before_date: c.before_date.map(|x| BeforeDate::BeforeTimestamp(x.timestamp())),
            after_date: c.after_date.map(|x| AfterDate::AfterTimestamp(x.timestamp())),
            in_community: c.in_community.map(InCommunity::InCommunityPresent),
            in_room: c.in_room.map(InRoom::InRoomPresent),
            status: c.status.map(|x| Status::StatusCode(x as i8 as u32))
        }
    }
}
