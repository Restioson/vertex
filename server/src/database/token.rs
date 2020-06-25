use crate::auth::HashSchemeVersion;
use crate::database::{Database, DbResult};
use chrono::{DateTime, Utc};
use std::convert::TryFrom;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;
use vertex::prelude::*;

pub(super) const CREATE_TOKENS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS login_tokens (
        device              UUID PRIMARY KEY,
        device_name          VARCHAR,
        token_hash           VARCHAR NOT NULL,
        hash_scheme_version  SMALLINT NOT NULL,
        user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        last_used            TIMESTAMP WITH TIME ZONE NOT NULL,
        expiration_date      TIMESTAMP WITH TIME ZONE,
        permission_flags     BIGINT NOT NULL
    )";

#[derive(Debug)]
pub struct Token {
    pub token_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub user: UserId,
    pub device: DeviceId,
    pub device_name: Option<String>,
    pub last_used: DateTime<Utc>,
    pub expiration_date: Option<DateTime<Utc>>,
    pub permission_flags: TokenPermissionFlags,
}

impl TryFrom<Row> for Token {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<Token, tokio_postgres::Error> {
        Ok(Token {
            token_hash: row.try_get("token_hash")?,
            hash_scheme_version: HashSchemeVersion::from(
                row.try_get::<&str, i16>("hash_scheme_version")?,
            ),
            user: UserId(row.try_get("user_id")?),
            device: DeviceId(row.try_get("device")?),
            device_name: row.try_get("device_name")?,
            last_used: row.try_get("last_used")?,
            expiration_date: row.try_get("expiration_date")?,
            permission_flags: TokenPermissionFlags::from_bits_truncate(
                row.try_get("permission_flags")?,
            ),
        })
    }
}

pub struct NonexistentDevice;
pub struct DeviceIdConflict;

impl Database {
    pub async fn get_token(&self, device: DeviceId) -> DbResult<Option<Token>> {
        const QUERY: &str = "SELECT * FROM login_tokens WHERE device=$1";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let opt = conn.client.query_opt(&query, &[&device.0]).await?;

        if let Some(row) = opt {
            Ok(Some(Token::try_from(row)?)) // Can't opt::map because of the ?
        } else {
            Ok(None)
        }
    }

    pub async fn create_token(&self, token: Token) -> DbResult<Result<(), DeviceIdConflict>> {
        const STMT: &str = "
            INSERT INTO login_tokens
                (
                    device,
                    device_name,
                    token_hash,
                    hash_scheme_version,
                    user_id,
                    last_used,
                    expiration_date,
                    permission_flags
                )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT DO NOTHING";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &token.device.0,
            &token.device_name,
            &token.token_hash,
            &(token.hash_scheme_version as i16),
            &token.user.0,
            &token.last_used,
            &token.expiration_date,
            &token.permission_flags.bits(),
        ];

        let res = conn.client.execute(&stmt, args).await.map(|r| {
            if r == 1 {
                Ok(())
            } else {
                Err(DeviceIdConflict)
            }
        });
        res.map_err(Into::into)
    }

    /// Returns whether any token existed with the given ID in the first place
    pub async fn revoke_token(
        &self,
        device_id: DeviceId,
    ) -> DbResult<Result<(), NonexistentDevice>> {
        let conn = self.pool.connection().await?;
        let stmt = conn
            .client
            .prepare("DELETE FROM login_tokens WHERE device = $1")
            .await?;

        // Result will be 1 if the token existed
        let res = conn.client.execute(&stmt, &[&device_id.0]).await.map(|r| {
            if r == 1 {
                Ok(())
            } else {
                Err(NonexistentDevice)
            }
        });

        res.map_err(Into::into)
    }

    /// Returns whether any token existed with the given ID in the first place
    pub async fn refresh_token(
        &self,
        device_id: DeviceId,
    ) -> DbResult<Result<(), NonexistentDevice>> {
        const STMT: &str = "UPDATE login_tokens SET last_used=NOW()::timestamp WHERE device = $1";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;

        // Result will be 1 if the token existed
        let res = conn.client.execute(&stmt, &[&device_id.0]).await.map(|r| {
            if r == 1 {
                Ok(())
            } else {
                Err(NonexistentDevice)
            }
        });

        res.map_err(Into::into)
    }
}
