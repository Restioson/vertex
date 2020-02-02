use xtra::Message;

pub use auth::*;
pub use session::{ActiveSession, Session};

pub mod session;

mod auth;

#[derive(Debug)]
pub struct LogoutThisSession;

impl Message for LogoutThisSession {
    type Result = ();
}
