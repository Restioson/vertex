use crate::database::{Database, DbResult};
use futures::{Stream, TryStreamExt};
use std::convert::TryFrom;
use tokio_postgres::Row;
use uuid::Uuid;
use vertex::prelude::*;

pub(super) const CREATE_ROOMS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS rooms (
        id         UUID PRIMARY KEY,
        community  UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        name       VARCHAR NOT NULL
    )";
// TODO(sql): indexing

#[derive(Debug, Clone)]
pub struct RoomRecord {
    pub id: RoomId,
    pub community: CommunityId,
    pub name: String,
}

impl TryFrom<Row> for RoomRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<RoomRecord, tokio_postgres::Error> {
        Ok(RoomRecord {
            id: RoomId(row.try_get("id")?),
            community: CommunityId(row.try_get("community")?),
            name: row.try_get("name")?,
        })
    }
}

impl Database {
    pub async fn get_room(&self, id: RoomId) -> DbResult<Option<RoomRecord>> {
        let row = self
            .query_opt("SELECT * FROM rooms WHERE id=$1", &[&id.0])
            .await?;
        if let Some(row) = row {
            Ok(Some(RoomRecord::try_from(row)?))
        } else {
            Ok(None)
        }
    }

    pub async fn create_room(&self, community: CommunityId, name: String) -> DbResult<RoomId> {
        const STMT: &str = "INSERT INTO rooms (id, community, name) VALUES ($1, $2, $3)";
        let id = Uuid::new_v4();
        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        conn.client
            .execute(&stmt, &[&id, &community.0, &name])
            .await?;
        Ok(RoomId(id))
    }

    pub async fn get_rooms_in_community(
        &self,
        community: CommunityId,
    ) -> DbResult<impl Stream<Item = DbResult<RoomRecord>>> {
        const QUERY: &str = "SELECT * FROM rooms WHERE community = $1";

        let stream = self.query_stream(QUERY, &[&community.0]).await?;
        let stream = stream
            .and_then(|row| async move { RoomRecord::try_from(row) })
            .map_err(|e| e.into());
        Ok(stream)
    }
}
