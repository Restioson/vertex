use vertex::prelude::*;
use tokio_postgres::Row;
use std::convert::{TryFrom, TryInto};

// No ON CASCADE DELETE here sometimes because we want reports to stick around...
// also, message *text*, not only ID is kept, so deletion can't be circumvented
pub(super) const CREATE_REPORTS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS reports (
        id             SERIAL PRIMARY KEY,
        reported_user  UUID NOT NULL REFERENCES users(id) ON DELETE CASCDE,
        reporter_user  UUID REFERENCES users(id) ON DELETE SET NULL,
        community      UUID REFERENCES communities(id) ON DELETE SET NULL,
        message_id     UUID REFERENCES messages(id) ON DELETE SET NULL,
        room           UUID REFERENCES rooms(id) ON DELETE SET NULL,
        message_text   VARCHAR NOT NULL,
        short_desc     VARCHAR NOT NULL,
        extended_desc  VARCHAR NOT NULL,
        status         "char" NOT NULL,
    )"#;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(i8)]
pub enum ReportStatus {
    Opened = 0,
    Accepted = 1,
    Denied = 2,
}

pub struct InvalidReportStatus;

impl TryFrom<i8> for ReportStatus {
    type Error = InvalidReportStatus;

    fn try_from(c: i8) -> Result<Self, Self::Error> {
        match c {
            0 => Ok(ReportStatus::Opened),
            1 => Ok(ReportStatus::Accepted),
            2 => Ok(ReportStatus::Denied),
            _ => Err(InvalidReportStatus)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReportRecord {
    pub id: u32,
    pub reported_user: UserId,
    pub reporter_user: Option<UserId>,
    pub community: Option<CommunityId>,
    pub room: Option<RoomId>,
    pub message_id: Option<MessageId>,
    pub message_text: String,
    pub short_desc: String,
    pub extended_desc: String,
    pub status: ReportStatus,
}

impl TryFrom<Row> for ReportRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<ReportRecord, tokio_postgres::Error> {
        let status: i8 = row.try_get("status")?;
        Ok(ReportRecord {
            id: row.try_get("id")?,
            reported_user: UserId(row.try_get("reported_user")?),
            reporter_user: row.try_get::<_, Option<_>>("reporter_user")?.map(UserId),
            community: row.try_get::<_, Option<_>>("community")?.map(CommunityId),
            message_id: row.try_get::<_, Option<_>>("message_id")?.map(MessageId),
            room: row.try_get::<_, Option<_>>("room")?.map(RoomId),
            message_text: row.try_get("message_text")?,
            short_desc: row.try_get("short_desc")?,
            extended_desc: row.try_get("extended_desc")?,
            status: status.try_into().unwrap_or(ReportStatus::Opened),
        })
    }
}
