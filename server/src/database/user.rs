use super::*;
use crate::auth::HashSchemeVersion;
use std::convert::TryFrom;
use tokio_postgres::row::Row;
use uuid::Uuid;
use vertex_common::UserId;

pub struct User {
    pub id: UserId,
    pub name: String,
    pub password_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub compromised: bool,
    pub banned: bool,
}

impl User {
    pub fn new(
        name: String,
        password_hash: String,
        hash_scheme_version: HashSchemeVersion,
    ) -> Self {
        User {
            id: UserId(Uuid::new_v4()),
            name,
            password_hash,
            hash_scheme_version,
            compromised: false,
            banned: false,
        }
    }
}

impl TryFrom<Row> for User {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<User, tokio_postgres::Error> {
        Ok(User {
            id: UserId(row.try_get("id")?),
            name: row.try_get("name")?,
            password_hash: row.try_get("password_hash")?,
            hash_scheme_version: HashSchemeVersion::from(
                row.try_get::<&str, i16>("hash_scheme_version")?,
            ),
            compromised: row.try_get("compromised")?,
            banned: row.try_get("banned")?,
        })
    }
}

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

pub struct ChangeUsername {
    pub user_id: UserId,
    pub new_name: String,
}

impl Message for ChangeUsername {
    type Result = Result<(), l337::Error<tokio_postgres::Error>>;
}

pub struct ChangePassword {
    pub user_id: UserId,
    pub new_password_hash: String,
    pub hash_version: HashSchemeVersion,
}

impl Message for ChangePassword {
    type Result = Result<(), l337::Error<tokio_postgres::Error>>;
}

impl Handler<CreateUser> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, create: CreateUser, _: &mut Context<Self>) -> Self::Result {
        let user = create.0;
        Box::new(self.pool.connection().and_then(|mut conn| {
            conn.client
                .prepare(
                    "INSERT INTO users
                        (id, name, password_hash, hash_scheme_version, compromised, banned)
                    VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .and_then(move |stmt| {
                    conn.client.execute(
                        &stmt,
                        &[
                            &user.id.0,
                            &user.name,
                            &user.password_hash,
                            &(user.hash_scheme_version as u8 as i16),
                            &user.compromised,
                            &user.banned,
                        ],
                    )
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

impl Handler<ChangeUsername> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, change: ChangeUsername, _: &mut Context<Self>) -> Self::Result {
        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("UPDATE users SET name = $1 WHERE id = $2")
                .and_then(move |stmt| {
                    conn.client
                        .execute(&stmt, &[&change.new_name, &change.user_id.0])
                })
                .map(|_| ())
                .map_err(l337::Error::External)
        }))
    }
}

impl Handler<ChangePassword> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, change: ChangePassword, _: &mut Context<Self>) -> Self::Result {
        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare(
                    "UPDATE users SET password_hash = $1, hash_scheme_version = $2 WHERE id = $3",
                )
                .and_then(move |stmt| {
                    conn.client.execute(
                        &stmt,
                        &[
                            &change.new_password_hash,
                            &(change.hash_version as u8 as i16),
                            &change.user_id.0,
                        ],
                    )
                })
                .map(|_| ())
                .map_err(l337::Error::External)
        }))
    }
}
