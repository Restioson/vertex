use std::sync::Arc;

use arc_swap::ArcSwapOption;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub narrate_new_messages: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            narrate_new_messages: false,
        }
    }
}

const CONFIG_NAME: &str = "vertex-client";

static CONFIG: Lazy<ArcSwapOption<Config>> = Lazy::new(|| ArcSwapOption::empty());

pub fn modify<F: FnOnce(&mut Config)>(f: F) {
    let mut config = (*get()).clone();
    f(&mut config);
    commit(config);
}

pub fn commit(config: Config) {
    CONFIG.store(Some(Arc::new(config.clone())));
    if let Err(err) = confy::store(CONFIG_NAME, config) {
        eprintln!("failed to commit config: {:?}", err);
    }
}

pub fn get() -> Arc<Config> {
    let config = CONFIG.load_full();
    if let Some(config) = config {
        return config;
    }

    match confy::load::<Config>(CONFIG_NAME) {
        Ok(config) => {
            let config = Arc::new(config);
            CONFIG.store(Some(config.clone()));
            config
        }
        Err(err) => {
            eprintln!("failed to load config: {:?}", err);
            Arc::new(Config::default())
        }
    }
}
