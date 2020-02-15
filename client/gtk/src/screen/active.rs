use gtk::prelude::*;

use chat::*;
use community::*;
use dialog::*;
use message::*;
use room::*;

use crate::{AuthParameters, Client, Error, Result, TryGetText};
use crate::auth;
use crate::client;
use crate::connect::AsConnector;
use crate::screen;
use crate::window;

mod community;
mod dialog;
mod message;
mod room;
mod chat;

#[derive(Clone)]
pub struct Ui {
    pub main: gtk::Box,
    content: gtk::Box,
    communities: gtk::ListBox,
    settings_button: gtk::Button,
    add_community_button: gtk::Button,
}

impl Ui {
    fn build() -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/main.glade");

        let main: gtk::Box = builder.get_object("main").unwrap();
        let content: gtk::Box = builder.get_object("content").unwrap();

        let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

        Ui {
            main,
            content,
            communities: builder.get_object("communities").unwrap(),
            settings_button,
            add_community_button: builder.get_object("add_community_button").unwrap(),
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
    }

    fn add_community(&self, name: String) -> CommunityEntryWidget {
        let entry = CommunityEntryWidget::build(name);
        self.communities.add(&entry.expander.widget);
        entry.expander.widget.show_all();

        entry
    }

    fn build_chat_widget(&self) -> ChatWidget {
        let chat = ChatWidget::build();
        self.content.add(&chat.main);
        self.content.set_child_packing(&chat.main, true, true, 0, gtk::PackType::Start);

        self.content.show_all();

        chat
    }

    fn window_focused(&self) -> bool {
        self.main.is_focus()
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
            println!("Encountered error connecting client: {:?}", error);

            let error = describe_error(error);
            let screen = screen::loading::build_error(error, move || start(parameters.clone()));
            window::set_screen(&screen);
        }
    }
}

async fn try_start(parameters: AuthParameters) -> Result<Client<Ui>> {
    let auth = auth::Client::new(parameters.instance);
    let ws = auth.authenticate(parameters.device, parameters.token).await?;

    Ok(Client::start(ws, Ui::build()).await?)
}

fn describe_error(error: Error) -> String {
    match error {
        Error::InvalidUrl => "Invalid instance ip".to_string(),
        Error::Http(http) => format!("{}", http),
        Error::Websocket(ws) => format!("{}", ws),
        Error::ProtocolError(_) => "Protocol error: check your server instance?".to_string(),
        Error::AuthErrorResponse(err) => match err {
            vertex::AuthError::Internal => "Internal server error".to_string(),
            vertex::AuthError::InvalidToken => "Invalid token".to_string(),
            _ => "Unknown auth error".to_string(),
        },

        _ => "Unknown error".to_string(),
    }
}
