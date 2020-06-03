use crate::database::{Database, DbResult, MessageRecord};
use chrono::{DateTime, Utc};
use futures::{Stream, TryStreamExt, StreamExt};
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::Row;
use vertex::prelude::*;
use vertex::requests::Report as VertexReport;

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
        msg_sent_at    TIMESTAMP WITH TIME ZONE NOT NULL,
        status         "char" NOT NULL
    )"#;

#[derive(Debug, Clone)]
pub struct ReportRecord {
    pub id: i32,
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

impl TryFrom<&Row> for ReportRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<ReportRecord, tokio_postgres::Error> {
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

macro_rules! build_where_clause {
    (let (mut $a:ident, $b:ident) = $criteria:ident: { $($field: ident $(as $ty:ty)? => $stmt:expr,)* }) => {
        let _casted: i8; // specific: there is one cast and it is to i8
        let (mut $a, $b) = {
            let mut _where_clause = String::new();
            let mut _cur_arg: usize = 0;
            let mut _args: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![];
            $(if let Some(ref $field) = $criteria.$field$(.map(|f| f as $ty))? {
                _cur_arg += 1;
                let join = if _cur_arg == 1 {
                    "WHERE"
                } else {
                    "AND"
                };

                _where_clause.push_str(
                    &format!("{} {}\n", join, format_args!($stmt, n = _cur_arg))
                );

                #[allow(unused_variables)]
                let push = $field;
                $(
                    _casted = (*$field) as $ty;
                    let push = &_casted;
                )?
                _args.push(push);
            })*

            (_args, _where_clause)
        };
    }
}

pub enum ReportUserError {
    InvalidMessage,
    InvalidReporter,
}

fn row_to_report(row: &Row) -> Result<VertexReport, tokio_postgres::Error> {
    let record: ReportRecord = row.try_into()?;
    let report = record.report;

    Ok(VertexReport {
        id: record.id,
        reporter: report.reporter_user.map(|id| Ok(ReportUser {
            id,
            username: row.try_get("reporter_username")?,
        })).transpose()?,
        reported: ReportUser {
            id: report.reported_user,
            username: row.try_get("reported_username")?,
        },
        message: ReportMessage {
            id: report.message_id,
            text: report.message_text,
            sent_at: row.try_get("msg_sent_at")?,
        },
        room: report.room.map(|id| Ok(ReportRoom {
            id,
            name: row.try_get("room_name")?
        })).transpose()?,
        community: report.community.map(|id| Ok(ReportCommunity {
            id,
            name: row.try_get("community_name")?
        })).transpose()?,
        datetime: record.datetime,
        short_desc: report.short_desc,
        extended_desc: report.extended_desc,
        status: report.status,
    })
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
                short_desc, extended_desc, msg_sent_at, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)";

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
                    &msg.date,
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

    pub async fn set_report_status(&self, id: i32, status: ReportStatus) -> DbResult<()> {
        const STMT: &str = "UPDATE reports SET status = $1 WHERE id = $2";
        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        conn.client.execute(&stmt, &[&(status as i8), &id]).await?;
        Ok(())
    }

    pub async fn search_reports(
        &self,
        criteria: SearchCriteria,
    ) -> DbResult<impl Stream<Item = DbResult<VertexReport>>> {
        const SELECT_QUERY: &str = "
            SELECT
                reports.id, reports.datetime, reports.message_text, reports.message_id,
                reports.short_desc, reports.extended_desc, reports.status, msg_sent_at,
                reports.reported_user, reported.username AS reported_username,
                reports.reporter_user, reporter.username AS reporter_username,
                reports.community, communities.name AS community_name,
                reports.room, rooms.name AS room_name
            FROM reports
            INNER JOIN users reported ON reports.reported_user = reported.id
            LEFT JOIN users reporter ON reports.reporter_user = reporter.id
            LEFT JOIN rooms ON reports.room = rooms.id
            LEFT JOIN communities ON reports.community = communities.id
            %where%
            %order%";

        build_where_clause! {
            let (mut args, where_clause) = criteria: {
                of_user => "reported.username = LOWER(${n})",
                by_user => "reporter.username = LOWER(${n})",
                before_date => "datetime < ${n}",
                after_date => "datetime > ${n}",
                in_community => "communities.name % ${n}",
                in_room => "rooms.name % ${n}",
                status as i8 => "status = ${n}",
            }
        };

        let trimmed = &criteria.words.trim();

        let query = SELECT_QUERY.replace("%where%", &where_clause);
        let order = if trimmed.len() == 0 {
            "ORDER BY reports.id DESC".to_string()
        } else {
            args.push(&trimmed);
            format!(
                "ORDER BY
                    SIMILARITY(${n}, reports.short_desc) + SIMILARITY(${n}, reports.extended_desc)
                DESC
                LIMIT 10",
                n = args.len()
            )
        };
        let query = query.replace("%order%", &order);
        dbg!(&query);
        let stream = self.query_stream(&query, &args).await?;
        let stream = stream
            .map(|row| Ok(row_to_report(&row?)?))
            .map_err(|e: tokio_postgres::Error| e.into());

        Ok(stream)
    }
}
