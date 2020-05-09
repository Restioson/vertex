use crate::database::{Database, DbResult};
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::types::ToSql;
use vertex::prelude::*;
use futures::{Stream, TryStreamExt};

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
            let args: &[&(dyn ToSql + Sync)] = &[&user.0, &permissions.bits()];
            conn.client.execute(UPDATE, args).await
        } else {
            conn.client.execute(DELETE, &[&user.0]).await
        };

        match res {
            Ok(1) => {
                // 1 row modified = successfully added
                Ok(Ok(()))
            }
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
                        Some("administrators_user_fkey") => {
                            dbg!("haha no");
                            Ok(Err(CreateAdminError::InvalidUser))
                        },
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

    pub async fn list_all_administrators(
        &self,
    ) -> DbResult<impl Stream<Item = DbResult<(UserId, AdminPermissionFlags)>>> {
        const QUERY: &str = "SELECT * FROM administrators";

        let stream = self.query_stream(QUERY, &[]).await?;
        let stream = stream
            .and_then(|row| async move {
                let user = UserId(row.try_get("user_id")?);
                let flags = AdminPermissionFlags::from_bits_truncate(
                    row.try_get("permission_flags")?
                );
                Ok((user, flags))
            })
            .map_err(|e| e.into());

        Ok(stream)
    }
}
