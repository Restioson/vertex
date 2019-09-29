use crate::auth::HashSchemeVersion;
use crate::database::{handle_error, DatabaseServer};
use actix::{Context, Handler, Message, ResponseFuture};
use chrono::{DateTime, Utc};
use futures::future::Future;
use futures::stream::Stream;
use std::convert::TryFrom;
use tokio_postgres::Row;
use vertex_common::{DeviceId, ServerError, TokenPermissionFlags, UserId};

pub(super) const CREATE_TOKENS_TABLE: &'static str = "CREATE TABLE IF NOT EXISTS login_tokens (
    device_id            UUID PRIMARY KEY,
    device_name          VARCHAR,
    token_hash           VARCHAR NOT NULL,
    hash_scheme_version  SMALLINT NOT NULL,
    user_id              UUID NOT NULL,
    last_used            TIMESTAMP WITH TIME ZONE NOT NULL,
    expiration_date      TIMESTAMP WITH TIME ZONE,
    permission_flags     BIGINT NOT NULL
)";

#[derive(Debug)]
pub struct Token {
    pub token_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub user_id: UserId,
    pub device_id: DeviceId,
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
            user_id: UserId(row.try_get("user_id")?),
            device_id: DeviceId(row.try_get("device_id")?),
            device_name: row.try_get("device_name")?,
            last_used: row.try_get("last_used")?,
            expiration_date: row.try_get("expiration_date")?,
            permission_flags: TokenPermissionFlags::from_bits_truncate(
                row.try_get("permission_flags")?,
            ),
        })
    }
}

pub struct GetToken {
    pub device_id: DeviceId,
}

impl Message for GetToken {
    type Result = Result<Option<Token>, ServerError>;
}

pub struct CreateToken(pub Token);

impl Message for CreateToken {
    type Result = Result<(), ServerError>;
}

pub struct RevokeToken(pub DeviceId);

impl Message for RevokeToken {
    type Result = Result<bool, ServerError>;
}

pub struct RefreshToken(pub DeviceId);

impl Message for RefreshToken {
    type Result = Result<bool, ServerError>;
}

impl Handler<GetToken> for DatabaseServer {
    type Result = ResponseFuture<Option<Token>, ServerError>;

    fn handle(&mut self, get: GetToken, _: &mut Context<Self>) -> Self::Result {
        Box::new(
            self.pool
                .connection()
                .and_then(move |mut conn| {
                    conn.client
                        .prepare("SELECT * FROM login_tokens WHERE device_id=$1")
                        .and_then(move |stmt| {
                            conn.client
                                .query(&stmt, &[&get.device_id.0])
                                .map(|row| Token::try_from(row))
                                .into_future()
                                .map(|(user, _stream)| user)
                                .map_err(|(err, _stream)| err)
                        })
                        .and_then(|x| x.transpose()) // Fut<Opt<Res<Usr, Err>>, Err> -> Fut<Opt<Usr>, Err>
                        .map_err(l337::Error::External)
                })
                .map_err(handle_error),
        )
    }
}

impl Handler<CreateToken> for DatabaseServer {
    type Result = ResponseFuture<(), ServerError>;

    fn handle(&mut self, create: CreateToken, _: &mut Context<Self>) -> Self::Result {
        let token = create.0;
        Box::new(
            self.pool
                .connection()
                .and_then(|mut conn| {
                    conn.client
                        .prepare(
                            "INSERT INTO login_tokens
                        (
                            device_id,
                            device_name,
                            token_hash,
                            hash_scheme_version,
                            user_id,
                            last_used,
                            expiration_date,
                            permission_flags
                        )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                        )
                        .and_then(move |stmt| {
                            conn.client.execute(
                                &stmt,
                                &[
                                    &token.device_id.0,
                                    &token.device_name,
                                    &token.token_hash,
                                    &(token.hash_scheme_version as u8 as i16),
                                    &token.user_id.0,
                                    &token.last_used,
                                    &token.expiration_date,
                                    &token.permission_flags.bits(),
                                ],
                            )
                        })
                        .map_err(l337::Error::External)
                        .map(|_| ())
                })
                .map_err(handle_error),
        )
    }
}

impl Handler<RevokeToken> for DatabaseServer {
    type Result = ResponseFuture<bool, ServerError>;

    fn handle(&mut self, revoke: RevokeToken, _: &mut Context<Self>) -> Self::Result {
        Box::new(
            self.pool
                .connection()
                .and_then(|mut conn| {
                    conn.client
                        .prepare("DELETE FROM login_tokens WHERE device_id = $1")
                        .and_then(move |stmt| conn.client.execute(&stmt, &[&(revoke.0).0]))
                        .map_err(l337::Error::External)
                        .map(|r| r == 1) // Result will be 1 if the token existed
                })
                .map_err(handle_error),
        )
    }
}

impl Handler<RefreshToken> for DatabaseServer {
    type Result = ResponseFuture<bool, ServerError>;

    fn handle(&mut self, revoke: RefreshToken, _: &mut Context<Self>) -> Self::Result {
        Box::new(
            self.pool
                .connection()
                .and_then(|mut conn| {
                    conn.client
                .prepare("UPDATE login_tokens SET last_used=NOW()::timestamp WHERE device_id = $1")
                .and_then(move |stmt| conn.client.execute(&stmt, &[&(revoke.0).0]))
                .map_err(l337::Error::External)
                .map(|r| r == 1) // Result will be 1 if the token existed
                })
                .map_err(handle_error),
        )
    }
}
