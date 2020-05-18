use crate::database::{Database, DbResult};
use futures::{Stream, TryStreamExt};
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::types::ToSql;
use vertex::prelude::*;

pub(super) const CREATE_ADMINISTRATORS_TABLE: &str = r"
    CREATE TABLE IF NOT EXISTS administrators (
        user_id              UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        permission_flags     BIGINT NOT NULL
    )";

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CreateAdminError {
    InvalidUser,
}

impl Database {
    pub async fn set_admin_permissions(
        &self,
        user: UserId,
        permissions: AdminPermissionFlags,
    ) -> DbResult<Result<(), CreateAdminError>> {
        const UPDATE: &str = "
            INSERT INTO administrators (user_id, permission_flags) VALUES ($1, $2)
                ON CONFLICT(user_id) DO UPDATE SET permission_flags = $2
        ";
        const DELETE: &str = "DELETE FROM administrators WHERE user_id = $1";

        let conn = self.pool.connection().await?;
        let res = if permissions == AdminPermissionFlags::from_bits_truncate(0) {
            conn.client.execute(DELETE, &[&user.0]).await
        } else {
            let args: &[&(dyn ToSql + Sync)] = &[&user.0, &permissions.bits()];
            conn.client.execute(UPDATE, args).await
        };

        match res {
            Ok(1) => Ok(Ok(())), // 1 row modified = successfully added
            Ok(_n) => {
                panic!("db error: create admin query returned != 1 row modified!");
            }
            Err(err) => {
                if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                    let constraint = err
                        .source()
                        .and_then(|e| e.downcast_ref::<DbError>())
                        .and_then(|e| e.constraint());

                    match constraint {
                        Some("administrators_user_id_fkey") => {
                            Ok(Err(CreateAdminError::InvalidUser))
                        }
                        Some(_) | None => Err(err.into()),
                    }
                } else {
                    Err(err.into())
                }
            }
        }
    }

    pub async fn get_admin_permissions(&self, user: UserId) -> DbResult<AdminPermissionFlags> {
        const QUERY: &str = "SELECT permission_flags FROM administrators WHERE user_id = $1";

        let conn = self.pool.connection().await?;
        let opt = conn.client.query_opt(QUERY, &[&user.0]).await?;

        if let Some(row) = opt {
            Ok(AdminPermissionFlags::from_bits_truncate(
                row.try_get("permission_flags")?,
            ))
        } else {
            Ok(AdminPermissionFlags::from_bits_truncate(0))
        }
    }

    pub async fn list_all_admins(&self) -> DbResult<impl Stream<Item = DbResult<Admin>>> {
        const QUERY: &str = "
            SELECT user_id, username, permission_flags
            FROM administrators
            INNER JOIN users ON administrators.user_id = users.id";

        let stream = self.query_stream(QUERY, &[]).await?;
        let stream = stream
            .and_then(|row| async move {
                let id = UserId(row.try_get("user_id")?);
                let username = row.try_get("username")?;
                let permissions =
                    AdminPermissionFlags::from_bits_truncate(row.try_get("permission_flags")?);

                Ok(Admin {
                    username,
                    id,
                    permissions,
                })
            })
            .map_err(|e| e.into());

        Ok(stream)
    }
}
