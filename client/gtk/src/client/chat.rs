use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use vertex::*;

use crate::{Client, Error, SharedMut};

use super::ClientUi;
use super::message::*;
use super::room::*;

fn create_fallback_profile(author: UserId) -> UserProfile {
    let name = format!("{}", author.0);
    UserProfile {
        version: ProfileVersion(0),
        username: name.clone(),
        display_name: name,
    }
}

pub trait ChatWidget<Ui: ClientUi> {
    fn set_room(&mut self, room: Option<&RoomEntry<Ui>>);

    fn push_message(&mut self, author: UserId, author_profile: UserProfile, content: String) -> Ui::MessageEntryWidget;

    fn bind_events(&self, client: &Client<Ui>, chat: &Chat<Ui>);
}

pub struct ChatState<Ui: ClientUi> {
    widget: Ui::ChatWidget,
    stream: Option<MessageStream<Ui>>,
}

#[derive(Clone)]
pub struct Chat<Ui: ClientUi> {
    state: SharedMut<ChatState<Ui>>,
    reading_new: Rc<AtomicBool>,
}

impl<Ui: ClientUi> Chat<Ui> {
    pub(super) fn new(widget: Ui::ChatWidget) -> Self {
        Chat {
            state: SharedMut::new(ChatState {
                widget,
                stream: None,
            }),
            reading_new: Rc::new(AtomicBool::new(true)),
        }
    }

    pub(super) async fn bind_events(&self, client: &Client<Ui>) {
        let state = self.state.read().await;
        state.widget.bind_events(client, &self);
    }

    pub(crate) async fn push(&self, client: &Client<Ui>, message: MessageSource) -> Ui::MessageEntryWidget {
        let profile = match client.profiles.get(message.author, message.author_profile_version).await {
            Ok(profile) => profile,
            Err(err) => {
                println!("failed to load profile for {:?}: {:?}", message.author, err);
                client.profiles.get_existing(message.author, None).await
                    .unwrap_or_else(|| create_fallback_profile(message.author))
            }
        };

        let mut state = self.state.write().await;
        let list = &mut state.widget;

        let rich = RichMessage::parse(message.content);
        let widget = list.push_message(message.author, profile, rich.text.clone());

        if rich.has_embeds() {
            glib::MainContext::ref_thread_default().spawn_local({
                let client = client.clone();
                let mut widget = widget.clone();
                async move {
                    for embed in rich.load_embeds().await {
                        widget.push_embed(&client, embed);
                    }
                }
            });
        }

        widget
    }

    pub(crate) async fn accepts(&self, accepts: &MessageStream<Ui>) -> bool {
        let state = self.state.read().await;
        match &state.stream {
            Some(stream) => stream.id == accepts.id,
            None => false,
        }
    }

    async fn populate_list(&self, stream: &MessageStream<Ui>) {
        let mut messages = Vec::with_capacity(50);
        stream.read_last(50, &mut messages).await;

        for message in messages {
            self.push(&stream.client, message).await;
        }
    }

    pub async fn set_room(&self, room: Option<&RoomEntry<Ui>>) {
        match room {
            Some(room) => {
                let stream = &room.message_stream;
                {
                    let mut state = self.state.write().await;
                    state.stream = Some(stream.clone());
                    state.widget.set_room(Some(&room));
                }

                self.populate_list(&stream).await;
            }
            None => {
                let mut state = self.state.write().await;
                state.stream = None;
                state.widget.set_room(None);
            }
        }
    }

    pub fn set_reading_new(&self, reading_new: bool) {
        self.reading_new.store(reading_new, Ordering::SeqCst);
    }

    pub fn reading_new(&self) -> bool {
        self.reading_new.load(Ordering::SeqCst)
    }
}
