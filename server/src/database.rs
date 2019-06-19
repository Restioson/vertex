use actix::prelude::*;
use futures::stream::Stream;
use l337_postgres::PostgresConnectionManager;
use std::convert::TryFrom;
use std::fs;
use tokio_postgres::row::Row;
use tokio_postgres::NoTls;
use vertex_common::UserId;

pub struct GetUserById(pub UserId);

impl Message for GetUserById {
    type Result = Result<Option<User>, l337::Error<tokio_postgres::Error>>;
}

pub struct GetUserByName(pub String);

impl Message for GetUserByName {
    type Result = Result<Option<User>, l337::Error<tokio_postgres::Error>>;
}

pub struct CreateUser(pub User);

impl Message for CreateUser {
    type Result = Result<(), l337::Error<tokio_postgres::Error>>;
}

pub struct User {
    pub id: UserId,
    pub name: String,
    pub password_hash: String,
}

impl TryFrom<Row> for User {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<User, tokio_postgres::Error> {
        Ok(User {
            id: UserId(row.try_get("id")?),
            name: row.try_get("name")?,
            password_hash: row.try_get("password_hash")?,
        })
    }
}

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
        self.pool
            .connection()
            .and_then(|mut conn| {
                conn.client
                    .prepare(
                        "CREATE TABLE IF NOT EXISTS users (
                        id            UUID PRIMARY KEY,
                        name          VARCHAR(64) NOT NULL UNIQUE,
                        password_hash VARCHAR NOT NULL
                    )",
                    )
                    .and_then(move |stmt| conn.client.execute(&stmt, &[]))
                    .map(|code| {
                        if code != 0 {
                            panic!("nonzero sql return code {}", code)
                        }
                    })
                    .map_err(|e| l337::Error::External(e))
            })
            .map_err(|e| panic!("db error: {:?}", e))
    }
}

impl Actor for DatabaseServer {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        Arbiter::spawn(self.create_tables());
    }
}

impl Handler<CreateUser> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, create: CreateUser, _: &mut Context<Self>) -> Self::Result {
        let user = create.0;
        Box::new(self.pool.connection().and_then(|mut conn| {
            conn.client
                .prepare("INSERT INTO users (id, name, password_hash) VALUES ($1, $2, $3)")
                .and_then(move |stmt| {
                    conn.client
                        .execute(&stmt, &[&user.id.0, &user.name, &user.password_hash])
                })
                .map_err(l337::Error::External)
                .map(|_| ())
        }))
    }
}

impl Handler<GetUserById> for DatabaseServer {
    type Result = ResponseFuture<Option<User>, l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, get: GetUserById, _: &mut Context<Self>) -> Self::Result {
        let id = get.0;

        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("SELECT * FROM users WHERE id=$1")
                .and_then(move |stmt| {
                    conn.client
                        .query(&stmt, &[&id.0])
                        .map(|row| User::try_from(row))
                        .into_future()
                        .map(|(user, _stream)| user)
                        .map_err(|(err, _stream)| err)
                })
                .and_then(|x| x.transpose()) // Fut<Opt<Res<Usr, Err>>, Err> -> Fut<Opt<Usr>, Err>
                .map_err(l337::Error::External)
        }))
    }
}

impl Handler<GetUserByName> for DatabaseServer {
    type Result = ResponseFuture<Option<User>, l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, get: GetUserByName, _: &mut Context<Self>) -> Self::Result {
        let name = get.0;

        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("SELECT * FROM users WHERE name=$1")
                .and_then(move |stmt| {
                    conn.client
                        .query(&stmt, &[&name])
                        .map(|row| User::try_from(row))
                        .into_future()
                        .map(|(user, _stream)| user)
                        .map_err(|(err, _stream)| err)
                })
                .and_then(|x| x.transpose()) // Fut<Opt<Res<Usr, Err>>, Err> -> Fut<Opt<Usr>, Err>
                .map_err(l337::Error::External)
        }))
    }
}
