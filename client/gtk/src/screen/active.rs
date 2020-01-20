use gtk::prelude::*;

use std::rc::Rc;

use vertex_client_backend as vertex;

use crate::screen::{self, Screen, DynamicScreen};

const GLADE_SRC: &str = include_str!("glade/active.glade");

pub struct Widgets {
    communities: gtk::ListBox,
    rooms: gtk::ListBox,
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
            rooms: builder.get_object("rooms").unwrap(),
            messages: builder.get_object("messages").unwrap(),
            message_entry: builder.get_object("message_entry").unwrap(),
            sign_out_button: builder.get_object("sign_out_button").unwrap(),
        },
    };

    let screen = Screen::new(viewport, model);
    bind_events(&screen);

    screen
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.rooms.connect_row_selected(
        screen.connector()
            .do_sync(|screen, (list, row): (gtk::ListBox, Option<gtk::ListBoxRow>)| {
                if let Some(row) = row {
                    // TODO
                    let row = row.get_index() as usize;
                    println!("select row {}", row);
                }
            })
            .build_widget_and_option_consumer()
    );

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
