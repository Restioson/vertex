use actix::prelude::*;
use l337_postgres::PostgresConnectionManager;
use log::{error, warn};
use std::fs;
use std::time::{Duration, Instant};
use tokio_postgres::NoTls;
use vertex_common::{DeviceId, ServerError, UserId};

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
pub use token::*;
pub use user::*;

pub struct DatabaseServer {
    pool: Arc<l337::Pool<PostgresConnectionManager<NoTls>>>,
    sweep_interval: Duration,
    token_expiry_days: u16,
}

impl DatabaseServer {
    pub fn new(sys: &mut SystemRunner, config: &Config) -> Self {
        let mgr = PostgresConnectionManager::new(
            fs::read_to_string("db.conf")
                .expect("db.conf not found")
                .parse()
                .unwrap(),
            NoTls,
        );

        let pool = Arc::new(
            sys.block_on(l337::Pool::new(mgr, Default::default()))
                .expect("db error"),
        );

        DatabaseServer {
            pool,
            sweep_interval: Duration::from_secs(config.tokens_sweep_interval_secs),
            token_expiry_days: config.token_expiry_days,
        }
    }

    fn create_tables(
        &mut self,
    ) -> impl Future<Output = Result<(), l337::Error<tokio_postgres::Error>>> {
        use l337::Error::External;

        let pool = self.pool.clone();

        async move {
            let conn = pool.connection().await?;
            let cmds = [
                CREATE_USERS_TABLE,
                CREATE_TOKENS_TABLE,
                CREATE_COMMUNITIES_TABLE,
                CREATE_COMMUNITY_MEMBERSHIP_TABLE,
            ];

            for cmd in &cmds {
                let stmt = conn.client.prepare(cmd).map_err(External).await?;
                conn.client.execute(&stmt, &[]).await.map_err(External)?;
            }

            Ok(())
        }
    }

    fn expired_tokens(
        &self,
        token_expiry_days: u16,
    ) -> impl Future<Output = Result<Vec<(UserId, DeviceId)>, l337::Error<tokio_postgres::Error>>>
    {
        let token_expiry_days = token_expiry_days as f64;

        let pool = self.pool.clone();

        async move {
            let conn = pool.connection().await?;

            let stmt = conn
                .client
                .prepare(
                    "DELETE FROM login_tokens
                        WHERE expiration_date < NOW()::timestamp OR
                        DATE_PART('days', NOW()::timestamp - last_used) > $1
                    RETURNING device_id, user_id",
                )
                .map_err(l337::Error::External)
                .await?;

            conn.client
                .query(&stmt, &[&token_expiry_days])
                .map_err(l337::Error::External)
                .await?
                .iter()
                .map(|row| {
                    Ok((
                        UserId(row.try_get("user_id").map_err(l337::Error::External)?),
                        DeviceId(row.try_get("device_id").map_err(l337::Error::External)?),
                    ))
                })
                .collect()
        }
    }

    fn sweep_tokens(&self) -> impl Future<Output = ()> {
        let begin = Instant::now();

        let f = self.expired_tokens(self.token_expiry_days);
        let sweep_interval = self.sweep_interval;

        async move {
            f.await
                .map_err(|e| panic!("db error: {:#?}", e))
                .unwrap()
                .iter()
                .filter_map(|(user_id, device_id)| USERS.get(user_id).map(|u| ((device_id, u))))
                .for_each(|(device_id, user)| {
                    user.get(device_id)
                        .map(|addr| addr.do_send(LogoutThisSession));
                });

            let time_taken = Instant::now().duration_since(begin);
            if time_taken > sweep_interval {
                warn!(
                    "Took {}s to sweep the database for expired tokens, but the interval is {}s!",
                    time_taken.as_secs(),
                    sweep_interval.as_secs(),
                );
            }
        }
    }
}

impl Actor for DatabaseServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        let f = self
            .create_tables()
            .map(|r| r.expect("Error creating SQL tables!"));
        Arbiter::spawn(f);

        ctx.run_interval(self.sweep_interval, |db, ctx| {
            ctx.spawn(db.sweep_tokens().into_actor(db));
        });
    }
}

fn handle_error(error: l337::Error<tokio_postgres::Error>) -> ServerError {
    match error {
        l337::Error::Internal(e) => {
            error!("Database connection pooling error: {:#?}", e);
        }
        l337::Error::External(sql_error) => {
            error!("Database error: {:#?}", sql_error);
        }
    }

    ServerError::Internal
}

fn handle_error_psql(error: tokio_postgres::Error) -> ServerError {
    error!("Database error: {:#?}", error);

    ServerError::Internal
}
