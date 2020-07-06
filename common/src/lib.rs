//! Some definitions common between server and client

#![feature(try_trait)]

use std::time::Duration;
use std::fs;
use std::fs::OpenOptions;
use chrono::SecondsFormat;
use log::LevelFilter;

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
    pub use crate::panic_error;
}

pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

pub const RATELIMIT_BURST_PER_MIN: u32 = 120;

pub fn setup_logging(
    name: &str,
    log_level: log::LevelFilter,
) {
    let dirs = directories_next::ProjectDirs::from("", "vertex_chat", name)
        .expect("Error getting project directories");
    let dir = dirs.data_dir().join("logs");

    fs::create_dir_all(&dir)
        .unwrap_or_else(|_| panic!("Error creating log dirs ({})", dir.to_string_lossy()));

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] [{}] {}",
                chrono::Local::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log_level)
        .level_for("hyper", LevelFilter::Info)
        .level_for("selectors", LevelFilter::Info)
        .level_for("html5ever", LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(
                    dir.join(
                        chrono::Local::now()
                            .format(&format!("{}_%Y-%m-%d_%H-%M-%S.log", name))
                            .to_string(),
                    ),
                )
                .expect("Error opening log file"),
        )
        .apply()
        .expect("Error setting logger settings");

    log::info!("Logging set up");
}

#[macro_export]
macro_rules! panic_error {
    ($($tt:tt)*) => {
        {
            log::error!($($tt)*);
            panic!($($tt)*)
        }
    }
}
