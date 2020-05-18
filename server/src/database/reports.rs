use crate::database::{Database, DbResult, MessageRecord};
use chrono::{DateTime, Utc};
use futures::{Stream, TryStreamExt};
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::Row;
use vertex::prelude::*;

// Not much ON CASCADE DELETE here sometimes because we want reports to stick around...
// also, message *text*, not only ID is kept, so deletion can't be circumvented
pub(super) const CREATE_REPORTS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS reports (
        id             SERIAL PRIMARY KEY,
        datetime       TIMESTAMP WITH TIME ZONE NOT NULL,
        reported_user  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        reporter_user  UUID REFERENCES users(id) ON DELETE SET NULL,
        community      UUID REFERENCES communities(id) ON DELETE SET NULL,
        message_id     UUID REFERENCES messages(id) ON DELETE SET NULL,
        room           UUID REFERENCES rooms(id) ON DELETE SET NULL,
        message_text   VARCHAR NOT NULL,
        short_desc     VARCHAR NOT NULL,
        extended_desc  VARCHAR NOT NULL,
        status         "char" NOT NULL
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
            _ => Err(InvalidReportStatus),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReportRecord {
    pub id: u32,
    pub datetime: DateTime<Utc>,
    pub report: Report,
}

#[derive(Debug, Clone)]
pub struct Report {
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
            datetime: row.try_get("datetime")?,
            report: Report {
                reported_user: UserId(row.try_get("reported_user")?),
                reporter_user: row.try_get::<_, Option<_>>("reporter_user")?.map(UserId),
                community: row.try_get::<_, Option<_>>("community")?.map(CommunityId),
                message_id: row.try_get::<_, Option<_>>("message_id")?.map(MessageId),
                room: row.try_get::<_, Option<_>>("room")?.map(RoomId),
                message_text: row.try_get("message_text")?,
                short_desc: row.try_get("short_desc")?,
                extended_desc: row.try_get("extended_desc")?,
                status: status.try_into().unwrap_or(ReportStatus::Opened),
            },
        })
    }
}

pub enum ReportUserError {
    InvalidMessage,
    InvalidReporter,
}

macro_rules! queries {
    ($($name:ident($($argname:ident: $argty:ty$(,)?)*) -> { $sql:expr; $($argdef:expr$(,)?)* })*) => {
        $(pub async fn $name(
            &self,
            $($argname: $argty,)*
        ) -> DbResult<impl Stream<Item = DbResult<ReportRecord>>> {
            const QUERY: &str = $sql;

            let stream = self.query_stream(QUERY, &[$(&$argdef,)*]).await?;
            let stream = stream
                .and_then(|row| async move { Ok(ReportRecord::try_from(row)?) })
                .map_err(|e| e.into());

            Ok(stream)
        })*
    }
}

impl Database {
    pub async fn report_message(
        &self,
        reporter: UserId,
        msg: MessageRecord,
        short_desc: &str,
        extended_desc: &str,
    ) -> DbResult<Result<(), ReportUserError>> {
        const STMT: &str = "
            INSERT INTO reports
            (
                datetime, reported_user, reporter_user, community, message_id, room, message_text,
                short_desc, extended_desc, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

        let conn = self.pool.connection().await?;

        let stmt = conn.client.prepare(STMT).await?;
        let res = conn
            .client
            .execute(
                &stmt,
                &[
                    &msg.date,
                    &msg.author.0,
                    &reporter.0,
                    &msg.community.0,
                    &msg.id.0,
                    &msg.room.0,
                    &msg.content,
                    &short_desc,
                    &extended_desc,
                    &(ReportStatus::Opened as i8),
                ],
            )
            .await;

        match res {
            Ok(1) => Ok(Ok(())), // 1 row modified = successfully added
            Ok(_n) => {
                panic!("db error: report user query returned more than one row modified!");
            }
            Err(err) => {
                if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                    let constraint = err
                        .source()
                        .and_then(|e| e.downcast_ref::<DbError>())
                        .and_then(|e| e.constraint());

                    match constraint {
                        Some("reports_reporter_user_fkey")
                        | Some("reports_reported_user_fkey")
                        | Some("reports_message_id_fkey")
                        | Some("reports_community_fkey")
                        | Some("reports_room_fkey") => Ok(Err(ReportUserError::InvalidReporter)),
                        Some(_) | None => Err(err.into()),
                    }
                } else {
                    Err(err.into())
                }
            }
        }
    }

    queries! {
        get_reports_by_user(user: UserId) -> {
            "SELECT * from reports WHERE reporter_user = $1 ORDER BY id DESC";
            (Some(user.0))
        }

        get_reports_of_user(user: UserId) -> {
            "SELECT * from reports WHERE reported_user = $1 ORDER BY id DESC";
            (Some(user.0))
        }

        get_reports_in_community(community: CommunityId) -> {
            "SELECT * from reports WHERE community = $1 ORDER BY id DESC";
            (Some(community.0))
        }

        get_reports_in_room(room: RoomId, community: CommunityId) -> {
             "SELECT * from reports WHERE room = $1 AND community = $1 ORDER BY id DESC";
             Some(room.0), Some(community.0)
        }
    }
}
