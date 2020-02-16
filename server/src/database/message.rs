use std::convert::TryFrom;

use chrono::{DateTime, Utc};
use futures::{Stream, TryStream, TryStreamExt};
use tokio_postgres::Row;

use vertex::{CommunityId, HistoricMessage, MessageId, MessageSelector, ProfileVersion, RoomId, UserId};

use crate::database::{Database, DatabaseError, DbResult};

/// Max messages the server will return at one time
const SERVER_MAX: usize = 50;

#[derive(Debug, Copy, Clone)]
pub struct InvalidSelector;

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

        let row = self.query_one(QUERY, &[
            &id.0,
            &author.0,
            &community.0,
            &room.0,
            &date,
            &Some(content),
        ]).await?;

        let ord = MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64);
        let profile_version = ProfileVersion(row.try_get::<&str, i32>("profile_version")? as u32);

        Ok((ord, profile_version))
    }

    pub async fn get_newest_message(&self, community: CommunityId, room: RoomId) -> DbResult<Option<MessageId>> {
        const QUERY: &str = "
            WITH last_message(ord) AS (
                SELECT COALESCE(
                    (SELECT MAX(ord) FROM messages WHERE messages.community = $1 AND messages.room = $2),
                    0::BIGINT
                )
            )
            SELECT id FROM messages, last_message WHERE messages.ord = last_message.ord
        ";

        match self.query_opt(QUERY, &[&community.0, &room.0]).await? {
            Some(row) => Ok(Some(MessageId(row.try_get("id")?))),
            None => Ok(None),
        }
    }

    async fn get_message_ord(&self, id: MessageId) -> DbResult<Option<MessageOrdinal>> {
        const QUERY: &str = "SELECT ord FROM messages WHERE id = $1";
        match self.query_opt(QUERY, &[&id.0]).await? {
            Some(row) => Ok(Some(MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64))),
            None => Ok(None),
        }
    }

    pub async fn read_messages(
        &self,
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
    ) -> DbResult<Result<impl Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>, InvalidSelector>> {
        // TODO: query duplication?
        let stream = match selector {
            MessageSelector::Before { message, count } => {
                let message = match self.get_message_ord(message).await? {
                    Some(message) => message,
                    None => return Ok(Err(InvalidSelector)),
                };

                let query = "
                    SELECT messages.*, users.profile_version FROM messages
                    INNER JOIN users ON messages.author = users.id
                        WHERE messages.community = $1 AND messages.room = $2
                        AND messages.ord <= $3
                        ORDER BY ord DESC
                        LIMIT $4
                ";
                self.query_stream(query, &[
                    &community.0,
                    &room.0,
                    &(message.0 as i64),
                    &(count.min(SERVER_MAX) as i64),
                ]).await?
            },
            MessageSelector::After { message, count } => {
                let message = match self.get_message_ord(message).await? {
                    Some(message) => message,
                    None => return Ok(Err(InvalidSelector)),
                };

                let query = "
                    SELECT messages.*, users.profile_version FROM messages
                    INNER JOIN users ON messages.author = users.id
                        WHERE messages.community = $1 AND messages.room = $2
                        AND messages.ord > $3
                        ORDER BY ord DESC
                        LIMIT $4
                ";
                self.query_stream(query, &[
                    &community.0,
                    &room.0,
                    &(message.0 as i64),
                    &(count.min(SERVER_MAX) as i64),
                ]).await?
            },
            MessageSelector::UpTo { from, up_to, count } => {
                let from = self.get_message_ord(from).await?;
                let up_to = self.get_message_ord(up_to).await?;
                let (from, up_to) = match (from, up_to) {
                    (Some(from), Some(up_to)) => (from, up_to),
                    _ => return Ok(Err(InvalidSelector)),
                };

                let query = "
                    SELECT messages.*, users.profile_version FROM messages
                    INNER JOIN users ON messages.author = users.id
                        WHERE community = $1 AND room = $2
                        AND ord <= $3 && ord > $4
                        ORDER BY ord DESC
                        LIMIT $5
                ";
                self.query_stream(query, &[
                    &community.0,
                    &room.0,
                    &(from.0 as i64),
                    &(up_to.0 as i64),
                    &(count.min(SERVER_MAX) as i64),
                ]).await?
            },
        };

        let stream = stream
            .and_then(|row| async move {
                let profile_version = row.try_get::<&str, i32>("profile_version")?;
                Ok((
                    ProfileVersion(profile_version as u32),
                    MessageRecord::try_from(row)?,
                ))
            })
            .map_err(|e| e.into());

        Ok(Ok(stream))
    }
}

pub trait MessageStreamExt: Stream<Item = DbResult<(ProfileVersion, MessageRecord)>> {
    type Historic: Stream<Item = DbResult<HistoricMessage>> + Sized;

    fn map_historic_messages(self) -> Self::Historic
        where
            Self: Sized;
}

impl<S> MessageStreamExt for S
where
    S: Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>,
    S: TryStream<Ok = (ProfileVersion, MessageRecord), Error = DatabaseError>,
{
    type Historic = impl Stream<Item = DbResult<HistoricMessage>> + Sized;

    fn map_historic_messages(self) -> Self::Historic
        where
            Self: Sized,
    {
        self.try_filter_map(|(profile_version, record)| async move {
            match record.content {
                Some(content) => Ok(Some(HistoricMessage {
                    id: record.id,
                    author: record.author,
                    author_profile_version: profile_version,
                    content,
                })),
                None => Ok(None),
            }
        })
    }
}
