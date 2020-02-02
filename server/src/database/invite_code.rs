use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};
use chrono::{DateTime, Utc};
use rand::Rng;
use tokio_postgres::types::ToSql;

use vertex::{CommunityId, InviteCode};

use crate::database::{Database, DbResult};

pub(super) const CREATE_INVITE_CODES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS invite_codes (
        id BIGINT NOT NULL,
        community_id UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        expiration_date TIMESTAMP WITH TIME ZONE,

        PRIMARY KEY (id)
    )";

#[derive(Copy, Clone, Debug)]
pub struct MalformedInviteCode;

#[derive(Debug)]
pub struct InviteCodeRecord {
    pub id: i64,
    pub expiration_date: Option<DateTime<Utc>>,
}

impl InviteCodeRecord {
    fn parse_id(code: InviteCode) -> Result<i64, MalformedInviteCode> {
        let bytes = base64::decode_config(&code.0, base64::URL_SAFE_NO_PAD)
            .map_err(|_| MalformedInviteCode)?;

        Cursor::new(bytes).read_i64::<LittleEndian>().map_err(|_| MalformedInviteCode)
    }
}

impl Into<String> for InviteCodeRecord {
    fn into(self) -> String {
        base64::encode_config(&self.id.to_le_bytes(), base64::URL_SAFE_NO_PAD)
    }
}

impl Database {
    pub async fn create_invite_code(
        &self,
        community: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    ) -> DbResult<InviteCode> {
        const QUERY: &str = "
            INSERT INTO invite_codes (id, community_id, expiration_date) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;

        let id = loop {
            let id = rand::thread_rng().gen::<i64>();
            let args: &[&(dyn ToSql + Sync)] = &[&id, &community.0, &expiration_date];

            let rows_inserted = conn.client.execute(&query, args).await.unwrap();

            // if we successfully inserted the id, it must be unique: return it
            if rows_inserted == 1 {
                break id;
            }
        };

        let record = InviteCodeRecord { id, expiration_date };
        Ok(InviteCode(record.into()))
    }

    pub async fn get_community_from_invite_code(
        &self,
        code: InviteCode,
    ) -> DbResult<Result<Option<CommunityId>, MalformedInviteCode>> {
        const QUERY: &str = "
            SELECT community_id FROM invite_codes WHERE id=$1
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;

        let id = match InviteCodeRecord::parse_id(code) {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };
        let args: &[&(dyn ToSql + Sync)] = &[&id];

        let row_opt = conn.client.query_opt(&query, args).await?;
        if let Some(row) = row_opt {
            Ok(Ok(Some(CommunityId(row.try_get("community_id")?))))
        } else {
            Ok(Ok(None))
        }
    }
}
