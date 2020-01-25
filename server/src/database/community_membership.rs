use super::*;
use std::convert::TryFrom;
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use tokio_postgres::Row;
use vertex_common::{CommunityId, ErrResponse, RoomId, UserId};

pub(super) const CREATE_COMMUNITY_MEMBERSHIP_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS community_membership (
    community UUID NOT NULL REFERENCES communities(id) ON DELETE CASCADE,
    \"user\" UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    UNIQUE(\"user\", community)
)";

/// Modified from https://stackoverflow.com/a/42217872/4871468
const ADD_TO_ROOM: &'static str = r#"
WITH input_rows(community, user) AS (
    VALUES ($1::UUID, $2::UUID)
), ins AS (
    INSERT INTO community_membership (community, user)
        SELECT * FROM input_rows
        ON CONFLICT DO NOTHING
        RETURNING *
), sel AS (
    SELECT 'i'::"char" AS source, * FROM ins           -- 'i' for 'inserted'
    UNION  ALL
    SELECT 's'::"char" AS source, * FROM input_rows    -- 's' for 'selected'
    JOIN community_membership c USING (community, user)    -- columns of unique index
), ups AS (                                            -- RARE corner case
   INSERT INTO community_membership AS c (community, user)
   SELECT i.*
   FROM input_rows i
   LEFT JOIN sel s USING (community, user)            -- columns of unique index
   WHERE s.user IS NULL                               -- missing!
   ON CONFLICT (community, user) DO UPDATE            -- we've asked nicely the 1st time ...
   SET user = c.user                                  -- ... this time we overwrite with old value
   RETURNING 'u'::"char" AS source, *                 -- 'u' for updated
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
            user: UserId(row.try_get("user")?),
        })
    }
}

pub struct AddToCommunity {
    pub community: CommunityId,
    pub user: UserId,
}

impl Message for AddToCommunity {
    type Result = Result<(), ErrResponse>;
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum AddToRoomSource {
    Insert,
    Select,
    Update,
}

impl TryFrom<&Row> for AddToRoomSource {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<AddToRoomSource, tokio_postgres::Error> {
        Ok(match row.try_get::<&str, i8>("source")? as u8 {
            b'i' => AddToRoomSource::Insert,
            b's' => AddToRoomSource::Select,
            b'u' => AddToRoomSource::Update,
            _ => panic!("Invalid AddToRoomSource type!"),
        })
    }
}

struct AddToRoomResult {
    /// How the data was obtained - insert, select, or (nop) update? See the query for more.
    source: AddToRoomSource,
    member: RoomMember,
}

impl TryFrom<&Row> for AddToRoomResult {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<AddToRoomResult, tokio_postgres::Error> {
        Ok(AddToRoomResult {
            source: AddToRoomSource::try_from(row)?,
            member: RoomMember::try_from(row)?,
        })
    }
}

impl Handler<AddToCommunity> for DatabaseServer {
    type Responder<'a> = impl Future<Output = Result<(), ErrResponse>> + 'a;

    fn handle(&mut self, add: AddToCommunity, _: &mut Context<Self>) -> Self::Responder<'_> {
        use AddToRoomSource::*;

        async move {
            let conn = self.pool.connection().await.map_err(handle_error)?;
            let query = conn
                .client
                .prepare(ADD_TO_ROOM)
                .await
                .map_err(handle_error_psql)?;
            let res = conn
                .client
                .query_opt(&query, &[&(add.community).0, &(add.user.0)])
                .await;

            match res {
                Ok(Some(row)) => {
                    let res = AddToRoomResult::try_from(&row).map_err(handle_error_psql)?;

                    match res.source {
                        // Row did not exist - user has been successfully added
                        Insert => Ok(()),

                        // Row already existed - conflict of some sort
                        Select | Update => {
                            Err(ErrResponse::AlreadyInCommunity) // TODO(room_persistence): banning
                        }
                    }
                }
                Ok(None) => panic!("db error: add to room query did not return anything"),
                Err(err) => {
                    let err = if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                        let constraint = err
                            .source()
                            .and_then(|e| e.downcast_ref::<DbError>())
                            .and_then(|e| e.constraint());

                        match constraint {
                            Some("community_membership_community_fkey") => {
                                ErrResponse::InvalidCommunity
                            }
                            Some("community_membership_user_fkey") => ErrResponse::InvalidUser,
                            Some(_) | None => handle_error(l337::Error::External(err)),
                        }
                    } else {
                        handle_error(l337::Error::External(err))
                    };

                    Err(err)
                }
            }
        }
    }
}
