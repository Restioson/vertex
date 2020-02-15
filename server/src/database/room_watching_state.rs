use tokio_postgres::Row;
use crate::database::{Database, DbResult};
use std::convert::TryFrom;
use vertex::{RoomId, UserId, CommunityId};
use tokio_postgres::error::{SqlState, Error, DbError};
use tokio_postgres::types::ToSql;
use futures::{Stream, TryStreamExt};
use std::error::{Error as ErrorTrait};

pub(super) const CREATE_ROOM_WATCHING_STATE_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS room_watching_state (
        room             UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
        user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        watching_state   "char" NOT NULL,

        UNIQUE(user_id, room)
    )"#;


pub struct RoomWatchingState {
    pub room: RoomId,
    user: UserId,
    pub watching_state: WatchingState
}

impl TryFrom<Row> for RoomWatchingState {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<RoomWatchingState, tokio_postgres::Error> {
        let ws = row.try_get::<&str, i8>("watching_state")? as u8;
        Ok(RoomWatchingState {
            room: RoomId(row.try_get("room")?),
            user: UserId(row.try_get("user_id")?),
            watching_state: WatchingState::from(ws),
        })
    }
}


#[derive(Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum WatchingState {
    Watching = 0,
    NotWatching = 1,
    MentionsOnly = 2,
}

impl Default for WatchingState {
    fn default() -> Self {
        WatchingState::NotWatching
    }
}

impl From<u8> for WatchingState {
    fn from(val: u8) -> Self {
        match val {
            0 => WatchingState::Watching,
            1 => WatchingState::NotWatching,
            _ => WatchingState::default(),
        }
    }
}

pub enum SetWatchingStateError {
    InvalidUser,
    InvalidRoom,
}

impl Database {
    pub async fn set_watching_state(
        &self,
        room: RoomId,
        user: UserId,
        state: WatchingState,
    ) -> DbResult<Result<(), SetWatchingStateError>> {
        let conn = self.pool.connection().await?;

        if state == WatchingState::default() {
            const SET_TO_DEFAULT: &str = "
                DELETE FROM room_watching_state
                WHERE room = $1 AND user_id = $2
            ";

            let stmt = conn.client.prepare(SET_TO_DEFAULT).await?;
            let res = conn.client.execute(&stmt, &[]).await;
            handle_sql_error(res)
        } else {
            const SET_WATCHING_STATE: &str = "
                INSERT INTO room_watching_state (room, user_id, watching_state)
                    VALUES ($1, $2, $3)
                    ON CONFLICT UPDATE SET watching_state = $3
                ";

            let stmt = conn.client.prepare(SET_WATCHING_STATE).await?;
            let args: &[&(dyn ToSql + Sync)] = &[&room.0, &user.0, &(state as u8 as i8)];
            let res = conn.client.execute(&stmt, args).await;

            handle_sql_error(res)
        }
    }

    pub async fn get_watching_states(
        &self,
        user: UserId,
        community: CommunityId
    ) -> DbResult<impl Stream<Item = DbResult<(RoomId, WatchingState)>>> {
        const QUERY: &str = "
            SELECT rooms.id, watching_state FROM rooms
            LEFT JOIN room_watching_state ON rooms.id = room_watching_state.room
                WHERE rooms.community = $1 AND
                    (room_watching_state.user_id = $2 OR room_watching_state.user_id IS NULL)
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let args = &[&community.0, &user.0]; // I hate
        let stream = conn.client.query_raw(&query, args.iter().map(|x| x as &dyn ToSql)).await?;

        let stream = stream
            .and_then(|row| async move {
                let ws = row.try_get::<&str, Option<i8>>("watching_state")?
                    .map(|v| WatchingState::from(v as u8));

                Ok((
                    RoomId(row.try_get("id")?),
                    ws.unwrap_or(WatchingState::default()),
                ))
            })
            .map_err(|e| e.into());

        Ok(stream)
    }
}

fn handle_sql_error(res: Result<u64, Error>) -> DbResult<Result<(), SetWatchingStateError>> {
    match res {
        Ok(_) => Ok(Ok(())),
        Err(err) => {
            if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                let constraint = err
                    .source()
                    .and_then(|e| e.downcast_ref::<DbError>())
                    .and_then(|e| e.constraint());

                match constraint {
                    Some("room_watching_state_room_fkey") => {
                        Ok(Err(SetWatchingStateError::InvalidRoom))
                    }
                    Some("room_watching_state_user_fkey") => {
                        Ok(Err(SetWatchingStateError::InvalidUser))
                    }
                    Some(_) | None => Err(err.into()),
                }
            } else {
                Err(err.into())
            }
        }
    }
}
