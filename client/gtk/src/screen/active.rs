use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen};

const GLADE_SRC: &str = include_str!("glade/active.glade");

pub struct Widgets {
    communities: gtk::ListBox,
    messages: gtk::ListBox,
    message_entry: gtk::Entry,
    sign_out_button: gtk::Button,
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<vertex::Client>,
    community: Option<vertex::CommunityId>,
    room: Option<vertex::RoomId>,
    widgets: Widgets,
}

fn push_message(messages: &gtk::ListBox, author: &str, content: &str) {
    let grid = gtk::Grid::new();
    grid.insert_column(0);
    grid.insert_column(1);
    grid.insert_column(2);

    grid.set_column_spacing(10);

    let author = gtk::Label::new(Some(author));
    author.set_xalign(0.0);
    grid.attach(&author, 0, 0, 1, 1);

    let content = gtk::Label::new(Some(content));
    content.set_xalign(0.0);
    grid.attach(&content, 1, 0, 1, 1);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);

    separator.show_all();
    grid.show_all();

    messages.insert(&separator, -1);
    messages.insert(&grid, -1);
}

fn push_community(communities: &gtk::ListBox, name: &str, rooms: &[&str]) {
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
        rooms_list.add(&gtk::LabelBuilder::new()
            .name("room_label")
            .label(&format!("<b>#</b> {}", room))
            .use_markup(true)
            .halign(gtk::Align::Start)
            .build()
        );
    }

    expander.add(&rooms_list);

    expander.show_all();
    communities.insert(&expander, -1);
}

pub fn build(app: Rc<crate::App>, client: Rc<vertex::Client>) -> Screen<Model> {
    let builder = gtk::Builder::new_from_string(GLADE_SRC);

    let viewport = builder.get_object("viewport").unwrap();

    let model = Model {
        app: app.clone(),
        client,
        community: None,
        room: None,
        widgets: Widgets {
            communities: builder.get_object("communities").unwrap(),
            messages: builder.get_object("messages").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            sign_out_button: builder.get_object("sign_out_button").unwrap(),
        },
    };

    for i in 1..=20 {
        push_community(&model.widgets.communities, &format!("Community {}", i), &["general", "off-topic"]);
    }

    let screen = Screen::new(viewport, model);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.message_entry.connect_activate(
        screen.connector()
            .do_sync(|screen, entry: gtk::Entry| {
                // TODO
                let message = entry.get_text().unwrap().trim().to_string();
                entry.set_text("");
                println!("send message {}", message);
            })
            .build_cloned_consumer()
    );

    widgets.sign_out_button.connect_button_press_event(
        screen.connector()
            .do_async(|screen, (button, event)| async move {
                let model = screen.model();
                model.client.revoke_current_token().await.expect("failed to revoke token");
                model.app.token_store.forget_token();

                let login = screen::login::build(model.app.clone());
                model.app.set_screen(DynamicScreen::Login(login));
            })
            .build_widget_event()
    );
}
