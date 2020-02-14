use gtk::prelude::*;

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

#[derive(Clone)]
pub struct Ui {
    pub main: gtk::Viewport,
    communities: gtk::ListBox,
    messages_scroll: gtk::ScrolledWindow,
    messages_list: gtk::ListBox,
    message_entry: gtk::Entry,
    settings_button: gtk::Button,
    add_community_button: gtk::Button,
}

impl Ui {
    fn build() -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/active.glade");

        let main: gtk::Viewport = builder.get_object("main").unwrap();

        let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();
        let settings_image = settings_button.get_child()
            .and_then(|img| img.downcast::<gtk::Image>().ok())
            .unwrap();

        settings_image.set_from_pixbuf(Some(
            &gdk_pixbuf::Pixbuf::new_from_file_at_size(
                "res/feather/settings.svg",
                20, 20,
            ).unwrap()
        ));

        Ui {
            main: main.clone(),
            communities: builder.get_object("communities").unwrap(),
            messages_scroll: builder.get_object("messages_scroll").unwrap(),
            messages_list: builder.get_object("messages").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button,
            add_community_button: builder.get_object("add_community_button").unwrap(),
        }
    }

    fn bind_events(&self, client: &Client<Ui>) {
        self.message_entry.connect_activate(
            client.connector()
                .do_async(|client, entry: gtk::Entry| async move {
                    if let Some(selected_room) = client.selected_room().await {
                        let content = entry.try_get_text().unwrap_or_default();
                        if !content.trim().is_empty() {
                            entry.set_text("");
                            let _ = selected_room.send_message(content).await;
                        }
                    }
                })
                .build_cloned_consumer()
        );

        self.settings_button.connect_button_press_event(
            client.connector()
                .do_async(|client, (_button, _event)| async move {
                    let screen = screen::settings::build(client);
                    window::set_screen(&screen.main);
                })
                .build_widget_event()
        );

        self.add_community_button.connect_button_press_event(
            client.connector()
                .do_sync(|screen, _| show_add_community(screen))
                .build_widget_event()
        );
    }
}

impl client::ClientUi for Ui {
    type CommunityEntryWidget = CommunityEntryWidget;
    type RoomEntryWidget = RoomEntryWidget;
    type MessageListWidget = MessageListWidget;
    type MessageEntryWidget = MessageEntryWidget;

    fn add_community(&self, name: String) -> CommunityEntryWidget {
        let widget = CommunityEntryWidget::build(name);

        self.communities.add(&widget.expander);
        widget.expander.show_all();

        widget
    }

    fn build_message_list(&self) -> MessageListWidget {
        MessageListWidget {
            scroll: self.messages_scroll.clone(),
            list: self.messages_list.clone(),
            last_group: None,
        }
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
            client.ui.bind_events(&client);
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
