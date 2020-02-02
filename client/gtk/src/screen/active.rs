use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Mutex;

use futures::{Stream, StreamExt};
use gtk::prelude::*;

use vertex::{CommunityId, InviteCode};

use crate::{auth, net};
use crate::screen::{self, Screen, TryGetText};

pub struct Widgets {
    main: gtk::Overlay,
    communities: gtk::ListBox,
    messages: RefCell<MessageList<String>>,
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

fn push_community(screen: Screen<Model>, community: CommunityId, name: &str, rooms: &[&str]) {
    let builder = gtk::Builder::new_from_file("res/glade/active/community_entry.glade");

    let community_expander: gtk::Expander = builder.get_object("community_expander").unwrap();

    let community_name: gtk::Label = builder.get_object("community_name").unwrap();
    let community_motd: gtk::Label = builder.get_object("community_motd").unwrap();

    let settings_button: gtk::Button = builder.get_object("settings_button").unwrap();
    let invite_button: gtk::Button = builder.get_object("invite_button").unwrap();

    let room_list: gtk::ListBox = builder.get_object("room_list").unwrap();

    community_name.set_text(name);
    community_name.set_text("5 users online");

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

    for &room in rooms {
        let room_label = gtk::LabelBuilder::new()
            .name("room_label")
            .label(room)
            .halign(gtk::Align::Start)
            .build();
        room_list.add(&room_label);
    }

    room_list.select_row(room_list.get_row_at_index(0).as_ref());

    screen.model_mut().selected_community_widget = Some((community_expander.clone(), 0)); // TODO@gegy1000 testing porpoises

    community_expander.connect_property_expanded_notify(
        screen.connector()
            .do_sync(|screen, expander: gtk::Expander| {
                if expander.get_expanded() {
//                    let last_expanded = screen.model_mut().selected_community_widget.take();
//                    if let Some((expander, _)) = last_expanded {
//                        expander.set_expanded(false);
//                    }

                    // TODO@gegy1000: help it needs to set the selected widget *with index* here
                } else {
                    // TODO@gegy1000 testing porpoises
//                    screen.model_mut().selected_community_widget = None;
                }
            })
            .build_cloned_consumer()
    );

    invite_button.connect_button_press_event(
        screen.connector()
            .do_async(move |screen, (widget, event)| async move {
                // TODO: error handling
                let invite = screen.model().client.create_invite(community).await.expect("failed to create invite");

                let builder = gtk::Builder::new_from_file("res/glade/active/invite_community.glade");
                let main: gtk::Box = builder.get_object("main").unwrap();

                let code_view: gtk::TextView = builder.get_object("code_view").unwrap();
                if let Some(code_view) = code_view.get_buffer() {
                    code_view.set_text(&invite.0);
                }

                code_view.connect_button_release_event(
                    screen.connector()
                        .do_sync(|screen, (code_view, _): (gtk::TextView, gdk::EventButton)| {
                            if let Some(buf) = code_view.get_buffer() {
                                let (start, end) = (buf.get_start_iter(), buf.get_end_iter());
                                buf.select_range(&start, &end);
                            }
                        })
                        .build_widget_event()
                );

                screen::show_dialog(&screen.model().widgets.main, main);
            })
            .build_widget_event()
    );

    community_expander.show_all();

    screen.model().widgets.communities.insert(&community_expander, -1);
}

pub struct Model {
    app: Rc<crate::App>,
    client: Rc<crate::Client>,
    widgets: Widgets,
    selected_community_widget: Option<(gtk::Expander, usize)>,
    pub(crate) communities: Mutex<Vec<crate::Community>>, // TODO better solution
}

pub fn build(app: Rc<crate::App>, ws: auth::AuthenticatedWs) -> Screen<Model> {
    let (client, stream) = crate::Client::new(ws);

    let builder = gtk::Builder::new_from_file("res/glade/active/active.glade");

    let main: gtk::Overlay = builder.get_object("main").unwrap();

    let model = Model {
        app: app.clone(),
        client: Rc::new(client),
        widgets: Widgets {
            main: main.clone(),
            communities: builder.get_object("communities").unwrap(),
            messages: RefCell::new(MessageList::new(builder.get_object("messages").unwrap())),
            message_entry: builder.get_object("message_entry").unwrap(),
            settings_button: builder.get_object("settings_button").unwrap(),
            add_community_button: builder.get_object("add_community_button").unwrap(),
        },
        selected_community_widget: None,
        communities: Mutex::new(Vec::new()),
    };

    let screen = Screen::new(main, model);
    bind_events(&screen);

    // FIXME: we need to stop these loops when this screen closes!
    glib::MainContext::ref_thread_default().spawn_local({
        let client = screen.model().client.clone();
        run(client, stream)
    });

    screen
}

async fn run<S>(client: Rc<crate::Client>, stream: S)
    where S: Stream<Item = net::Result<vertex::ServerAction>> + Unpin
{
    futures::future::join(
        async move {
            let mut stream = stream;
            while let Some(result) = stream.next().await {
                println!("{:?}", result);
            }
        },
        async move {
            client.keep_alive_loop().await;
        },
    ).await;
}

fn bind_events(screen: &Screen<Model>) {
    let model = screen.model();
    let widgets = &model.widgets;

    widgets.message_entry.connect_activate(
        screen.connector()
            .do_async(|screen, entry: gtk::Entry| async move {
                let content = entry.try_get_text().unwrap_or_default();
                entry.set_text("");

                // TODO handle error
                let (expander, idx) = screen.model().selected_community_widget.clone().unwrap();
                let model = screen.model();
                let communities = model.communities.lock();
                let community = &communities.unwrap()[idx];

                let list = expander.get_child().unwrap().downcast::<gtk::ListBox>().unwrap();
                let row = list.get_selected_row().unwrap();
                let room = &community.rooms[row.get_index() as usize];

                screen.model().client.send_message(content.clone(), community.id, room.id).await.unwrap(); // TODO handle error?
                screen.model().widgets.messages.borrow_mut().push("You".to_owned(), &content);
            })
            .build_cloned_consumer()
    );

    widgets.settings_button.connect_button_press_event(
        screen.connector()
            .do_sync(|screen, (_button, _event)| {
                let model = screen.model();
                model.app.set_screen(screen::settings::build(
                    screen.clone(),
                    model.app.clone(),
                    model.client.clone(),
                ));
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
    let builder = gtk::Builder::new_from_file("res/glade/active/add_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let create_community_button: gtk::Button = builder.get_object("create_community_button").unwrap();
    let join_community_button: gtk::Button = builder.get_object("join_community_button").unwrap();

    let dialog = screen::show_dialog(&screen.model().widgets.main, main);

    create_community_button.connect_button_press_event(
        screen.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |screen, _| {
                    dialog.close();
                    show_create_community(screen);
                }
            })
            .build_widget_event()
    );

    join_community_button.connect_button_press_event(
        screen.connector()
            .do_sync({
                let dialog = dialog.clone();
                move |screen, _| {
                    dialog.close();
                    show_join_community(screen);
                }
            })
            .build_widget_event()
    );
}

fn show_create_community(screen: Screen<Model>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/create_community.glade");
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
                        match result {
                            Ok(id) => {
                                let (general, off_topic) = {
                                    // TODO@gegy1000 tidy up when we do this properly
                                    let client = &screen.model().client;
                                    (
                                        client.create_room("General".into(), id).await.unwrap(),
                                        client.create_room("Off Topic".into(), id).await.unwrap(),
                                    )
                                };

                                screen.model.borrow().communities.lock().unwrap().push(crate::Community {
                                    id,
                                    name: name.clone(),
                                    rooms: vec![
                                        crate::Room { id: general, name: "General".into() },
                                        crate::Room { id: off_topic, name: "Off Topic".into() },
                                    ],
                                });

                                push_community(screen, id, &name, &["General", "Off Topic"]);
                            }
                            Err(e) => panic!("{:?}", e),
                        }
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}

fn show_join_community(screen: Screen<Model>) {
    let builder = gtk::Builder::new_from_file("res/glade/active/join_community.glade");
    let main: gtk::Box = builder.get_object("main").unwrap();

    let code_entry: gtk::Entry = builder.get_object("invite_code_entry").unwrap();
    let join_button: gtk::Button = builder.get_object("join_button").unwrap();

    let dialog = screen::show_dialog(&screen.model().widgets.main, main);

    join_button.connect_button_press_event(
        screen.connector()
            .do_async(move |screen, _| {
                let dialog = dialog.clone();
                let code_entry = code_entry.clone();
                async move {
                    if let Ok(code) = code_entry.try_get_text() {
                        let code = InviteCode(code);
                        // TODO: bad error handling
                        if let Err(e) = screen.model().client.join_community(code).await {
                            panic!("{:?}", e);
                        }
                    }
                    dialog.close();
                }
            })
            .build_widget_event()
    );
}
