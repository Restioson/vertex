use std::cell::RefCell;
use std::env;
use std::rc::Rc;

use ears::{AudioController, Sound};

use vertex::*;

#[derive(Clone)]
pub struct Notifier {
    sound: Option<Rc<RefCell<Sound>>>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Notifier {
    pub fn new() -> Self {
        let sound = Sound::new("res/notification_sound_clearly.ogg").ok();
        Notifier {
            sound: sound.map(|sound| Rc::new(RefCell::new(sound))),
        }
    }

    pub async fn notify_message(&self, author: &UserProfile, content: &str) {
        let body = format!("{}: {}", author.display_name, content);

        #[cfg(windows)]
            notifica::notify("Vertex", &body);

        #[cfg(unix)]
            {
                let mut icon_path = env::current_dir().unwrap();
                icon_path.push("res");
                icon_path.push("icon.png");

                tokio::task::spawn_blocking(move || {
                    let res = notify_rust::Notification::new()
                        .summary("Vertex")
                        .appname("Vertex")
                        .icon(&icon_path.to_str().unwrap())
                        .body(&body)
                        .show();

                    if let Ok(handle) = res {
                        handle.on_close(|| {});
                    }
                });
            };

        if let Some(sound) = &self.sound {
            if let Ok(mut sound) = sound.try_borrow_mut() {
                sound.play();
            }
        }
    }
}
