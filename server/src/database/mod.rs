use actix::prelude::*;
use l337_postgres::PostgresConnectionManager;
use std::fs;
use tokio_postgres::NoTls;

mod token;
mod user;

pub use token::*;
pub use user::*;

pub struct DatabaseServer {
    pool: l337::Pool<PostgresConnectionManager<NoTls>>,
}

impl DatabaseServer {
    pub fn new(sys: &mut SystemRunner) -> Self {
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
        DatabaseServer { pool }
    }
}

impl DatabaseServer {
    fn create_tables(&mut self) -> impl Future<Item = (), Error = ()> {
        let users = self
            .pool
            .connection()
            .and_then(|mut conn| {
                conn.client
                    .prepare(
                        // TODO configure max length of display name/username
                        "CREATE TABLE IF NOT EXISTS users (
                            id                   UUID PRIMARY KEY,
                            display_name         VARCHAR NOT NULL UNIQUE,
                            username             VARCHAR NOT NULL UNIQUE,
                            password_hash        VARCHAR NOT NULL,
                            hash_scheme_version  SMALLINT NOT NULL,
                            compromised          BOOLEAN NOT NULL,
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
                            token_hash           VARCHAR PRIMARY KEY,
                            hash_scheme_version  SMALLINT NOT NULL,
                            user_id              UUID NOT NULL,
                            device_id            UUID NOT NULL,
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
}

impl Actor for DatabaseServer {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        Arbiter::spawn(self.create_tables());
    }
}
