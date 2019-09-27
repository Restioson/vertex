use actix::prelude::*;
use l337_postgres::PostgresConnectionManager;
use std::fs;
use std::time::{Duration, Instant};
use tokio_postgres::NoTls;
use vertex_common::{DeviceId, UserId};

mod token;
mod user;

use crate::client::{ClientServer, LogoutSessions};
use crate::config::Config;
pub use token::*;
pub use user::*;

pub struct DatabaseServer {
    pool: l337::Pool<PostgresConnectionManager<NoTls>>,
    sweep_interval: Duration,
    token_expiry_days: u16,
    client_server: Addr<ClientServer>,
}

impl DatabaseServer {
    pub fn new(sys: &mut SystemRunner, client_server: Addr<ClientServer>, config: &Config) -> Self {
        let mgr = PostgresConnectionManager::new(
            fs::read_to_string("db.conf")
                .expect("db.conf not found")
                .parse()
                .unwrap(),
            NoTls,
        );

        let pool = sys
            .block_on(l337::Pool::new(mgr, Default::default()))
            .expect("db error");
        DatabaseServer {
            pool,
            sweep_interval: Duration::from_secs(config.tokens_sweep_interval_secs),
            token_expiry_days: config.token_expiry_days,
            client_server,
        }
    }

    fn create_tables(&mut self) -> impl Future<Item = (), Error = ()> {
        let users = self
            .pool
            .connection()
            .and_then(|mut conn| {
                conn.client
                    .prepare(
                        "CREATE TABLE IF NOT EXISTS users (
                            id                   UUID PRIMARY KEY,
                            username             VARCHAR NOT NULL UNIQUE,
                            display_name         VARCHAR NOT NULL,
                            password_hash        VARCHAR NOT NULL,
                            hash_scheme_version  SMALLINT NOT NULL,
                            compromised          BOOLEAN NOT NULL,
                            locked               BOOLEAN NOT NULL,
                            banned               BOOLEAN NOT NULL
                        )",
                    )
                    .and_then(move |stmt| conn.client.execute(&stmt, &[]))
                    .map(|_| ())
                    .map_err(|e| panic!("db error: {:?}", e))
            })
            .map_err(|e| panic!("db connection pool error: {:?}", e));

        let login_tokens = self
            .pool
            .connection()
            .and_then(|mut conn| {
                conn.client
                    .prepare(
                        "CREATE TABLE IF NOT EXISTS login_tokens (
                            device_id            UUID PRIMARY KEY,
                            token_hash           VARCHAR NOT NULL,
                            hash_scheme_version  SMALLINT NOT NULL,
                            user_id              UUID NOT NULL,
                            last_used            TIMESTAMP WITH TIME ZONE NOT NULL,
                            expiration_date      TIMESTAMP WITH TIME ZONE,
                            permission_flags     BIGINT NOT NULL
                        )",
                    )
                    .and_then(move |stmt| conn.client.execute(&stmt, &[]))
                    .map(|_| ())
                    .map_err(|e| panic!("db error: {:?}", e))
            })
            .map_err(|e| panic!("db connection pool error: {:?}", e));

        users.and_then(|_| login_tokens)
    }

    fn expired_tokens(
        &self,
        token_expiry_days: u16,
    ) -> impl Stream<Item = (UserId, DeviceId), Error = l337::Error<tokio_postgres::Error>> {
        let token_expiry_days = token_expiry_days as f64;
        self.pool
            .connection()
            .map(move |mut conn| {
                conn.client
                    .prepare(
                        "DELETE FROM login_tokens
                            WHERE expiration_date < NOW()::timestamp OR
                                DATE_PART('days', NOW()::timestamp - last_used) > $1
                            RETURNING device_id, user_id",
                    )
                    .map_err(l337::Error::External)
                    .map(move |stmt| {
                        conn.client
                            .query(&stmt, &[&token_expiry_days])
                            .map(|row| {
                                Ok((
                                    UserId(row.try_get("user_id")?),
                                    DeviceId(row.try_get("device_id")?),
                                ))
                            })
                            .map_err(l337::Error::External)
                    })
                    .flatten_stream()
            })
            .flatten_stream()
            .then(|result| result.and_then(|inner| inner.map_err(l337::Error::External)))
    }

    fn sweep_tokens(&self) -> impl ActorFuture<Actor = Self, Item = (), Error = ()> {
        let begin = Instant::now();

        self.expired_tokens(self.token_expiry_days)
            .collect()
            .map_err(|e| panic!("db error: {:?}", e))
            .into_actor(self)
            .map(move |list, act, _ctx| act.client_server.do_send(LogoutSessions { list }))
            .map(move |_, act, _ctx| {
                let time_taken = Instant::now().duration_since(begin);
                if time_taken > act.sweep_interval {
                    eprintln!(
                        "Warning! Took {}s to sweep the database for expired tokens, but the interval is {}s!",
                        time_taken.as_secs(),
                        act.sweep_interval.as_secs(),
                    );
                }
            })
    }
}

impl Actor for DatabaseServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        Arbiter::spawn(self.create_tables());

        ctx.run_interval(self.sweep_interval, |db, ctx| {
            ctx.spawn(db.sweep_tokens());
        });
    }
}
