use crate::database::{Database, DbResult};
use std::convert::TryFrom;
use tokio_postgres::Row;
use uuid::Uuid;
use vertex::CommunityId;
use futures::{Stream, TryStreamExt};
use tokio_postgres::types::ToSql;

pub(super) const CREATE_COMMUNITIES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS communities (
        id   UUID PRIMARY KEY,
        name VARCHAR NOT NULL
    )";

#[derive(Debug, Clone)]
pub struct CommunityRecord {
    pub id: CommunityId,
    pub name: String,
}

impl TryFrom<Row> for CommunityRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<CommunityRecord, tokio_postgres::Error> {
        Ok(CommunityRecord {
            id: CommunityId(row.try_get("id")?),
            name: row.try_get("name")?,
        })
    }
}

impl Database {
    // TODO(room_persistence): load at boot
    pub async fn get_community_metadata(
        &self,
        id: CommunityId,
    ) -> DbResult<Option<CommunityRecord>> {
        let conn = self.pool.connection().await?;
        let query = conn
            .client
            .prepare("SELECT * FROM communities WHERE id=$1")
            .await?;
        let opt = conn.client.query_opt(&query, &[&id.0]).await?;

        if let Some(row) = opt {
            Ok(Some(CommunityRecord::try_from(row)?))
        } else {
            Ok(None)
        }
    }

    pub async fn create_community(&self, name: String) -> DbResult<CommunityId> {
        const STMT: &str = "INSERT INTO communities (id, name) VALUES ($1, $2)";
        let id = Uuid::new_v4();
        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        conn.client.execute(&stmt, &[&id, &name]).await?;
        Ok(CommunityId(id))
    }

    pub async fn get_all_communities(&self) -> DbResult<impl Stream<Item = DbResult<CommunityRecord>>> {
        const QUERY: &str = "SELECT * FROM communities";
        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        // I hate
        let args = ([]: [u32; 0]).iter().map(|x| x as &dyn ToSql);

        let stream = conn.client.query_raw(&query, args).await?;
        let stream = stream
            .and_then(|row| async move { CommunityRecord::try_from(row) })
            .map_err(|e| e.into());
        Ok(stream)
    }
}
