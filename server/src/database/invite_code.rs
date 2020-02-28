use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};
use chrono::{DateTime, Utc};
use rand::Rng;
use tokio_postgres::types::ToSql;
use tokio_postgres::IsolationLevel;

use vertex::prelude::*;

use crate::database::{Database, DbResult};

pub(super) const CREATE_INVITE_CODES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS invite_codes (
        id BIGINT PRIMARY KEY,
        community UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        expiration_date TIMESTAMP WITH TIME ZONE
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

        Cursor::new(bytes)
            .read_i64::<LittleEndian>()
            .map_err(|_| MalformedInviteCode)
    }
}

impl Into<String> for InviteCodeRecord {
    fn into(self) -> String {
        base64::encode_config(&self.id.to_le_bytes(), base64::URL_SAFE_NO_PAD)
    }
}

pub struct TooManyInviteCodes;

impl Database {
    pub async fn create_invite_code(
        &self,
        community: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
        max_per_community: i64,
    ) -> DbResult<Result<InviteCode, TooManyInviteCodes>> {
        // From https://stackoverflow.com/a/26448803/4871468
        const INSERT: &str = "
            INSERT INTO invite_codes (id, community, expiration_date)
            SELECT
              $1 AS id,
              $2 AS community,
              $3 AS expiration_date
            FROM
              invite_codes
            WHERE community = $2
            HAVING
              COUNT(*) < $4
            ON CONFLICT DO NOTHING;
        ";
        const COUNT: &str = "SELECT COUNT(*) FROM invite_codes WHERE community = $1;";

        let mut conn = self.pool.connection().await?;

        let id = loop {
            let id = rand::thread_rng().gen::<i64>();
            let args: &[&(dyn ToSql + Sync)] =
                &[&id, &community.0, &expiration_date, &max_per_community];

            let builder = conn.client.build_transaction();
            let insert_transaction = builder
                .isolation_level(IsolationLevel::Serializable) // Needed for our query but doesn't
                // lock too much
                .start()
                .await?;
            let insert_stmt = insert_transaction.prepare(INSERT).await?;

            let ret = insert_transaction.execute(&insert_stmt, args).await?;
            insert_transaction.commit().await?;

            // If 1 row modified, then it was successful
            if ret == 1 {
                break id; // If we successfully inserted the id, it must be unique: return it
            } else {
                // Something went wrong...
                // Note: we can spuriously fail here, but spurious failure is okay in the grand
                // scheme of things. It's far better than spurious success...
                let row = conn
                    .client
                    .query_opt(COUNT, &[&community.0])
                    .await?
                    .unwrap();
                let count: i64 = row.try_get(0)?;

                if count >= max_per_community {
                    // Failed because of many invite codes
                    return Ok(Err(TooManyInviteCodes));
                } // ... or else it failed because of conflicting ID
            }
        };

        let record = InviteCodeRecord {
            id,
            expiration_date,
        };
        Ok(Ok(InviteCode(record.into())))
    }

    pub async fn get_community_from_invite_code(
        &self,
        code: InviteCode,
    ) -> DbResult<Result<Option<CommunityId>, MalformedInviteCode>> {
        const QUERY: &str = "
            SELECT community FROM invite_codes WHERE id=$1
        ";

        let id = match InviteCodeRecord::parse_id(code) {
            Ok(id) => id,
            Err(e) => return Ok(Err(e)),
        };

        let community = match self.query_opt(QUERY, &[&id]).await? {
            Some(row) => Some(CommunityId(row.try_get("community")?)),
            None => None,
        };

        Ok(Ok(community))
    }
}
