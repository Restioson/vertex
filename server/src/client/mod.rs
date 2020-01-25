mod session;
mod user;

pub use session::*;
pub use user::*;
use xtra::Message;

#[derive(Debug)]
pub struct LogoutThisSession;

impl Message for LogoutThisSession {
    type Result = ();
}
