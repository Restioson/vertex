use super::*;
use std::convert::TryFrom;
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::Row;
use vertex_common::{CommunityId, ErrResponse, RoomId, UserId};

pub(super) const CREATE_COMMUNITY_MEMBERSHIP_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS community_membership (
    community UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    UNIQUE(user_id, community)
)";

/// Modified from https://stackoverflow.com/a/42217872/4871468
const ADD_TO_ROOM: &'static str = r#"
WITH input_rows(community, user_id) AS (
    VALUES ($1::UUID, $2::UUID)
), ins AS (
    INSERT INTO community_membership (community, user_id)
        SELECT * FROM input_rows
        ON CONFLICT DO NOTHING
        RETURNING *
), sel AS (
    SELECT 'i'::"char" AS source, * FROM ins           -- 'i' for 'inserted'
    UNION  ALL
    SELECT 's'::"char" AS source, * FROM input_rows    -- 's' for 'selected'
    JOIN community_membership c USING (community, user_id)    -- columns of unique index
), ups AS (                                            -- RARE corner case
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

pub struct RoomMember {
    community: RoomId,
    user: UserId,
}

impl TryFrom<&Row> for RoomMember {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<RoomMember, tokio_postgres::Error> {
        Ok(RoomMember {
            community: RoomId(row.try_get("community")?),
            user: UserId(row.try_get("user_id")?),
        })
    }
}

/// How the user was (or wasn't) added to a community. This is needed for the complicated (
/// but resilient) SQL query.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum AddToCommunitySource {
    Insert,
    Select,
    Update,
}

impl TryFrom<&Row> for AddToCommunitySource {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<AddToCommunitySource, tokio_postgres::Error> {
        Ok(match row.try_get::<&str, i8>("source")? as u8 {
            b'i' => AddToCommunitySource::Insert,
            b's' => AddToCommunitySource::Select,
            b'u' => AddToCommunitySource::Update,
            _ => panic!("Invalid AddToRoomSource type!"),
        })
    }
}

struct AddToRoomResult {
    /// How the data was obtained - insert, select, or (nop) update? See the query for more.
    source: AddToCommunitySource,
    member: RoomMember,
}

impl TryFrom<&Row> for AddToRoomResult {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<AddToRoomResult, tokio_postgres::Error> {
        Ok(AddToRoomResult {
            source: AddToCommunitySource::try_from(row)?,
            member: RoomMember::try_from(row)?,
        })
    }
}

pub enum AddToCommunityError {
    InvalidUser,
    InvalidCommunity,
    AlreadyInCommunity,
}

impl Database {
    pub async fn add_to_community(
        &self,
        community: CommunityId,
        user: UserId,
    ) -> DbResult<Result<(), AddToCommunityError>> {
        use AddToCommunitySource::*;

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(ADD_TO_ROOM).await?;
        let res = conn
            .client
            .query_opt(&query, &[&community.0, &user.0])
            .await;

        match res {
            Ok(Some(row)) => {
                let res = AddToRoomResult::try_from(&row)?;

                match res.source {
                    // Membership row did not exist - user has been successfully added
                    Insert => Ok(Ok(())),

                    // Membership row already existed - conflict of some sort
                    Select | Update => {
                        Ok(Err(AddToCommunityError::AlreadyInCommunity)) // TODO(room_persistence): banning
                    }
                }
            }
            Ok(None) => panic!("db error: add to room query did not return anything"),
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
