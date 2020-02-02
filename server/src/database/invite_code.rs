use crate::database::{Database, DbResult};
use base64::DecodeError;
use byteorder::{NativeEndian, ReadBytesExt};
use chrono::{DateTime, Utc};
use rand::Rng;
use std::io::Cursor;
use tokio_postgres::types::ToSql;
use vertex::{CommunityId, InviteCode};

pub(super) const CREATE_INVITE_CODES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS invite_codes (
        random_bits SMALLINT,
        incrementing_number BIGSERIAL,
        community_id UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        expiration_date TIMESTAMP WITH TIME ZONE,

        PRIMARY KEY (random_bits, incrementing_number)
    )";

#[derive(Debug)]
pub struct InviteCodeRecord {
    pub random_bits: u16,
    pub incrementing_number: u64,
    pub expiration_date: Option<DateTime<Utc>>,
}

impl InviteCodeRecord {
    fn bits_and_incrementing_number(code: InviteCode) -> Result<(u16, u64), DecodeError> {
        let b64 = base64::decode_config(&code.0, base64::URL_SAFE_NO_PAD)?;
        let mut cursor = Cursor::new(b64);
        let combined = cursor.read_u64::<NativeEndian>().unwrap();
        let bottom_mask = (1u64 << 48) - 1;

        let random_bits = (combined >> 48) as u16;
        let incrementing_number = combined & bottom_mask;

        Ok((random_bits, incrementing_number))
    }
}

impl Into<String> for InviteCodeRecord {
    fn into(self) -> String {
        let combined = ((self.random_bits as u64) << 48) | self.incrementing_number;
        base64::encode_config(&combined.to_le_bytes(), base64::URL_SAFE_NO_PAD)
    }
}

impl Database {
    pub async fn create_invite_code(
        &self,
        community: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    ) -> DbResult<InviteCode> {
        const QUERY: &str = "
            INSERT INTO invite_codes (random_bits, community_id, expiration_date) VALUES ($1, $2, $3)
                RETURNING incrementing_number
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;

        let random_bits = rand::thread_rng().gen::<u16>();
        let args: &[&(dyn ToSql + Sync)] = &[&(random_bits as i16), &community.0, &expiration_date];

        let row = conn.client.query_opt(&query, args).await.unwrap().unwrap();

        let record = InviteCodeRecord {
            random_bits,
            incrementing_number: row.get::<&str, i64>("incrementing_number") as u64,
            expiration_date,
        };

        Ok(InviteCode(record.into()))
    }

    pub async fn get_community_from_invite_code(
        &self,
        code: InviteCode,
    ) -> DbResult<Result<Option<CommunityId>, DecodeError>> {
        const QUERY: &str = "
            SELECT community_id FROM invite_codes WHERE random_bits=$1 AND incrementing_number=$2
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;

        let res = InviteCodeRecord::bits_and_incrementing_number(code);
        let (random_bits, incrementing_number) = match res {
            Ok(tuple) => tuple,
            Err(e) => return Ok(Err(e)),
        };
        let args: &[&(dyn ToSql + Sync)] = &[&(random_bits as i16), &(incrementing_number as i64)];

        let row_opt = conn.client.query_opt(&query, args).await?;
        if let Some(row) = row_opt {
            Ok(Ok(Some(CommunityId(row.try_get("community")?))))
        } else {
            Ok(Ok(None))
        }
    }
}
