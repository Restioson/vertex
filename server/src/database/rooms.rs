use vertex_common::{RoomId, ServerError};
use tokio_postgres::Row;
use std::convert::TryFrom;
use actix::{Message, Handler, ResponseFuture, Context};
use crate::database::{DatabaseServer, handle_error};
use futures::{Future, Stream};
use uuid::Uuid;

pub(super) const CREATE_ROOMS_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS rooms (
    id   UUID PRIMARY KEY,
    name VARCHAR NOT NULL
)";

#[derive(Debug, Clone)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
}

impl TryFrom<Row> for Room {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<Room, tokio_postgres::Error> {
        Ok(Room {
            id: RoomId(row.try_get("id")?),
            name: row.try_get("name")?,
        })
    }
}

pub struct GetRoom(RoomId);

impl Message for GetRoom {
    type Result = Result<Option<Room>, ServerError>;
}

pub struct CreateRoom {
    pub name: String,
}

impl Message for CreateRoom {
    type Result = Result<Room, ServerError>;
}

// TODO(next): load at boot
impl Handler<GetRoom> for DatabaseServer {
    type Result = ResponseFuture<Option<Room>, ServerError>;

    fn handle(&mut self, get: GetRoom, _: &mut Context<Self>) -> Self::Result {
        Box::new(
            self.pool
                .connection()
                .and_then(move |mut conn| {
                    conn.client
                        .prepare("SELECT * FROM rooms WHERE id=$1")
                        .and_then(move |stmt| {
                            conn.client
                                .query(&stmt, &[&(get.0).0])
                                .map(|row| Room::try_from(row))
                                .into_future()
                                .map(|(user, _stream)| user)
                                .map_err(|(err, _stream)| err)
                        })
                        .and_then(|x| x.transpose()) // Fut<Opt<Res<Usr, Err>>, Err> -> Fut<Opt<Usr>, Err>
                        .map_err(l337::Error::External)
                })
                .map_err(handle_error),
        )
    }
}

impl Handler<CreateRoom> for DatabaseServer {
    type Result = ResponseFuture<Room, ServerError>;

    fn handle(&mut self, create: CreateRoom, _: &mut Context<Self>) -> Self::Result {
        let id = Uuid::new_v4();

        Box::new(
            self.pool
                .connection()
                .and_then(move |mut conn| {
                    conn.client
                        .prepare("INSERT INTO rooms (id, name) VALUES ($1, $2) RETURNING *")
                        .and_then(move |stmt| {
                            conn.client.query(
                                    &stmt,
                                    &[&id, &create.name],
                                ).map(|row| Room::try_from(row))
                                .into_future()
                                .map(|(room, _stream)| room)
                                .map_err(|(err, _stream)| err)
                        })
                        .and_then(|x| x.transpose())
                        .map(|res| res.expect("Create room query did not return anything"))
                        .map_err(l337::Error::External)
                })
                .map_err(handle_error),
        )
    }
}
