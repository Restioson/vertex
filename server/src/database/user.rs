use super::*;
use crate::auth::HashSchemeVersion;
use std::convert::TryFrom;
use tokio_postgres::{error::SqlState, row::Row, types::ToSql};
use uuid::Uuid;
use vertex_common::{ErrResponse, UserId};

pub(super) const CREATE_USERS_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS users (
    id                   UUID PRIMARY KEY,
    username             VARCHAR NOT NULL UNIQUE,
    display_name         VARCHAR NOT NULL,
    password_hash        VARCHAR NOT NULL,
    hash_scheme_version  SMALLINT NOT NULL,
    compromised          BOOLEAN NOT NULL,
    locked               BOOLEAN NOT NULL,
    banned               BOOLEAN NOT NULL
)";

pub struct UserRecord {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub hash_scheme_version: HashSchemeVersion,
    pub compromised: bool,
    pub locked: bool,
    pub banned: bool,
}

impl UserRecord {
    pub fn new(
        username: String,
        display_name: String,
        password_hash: String,
        hash_scheme_version: HashSchemeVersion,
    ) -> Self {
        UserRecord {
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

impl TryFrom<Row> for UserRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<UserRecord, tokio_postgres::Error> {
        Ok(UserRecord {
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

pub enum ChangeUsernameError {
    NonexistentUser,
    UsernameConflict,
}

impl Database {
    pub async fn get_user_by_id(&self, id: UserId) -> Result<Option<UserRecord>, DatabaseError> {
        let conn = self.pool.connection().await?;
        let query = conn
            .client
            .prepare("SELECT * FROM users WHERE id=$1")
            .await?;
        let opt = conn.client.query_opt(&query, &[&id.0]).await?;

        if let Some(row) = opt {
            Ok(Some(UserRecord::try_from(row)?)) // Can't opt::map because of ?
        } else {
            Ok(None)
        }
    }

    pub async fn get_user_by_name(
        &self,
        name: String,
    ) -> Result<Option<UserRecord>, DatabaseError> {
        let conn = self.pool.connection().await?;
        let query = conn
            .client
            .prepare("SELECT * FROM users WHERE username=$1")
            .await?;
        let opt = conn.client.query_opt(&query, &[&name]).await?;

        if let Some(row) = opt {
            Ok(Some(UserRecord::try_from(row)?)) // Can't opt::map because of ?
        } else {
            Ok(None)
        }
    }

    /// Creates a user, returning whether it was successful (i.e, if there were no conflicts with
    /// respect to the ID and username).
    pub async fn create_user(&self, user: UserRecord) -> Result<bool, DatabaseError> {
        const STMT: &'static str = "
            INSERT INTO users
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
            ON CONFLICT DO NOTHING";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &user.id.0,
            &user.username,
            &user.display_name,
            &user.password_hash,
            &(user.hash_scheme_version as u8 as i16),
            &user.compromised,
            &user.locked,
            &user.banned,
        ];

        let ret = conn.client.execute(&stmt, args).await?;

        Ok(ret == 1) // Return true if 1 item was inserted (insert was successful)
    }

    pub async fn change_username(
        &self,
        user: UserId,
        new_username: String,
    ) -> Result<Result<(), ChangeUsernameError>, DatabaseError> {
        let conn = self.pool.connection().await?;
        let stmt = conn
            .client
            .prepare("UPDATE users SET username = $1 WHERE id = $2")
            .await?;
        let res = conn.client.execute(&stmt, &[&new_username, &user.0]).await;

        match res {
            Ok(ret) => {
                if ret == 1 {
                    Ok(Ok(()))
                } else {
                    Ok(Err(ChangeUsernameError::NonexistentUser))
                }
            }
            Err(e) => {
                if e.code() == Some(&SqlState::INTEGRITY_CONSTRAINT_VIOLATION)
                    || e.code() == Some(&SqlState::UNIQUE_VIOLATION)
                {
                    Ok(Err(ChangeUsernameError::UsernameConflict))
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// Changes the display name of a user, returning whether the user existed at all.
    pub async fn change_display_name(
        &self,
        user: UserId,
        new_display_name: String,
    ) -> Result<bool, DatabaseError> {
        let conn = self.pool.connection().await?;
        let stmt = conn
            .client
            .prepare("UPDATE users SET display_name = $1 WHERE id = $2")
            .await?;
        let res = conn
            .client
            .execute(&stmt, &[&new_display_name, &user.0])
            .await?;
        Ok(res == 1)
    }

    /// Changes the password of a user, returning whether the user existed at all.
    pub async fn change_password(
        &self,
        user: UserId,
        new_password_hash: String,
        hash_scheme_version: HashSchemeVersion,
    ) -> Result<bool, DatabaseError> {
        const STMT: &'static str = "
            UPDATE users SET
                password_hash = $1, hash_scheme_version = $2, compromised = $3
            WHERE id = $4";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &new_password_hash,
            &(hash_scheme_version as u8 as i16),
            &false,
            &user.0,
        ];

        let res = conn.client.execute(&stmt, args).await?;
        Ok(res == 1)
    }
}
