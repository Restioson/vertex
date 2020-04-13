use crate::database::{Database, DbResult, InvalidUser, MessageOrdinal};
use futures::{Stream, TryStreamExt};
use std::convert::TryFrom;
use std::error::Error as ErrorTrait;
use tokio_postgres::error::{DbError, Error, SqlState};
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;
use vertex::prelude::*;

pub(super) const CREATE_USER_ROOM_STATES_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS user_room_states (
        room             UUID NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
        user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        watch_level      "char" NOT NULL,
        last_read        BIGINT,

        UNIQUE(user_id, room)
    )"#;

pub struct UserRoomState {
    pub room: RoomId,
    pub watch_level: WatchLevel,
    pub unread: bool,
}

impl TryFrom<Row> for UserRoomState {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<UserRoomState, tokio_postgres::Error> {
        let ws = row.try_get::<&str, i8>("watch_level")? as u8;

        Ok(UserRoomState {
            room: RoomId(row.try_get("room")?),
            watch_level: WatchLevel::from(ws),
            unread: row
                .try_get::<&str, Option<bool>>("unread")?
                .unwrap_or(false),
        })
    }
}

#[derive(Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum WatchLevel {
    Watching = 0,
    NotWatching = 1,
    MentionsOnly = 2,
}

impl Default for WatchLevel {
    fn default() -> Self {
        WatchLevel::NotWatching
    }
}

impl From<u8> for WatchLevel {
    fn from(val: u8) -> Self {
        match val {
            0 => WatchLevel::Watching,
            1 => WatchLevel::NotWatching,
            _ => WatchLevel::default(),
        }
    }
}

#[derive(Debug)]
pub enum SetUserRoomStateError {
    InvalidUser,
    InvalidRoom,
}

impl Database {
    pub async fn create_default_user_room_states_for_user(
        &self,
        community: CommunityId,
        user: UserId,
    ) -> DbResult<Result<(), InvalidUser>> {
        const STMT: &str = "
            INSERT INTO user_room_states (room, user_id, watch_level, last_read)
                SELECT rooms.id, $1, $2, NULL::BIGINT
                    FROM rooms
                    WHERE rooms.community = $3
        ";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] =
            &[&user.0, &(WatchLevel::default() as u8 as i8), &community.0];
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

    pub async fn create_default_user_room_states_for_room(
        &self,
        community: CommunityId,
        room: RoomId,
    ) -> DbResult<Result<(), SetUserRoomStateError>> {
        const STMT: &str = "
            INSERT INTO user_room_states (room, user_id, watch_level, last_read)
                SELECT $1, community_membership.user_id, $2, NULL::BIGINT
                    FROM community_membership
                    WHERE community_membership.community = $3
        ";

        let conn = self.pool.connection().await?;
        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] =
            &[&room.0, &(WatchLevel::default() as u8 as i8), &community.0];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res)
    }

    pub async fn set_room_read(
        &self,
        room: RoomId,
        user: UserId,
    ) -> DbResult<Result<(), SetUserRoomStateError>> {
        const STMT: &str = "
            WITH last_read_ord(ord) AS (
                SELECT COALESCE((SELECT MAX(ord) FROM messages WHERE room = $2), 0::BIGINT)
            )
            UPDATE user_room_states
                SET last_read = last_read_ord.ord
                FROM last_read_ord
                WHERE user_id = $1 AND room = $2
            ";

        let conn = self.pool.connection().await?;

        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[&user.0, &room.0];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res)
    }

    pub async fn get_last_read(&self, user: UserId, room: RoomId) -> DbResult<Option<MessageId>> {
        const QUERY: &str =
            "SELECT last_read FROM user_room_states WHERE user_id = $1 AND room = $2";

        let ord = match self.query_opt(QUERY, &[&user.0, &room.0]).await? {
            Some(row) => row
                .try_get::<&str, Option<i64>>("last_read")?
                .map(|last_read| MessageOrdinal(last_read as u64)),
            None => None,
        };

        match ord {
            Some(ord) => self.get_message_id(ord).await,
            None => Ok(None),
        }
    }

    pub async fn set_watch_level(
        &self,
        room: RoomId,
        user: UserId,
        level: WatchLevel,
    ) -> DbResult<Result<(), SetUserRoomStateError>> {
        const STMT: &str = "
            UPDATE user_room_states
                SET watch_level = $3
                WHERE user_id = $1 AND room = $2
            ";

        let conn = self.pool.connection().await?;

        let stmt = conn.client.prepare(STMT).await?;
        let args: &[&(dyn ToSql + Sync)] = &[&user.0, &room.0, &(level as u8 as i8)];
        let res = conn.client.execute(&stmt, args).await;

        handle_sql_error(res)
    }

    pub async fn get_user_room_states(
        &self,
        user: UserId,
        community: CommunityId,
    ) -> DbResult<impl Stream<Item = DbResult<UserRoomState>>> {
        const QUERY: &str = "
            SELECT
                rooms.id AS room,
                user_room_states.watch_level,
                (
                    SELECT user_room_states.last_read IS DISTINCT FROM MAX(messages.ord)
                    FROM messages
                    GROUP BY rooms.id
                ) AS unread
            FROM rooms
            INNER JOIN user_room_states ON rooms.id = user_room_states.room
            WHERE rooms.community = $1 AND user_room_states.user_id = $2
        ";

        let stream = self.query_stream(QUERY, &[&community.0, &user.0]).await?;
        let stream = stream
            .and_then(|row| async move { Ok(UserRoomState::try_from(row)?) })
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
