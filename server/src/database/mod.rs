use std::convert::TryFrom;
use std::time::{Duration, Instant};

use crate::{client, config};
use futures::{Stream, TryStreamExt};
use l337_postgres::PostgresConnectionManager;
use log::{error, warn};
use tokio_postgres::types::ToSql;
use tokio_postgres::{NoTls, Row, RowStream};
use vertex::prelude::*;

mod administrators;
mod communities;
mod community_membership;
mod invite_code;
mod message;
mod rooms;
mod token;
mod user;
mod user_room_states;

pub use administrators::*;
pub use communities::*;
pub use community_membership::*;
pub use invite_code::*;
pub use message::*;
pub use rooms::*;
pub use token::*;
pub use user::*;
pub use user_room_states::*;

pub type DbResult<T> = Result<T, DatabaseError>;

#[derive(Debug)]
pub struct DatabaseError(l337::Error<tokio_postgres::Error>);

impl From<l337::Error<tokio_postgres::Error>> for DatabaseError {
    fn from(e: l337::Error<tokio_postgres::Error>) -> Self {
        DatabaseError(e)
    }
}

impl From<tokio_postgres::Error> for DatabaseError {
    fn from(e: tokio_postgres::Error) -> Self {
        DatabaseError(l337::Error::External(e))
    }
}

impl From<DatabaseError> for Error {
    fn from(e: DatabaseError) -> Error {
        let backtrace = backtrace::Backtrace::new();

        match e.0 {
            l337::Error::Internal(err) => {
                error!(
                    "Database connection pooling error: {:?}\n {:#?}",
                    err, backtrace
                );
            }
            l337::Error::External(err) => {
                error!("Database error: {:?}\n {:#?}", err, backtrace);
            }
        }

        Error::Internal
    }
}

impl From<DatabaseError> for AuthError {
    fn from(e: DatabaseError) -> AuthError {
        warn!("db error: {:#?}", e);
        AuthError::Internal
    }
}

pub struct InvalidUser;

#[derive(Clone)]
pub struct Database {
    pool: l337::Pool<PostgresConnectionManager<NoTls>>,
}

impl Database {
    pub async fn new() -> DbResult<Self> {
        let mgr = PostgresConnectionManager::new(config::db_config(), NoTls);

        let pool = l337::Pool::new(mgr, Default::default())
            .await
            .expect("db error");

        let db = Database { pool };
        db.create_tables().await?;
        Ok(db)
    }

    pub async fn query_one(&self, query: &str, args: &[&(dyn ToSql + Sync)]) -> DbResult<Row> {
        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(query).await?;
        Ok(conn.client.query_one(&query, args).await?)
    }

    pub async fn query_opt(
        &self,
        query: &str,
        args: &[&(dyn ToSql + Sync)],
    ) -> DbResult<Option<Row>> {
        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(query).await?;
        Ok(conn.client.query_opt(&query, args).await?)
    }

    pub async fn query_stream(
        &self,
        query: &str,
        args: &[&(dyn ToSql + Sync)],
    ) -> DbResult<RowStream> {
        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(query).await?;
        Ok(conn.client.query_raw(&query, slice_iter(args)).await?)
    }

    async fn create_tables(&self) -> DbResult<()> {
        let conn = self.pool.connection().await?;
        let cmds = [
            CREATE_USERS_TABLE,
            CREATE_TOKENS_TABLE,
            CREATE_COMMUNITIES_TABLE,
            CREATE_COMMUNITY_MEMBERSHIP_TABLE,
            CREATE_ROOMS_TABLE,
            CREATE_INVITE_CODES_TABLE,
            CREATE_MESSAGES_TABLE,
            CREATE_USER_ROOM_STATES_TABLE,
            CREATE_ADMINISTRATORS_TABLE,
        ];

        for cmd in &cmds {
            let stmt = conn.client.prepare(cmd).await?;
            conn.client.execute(&stmt, &[]).await?;
        }

        Ok(())
    }

    pub async fn sweep_tokens_loop(self, token_expiry_days: u16, interval: Duration) {
        let mut timer = tokio::time::interval(interval);

        loop {
            timer.tick().await;
            let begin = Instant::now();
            self.expired_tokens(token_expiry_days)
                .await
                .expect("Database error while sweeping tokens")
                .try_for_each(|(user, device)| async move {
                    client::session::remove_and_notify(user, device);
                    Ok(())
                })
                .await
                .expect("Database error while sweeping tokens");

            let time_taken = Instant::now().duration_since(begin);
            if time_taken > interval {
                warn!(
                    "Took {}s to sweep the database for expired tokens, but the interval is {}s!",
                    time_taken.as_secs(),
                    interval.as_secs(),
                );
            }
        }
    }

    async fn expired_tokens(
        &self,
        token_expiry_days: u16,
    ) -> DbResult<impl Stream<Item = DbResult<(UserId, DeviceId)>>> {
        const QUERY: &str = "
            DELETE FROM login_tokens
                WHERE expiration_date < NOW()::timestamp OR
                DATE_PART('days', NOW()::timestamp - last_used) > $1
            RETURNING device, user_id";

        let token_expiry_days = token_expiry_days as f64;
        let args = [token_expiry_days];
        let args = args.iter().map(|x| x as &dyn ToSql);
        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(QUERY).await?;

        let stream = conn
            .client
            .query_raw(&stmt, args)
            .await?
            .and_then(|row| async move {
                Ok((
                    UserId(row.try_get("user_id")?),
                    DeviceId(row.try_get("device")?),
                ))
            })
            .map_err(|e| e.into());
        Ok(stream)
    }

    pub async fn sweep_invite_codes_loop(self, interval: Duration) {
        let mut timer = tokio::time::interval(interval);

        loop {
            timer.tick().await;
            let begin = Instant::now();
            self.delete_expired_invite_codes()
                .await
                .expect("Database error while sweeping invite codes");

            let time_taken = Instant::now().duration_since(begin);
            if time_taken > interval {
                warn!(
                    "Took {}s to sweep the database for expired invite codes, but the interval is {}s!",
                    time_taken.as_secs(),
                    interval.as_secs(),
                );
            }
        }
    }

    async fn delete_expired_invite_codes(&self) -> DbResult<()> {
        const STMT: &str = "DELETE FROM invite_codes WHERE expiration_date < NOW()::timestamp";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        conn.client.execute(&stmt, &[]).await?;
        Ok(())
    }
}

/// How the user was (or wasn't) added to a community or room. This is needed for the complicated (
/// but resilient) SQL queries used.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum InsertIntoTableSource {
    Insert,
    Select,
    Update,
}

impl TryFrom<&Row> for InsertIntoTableSource {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<InsertIntoTableSource, tokio_postgres::Error> {
        Ok(match row.try_get::<&str, i8>("source")? as u8 {
            b'i' => InsertIntoTableSource::Insert,
            b's' => InsertIntoTableSource::Select,
            b'u' => InsertIntoTableSource::Update,
            _ => panic!("Invalid AddToRoomSource type!"),
        })
    }
}

/// Taken from tokio_postgres
pub fn slice_iter<'a>(
    s: &'a [&'a (dyn ToSql + Sync)],
) -> impl ExactSizeIterator<Item = &'a dyn ToSql> + 'a {
    s.iter().map(|s| *s as _)
}
