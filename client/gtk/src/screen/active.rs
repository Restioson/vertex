use std::collections::LinkedList;
use lazy_static::lazy_static;
use gtk::prelude::*;

use crate::{AuthParameters, Client, Error, Result, token_store, scheduler};
use crate::auth;
use crate::client::RoomEntry;
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;
use crate::window;
use std::time::{Instant, Duration};
use std::sync::RwLock;
use std::rc::Rc;
use gdk::enums::key;
use vertex::requests::AuthError;

pub mod community;
pub mod dialog;
pub mod message;
pub mod room;
pub mod chat;
pub mod message_scroll;

pub use chat::*;
pub use community::*;
pub use dialog::*;
pub use message::*;
pub use room::*;

struct MessageScrollState {
    bottom: f64,
    top: f64,
    last_scrolled: Instant,
    just_scrolled_up: Option<f64>,
}

impl Default for MessageScrollState {
    fn default() -> Self {
        MessageScrollState {
            bottom: 0.0,
            top: 0.0,
            last_scrolled: Instant::now() - Duration::from_secs(1),
            just_scrolled_up: None,
        }
    }
}

#[derive(Clone)]
pub struct Ui {
    pub main: gtk::Box,
    content: gtk::Box,
    communities: gtk::ListBox,
    settings_button: gtk::Button,
    add_community_button: gtk::Button,

    pub chat: gtk::Box,
    pub room_name: gtk::Label,
    pub message_scroll: gtk::ScrolledWindow,
    pub message_list: gtk::ListBox,
    pub message_entry: gtk::TextView,

    message_scroll_state: Rc<RwLock<MessageScrollState>>,
}

impl Ui {
    fn build() -> Self {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/main.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();
        let message_entry: gtk::TextView = builder.get_object("message_entry").unwrap();

        Ui {
            main: builder.get_object("main").unwrap(),
            content: builder.get_object("content").unwrap(),
            communities: builder.get_object("communities").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
            add_community_button: builder.get_object("add_community_button").unwrap(),

            chat: builder.get_object("chat").unwrap(),
            room_name: builder.get_object("room_name").unwrap(),
            message_scroll: builder.get_object("message_scroll").unwrap(),
            message_list: builder.get_object("message_list").unwrap(),
            message_entry,
            message_scroll_state: Rc::new(RwLock::new(MessageScrollState::default())),
        }
    }

    fn clear_messages(&self) {
        for child in self.message_list.get_children() {
            self.message_list.remove(&child);
        }
    }
}

impl Ui {
    pub fn bind_events(&self, client: &Client) {
        self.settings_button.connect_clicked(
            client.connector()
                .do_async(|client, _| async move {
                    let screen = screen::settings::build(client).await;
                    window::set_screen(&screen.main);
                })
                .build_cloned_consumer()
        );

        self.add_community_button.connect_clicked(
            client.connector()
                .do_sync(|screen, _| show_add_community(screen))
                .build_cloned_consumer()
        );

        let client_cloned = client.clone();
        self.message_entry.connect_focus_out_event(
            move |entry, _| {
                let client = client_cloned.clone();
                let buf = entry.get_buffer().unwrap();
                let (begin, end) = buf.get_bounds();

                let entry = entry.clone();
                scheduler::spawn(async move {
                    let state = client.state.upgrade().unwrap();
                    let mut state = state.write().await;

                    state.message_entry_is_empty = begin == end;

                    if state.selected_room.is_none() {
                        entry.get_buffer().unwrap().set_text("Select a room to send a message...");
                    } else if state.message_entry_is_empty {
                        entry.get_buffer().unwrap().set_text("Send a message...");
                    }
                });

                Inhibit(false)
        });

        let client_cloned = client.clone();
        self.message_entry.connect_focus_in_event(
            move |entry, _| {
                let entry = entry.clone();
                let client = client_cloned.clone();
                scheduler::spawn(async move {
                    let state = client.state.upgrade().unwrap();
                    let state = state.read().await;

                    if entry.has_focus() && state.message_entry_is_empty &&
                        state.selected_room.is_some()
                    {
                        entry.get_buffer().unwrap().set_text("");
                    }
                });

                Inhibit(false)
            }
        );

        let client_cloned = client.clone();
        self.message_entry.connect_key_press_event(
            move |entry, key_event| {
                let client = client_cloned.clone();
                match key_event.get_keyval() {
                    key::Return => {},
                    key::Escape => {
                        entry.grab_remove();
                        return Inhibit(true);
                    },
                    _ => return Inhibit(false),
                }

                if key_event.get_state().contains(gdk::ModifierType::SHIFT_MASK) {
                    return Inhibit(false);
                }

                let entry = entry.clone();
                scheduler::spawn(async move {
                    if let Some(selected_room) = client.selected_room().await {
                        let buf = entry.get_buffer().unwrap();
                        let (begin, end) = &buf.get_bounds();
                        let content = buf.get_text(begin, end, false);
                        let content = content.as_ref().map(|c| c.as_str()).unwrap_or_default();

                        if !content.trim().is_empty() {
                            buf.set_text("");
                            selected_room.send_message(content.to_string()).await;
                        }
                    }
                });

                Inhibit(true)
            }
        );


    }

    pub fn select_room(&self, room: &RoomEntry) -> ChatWidget {
        self.clear_messages();

        self.message_entry.set_editable(true);
        self.message_entry.get_style_context().remove_class("disabled");
        self.message_entry.get_buffer().unwrap().set_text("");

        self.room_name.set_text(&room.name);

        ChatWidget {
            main: self.chat.clone(),
            room_name: self.room_name.clone(),
            message_scroll: self.message_scroll.clone(),
            message_list: self.message_list.clone(),
            message_entry: self.message_entry.clone(),
            groups: LinkedList::new(),
        }
    }

    pub fn deselect_room(&self) {
        self.clear_messages();

        self.message_entry.set_editable(false);
        self.message_entry.get_style_context().add_class("disabled");
        self.message_entry.get_buffer().unwrap().set_text("Select a room to send a message...");

        self.room_name.set_text("");
    }

    pub fn add_community(&self, name: String, description: String) -> CommunityEntryWidget {
        let entry = CommunityEntryWidget::build(name, description);
        self.communities.add(&entry.widget);
        entry.widget.show_all();

        entry
    }

    pub fn window_focused(&self) -> bool {
        window::is_focused()
    }
}

pub async fn start(parameters: AuthParameters) {
    let loading = screen::loading::build();
    window::set_screen(&loading);

    match try_start(parameters.clone()).await {
        Ok(client) => {
            window::set_screen(&client.ui.main);
        }
        Err(error) => {
            log::error!("encountered error connecting client: {:?}", error);

            match error {
                Error::AuthErrorResponse(e) => {
                    if e != AuthError::TokenInUse || e != AuthError::UserCompromised {
                        token_store::forget_token();
                    }

                    if e == AuthError::UserCompromised {
                        let screen = screen::compromised::build(parameters).await;
                        window::set_screen(&screen.main);
                    } else {
                        let screen = screen::login::build().await;
                        window::set_screen(&screen.main);
                    }
                }
                _ => {
                    let error = describe_error(error);
                    let screen = screen::loading::build_error(error, move || start(parameters.clone()));
                    window::set_screen(&screen);
                }
            }
        }
    }
}

async fn try_start(parameters: AuthParameters) -> Result<Client> {
    let auth = auth::Client::new(parameters.instance);
    let ws = auth.login(parameters.device, parameters.token).await?;

    Ok(Client::start(ws, Ui::build(), auth.server.url().scheme() == "https").await?)
}

fn describe_error(error: Error) -> String {
    match error {
        Error::InvalidUrl => "Invalid instance ip".to_string(),
        Error::Http(http) => format!("{}", http),
        Error::Websocket(ws) => format!("{}", ws),
        Error::ProtocolError(_) => "Protocol error: check your server instance?".to_string(),
        Error::AuthErrorResponse(err) => match err {
            AuthError::Internal => "Internal server error".to_string(),
            AuthError::InvalidToken => "Invalid token".to_string(),
            _ => "Unknown auth error".to_string(),
        },

        _ => "Unknown error".to_string(),
    }
}
