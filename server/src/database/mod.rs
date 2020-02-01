use l337_postgres::PostgresConnectionManager;
use log::{error, warn};
use std::fs;
use std::time::{Duration, Instant};
use tokio_postgres::NoTls;
use vertex::{DeviceId, ErrResponse, UserId};

mod communities;
mod community_membership;
mod token;
mod user;

use crate::client::LogoutThisSession;
use crate::client::USERS;
pub use communities::*;
pub use community_membership::*;
use xtra::prelude::*;

use futures::{Stream, TryStreamExt};
pub use token::*;
use tokio_postgres::types::ToSql;
pub use user::*;

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

impl From<DatabaseError> for ErrResponse {
    fn from(e: DatabaseError) -> ErrResponse {
        match e.0 {
            l337::Error::Internal(e) => {
                error!("Database connection pooling error: {:#?}", e);
            }
            l337::Error::External(sql_error) => {
                error!("Database error: {:#?}", sql_error);
            }
        }

        ErrResponse::Internal
    }
}

#[derive(Clone)]
pub struct Database {
    pool: l337::Pool<PostgresConnectionManager<NoTls>>,
}

impl Database {
    pub async fn new() -> DbResult<Self> {
        let mgr = PostgresConnectionManager::new(
            fs::read_to_string("db.conf") // TODO use config dirs
                .expect("db.conf not found")
                .parse()
                .unwrap(),
            NoTls,
        );

        let pool = l337::Pool::new(mgr, Default::default())
            .await
            .expect("db error");

        let db = Database { pool };
        db.create_tables().await?;
        Ok(db)
    }

    async fn create_tables(&self) -> DbResult<()> {
        let conn = self.pool.connection().await?;
        let cmds = [
            CREATE_USERS_TABLE,
            CREATE_TOKENS_TABLE,
            CREATE_COMMUNITIES_TABLE,
            CREATE_COMMUNITY_MEMBERSHIP_TABLE,
        ];

        for cmd in &cmds {
            let stmt = conn.client.prepare(cmd).await?;
            conn.client.execute(&stmt, &[]).await?;
        }

        Ok(())
    }

    pub async fn sweep_loop(self, token_expiry_days: u16, interval: Duration) {
        let mut timer = tokio::time::interval(interval);

        loop {
            timer.tick().await;
            let begin = Instant::now();
            self.expired_tokens(token_expiry_days)
                .await
                .expect("Database error while sweeping tokens")
                .try_filter_map(|(user, device)| async move {
                    Ok(USERS.get(&user).map(|u| (device, u)))
                })
                .try_for_each(|(device, user)| async move {
                    if let Some(addr) = user.get(&device) {
                        if addr.do_send(LogoutThisSession).is_err() {
                            warn!("ClientWsSession actor disconnected. This is probably a timing anomaly.");
                        }
                    }

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
}
