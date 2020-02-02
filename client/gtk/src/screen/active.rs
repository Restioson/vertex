use std::cell::RefCell;
use std::fmt;

use gtk::prelude::*;

use vertex::*;

use crate::{Client, client, screen, TryGetText, window};
use crate::auth;
use crate::connect::AsConnector;
use crate::UiShared;

struct MessageList<Author: Eq + fmt::Display> {
    list: gtk::ListBox,
    last_widget: Option<MessageWidget<Author>>,
}

impl<Author: Eq + fmt::Display> MessageList<Author> {
    fn new(list: gtk::ListBox) -> MessageList<Author> {
        MessageList { list, last_widget: None }
    }

    fn push(&mut self, author: Author, message: &str) {
        if self.last_widget.is_none() {
            let widget = MessageWidget::build(author);
            self.list.insert(&widget.widget, -1);
            self.last_widget = Some(widget);
        }

        if let Some(widget) = &mut self.last_widget {
            widget.push_content(message.trim());
        }
    }
}

struct MessageWidget<Author: fmt::Display> {
    author: Author,
    widget: gtk::Box,
    inner: gtk::Box,
}

impl<Author: fmt::Display> MessageWidget<Author> {
    fn build(author: Author) -> MessageWidget<Author> {
        let builder = gtk::Builder::new_from_file("res/glade/active/message_entry.glade");

        let widget: gtk::Box = builder.get_object("message").unwrap();
        let inner: gtk::Box = builder.get_object("message_inner").unwrap();

        let author_name: gtk::Label = builder.get_object("author_name").unwrap();
        author_name.set_text(&format!("{}", author));

        widget.show_all();

        MessageWidget { author, widget, inner }
    }

    fn push_content(&mut self, content: &str) {
        self.inner.add(&gtk::LabelBuilder::new()
            .name("message_content")
            .label(content)
            .halign(gtk::Align::Start)
            .build()
        );
        self.widget.show_all();
    }
}

pub struct Ui {
    pub main: gtk::Viewport,
    communities: gtk::ListBox,
    messages: RefCell<MessageList<String>>,
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
            messages: RefCell::new(MessageList::new(builder.get_object("messages").unwrap())),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
            add_community_button: builder.get_object("add_community_button").unwrap(),
        }
    }

    fn bind_events(&self, screen: &UiShared<Client<Ui>>) {
        self.message_entry.connect_activate(
            screen.connector()
                .do_async(|screen, entry: gtk::Entry| async move {
                    let mut client = screen.borrow_mut();
                    let selected_room = client.selected_community()
                        .and_then(|community| community.borrow().selected_room().cloned());

                    if let Some(selected_room) = selected_room {
                        let mut selected_room = selected_room.borrow_mut();

                        let content = entry.try_get_text().unwrap_or_default();
                        entry.set_text("");

                        // TODO: error handling
                        selected_room.send_message(content).await.unwrap();
                    }
                })
                .build_cloned_consumer()
        );

        self.settings_button.connect_button_press_event(
            screen.connector()
                .do_sync(|screen, (_button, _event)| {
                    let screen = screen::settings::build(screen.clone());
                    window::set_screen(&screen.borrow().main);
                })
                .build_widget_event()
        );

        self.add_community_button.connect_button_press_event(
            screen.connector()
                .do_sync(|screen, _| show_add_community(screen))
                .build_widget_event()
        );
    }
}

impl client::ClientUi for Ui {
    type CommunityEntryWidget = CommunityEntryWidget;
    type RoomEntryWidget = RoomEntryWidget;

    fn add_community(&self, name: String) -> CommunityEntryWidget {
        let widget = CommunityEntryWidget::build(name);

        self.communities.add(&widget.expander);
        widget.expander.show_all();

        widget
    }
}

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
    fn bind_events(&self, community_entry: &UiShared<client::CommunityEntry<Ui>>) {
//        self.expander.connect_property_expanded_notify(
//            client.connector()
//                .do_sync(|client, expander: gtk::Expander| {
//                    if expander.get_expanded() {
//                    let last_expanded = screen.model_mut().selected_community_widget.take();
//                    if let Some((expander, _)) = last_expanded {
//                        expander.set_expanded(false);
//                    }
//                    // TODO@gegy1000: help it needs to set the selected widget *with index* here
//                    } else {
//                        // TODO@gegy1000 testing porpoises
//                        screen.model_mut().selected_community_widget = None;
//                    }
//                })
//                .build_cloned_consumer()
//        );

        self.invite_button.connect_button_press_event(
            community_entry.connector()
                .do_async(move |community_entry, (widget, event)| async move {
                    // TODO: error handling
                    let community_entry = community_entry.borrow();

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
    fn bind_events(&self, room: &UiShared<client::RoomEntry<Ui>>) {}
}

pub fn build(ws: auth::AuthenticatedWs) -> UiShared<Client<Ui>> {
    let screen = Client::spawn(ws, Ui::build());
    screen.borrow().ui.bind_events(&screen);

    screen
}

fn show_add_community(client: UiShared<Client<Ui>>) {
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

fn show_create_community(client: UiShared<Client<Ui>>) {
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
                        let mut client = client.borrow_mut();
                        let community = client.create_community(&name).await.unwrap();
                        let mut community = community.borrow_mut();
                        community.create_room("General").await.unwrap();
                        community.create_room("Off Topic").await.unwrap();
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}

fn show_join_community(client: UiShared<Client<Ui>>) {
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
                        client.borrow_mut().join_community(code).await.unwrap();
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}
