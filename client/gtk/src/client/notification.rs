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

    pub async fn notify_message(
        &self,
        author: &UserProfile,
        community_name: &str,
        room_name: &str,
        content: &str,
    ) {
        let title = format!("{} in {}", room_name, community_name);
        let content = format!("{}: {}", author.display_name, content);

        let mut icon_path = env::current_dir().unwrap();
        icon_path.push("res");
        icon_path.push("icon.png");

        #[cfg(windows)]
            tokio::task::spawn_blocking(move || {
                // TODO: AppId when we have installer
                let _ = winrt_notification::Toast::new(winrt_notification::Toast::POWERSHELL_APP_ID)
                    .icon(icon_path.as_path(), winrt_notification::IconCrop::Circular, "Vertex")
                    .title(&title)
                    .text1(&content)
                    .sound(None)
                    .duration(winrt_notification::Duration::Short)
                    .show();
            });

        #[cfg(unix)]
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

        if let Some(sound) = &self.sound {
            if let Ok(mut sound) = sound.try_borrow_mut() {
                sound.play();
            }
        }
    }
}
