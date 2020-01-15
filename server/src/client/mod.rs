use actix::prelude::*;

mod session;
mod user;

pub use session::*;
pub use user::*;

#[derive(Debug, Message)]
#[rtype(type = "()")]
pub struct LogoutThisSession;
