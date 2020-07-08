use std::cell::RefCell;
use std::env;
use std::rc::Rc;

use ears::{AudioController, Sound};

use vertex::prelude::*;
use crate::resource;

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
        let sound = Sound::new(&resource("notification_sound_clearly.ogg")).ok();
        Notifier {
            sound: sound.map(|sound| Rc::new(RefCell::new(sound))),
        }
    }

    pub async fn notify_message(
        &self,
        author: &Profile,
        community_name: &str,
        room_name: &str,
        content: Option<&str>,
        a11y_narration: bool, // Whether the notification is just for a11y to narrate new msgs
    ) {
        let title = if a11y_narration {
            "Vertex".to_string()
        } else {
            format!("{} in {}", room_name, community_name)
        };

        let content = if let Some(content) = content {
            format!("{}: {}", author.display_name, content)
        } else {
            format!("{}: <Deleted>", author.display_name) // TODO deletion
        };

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
                .hint(notify_rust::Hint::Transient(true))
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
