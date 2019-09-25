use actix::prelude::*;

mod server;
mod session;

pub use server::*;
pub use session::*;

#[derive(Debug, Message)]
pub struct LogoutThisSession;
