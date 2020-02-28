use super::*;
use crate::auth::HashSchemeVersion;
use std::convert::TryFrom;
use tokio_postgres::{error::SqlState, row::Row, types::ToSql};
use uuid::Uuid;

pub(super) const CREATE_USERS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS users (
        id                   UUID PRIMARY KEY,
        username             VARCHAR NOT NULL UNIQUE,
        display_name         VARCHAR NOT NULL,
        profile_version      INTEGER NOT NULL,
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
    pub profile_version: ProfileVersion,
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
            profile_version: ProfileVersion(0),
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
            profile_version: ProfileVersion(row.try_get::<&str, i32>("profile_version")? as u32),
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

pub struct UsernameConflict;
pub struct NonexistentUser;

pub enum ChangeUsernameError {
    NonexistentUser,
    UsernameConflict,
}

impl Database {
    pub async fn get_user_by_id(&self, id: UserId) -> DbResult<Option<UserRecord>> {
        let query = "SELECT * FROM users WHERE id=$1";
        let row = self.query_opt(query, &[&id.0]).await?;
        if let Some(row) = row {
            Ok(Some(UserRecord::try_from(row)?)) // Can't opt::map because of ?
        } else {
            Ok(None)
        }
    }

    pub async fn get_user_by_name(&self, name: String) -> DbResult<Option<UserRecord>> {
        let query = "SELECT * FROM users WHERE username=$1";
        let row = self.query_opt(query, &[&name]).await?;
        if let Some(row) = row {
            Ok(Some(UserRecord::try_from(row)?)) // Can't opt::map because of ?
        } else {
            Ok(None)
        }
    }

    pub async fn get_user_profile(&self, id: UserId) -> DbResult<Option<UserProfile>> {
        let query = "SELECT username, display_name, profile_version FROM users WHERE id=$1";
        let opt = self.query_opt(query, &[&id.0]).await?;
        if let Some(row) = opt {
            // Can't opt::map because of ?
            Ok(Some(UserProfile {
                version: ProfileVersion(row.try_get::<&str, i32>("profile_version")? as u32),
                username: row.try_get("username")?,
                display_name: row.try_get("display_name")?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Creates a user, returning whether it was successful (i.e, if there were no conflicts with
    /// respect to the ID and username).
    pub async fn create_user(&self, user: UserRecord) -> DbResult<Result<(), UsernameConflict>> {
        const STMT: &str = "
            INSERT INTO users
                (
                    id,
                    username,
                    display_name,
                    profile_version,
                    password_hash,
                    hash_scheme_version,
                    compromised,
                    locked,
                    banned
                )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT DO NOTHING";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &user.id.0,
            &user.username,
            &user.display_name,
            &(user.profile_version.0 as i32),
            &user.password_hash,
            &(user.hash_scheme_version as u8 as i16),
            &user.compromised,
            &user.locked,
            &user.banned,
        ];

        let ret = conn.client.execute(&stmt, args).await?;

        Ok(if ret == 1 {
            // 1 item was inserted (insert was successful)
            Ok(())
        } else {
            Err(UsernameConflict)
        })
    }

    pub async fn change_username(
        &self,
        user: UserId,
        new_username: String,
    ) -> DbResult<Result<(), ChangeUsernameError>> {
        const STMT: &str = "
            UPDATE users
                SET username = $1, profile_version = profile_version + 1
                WHERE id = $2
        ";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
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
    ) -> DbResult<Result<(), NonexistentUser>> {
        const STMT: &str = "
            UPDATE users
                SET display_name = $1, profile_version = profile_version + 1
                WHERE id = $2
        ";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let res = conn
            .client
            .execute(&stmt, &[&new_display_name, &user.0])
            .await?;
        Ok(if res == 1 {
            Ok(())
        } else {
            Err(NonexistentUser)
        })
    }

    /// Changes the password of a user, returning whether the user existed at all.
    pub async fn change_password(
        &self,
        user: UserId,
        new_password_hash: String,
        hash_scheme_version: HashSchemeVersion,
    ) -> DbResult<Result<(), NonexistentUser>> {
        const STMT: &str = "
            UPDATE users
                SET password_hash = $1, hash_scheme_version = $2, compromised = $3
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
        Ok(if res == 1 {
            Ok(())
        } else {
            Err(NonexistentUser)
        })
    }
}
