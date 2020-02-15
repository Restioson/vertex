use crate::database::{Database, DbResult, MessageOrdinal};
use futures::{Stream, TryStreamExt};
use std::convert::TryFrom;
use std::error::Error as ErrorTrait;
use tokio_postgres::error::{DbError, Error, SqlState};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;
use vertex::{CommunityId, MessageId, RoomId, UserId};

pub(super) const CREATE_USER_ROOM_STATES_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS user_room_states (
        room             UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
        user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        watching_state   "char" NOT NULL,
        last_read        BIGINT NOT NULL,

        UNIQUE(user_id, room)
    )"#;

pub struct UserRoomState {
    pub room: RoomId,
    user: UserId,
    pub watching_state: WatchingState,
    pub last_read: MessageOrdinal,
}

impl TryFrom<Row> for UserRoomState {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<UserRoomState, tokio_postgres::Error> {
        let ws = row.try_get::<&str, i8>("watching_state")? as u8;
        let last_read = row.try_get::<&str, i64>("last_read")? as u64;

        Ok(UserRoomState {
            room: RoomId(row.try_get("room")?),
            user: UserId(row.try_get("user_id")?),
            watching_state: WatchingState::from(ws),
            last_read: MessageOrdinal(last_read),
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

pub enum SetUserRoomStateError {
    InvalidUser,
    InvalidRoom,
}

pub struct InvalidUser;

impl Database {
    pub async fn create_default_user_room_states(
        &self,
        community: CommunityId,
        user: UserId,
    ) -> DbResult<Result<(), InvalidUser>> {
        const STMT: &str = "
            INSERT INTO user_room_states (room, user_id, watching_state, last_read)
                SELECT rooms.id, $1, $2, MAX(messages.ord)
                    FROM rooms
                    INNER JOIN messages ON rooms.id = messages.room
                    WHERE messages.community = $3
                    GROUP BY rooms.id
        ";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[
            &user.0,
            &(WatchingState::default() as u8 as i8),
            &community.0,
        ];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res).map(|res| {
            res.map_err(|e| match e {
                SetUserRoomStateError::InvalidUser => InvalidUser,
                SetUserRoomStateError::InvalidRoom => panic!(
                    "{}{}",
                    "Create default user room states returned invalid room",
                    "; this should be impossible!",
                ),
            })
        })
    }

    pub async fn set_last_read(
        &self,
        room: RoomId,
        user: UserId,
        last_read: MessageId,
    ) -> DbResult<Result<(), SetUserRoomStateError>> {
        const STMT: &str = "
            WITH last_read_ord AS (
                SELECT ord FROM messages WHERE id = $1
            )
            UPDATE user_room_state
                WHERE user = $2 AND room = $3
                SET last_read = last_read_ord.ord
            ";

        let conn = self.pool.connection().await?;

        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[&last_read.0, &user.0, &room.0];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res)
    }

    pub async fn set_watching(
        &self,
        room: RoomId,
        user: UserId,
        state: WatchingState,
    ) -> DbResult<Result<(), SetUserRoomStateError>> {
        const STMT: &str = "
            UPDATE user_room_state
                WHERE user = $1 AND room = $2
                SET watching_state = $3
            ";

        let conn = self.pool.connection().await?;

        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[&user.0, &room.0, &(state as u8 as i8)];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res)
    }

    pub async fn get_watching_states(
        &self,
        user: UserId,
        community: CommunityId,
    ) -> DbResult<impl Stream<Item = DbResult<(RoomId, WatchingState)>>> {
        const QUERY: &str = "
            SELECT rooms.id, watching_state FROM rooms
            INNER JOIN room_watching_state ON rooms.id = room_watching_state.room
                WHERE rooms.community = $1 AND room_watching_state.user_id = $2
        ";

        let conn = self.pool.connection().await?;
        let query = conn.client.prepare(QUERY).await?;
        let args = &[&community.0, &user.0]; // I hate
        let stream = conn
            .client
            .query_raw(&query, args.iter().map(|x| x as &dyn ToSql))
            .await?;

        let stream = stream
            .and_then(|row| async move {
                let ws = row
                    .try_get::<&str, Option<i8>>("watching_state")?
                    .map(|v| WatchingState::from(v as u8));

                Ok((RoomId(row.try_get("id")?), ws.unwrap_or_default()))
            })
            .map_err(|e| e.into());

        Ok(stream)
    }
}

fn handle_sql_error(res: Result<u64, Error>) -> DbResult<Result<(), SetUserRoomStateError>> {
    match res {
        Ok(_) => Ok(Ok(())),
        Err(err) => {
            if err.code() == Some(&SqlState::FOREIGN_KEY_VIOLATION) {
                let constraint = err
                    .source()
                    .and_then(|e| e.downcast_ref::<DbError>())
                    .and_then(|e| e.constraint());

                match constraint {
                    Some("user_room_states_room_fkey") => {
                        Ok(Err(SetUserRoomStateError::InvalidRoom))
                    }
                    Some("user_room_states_user_fkey") => {
                        Ok(Err(SetUserRoomStateError::InvalidUser))
                    }
                    Some(_) | None => Err(err.into()),
                }
            } else {
                Err(err.into())
            }
        }
    }
}
