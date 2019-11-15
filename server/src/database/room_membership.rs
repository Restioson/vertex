use vertex_common::{RoomId, UserId, ServerError};
use std::convert::TryFrom;
use tokio_postgres::Row;
use actix::{Message, Handler, ResponseFuture, Context};
use super::*;
use std::error::Error;
use tokio_postgres::error::{DbError, SqlState};
use futures::future;

pub(super) const CREATE_ROOM_MEMBERSHIP_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS room_membership (
    room_id UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    UNIQUE(user_id, room_id)
)";

/// Modified from https://stackoverflow.com/a/42217872/4871468
const ADD_TO_ROOM: &'static str = r#"
WITH input_rows(room_id, user_id) AS (
    VALUES ($1::UUID, $2::UUID)
), ins AS (
    INSERT INTO room_membership (room_id, user_id)
        SELECT * FROM input_rows
        ON CONFLICT DO NOTHING
        RETURNING *
), sel AS (
    SELECT 'i'::"char" AS source, * FROM ins           -- 'i' for 'inserted'
    UNION  ALL
    SELECT 's'::"char" AS source, * FROM input_rows    -- 's' for 'selected'
    JOIN room_membership c USING (room_id, user_id)    -- columns of unique index
), ups AS (                                            -- RARE corner case
   INSERT INTO room_membership AS c (room_id, user_id)
   SELECT i.*
   FROM input_rows i
   LEFT JOIN sel s USING (room_id, user_id)            -- columns of unique index
   WHERE s.user_id IS NULL                             -- missing!
   ON CONFLICT (room_id, user_id) DO UPDATE            -- we've asked nicely the 1st time ...
   SET user_id = c.user_id                             -- ... this time we overwrite with old value
   RETURNING 'u'::"char" AS source, *                  -- 'u' for updated
)

SELECT * FROM sel
UNION  ALL
TABLE  ups;
"#;

pub struct RoomMember {
    room_id: RoomId,
    user_id: UserId,
}

impl TryFrom<&Row> for RoomMember {
    type Error = tokio_postgres::Error;

    fn try_from(row: &Row) -> Result<RoomMember, tokio_postgres::Error> {
        Ok(RoomMember {
            room_id: RoomId(row.try_get("room_id")?),
            user_id: UserId(row.try_get("user_id")?),
        })
    }
}

pub struct AddToRoom {
    pub room: RoomId,
    pub user: UserId,
}

impl Message for AddToRoom {
    type Result = Result<(), ServerError>;
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
            b'u' =>AddToRoomSource::Update,
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

impl Handler<AddToRoom> for DatabaseServer {
    type Result = ResponseFuture<(), ServerError>;

    fn handle(&mut self, add: AddToRoom, _: &mut Context<Self>) -> Self::Result {
        use AddToRoomSource::*;

        Box::new(
            self.pool
                .connection()
                .map_err(handle_error)
                .and_then(|mut conn| {
                    conn.client
                        .prepare(ADD_TO_ROOM)
                        .and_then(move |stmt| conn.client.query(&stmt, &[&(add.room).0, &(add.user.0)])
                            .into_future()
                            .map(|(user, _stream)| user)
                            .map_err(|(err, _stream)| err)
                        )
                        .then(|res| match res {
                            Ok(Some(row)) => {
                                let res = AddToRoomResult::try_from(&row);
                                match res {
                                    Ok(AddToRoomResult { source, .. }) => {
                                        match source {
                                            // Row did not exist - user has been successfully added
                                            Insert => future::ok(()),

                                            // Row already existed - conflict of some sort
                                            Select | Update => {
                                                let err = ServerError::AlreadyInRoom;// TODO banning
                                                future::err(err)
                                            }
                                        }
                                    }
                                    Err(e) => future::err(handle_error(l337::Error::External(e))),
                                }

                            },
                            Ok(None) => panic!("Add to room query did not return anything"),
                            Err(err) => {
                                let err = if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                                    let constraint = err.source()
                                        .and_then(|e| e.downcast_ref::<DbError>())
                                        .and_then(|e| e.constraint());

                                    eprintln!("{:#?}", err);

                                    match constraint {
                                        Some("room_membership_room_id_fkey") => ServerError::InvalidRoom,
                                        Some("room_membership_user_id_fkey") => ServerError::InvalidUser,
                                        Some(_) | None => handle_error(l337::Error::External(err)),
                                    }
                                } else {
                                    handle_error(l337::Error::External(err))
                                };

                                future::err(err)
                            }
                        })
                })
        )
    }
}
