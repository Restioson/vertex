use crate::database::{Database, DatabaseError, DbResult};
use chrono::{DateTime, Utc};
use futures::{Stream, TryStream, TryStreamExt};
use std::cmp;
use std::convert::TryFrom;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;
use vertex::{CommunityId, ForwardedMessage, MessageId, ProfileVersion, RoomId, UserId};

/// Max messages the server will return at one time
const SERVER_MAX: usize = 50;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct MessageOrdinal(pub u64);

pub(super) const CREATE_MESSAGES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS messages (
        id          UUID PRIMARY KEY,
        ord         BIGSERIAL,
        author      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        community   UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        room        UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
        date        TIMESTAMP WITH TIME ZONE NOT NULL,
        content     VARCHAR
    )
    ";

#[derive(Debug)]
pub struct MessageRecord {
    pub id: MessageId,
    pub ord: MessageOrdinal,
    pub author: UserId,
    pub community: CommunityId,
    pub room: RoomId,
    pub date: DateTime<Utc>,
    pub content: Option<String>,
}

impl TryFrom<Row> for MessageRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<MessageRecord, tokio_postgres::Error> {
        Ok(MessageRecord {
            id: MessageId(row.try_get("id")?),
            ord: MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64),
            author: UserId(row.try_get("author")?),
            community: CommunityId(row.try_get("community")?),
            room: RoomId(row.try_get("room")?),
            date: row.try_get("date")?,
            content: row.try_get("content")?,
        })
    }
}

impl Database {
    pub async fn create_message(
        &self,
        id: MessageId,
        author: UserId,
        community: CommunityId,
        room: RoomId,
        date: DateTime<Utc>,
        content: String,
    ) -> DbResult<(MessageOrdinal, ProfileVersion)> {
        const QUERY: &str = "
            WITH inserted AS
                (INSERT INTO messages (id, author, community, room, date, content)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    RETURNING ord, author
                )
            SELECT inserted.ord, users.profile_version FROM inserted
            INNER JOIN users ON inserted.author = users.id
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let opt = conn
            .client
            .query_opt(
                &query,
                &[
                    &id.0,
                    &author.0,
                    &community.0,
                    &room.0,
                    &date,
                    &Some(content),
                ],
            )
            .await?;

        let row = opt.unwrap();
        let ord = MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64);
        let profile_version = ProfileVersion(row.try_get::<&str, i32>("profile_version")? as u32);

        Ok((ord, profile_version))
    }

    pub async fn get_new_messages(
        &self,
        user: UserId,
        community: CommunityId,
        room: RoomId,
        client_max: usize,
    ) -> DbResult<impl Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>> {
        const QUERY: &str = "
            WITH last_read_tbl AS (
                COALESCE(
                    SELECT last_read FROM user_room_states WHERE user_id = $3,
                     0::BIGINT
                 )
            )
            SELECT messages.*, users.profile_version FROM messages
            INNER JOIN users ON messages.author = users.id
                WHERE community = $1 AND room = $2 AND ord > last_read_tbl.last_read
                LIMIT $3
                ORDER BY ord DESC
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &community.0,
            &room.0,
            &user.0,
            &(cmp::min(SERVER_MAX, client_max) as i64),
        ];

        let stream = conn.client.query_raw(&query, slice_iter(args)).await?;
        let stream = stream
            .and_then(|row| async move {
                let profile_version = row.try_get::<&str, i32>("profile_version")?;
                Ok((
                    ProfileVersion(profile_version as u32),
                    MessageRecord::try_from(row)?,
                ))
            })
            .map_err(|e| e.into());

        Ok(stream)
    }

    pub async fn get_messages_before_base(
        &self,
        community: CommunityId,
        room: RoomId,
        base: MessageId,
        client_max: usize,
    ) -> DbResult<impl Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>> {
        const QUERY: &str = "
            WITH base_ord AS (
                COALESCE(
                    SELECT ord FROM messages WHERE id = $3,
                    0::BIGINT,
                )
            )
            SELECT messages.*, users.profile_version FROM messages
            INNER JOIN users ON messages.author = users.id
                WHERE community = $1 AND room = $2 AND ord < base_ord.ord
                LIMIT $3
                ORDER BY ord DESC
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &community.0,
            &room.0,
            &base.0,
            &(cmp::min(SERVER_MAX, client_max) as i64),
        ];

        let stream = conn.client.query_raw(&query, slice_iter(args)).await?;
        let stream = stream
            .and_then(|row| async move {
                let profile_version = row.try_get::<&str, i32>("profile_version")?;
                Ok((
                    ProfileVersion(profile_version as u32),
                    MessageRecord::try_from(row)?,
                ))
            })
            .map_err(|e| e.into());

        Ok(stream)
    }
}

pub trait MessageStreamExt: Stream<Item = DbResult<(ProfileVersion, MessageRecord)>> {
    type Out: Stream<Item = DbResult<ForwardedMessage>> + Sized;

    fn map_forwarded_messages(self) -> Self::Out
    where
        Self: Sized;
}

impl<S> MessageStreamExt for S
where
    S: Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>,
    S: TryStream<Ok = (ProfileVersion, MessageRecord), Error = DatabaseError>,
{
    type Out = impl Stream<Item = DbResult<ForwardedMessage>> + Sized;

    fn map_forwarded_messages(self) -> Self::Out
    where
        Self: Sized,
    {
        self.try_filter_map(|(profile_version, record)| async move {
            match record.content {
                Some(content) => Ok(Some(ForwardedMessage {
                    id: record.id,
                    community: record.community,
                    room: record.room,
                    author: record.author,
                    author_profile_version: profile_version,
                    content,
                })),
                None => Ok(None),
            }
        })
    }
}

/// Taken from tokio_postgres
fn slice_iter<'a>(
    s: &'a [&'a (dyn ToSql + Sync)],
) -> impl ExactSizeIterator<Item = &'a dyn ToSql> + 'a {
    s.iter().map(|s| *s as _)
}
