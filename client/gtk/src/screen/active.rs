use std::collections::LinkedList;

use gtk::prelude::*;

use chat::*;
use community::*;
use dialog::*;
use lazy_static::lazy_static;
use message::*;
use room::*;

use crate::{AuthParameters, Client, Error, Result, token_store, TryGetText};
use crate::auth;
use crate::client;
use crate::client::RoomEntry;
use crate::connect::AsConnector;
use crate::Glade;
use crate::screen;
use crate::window;
use std::time::{Instant, Duration};
use std::sync::RwLock;
use std::rc::Rc;

mod community;
mod dialog;
mod message;
mod room;
mod chat;

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
    pub message_entry: gtk::Entry,

    message_scroll_state: Rc<RwLock<MessageScrollState>>,
}

impl Ui {
    fn build() -> Self {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("res/glade/active/main.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();

        let main: gtk::Box = builder.get_object("main").unwrap();
        let content: gtk::Box = builder.get_object("content").unwrap();

        let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

        Ui {
            main,
            content,
            communities: builder.get_object("communities").unwrap(),
            settings_button,
            add_community_button: builder.get_object("add_community_button").unwrap(),

            chat: builder.get_object("chat").unwrap(),
            room_name: builder.get_object("room_name").unwrap(),
            message_scroll: builder.get_object("message_scroll").unwrap(),
            message_list: builder.get_object("message_list").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            message_scroll_state: Rc::new(RwLock::new(MessageScrollState::default())),
        }
    }

    fn clear_messages(&self) {
        for child in self.message_list.get_children() {
            self.message_list.remove(&child);
        }
    }
}

impl client::ClientUi for Ui {
    type CommunityEntryWidget = CommunityEntryWidget;
    type RoomEntryWidget = RoomEntryWidget;
    type ChatWidget = ChatWidget;
    type MessageEntryWidget = MessageEntryWidget;

    fn bind_events(&self, client: &Client<Ui>) {
        self.settings_button.connect_button_release_event(
            client.connector()
                .do_async(|client, (_button, _event)| async move {
                    let screen = screen::settings::build(client);
                    window::set_screen(&screen.main);
                })
                .build_widget_event()
        );

        self.add_community_button.connect_button_release_event(
            client.connector()
                .do_sync(|screen, _| show_add_community(screen))
                .build_widget_event()
        );

        self.message_entry.connect_activate(
            client.connector()
                .do_async(|client, entry: gtk::Entry| async move {
                    if let Some(selected_room) = client.selected_room().await {
                        let content = entry.try_get_text().unwrap_or_default();
                        if !content.trim().is_empty() {
                            entry.set_text("");
                            selected_room.send_message(content).await;
                        }
                    }
                })
                .build_cloned_consumer()
        );

        let adjustment = self.message_scroll.get_vadjustment().unwrap();
        adjustment.connect_value_changed(
            (client.clone(), self.message_scroll_state.clone()).connector()
                .do_async(|(client, scroll), adjustment: gtk::Adjustment| async move {
                    if let Some(chat) = client.chat().await {
                        let mut state = scroll.write().unwrap();
                        match &mut state.just_scrolled_up {
                            Some(v) if adjustment.get_value() <= f64::EPSILON => {
                                // Roll back - this change is due to a gtk glitch
                                adjustment.set_value(*v);
                            },
                            opt @ Some(_) => *opt = None,
                            None => {}
                        }

                        let upper = adjustment.get_upper() - adjustment.get_page_size();
                        let reading_new = adjustment.get_value() + 10.0 >= upper;
                        chat.set_reading_new(reading_new).await;
                    }
                })
                .build_cloned_consumer()
        );

        self.message_scroll.connect_edge_reached(
            (client.clone(), self.message_scroll_state.clone()).connector()
                .do_async(|(client, scroll_state), (_scroll, position)| async move {
                    if let Some(chat) = client.chat().await {
                        let state = scroll_state.read().unwrap();
                        if state.just_scrolled_up.is_none() {
                            let _ = match position {
                                gtk::PositionType::Top => {
                                    if state.last_scrolled.elapsed() > Duration::from_secs(1) {
                                        drop(state);
                                        chat.extend_older().await.map(|_| {
                                            let mut state = scroll_state.write().unwrap();
                                            state.last_scrolled = Instant::now();
                                        })
                                    } else {
                                        Ok(())
                                    }
                                },
                                gtk::PositionType::Bottom => {
                                    drop(state);
                                    chat.extend_newer().await
                                },
                                _ => Ok(()),
                            };
                        }

                        // TODO: handle error
                    }
                })
                .build_widget_and_owned_listener()
        );

        self.message_list.connect_size_allocate(
            (self.message_scroll_state.clone(), adjustment).connector()
                .do_async(|(scroll_state, adjustment), (_, _)| async move {
                    let mut old = scroll_state.write().unwrap();

                    let new_bottom = adjustment.get_upper() - adjustment.get_page_size();
                    let new_top = adjustment.get_lower();

                    if old.bottom == new_bottom {
                        return;
                    }

                    let old_value = adjustment.get_value();

                    let on_bottom = old_value + 10.0 >= old.bottom;
                    let on_top = old_value - 10.0 <= old.top;

                    if on_top || on_bottom {
                        let mut val = (new_bottom - old.bottom) + old_value;

                        if on_top {
                            if old_value != new_top && val > adjustment.get_step_increment() {
                                val -= adjustment.get_step_increment();
                            }
                            adjustment.set_value(val);
                            old.just_scrolled_up = Some(val);
                        }

                        adjustment.set_value(val);
                    }

                    old.bottom = new_bottom;
                    old.top = new_top;
                })
                .build_widget_listener()
        );
    }

    fn select_room(&self, room: &RoomEntry<Self>) -> ChatWidget {
        self.clear_messages();

        self.message_entry.set_can_focus(true);
        self.message_entry.set_editable(true);

        self.message_entry.set_placeholder_text(Some("Send message..."));
        self.message_entry.get_style_context().remove_class("disabled");

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

    fn deselect_room(&self) {
        self.clear_messages();

        self.message_entry.set_can_focus(false);
        self.message_entry.set_editable(false);

        self.message_entry.set_placeholder_text(Some("Select a room to send messages..."));
        self.message_entry.get_style_context().add_class("disabled");

        self.room_name.set_text("");
    }

    fn add_community(&self, name: String) -> CommunityEntryWidget {
        let entry = CommunityEntryWidget::build(name);
        self.communities.add(&entry.expander.widget);
        entry.expander.widget.show_all();

        entry
    }

    fn window_focused(&self) -> bool {
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
            println!("encountered error connecting client: {:?}", error);

            match error {
                Error::AuthErrorResponse(_) => {
                    token_store::forget_token();
                    let screen = screen::login::build().await;
                    window::set_screen(&screen.main);
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

async fn try_start(parameters: AuthParameters) -> Result<Client<Ui>> {
    let auth = auth::Client::new(parameters.instance);
    let ws = auth.login(parameters.device, parameters.token).await?;

    Ok(Client::start(ws, Ui::build()).await?)
}

fn describe_error(error: Error) -> String {
    use vertex::requests::AuthError;
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
