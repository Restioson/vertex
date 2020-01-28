use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen, TryGetText};
use std::fmt;

const SCREEN_SRC: &str = include_str!("glade/active/active.glade");

const ADD_COMMUNITY_SRC: &str = include_str!("glade/active/add_community.glade");
const CREATE_COMMUNITY_SRC: &str = include_str!("glade/active/create_community.glade");

pub struct Widgets {
    main: gtk::Overlay,
    communities: gtk::ListBox,
    messages: MessageList<String>,
    message_entry: gtk::Entry,
    settings_button: gtk::Button,
    add_community_button: gtk::Button,
}

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
        let widget = gtk::BoxBuilder::new()
            .name("message")
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        widget.add(&gtk::FrameBuilder::new()
            .name("author_icon")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .build()
        );

        let inner = gtk::BoxBuilder::new()
            .name("message_inner")
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();

        inner.add(&gtk::LabelBuilder::new()
            .name("author_name")
            .label(&format!("{}", author))
            .halign(gtk::Align::Start)
            .build()
        );

        widget.add(&inner);
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

fn push_community(screen: Screen<Model>, name: &str, rooms: &[&str]) {
    let community_header = gtk::BoxBuilder::new()
        .name("community_header")
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    community_header.add(&gtk::FrameBuilder::new()
        .name("community_icon")
        .build()
    );

    let community_description = gtk::BoxBuilder::new()
        .name("community_description")
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();

    community_description.add(&gtk::LabelBuilder::new()
        .name("community_name")
        .label(name)
        .halign(gtk::Align::Start)
        .build()
    );

    community_description.add(&gtk::LabelBuilder::new()
        .name("community_motd")
        .label("Message of the day!")
        .halign(gtk::Align::Start)
        .build()
    );

    community_header.add(&community_description);

    let expander = gtk::ExpanderBuilder::new()
        .name("community_expander")
        .label_widget(&community_header)
        .build();

    let rooms_list = gtk::ListBoxBuilder::new()
        .name("room_list")
        .build();

    for &room in rooms {
        let room_label = gtk::LabelBuilder::new()
            .name("room_label")
            .label(room)
            .halign(gtk::Align::Start)
            .build();
        rooms_list.add(&room_label);
    }

    rooms_list.select_row(rooms_list.get_row_at_index(0).as_ref());

    expander.add(&rooms_list);

    expander.connect_property_expanded_notify(
        screen.connector()
            .do_sync(|screen, expander: gtk::Expander| {
                if expander.get_expanded() {
                    let last_expanded = screen.model_mut().selected_community_widget.take();
                    if let Some(expander) = last_expanded {
                        expander.set_expanded(false);
                    }

                    screen.model_mut().selected_community_widget = Some(expander);
                } else {
                    screen.model_mut().selected_community_widget = None;
                }
            })
            .build_cloned_consumer()
    );

    expander.show_all();

    screen.model().widgets.communities.insert(&expander, -1);
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<vertex::Client>,
    widgets: Widgets,
    selected_community_widget: Option<gtk::Expander>,
}

pub fn build(app: Rc<crate::App>, client: Rc<vertex::Client>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(SCREEN_SRC);

    let main: gtk::Overlay = builder.get_object("main").unwrap();

    let model = Model {
        app: app.clone(),
        client,
        widgets: Widgets {
            main: main.clone(),
            communities: builder.get_object("communities").unwrap(),
            messages: MessageList::new(builder.get_object("messages").unwrap()),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
            add_community_button: builder.get_object("add_community_button").unwrap(),
        },
        selected_community_widget: None,
    };

    let screen = Screen::new(main, model);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.message_entry.connect_activate(
        screen.connector()
            .do_sync(|screen, entry: gtk::Entry| {
                let content = entry.try_get_text().unwrap_or_default();
                entry.set_text("");

                screen.model_mut().widgets.messages.push("You".to_owned(), &content);
            })
            .build_cloned_consumer()
    );

    widgets.settings_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (_button, _event)| {
                let model = screen.model();
                let settings = screen::settings::build(model.app.clone(), model.client.clone());
                model.app.set_screen(DynamicScreen::Settings(settings));
            })
            .build_widget_event()
    );

    widgets.add_community_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, _| show_add_community(screen))
            .build_widget_event()
    );
}

fn show_add_community(screen: Screen<Model>) {
    let builder = gtk::Builder::new_from_string(ADD_COMMUNITY_SRC);
    let main: gtk::Box = builder.get_object("main").unwrap();

    let create_community_button: gtk::Button = builder.get_object("create_community_button").unwrap();
    let join_community_button: gtk::Button = builder.get_object("join_community_button").unwrap();

    let dialog = screen::show_dialog(&screen.model().widgets.main, main);

    create_community_button.connect_button_press_event(
        screen.connector()
            .do_sync(move |screen, _| {
                dialog.close();
                show_create_community(screen);
            })
            .build_widget_event()
    );
}

fn show_create_community(screen: Screen<Model>) {
    let builder = gtk::Builder::new_from_string(CREATE_COMMUNITY_SRC);
    let main: gtk::Box = builder.get_object("main").unwrap();

    let name_entry: gtk::Entry = builder.get_object("name_entry").unwrap();
    let create_button: gtk::Button = builder.get_object("create_button").unwrap();

    let dialog = screen::show_dialog(&screen.model().widgets.main, main);

    create_button.connect_button_press_event(
        screen.connector()
            .do_async(move |screen, _| {
                let dialog = dialog.clone();
                let name_entry = name_entry.clone();
                async move {
                    if let Ok(name) = name_entry.try_get_text() {
                        let result = screen.model().client.create_community(name.clone()).await;
                        if let Ok(id) = result {
                            push_community(screen, &name, &["General", "Off Topic"]);
                        }
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}
