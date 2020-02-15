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

/// Modified from https://stackoverflow.com/a/42217872/4871468
const ADD_TO_COMMUNITY: &str = r#"
WITH input_rows(community, user_id) AS (
    VALUES ($1::UUID, $2::UUID)
), ins AS (
    INSERT INTO community_membership (community, user_id)
        SELECT * FROM input_rows
        ON CONFLICT DO NOTHING
        RETURNING *
), sel AS (
    SELECT 'i'::"char" AS source, * FROM ins                  -- 'i' for 'inserted'
    UNION  ALL
    SELECT 's'::"char" AS source, * FROM input_rows           -- 's' for 'selected'
    JOIN community_membership c USING (community, user_id)    -- columns of unique index
), ups AS (                                                   -- RARE corner case
   INSERT INTO community_membership AS c (community, user_id)
   SELECT i.*
   FROM input_rows i
   LEFT JOIN sel s USING (community, user_id)            -- columns of unique index
   WHERE s.user_id IS NULL                               -- missing!
   ON CONFLICT (community, user_id) DO UPDATE            -- we've asked nicely the 1st time ...
   SET user_id = c.user_id                               -- ... this time we overwrite with old value
   RETURNING 'u'::"char" AS source, *                    -- 'u' for updated
)

SELECT * FROM sel
UNION  ALL
TABLE  ups;
"#;

pub struct CommunityMember {
    pub community: CommunityId,
    user: UserId,
}

impl TryFrom<Row> for CommunityMember {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<CommunityMember, tokio_postgres::Error> {
        Ok(CommunityMember {
            community: CommunityId(row.try_get("community")?),
            user: UserId(row.try_get("user_id")?),
        })
    }
}

struct AddToCommunityResult {
    /// How the data was obtained - insert, select, or (nop) update? See the query for more.
    source: InsertIntoTableSource,
    member: CommunityMember,
}

impl TryFrom<Row> for AddToCommunityResult {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<AddToCommunityResult, tokio_postgres::Error> {
        Ok(AddToCommunityResult {
            source: InsertIntoTableSource::try_from(&row)?,
            member: CommunityMember::try_from(row)?,
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
        const QUERY: &str = "
            SELECT * from community_membership WHERE user_id = $1
        ";

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
        use InsertIntoTableSource::*;

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(ADD_TO_COMMUNITY).await?;
        let res = conn
            .client
            .query_opt(&query, &[&community.0, &user.0])
            .await;

        match res {
            Ok(Some(row)) => {
                let res = AddToCommunityResult::try_from(row)?;

                match res.source {
                    // Membership row did not exist - user has been successfully added
                    Insert => {
                        let res = self
                            .create_default_user_room_states(community, user)
                            .await?;
                        match res {
                            Ok(_) => Ok(Ok(())),
                            Err(InvalidUser) => Ok(Err(AddToCommunityError::InvalidUser)),
                        }
                    }

                    // Membership row already existed - conflict of some sort
                    Select | Update => Ok(Err(AddToCommunityError::AlreadyInCommunity)),
                }
            }
            Ok(None) => panic!("db error: add to community query did not return anything"),
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
