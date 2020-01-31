use l337_postgres::PostgresConnectionManager;
use log::{error, warn};
use std::fs;
use std::time::{Duration, Instant};
use tokio_postgres::NoTls;
use vertex_common::{DeviceId, ErrResponse, UserId};

mod communities;
mod community_membership;
mod token;
mod user;

use crate::client::LogoutThisSession;
use crate::client::USERS;
use crate::config::Config;
pub use communities::*;
pub use community_membership::*;
use futures::{Future, FutureExt, TryFutureExt};
use std::sync::Arc;
use xtra::prelude::*;

pub use token::*;
pub use user::*;

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
    pub async fn new() -> Result<Self, DatabaseError> {
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

    async fn create_tables(&self) -> Result<(), DatabaseError> {
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
                .map_err(|e| panic!("Database error while sweeping tokens: {:#?}", e))
                .unwrap()
                .iter()
                .filter_map(|(user, device)| USERS.get(user).map(|u| ((device, u))))
                .for_each(|(device, user)| {
                    user.get(device).map(|addr| addr.do_send(LogoutThisSession));
                });

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
    ) -> Result<Vec<(UserId, DeviceId)>, DatabaseError> {
        const QUERY: &'static str = "
            DELETE FROM login_tokens
                WHERE expiration_date < NOW()::timestamp OR
                DATE_PART('days', NOW()::timestamp - last_used) > $1
            RETURNING device, user_id";

        let token_expiry_days = token_expiry_days as f64;
        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(QUERY).await?;

        conn.client
            .query(&stmt, &[&token_expiry_days])
            .await?
            .iter()
            .map(|row| {
                Ok((
                    UserId(row.try_get("user_id")?),
                    DeviceId(row.try_get("device")?),
                ))
            })
            .collect()
    }
}
