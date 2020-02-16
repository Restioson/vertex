use std::convert::TryFrom;
use std::error::Error;
use std::iter;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::Row;
use vertex::{CommunityId, UserId};

use super::*;

pub(super) const CREATE_COMMUNITY_MEMBERSHIP_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS community_membership (
        community        UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
        user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

        UNIQUE(user_id, community)
    )"#;

pub struct CommunityMember {
    pub community: CommunityId,
}

impl TryFrom<Row> for CommunityMember {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<CommunityMember, tokio_postgres::Error> {
        Ok(CommunityMember {
            community: CommunityId(row.try_get("community")?),
        })
    }
}

pub enum AddToCommunityError {
    InvalidUser,
    InvalidCommunity,
    AlreadyInCommunity,
}

impl Database {
    pub async fn get_communities_for_user(
        &self,
        user: UserId,
    ) -> DbResult<impl Stream<Item = DbResult<CommunityMember>>> {
        const QUERY: &str = "SELECT * from community_membership WHERE user_id = $1";

        let conn = self.pool.connection().await?;

        let query = conn.client.prepare(QUERY).await?;
        let rows = {
            let args = iter::once(&user.0 as &(dyn ToSql + Sync));
            conn.client
                .query_raw(&query, args.map(|x| x as &dyn ToSql))
                .await?
        };

        let stream = rows
            .and_then(|row| async move { Ok(CommunityMember::try_from(row)?) })
            .map_err(|e| e.into());

        Ok(stream)
    }

    pub async fn get_community_membership(
        &self,
        community: CommunityId,
        user: UserId,
    ) -> DbResult<Option<CommunityMember>> {
        const QUERY: &str = "
            SELECT * from community_membership
                WHERE community = $1 AND user_id = $2";
        let conn = self.pool.connection().await?;

        let query = conn.client.prepare(QUERY).await?;
        let opt = conn
            .client
            .query_opt(&query, &[&community.0, &user.0])
            .await?;

        if let Some(row) = opt {
            Ok(Some(CommunityMember::try_from(row)?)) // Can't opt::map because of ?
        } else {
            Ok(None)
        }
    }

    pub async fn add_to_community(
        &self,
        community: CommunityId,
        user: UserId,
    ) -> DbResult<Result<(), AddToCommunityError>> {
        const STMT: &str = "
            INSERT (community, user_id) INTO community_membership
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(STMT).await?;
        let res = conn.client.execute(&query, &[&community.0, &user.0]).await;

        match res {
            Ok(1) => {
                // 1 row modified = successfully added
                let res = self
                    .create_default_user_room_states_for_user(community, user)
                    .await;

                match res {
                    Ok(_) => Ok(Ok(())),
                    Err(_) => Ok(Err(AddToCommunityError::InvalidUser)),
                }
            }
            Ok(0) => {
                // 0 rows modified = failed to add
                Ok(Err(AddToCommunityError::AlreadyInCommunity))
            }
            Ok(_n) => {
                panic!("db error: add to community query returned more than one row modified!");
            }
            Err(err) => {
                if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                    let constraint = err
                        .source()
                        .and_then(|e| e.downcast_ref::<DbError>())
                        .and_then(|e| e.constraint());

                    match constraint {
                        Some("community_membership_community_fkey") => {
                            Ok(Err(AddToCommunityError::InvalidCommunity))
                        }
                        Some("community_membership_user_fkey") => {
                            Ok(Err(AddToCommunityError::InvalidUser))
                        }
                        Some(_) | None => Err(err.into()),
                    }
                } else {
                    Err(err.into())
                }
            }
        }
    }
}
