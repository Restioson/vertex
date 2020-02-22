use std::convert::TryFrom;

use chrono::{DateTime, Utc};
use futures::{Stream, TryStream, TryStreamExt};
use tokio_postgres::Row;

use vertex::{Bound, CommunityId, Message, MessageId, MessageSelector, ProfileVersion, RoomId, UserId};

use crate::database::{Database, DatabaseError, DbResult};

/// Max messages the server will return at one time
const SERVER_MAX: usize = 50;

#[derive(Debug, Copy, Clone)]
pub struct InvalidSelector;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
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

        let row = self
            .query_one(
                QUERY,
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

        let ord = MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64);
        let profile_version = ProfileVersion(row.try_get::<&str, i32>("profile_version")? as u32);

        Ok((ord, profile_version))
    }

    pub async fn get_newest_message(
        &self,
        community: CommunityId,
        room: RoomId,
    ) -> DbResult<Option<MessageId>> {
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

    pub async fn get_message_id(&self, ord: MessageOrdinal) -> DbResult<Option<MessageId>> {
        const QUERY: &str = "SELECT id FROM messages WHERE ord = $1";
        match self.query_opt(QUERY, &[&(ord.0 as i64)]).await? {
            Some(row) => Ok(Some(
                MessageId(row.try_get("id")?),
            )),
            None => Ok(None),
        }
    }

    pub async fn get_message_ord(&self, id: MessageId) -> DbResult<Option<MessageOrdinal>> {
        const QUERY: &str = "SELECT ord FROM messages WHERE id = $1";
        match self.query_opt(QUERY, &[&id.0]).await? {
            Some(row) => Ok(Some(
                MessageOrdinal(row.try_get::<&str, i64>("ord")? as u64),
            )),
            None => Ok(None),
        }
    }

    pub async fn get_messages(
        &self,
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
        count: usize,
    ) -> DbResult<
        Result<impl Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>, InvalidSelector>,
    > {
        let bound = match selector {
            MessageSelector::Before(bound) => bound,
            MessageSelector::After(bound) => bound,
        };

        let bound_message = match self.get_message_ord(*bound.get()).await? {
            Some(message) => message,
            None => return Ok(Err(InvalidSelector)),
        };

        let comparator = match selector {
            MessageSelector::Before(_) => "<",
            MessageSelector::After(_) => ">",
        };

        let comparator = match bound {
            Bound::Inclusive(_) => format!("{}=", comparator),
            _ => comparator.to_owned(),
        };

        let query = format!(
            "SELECT messages.*, users.profile_version FROM messages
            INNER JOIN users ON messages.author = users.id
                WHERE messages.community = $1 AND messages.room = $2
                AND messages.ord {} $4
                ORDER BY ord DESC
                LIMIT $3",
            comparator
        );

        let stream = self.query_stream(
            &query,
            &[
                &community.0,
                &room.0,
                &(count.min(SERVER_MAX) as i64),
                &(bound_message.0 as i64),
            ],
        ).await?;

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
    type Output: Stream<Item = DbResult<Message>> + Sized;

    fn map_messages(self) -> Self::Output
    where
        Self: Sized;
}

impl<S> MessageStreamExt for S
where
    S: Stream<Item = DbResult<(ProfileVersion, MessageRecord)>>,
    S: TryStream<Ok = (ProfileVersion, MessageRecord), Error = DatabaseError>,
{
    type Output = impl Stream<Item = DbResult<Message>> + Sized;

    fn map_messages(self) -> Self::Output
    where
        Self: Sized,
    {
        self.try_filter_map(|(profile_version, record)| async move {
            match record.content {
                Some(content) => Ok(Some(Message {
                    id: record.id,
                    author: record.author,
                    author_profile_version: profile_version,
                    sent: record.date,
                    content,
                })),
                None => Ok(None),
            }
        })
    }
}
