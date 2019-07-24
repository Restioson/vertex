use super::*;
use crate::auth::HashSchemeVersion;
use std::convert::TryFrom;
use tokio_postgres::row::Row;
use uuid::Uuid;
use vertex_common::UserId;

pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub compromised: bool,
    pub locked: bool,
    pub banned: bool,
}

impl User {
    pub fn new(
        username: String,
        display_name: String,
        password_hash: String,
        hash_scheme_version: HashSchemeVersion,
    ) -> Self {
        User {
            id: UserId(Uuid::new_v4()),
            username,
            display_name,
            password_hash,
            hash_scheme_version,
            compromised: false,
            locked: false,
            banned: false,
        }
    }
}

impl TryFrom<Row> for User {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<User, tokio_postgres::Error> {
        Ok(User {
            id: UserId(row.try_get("id")?),
            username: row.try_get("username")?,
            display_name: row.try_get("display_name")?,
            password_hash: row.try_get("password_hash")?,
            hash_scheme_version: HashSchemeVersion::from(
                row.try_get::<&str, i16>("hash_scheme_version")?,
            ),
            compromised: row.try_get("compromised")?,
            locked: row.try_get("locked")?,
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
    type Result = Result<bool, l337::Error<tokio_postgres::Error>>;
}

pub struct ChangeUsername {
    pub user_id: UserId,
    pub new_username: String,
}

impl Message for ChangeUsername {
    type Result = Result<bool, l337::Error<tokio_postgres::Error>>;
}

pub struct ChangeDisplayName {
    pub user_id: UserId,
    pub new_display_name: String,
}

impl Message for ChangeDisplayName {
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
    type Result = ResponseFuture<bool, l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, create: CreateUser, _: &mut Context<Self>) -> Self::Result {
        let user = create.0;
        Box::new(self.pool.connection().and_then(|mut conn| {
            conn.client
                .prepare(
                    "INSERT INTO users
                        (
                            id,
                            username,
                            display_name,
                            password_hash,
                            hash_scheme_version,
                            compromised,
                            locked,
                            banned
                        )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    ON CONFLICT DO NOTHING",
                )
                .and_then(move |stmt| {
                    conn.client.execute(
                        &stmt,
                        &[
                            &user.id.0,
                            &user.username,
                            &user.display_name,
                            &user.password_hash,
                            &(user.hash_scheme_version as u8 as i16),
                            &user.compromised,
                            &user.locked,
                            &user.banned,
                        ],
                    )
                })
                .map_err(l337::Error::External)
                .map(|ret| ret == 1) // Return true if 1 item was inserted (insert was sucessful)
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
                .prepare("SELECT * FROM users WHERE username=$1")
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
    type Result = ResponseFuture<bool, l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, change: ChangeUsername, _: &mut Context<Self>) -> Self::Result {
        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("UPDATE users SET username = $1 WHERE id = $2 ON CONFLICT DO NOTHING") // TODO test on conflict
                .and_then(move |stmt| {
                    conn.client
                        .execute(&stmt, &[&change.new_username, &change.user_id.0])
                })
                .map(|ret| ret == 1) // Return true if 1 item was updated (update was sucessful)
                .map_err(l337::Error::External)
        }))
    }
}

impl Handler<ChangeDisplayName> for DatabaseServer {
    type Result = ResponseFuture<(), l337::Error<tokio_postgres::Error>>;

    fn handle(&mut self, change: ChangeDisplayName, _: &mut Context<Self>) -> Self::Result {
        Box::new(self.pool.connection().and_then(move |mut conn| {
            conn.client
                .prepare("UPDATE users SET display_name = $1 WHERE id = $2")
                .and_then(move |stmt| {
                    conn.client
                        .execute(&stmt, &[&change.new_display_name, &change.user_id.0])
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
