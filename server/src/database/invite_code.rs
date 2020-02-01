use crate::database::{Database, DbResult};
use base64::DecodeError;
use byteorder::{NativeEndian, ReadBytesExt};
use rand::Rng;
use std::convert::{TryFrom, TryInto};
use std::io::Cursor;
use tokio_postgres::types::ToSql;
use vertex::{CommunityId, InviteCode};

pub(super) const CREATE_INVITE_CODES_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS invite_codes (
        random_bits SMALLINT,
        incrementing_number BIGSERIAL,
        community_id UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,

        PRIMARY KEY (random_bits, incrementing_number)
    )";

#[derive(Debug)]
pub struct InviteCodeRecord {
    pub random_bits: u16,
    pub incrementing_number: u64,
}

impl Into<String> for InviteCodeRecord {
    fn into(self) -> String {
        let combined = ((self.random_bits as u64) << 48) | self.incrementing_number;
        base64::encode_config(&combined.to_le_bytes(), base64::URL_SAFE_NO_PAD)
    }
}

impl TryFrom<String> for InviteCodeRecord {
    type Error = DecodeError;

    fn try_from(string: String) -> Result<InviteCodeRecord, DecodeError> {
        let b64 = base64::decode_config(&string, base64::URL_SAFE_NO_PAD)?;
        let mut cursor = Cursor::new(b64);
        let combined = cursor.read_u64::<NativeEndian>().unwrap();
        let bottom_mask = (1u64 << 48) - 1;

        Ok(InviteCodeRecord {
            random_bits: (combined >> 48) as u16,
            incrementing_number: combined & bottom_mask,
        })
    }
}

impl Database {
    pub async fn create_invite_code(&self, community: CommunityId) -> DbResult<InviteCode> {
        const QUERY: &str = "
            INSERT INTO invite_codes (random_bits, community_id) VALUES ($1, $2)
                RETURNING incrementing_number
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;

        let random_bits = rand::thread_rng().gen::<u16>();
        let args: &[&(dyn ToSql + Sync)] = &[&(random_bits as i16), &community.0];

        let row = conn.client.query_opt(&query, args).await.unwrap().unwrap();

        let record = InviteCodeRecord {
            random_bits,
            incrementing_number: row.get::<&str, i64>("incrementing_number") as u64,
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

        let invite_code: InviteCodeRecord = match code.0.try_into() {
            Ok(record) => record,
            Err(e) => return Ok(Err(e)),
        };
        let args: &[&(dyn ToSql + Sync)] = &[
            &(invite_code.random_bits as i16),
            &(invite_code.incrementing_number as i64),
        ];

        let row_opt = conn.client.query_opt(&query, args).await?;
        if let Some(row) = row_opt {
            Ok(Ok(Some(CommunityId(row.try_get("community")?))))
        } else {
            Ok(Ok(None))
        }
    }
}
