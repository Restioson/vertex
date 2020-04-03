//! Some definitions common between server and client

#![feature(try_trait)]

use std::time::Duration;

pub mod events;
pub mod proto;
pub mod requests;
pub mod responses;
pub mod structures;
pub mod types;

pub mod prelude {
    pub use crate::events::*;
    pub use crate::requests::*;
    pub use crate::responses::*;
    pub use crate::structures::*;
    pub use crate::types::*;
    pub use crate::HEARTBEAT_TIMEOUT;
}

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
