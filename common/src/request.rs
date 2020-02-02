pub use active::*;
pub use auth::*;

use crate::*;

mod auth;
mod active;

/// Does not need to be sequential; just unique within a desired time-span (or not, if you're a fan
/// of trying to handle two responses with the same id attached). This exists for the client-side
/// programmer's ease-of-use only - the server is request-id-agnostic.
#[derive(Hash, Eq, PartialEq, Debug, Copy, Clone, Serialize, Deserialize)]
pub struct RequestId(u32);

impl RequestId {
    pub const fn new(id: u32) -> Self {
        RequestId(id)
    }
}
