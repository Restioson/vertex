use actix::prelude::*;

mod user;
mod session;

pub use user::*;
pub use session::*;

#[derive(Debug, Message)]
#[rtype(type = "()")]
pub struct LogoutThisSession;
