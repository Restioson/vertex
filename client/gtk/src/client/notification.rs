use std::cell::RefCell;
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

    pub async fn notify_message(
        &self,
        author: &UserProfile,
        community_name: &str,
        room_name: &str,
        content: &str
    ) {
        let title = format!("{} in {} - {}", community_name, room_name, author.display_name);
        let content = content.to_owned();

        #[cfg(windows)]
        notifica::notify(&title, &content);

        #[cfg(unix)]
        {
            let mut icon_path = std::env::current_dir().unwrap();
            icon_path.push("res");
            icon_path.push("icon.png");

            tokio::task::spawn_blocking(move || {
                let res = notify_rust::Notification::new()
                    .summary(&title)
                    .appname("Vertex")
                    .icon(&icon_path.to_str().unwrap())
                    .body(&content)
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
