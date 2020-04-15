use crate::client::ActiveSession;
use vertex::prelude::*;

pub struct RequestHandler<'a> {
    pub session: &'a mut ActiveSession,
    pub perms: AdminPermissionFlags,
}

impl<'a> RequestHandler<'a> {
    pub async fn handle_request(self, request: AdminRequest) -> Result<OkResponse, Error> {
        match request {
            _ => Err(Error::Unimplemented),
        }
    }
}
