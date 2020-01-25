use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen, TryGetText};

const GLADE_SRC: &str = include_str!("glade/active.glade");

pub struct Widgets {
    communities: gtk::ListBox,
    messages: gtk::ListBox,
    message_entry: gtk::Entry,
    settings_button: gtk::Button,
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<vertex::Client>,
    widgets: Widgets,
    selected_community_widget: Option<gtk::Expander>,
}

fn push_message(messages: &gtk::ListBox, author: &str, content: &str) {
    let outer_box = gtk::BoxBuilder::new()
        .name("message")
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    outer_box.add(&gtk::FrameBuilder::new()
        .name("author_icon")
        .build()
    );

    let message_inner = gtk::BoxBuilder::new()
        .name("message_inner")
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    message_inner.add(&gtk::LabelBuilder::new()
        .name("author_name")
        .label(author)
        .halign(gtk::Align::Start)
        .build()
    );

    message_inner.add(&gtk::LabelBuilder::new()
        .name("message_content")
        .label(content)
        .halign(gtk::Align::Start)
        .build()
    );

    outer_box.add(&message_inner);

    outer_box.show_all();
    messages.insert(&outer_box, -1);
}

fn push_community(screen: Screen<Model>, communities: &gtk::ListBox, name: &str, rooms: &[&str]) {
    let community_header = gtk::BoxBuilder::new()
        .name("community_header")
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();

    community_header.add(&gtk::FrameBuilder::new()
        .name("community_icon")
        .build()
    );
    community_header.add(&gtk::LabelBuilder::new()
        .name("community_label")
        .label(name)
        .valign(gtk::Align::Start)
        .build()
    );

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
            .label(&format!("<b>#</b> {}", room))
            .use_markup(true)
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
    communities.insert(&expander, -1);
}

pub fn build(app: Rc<crate::App>, client: Rc<vertex::Client>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(GLADE_SRC);

    let viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app: app.clone(),
        client,
        widgets: Widgets {
            communities: builder.get_object("communities").unwrap(),
            messages: builder.get_object("messages").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
        },
        selected_community_widget: None,
    };

    let screen = Screen::new(viewport, model);

    for i in 1..=5 {
        push_community(screen.clone(), &screen.model().widgets.communities, &format!("Community {}", i), &["general", "off-topic"]);
    }

    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.message_entry.connect_activate(
        screen.connector()
            .do_sync(|screen, entry: gtk::Entry| {
                let message = entry.try_get_text().unwrap_or_default();
                entry.set_text("");

                push_message(&screen.model().widgets.messages, "You", message.trim());
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
}
