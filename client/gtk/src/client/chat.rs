use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use futures::StreamExt;

use vertex::*;

use crate::{Client, net, SharedMut};

use super::ClientUi;
use super::message::*;
use super::room::*;

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
    request: Rc<net::RequestSender>,
    state: SharedMut<ChatState<Ui>>,
    reading_new: Rc<AtomicBool>,
}

impl<Ui: ClientUi> Chat<Ui> {
    pub(super) fn new(request: Rc<net::RequestSender>, widget: Ui::ChatWidget) -> Self {
        Chat {
            request,
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

    pub(crate) async fn push_historic(
        &self,
        client: &Client<Ui>,
        message: HistoricMessage
    ) -> Ui::MessageEntryWidget {
        let profile = client.profiles.get_or_default(message.author, message.author_profile_version).await;
        self.push(client, message.author, profile, message.content).await
    }

    pub(crate) async fn push(
        &self,
        client: &Client<Ui>,
        author: UserId,
        author_profile: UserProfile,
        content: String,
    ) -> Ui::MessageEntryWidget {
        let mut state = self.state.write().await;
        let list = &mut state.widget;

        let rich = RichMessage::parse(content);
        let widget = list.push_message(author, author_profile, rich.text.clone());

        if rich.has_embeds() {
            glib::MainContext::ref_thread_default().spawn_local({
                let client = client.clone();
                let mut widget = widget.clone();

                async move {
                    let embeds = rich.load_embeds();
                    futures::pin_mut!(embeds);

                    while let Some(embed) = embeds.next().await {
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
            Some(stream) => stream.community == accepts.community && stream.room == accepts.room,
            None => false,
        }
    }

    pub async fn set_room(&self, room: Option<&RoomEntry<Ui>>) {
        match room {
            Some(room) => {
                let stream = &room.message_stream;
                let mut state = self.state.write().await;
                state.stream = Some(stream.clone());
                state.widget.set_room(Some(&room));
            }
            None => {
                let mut state = self.state.write().await;
                state.stream = None;
                state.widget.set_room(None);
            }
        }
    }

    pub async fn set_reading_new(&self, reading_new: bool) {
        self.reading_new.store(reading_new, Ordering::SeqCst);

        let state = self.state.read().await;
        if let Some(stream) = &state.stream {
            stream.mark_as_read().await;
        }
    }

    pub fn reading_new(&self) -> bool {
        self.reading_new.load(Ordering::SeqCst)
    }
}
