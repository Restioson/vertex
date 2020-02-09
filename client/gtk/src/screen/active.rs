use gtk::prelude::*;

use vertex::*;

use crate::{Client, client, screen, TryGetText, window};
use crate::auth;
use crate::client::MessageStatus;
use crate::connect::AsConnector;

#[derive(Clone)]
pub struct Ui {
    pub main: gtk::Viewport,
    communities: gtk::ListBox,
    messages: gtk::ListBox,
    message_entry: gtk::Entry,
    settings_button: gtk::Button,
    add_community_button: gtk::Button,
}

impl Ui {
    fn build() -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/active.glade");

        let main: gtk::Viewport = builder.get_object("main").unwrap();

        Ui {
            main: main.clone(),
            communities: builder.get_object("communities").unwrap(),
            messages: builder.get_object("messages").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
            add_community_button: builder.get_object("add_community_button").unwrap(),
        }
    }

    fn bind_events(&self, client: &Client<Ui>) {
        self.message_entry.connect_activate(
            client.connector()
                .do_async(|client, entry: gtk::Entry| async move {
                    if let Some(selected_room) = client.selected_room().await {
                        let content = entry.try_get_text().unwrap_or_default();
                        entry.set_text("");

                        // TODO: error handling
                        selected_room.send_message(content).await.unwrap();
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
        MessageListWidget { list: self.messages.clone(), last_group: None }
    }
}

#[derive(Clone)]
pub struct CommunityEntryWidget {
    expander: gtk::Expander,
    room_list: gtk::ListBox,
    invite_button: gtk::Button,
    settings_button: gtk::Button,
}

impl CommunityEntryWidget {
    fn build(name: String) -> Self {
        let builder = gtk::Builder::new_from_file("res/glade/active/community_entry.glade");

        let expander: gtk::Expander = builder.get_object("community_expander").unwrap();

        let community_name: gtk::Label = builder.get_object("community_name").unwrap();
        let community_motd: gtk::Label = builder.get_object("community_motd").unwrap();

        let invite_button: gtk::Button = builder.get_object("invite_button").unwrap();
        let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();

        let room_list: gtk::ListBox = builder.get_object("room_list").unwrap();

        community_name.set_text(&name);
        community_motd.set_text("5 users online");

        let settings_image = settings_button.get_child()
            .and_then(|img| img.downcast::<gtk::Image>().ok())
            .unwrap();

        settings_image.set_from_pixbuf(Some(
            &gdk_pixbuf::Pixbuf::new_from_file_at_size(
                "res/feather/settings.svg",
                20, 20,
            ).unwrap()
        ));

        let invite_image = invite_button.get_child()
            .and_then(|img| img.downcast::<gtk::Image>().ok())
            .unwrap();

        invite_image.set_from_pixbuf(Some(
            &gdk_pixbuf::Pixbuf::new_from_file_at_size(
                "res/feather/user-plus.svg",
                20, 20,
            ).unwrap()
        ));

        CommunityEntryWidget {
            expander,
            room_list,
            invite_button,
            settings_button,
        }
    }
}

impl client::CommunityEntryWidget<Ui> for CommunityEntryWidget {
    fn bind_events(&self, community_entry: &client::CommunityEntry<Ui>) {
        self.invite_button.connect_button_press_event(
            community_entry.connector()
                .do_async(move |community_entry, (_widget, event)| async move {
                    // TODO: error handling
                    let invite = community_entry.create_invite(None).await.expect("failed to create invite");

                    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/invite_community.glade");
                    let main: gtk::Box = builder.get_object("main").unwrap();

                    let code_view: gtk::TextView = builder.get_object("code_view").unwrap();
                    if let Some(code_view) = code_view.get_buffer() {
                        code_view.set_text(&invite.0);
                    }

                    code_view.connect_button_release_event(|code_view, _| {
                        if let Some(buf) = code_view.get_buffer() {
                            let (start, end) = (buf.get_start_iter(), buf.get_end_iter());
                            buf.select_range(&start, &end);
                        }
                        gtk::Inhibit(false)
                    });

                    window::show_dialog(main);
                })
                .build_widget_event()
        );
    }

    fn add_room(&self, name: String) -> RoomEntryWidget {
        let widget = RoomEntryWidget::build(name);

        self.room_list.add(&widget.label);
        widget.label.show_all();

        widget
    }
}

#[derive(Clone)]
pub struct RoomEntryWidget {
    label: gtk::Label,
}

impl RoomEntryWidget {
    fn build(name: String) -> Self {
        RoomEntryWidget {
            label: gtk::LabelBuilder::new()
                .name("room_label")
                .label(&name)
                .halign(gtk::Align::Start)
                .build()
        }
    }
}

impl client::RoomEntryWidget<Ui> for RoomEntryWidget {
    fn bind_events(&self, room: &client::RoomEntry<Ui>) {}
}

pub struct MessageListWidget {
    list: gtk::ListBox,
    last_group: Option<GroupedMessageWidget>,
}

impl MessageListWidget {
    fn next_group(&mut self, author: UserId) -> &GroupedMessageWidget {
        match &self.last_group {
            Some(group) if group.author == author => {}
            _ => {
                let group = GroupedMessageWidget::build(author);
                self.list.insert(&group.widget, -1);
                self.last_group = Some(group);
            }
        }

        self.last_group.as_ref().unwrap()
    }
}

impl client::MessageListWidget<Ui> for MessageListWidget {
    fn push_message(&mut self, author: UserId, content: String) -> MessageEntryWidget {
        let group = self.next_group(author);
        group.push_message(content)
    }
}

pub struct GroupedMessageWidget {
    author: UserId,
    widget: gtk::Box,
    inner: gtk::Box,
}

impl GroupedMessageWidget {
    fn build(author: UserId) -> GroupedMessageWidget {
        let builder = gtk::Builder::new_from_file("res/glade/active/message_entry.glade");

        let widget: gtk::Box = builder.get_object("message").unwrap();
        let inner: gtk::Box = builder.get_object("message_inner").unwrap();

        let author_name: gtk::Label = builder.get_object("author_name").unwrap();
        author_name.set_text(&format!("{}", author.0));

        widget.show_all();

        GroupedMessageWidget { author, widget, inner }
    }

    fn push_message(&self, content: String) -> MessageEntryWidget {
        let entry = MessageEntryWidget::build(content);
        self.widget.add(&entry.label);
        self.widget.show_all();

        entry
    }
}

pub struct MessageEntryWidget {
    label: gtk::Label,
}

impl MessageEntryWidget {
    fn build(content: String) -> MessageEntryWidget {
        let label = gtk::LabelBuilder::new()
            .name("message_content")
            .label(content.trim())
            .halign(gtk::Align::Start)
            .build();

        MessageEntryWidget { label }
    }
}

impl client::MessageEntryWidget<Ui> for MessageEntryWidget {
    fn set_status(&mut self, status: client::MessageStatus) {
        let style = self.label.get_style_context();
        style.remove_class("pending");
        style.remove_class("error");

        match status {
            MessageStatus::Pending => style.add_class("pending"),
            MessageStatus::Err => style.add_class("error"),
            _ => (),
        }
    }
}

pub async fn start(ws: auth::AuthenticatedWs) -> Client<Ui> {
    // TODO: extract login process such that this error can be properly handled
    let client = Client::start(ws, Ui::build()).await
        .expect("client failed to start");

    client.ui.bind_events(&client);

    client
}

fn show_add_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/add_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let create_community_button: gtk::Button = builder.get_object("create_community_button").unwrap();
    let join_community_button: gtk::Button = builder.get_object("join_community_button").unwrap();

    let dialog = window::show_dialog(main);

    create_community_button.connect_button_press_event(
        client.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |client, _| {
                    dialog.close();
                    show_create_community(client);
                }
            })
            .build_widget_event()
    );

    join_community_button.connect_button_press_event(
        client.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |client, _| {
                    dialog.close();
                    show_join_community(client);
                }
            })
            .build_widget_event()
    );
}

fn show_create_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/create_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let name_entry: gtk::Entry = builder.get_object("name_entry").unwrap();
    let create_button: gtk::Button = builder.get_object("create_button").unwrap();

    let dialog = window::show_dialog(main);

    create_button.connect_button_press_event(
        client.connector()
            .do_async(move |client, _| {
                let name_entry = name_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(name) = name_entry.try_get_text() {
                        // TODO: error handling
                        let community = client.create_community(&name).await.unwrap();

                        community.create_room("General").await.unwrap();
                        community.create_room("Off Topic").await.unwrap();
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}

fn show_join_community(client: Client<Ui>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/dialog/join_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let code_entry: gtk::Entry = builder.get_object("invite_code_entry").unwrap();
    let join_button: gtk::Button = builder.get_object("join_button").unwrap();

    let dialog = window::show_dialog(main);

    join_button.connect_button_press_event(
        client.connector()
            .do_async(move |client, _| {
                let code_entry = code_entry.clone();
                let dialog = dialog.clone();
                async move {
                    if let Ok(code) = code_entry.try_get_text() {
                        let code = InviteCode(code);
                        // TODO: error handling
                        client.join_community(code).await.unwrap();
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}
