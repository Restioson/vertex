use vertex_common::{RoomId, ServerError, CommunityId};
use tokio_postgres::Row;
use std::convert::TryFrom;
use actix::{Message, Handler, ResponseFuture, Context};
use crate::database::{DatabaseServer, handle_error};
use futures::{Future, Stream};
use uuid::Uuid;

pub(super) const CREATE_COMMUNITIES_TABLE: &'static str = "
CREATE TABLE IF NOT EXISTS communities (
    id   UUID PRIMARY KEY,
    name VARCHAR NOT NULL
)";

#[derive(Debug, Clone)]
pub struct CommunityRecord {
    pub id: CommunityId,
    pub name: String,
}

impl TryFrom<Row> for CommunityRecord {
    type Error = tokio_postgres::Error;

    fn try_from(row: Row) -> Result<CommunityRecord, tokio_postgres::Error> {
        Ok(CommunityRecord {
            id: CommunityId(row.try_get("id")?),
            name: row.try_get("name")?,
        })
    }
}

pub struct GetCommunityMetadata(CommunityId);

impl Message for GetCommunityMetadata {
    type Result = Result<Option<CommunityRecord>, ServerError>;
}

pub struct CreateCommunity {
    pub name: String,
}

impl Message for CreateCommunity {
    type Result = Result<CommunityRecord, ServerError>;
}

// TODO(next): load at boot
impl Handler<GetCommunityMetadata> for DatabaseServer {
    type Result = ResponseFuture<Option<CommunityRecord>, ServerError>;

    fn handle(&mut self, get: GetCommunityMetadata, _: &mut Context<Self>) -> Self::Result {
        Box::new(
            self.pool
                .connection()
                .and_then(move |mut conn| {
                    conn.client
                        .prepare("SELECT * FROM communities WHERE id=$1")
                        .and_then(move |stmt| {
                            conn.client
                                .query(&stmt, &[&(get.0).0])
                                .map(|row| CommunityRecord::try_from(row))
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

impl Handler<CreateCommunity> for DatabaseServer {
    type Result = ResponseFuture<CommunityRecord, ServerError>;

    fn handle(&mut self, create: CreateCommunity, _: &mut Context<Self>) -> Self::Result {
        let id = Uuid::new_v4();

        Box::new(
            self.pool
                .connection()
                .and_then(move |mut conn| {
                    conn.client
                        .prepare("INSERT INTO communities (id, name) VALUES ($1, $2) RETURNING *")
                        .and_then(move |stmt| {
                            conn.client.query(
                                    &stmt,
                                    &[&id, &create.name],
                                ).map(|row| CommunityRecord::try_from(row))
                                .into_future()
                                .map(|(community, _stream)| community)
                                .map_err(|(err, _stream)| err)
                        })
                        .and_then(|x| x.transpose())
                        .map(|res| res.expect("Create community query did not return anything"))
                        .map_err(l337::Error::External)
                })
                .map_err(handle_error),
        )
    }
}
