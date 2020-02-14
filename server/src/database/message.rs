use vertex::{MessageId, CommunityId, UserId, RoomId};
use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use std::convert::TryFrom;
use crate::database::{Database, DbResult};

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
    ) -> DbResult<MessageOrdinal> {
        const QUERY: &str = "
            INSERT INTO messages (id, author, community, room, date, content)
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING ord
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let opt = conn.client.query_opt(
            &query,
            &[&id.0, &author.0, &community.0, &room.0, &date, &Some(content)]
        ).await?;

        Ok(MessageOrdinal(opt.unwrap().try_get::<&str, i64>("ord")? as u64))
    }
}
