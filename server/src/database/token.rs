use crate::auth::HashSchemeVersion;
use crate::database::DatabaseServer;
use actix::{Context, Handler, Message, ResponseFuture};
use chrono::{DateTime, Utc};
use futures::future::Future;
use futures::stream::Stream;
use std::convert::TryFrom;
use tokio_postgres::Row;
use vertex_common::{DeviceId, TokenPermissionFlags, UserId};

pub struct Token {
    pub token_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub user_id: UserId,
    pub device_id: DeviceId,
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
            last_used: row.try_get("last_used")?,
            expiration_date: row.try_get("expiration_date")?,
            permission_flags: TokenPermissionFlags::from_bits_truncate(
                row.try_get("permission_flags")?,
            ),
        })
    }
}

pub struct GetToken {
    pub token_hash: String,
}

impl Message for GetToken {
    type Result = Result<Option<Token>, l337::Error<tokio_postgres::Error>>;
}

pub struct CreateToken(pub Token);

impl Message for CreateToken {
    type Result = Result<(), l337::Error<tokio_postgres::Error>>;
}

impl Handler<GetToken> for DatabaseServer {
    type Result = ResponseFuture<Option<Token>, l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, get: GetToken, _: &mut Context<Self>) -> Self::Result {
        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("SELECT * FROM login_tokens WHERE token_hash=$1")
                .and_then(move |stmt| {
                    conn.client
                        .query(&stmt, &[&get.token_hash])
                        .map(|row| Token::try_from(row))
                        .into_future()
                        .map(|(user, _stream)| user)
                        .map_err(|(err, _stream)| err)
                })
                .and_then(|x| x.transpose()) // Fut<Opt<Res<Usr, Err>>, Err> -> Fut<Opt<Usr>, Err>
                .map_err(l337::Error::External)
        }))
    }
}

impl Handler<CreateToken> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, create: CreateToken, _: &mut Context<Self>) -> Self::Result {
        let token = create.0;
        Box::new(self.pool.connection().and_then(|mut conn| {
            conn.client
                .prepare(
                    "INSERT INTO users
                        (
                            token_hash,
                            hash_scheme_version,
                            user_id,
                            device_id,
                            last_used,
                            expiration_date,
                            permission_flags
                        )
                    VALUES ($1, $2, $3, $4, $5, $6, $7)",
                )
                .and_then(move |stmt| {
                    conn.client.execute(
                        &stmt,
                        &[
                            &token.token_hash,
                            &(token.hash_scheme_version as u8 as i16),
                            &token.user_id.0,
                            &token.device_id.0,
                            &token.last_used,
                            &token.expiration_date,
                            &token.permission_flags.bits(),
                        ],
                    )
                })
                .map_err(l337::Error::External)
                .map(|_| ())
        }))
    }
}
