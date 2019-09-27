use gtk::prelude::*;
use gtk::{Window, Entry, Label, ListBox, Separator, Orientation, Grid};
use relm::{connect, connect_stream, Relm, Update, Widget};
use relm_derive::*;
use url::Url;
use uuid::Uuid;
use vertex_client_backend::*;
use vertex_common::*;

use clap::{App, Arg};

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

const GLADE_SRC: &str = include_str!("client.glade");

struct VertexModel {
    vertex: Vertex,
    room: Option<RoomId>,
    room_list: Vec<RoomId>,
}

struct VertexArgs {
    user_id: Option<Uuid>,
}

#[derive(Msg)]
enum VertexMsg {
    SetRoom(usize),
    SendMessage(String),
    Lifecycle,
    Heartbeat,
    Quit,
}

struct Win {
    model: VertexModel,
    window: Window,
    widgets: Widgets,
}

impl Win {
    fn handle_action(&mut self, action: Action) -> () {
        match action {
            Action::AddMessage(msg) => {
                self.push_message(&msg.author, &msg.content);
            }
            Action::AddRoom(room) => {
                self.push_message("Info", &format!("Joined room {}", room.0));

                self.model.room = Some(room);
                let txt: &str = &format!("#{}", room.0);
                let room_label = Label::new(Some(txt));
                self.widgets.rooms.insert(&room_label, -1);
                self.model.room_list.push(room);
                room_label.show_all();
            }
            Action::Error(_error) => {}
        }
    }

    fn push_message(&self, author: &str, content: &str) {
        let message = self.build_message(author, content);

        let separator = Separator::new(Orientation::Horizontal);

        separator.show_all();
        message.show_all();

        self.widgets.messages.insert(&separator, -1);
        self.widgets.messages.insert(&message, -1);
    }

    fn build_message(&self, author: &str, content: &str) -> Grid {
        let grid = Grid::new();
        grid.insert_column(0);
        grid.insert_column(1);
        grid.insert_column(2);

        grid.set_column_spacing(10);

        let author = Label::new(Some(author));
        author.set_xalign(0.0);
        grid.attach(&author, 0, 0, 1, 1);

        let content = Label::new(Some(content));
        content.set_xalign(0.0);
        grid.attach(&content, 1, 0, 1, 1);

        grid
    }
}

impl Update for Win {
    type Model = VertexModel;
    type ModelParam = VertexArgs;
    type Msg = VertexMsg;

    fn model(_relm: &Relm<Win>, args: VertexArgs) -> VertexModel {
        let config = Config {
            url: Url::parse("ws://127.0.0.1:8080/client/").unwrap(),
            client_id: UserId(args.user_id.unwrap_or_else(|| Uuid::new_v4())),
        };

        let vertex = Vertex::connect(config);

        let mut model = VertexModel {
            vertex,
            room: None,
            room_list: Vec::new(),
        };

        // TODO: Where should this go?
        model.vertex.login();

        model
    }

    fn update(&mut self, event: VertexMsg) {
        match event {
            VertexMsg::SetRoom(idx) => {
                let room = self.model.room_list[idx];
                self.model.room = Some(room);
            }
            VertexMsg::SendMessage(msg) => {
                if msg.starts_with("/") {
                    let v: Vec<&str> = msg.splitn(2, ' ').collect();

                    match v[0] {
                        "/join" => {
                            if v.len() == 2 {
                                let room = RoomId(Uuid::parse_str(v[1]).expect("Invalid room id"));
                                self.model.vertex.join_room(room);
                                self.push_message("Info", &format!("Joined room {}", room.0));

                                self.model.room = Some(room);
                                let txt: &str = &format!("#{}", room.0);
                                let room_label = Label::new(Some(txt));
                                room_label.set_xalign(0.0);
                                self.widgets.rooms.insert(&room_label, -1);
                                self.model.room_list.push(room);
                                room_label.show_all();
                            } else {
                                self.push_message("Info", "Room id required");
                            }
                        }
                        "/createroom" => {
                            self.model.vertex.create_room();
                        }
                        _ => self.push_message("Info", "Unknown command"),
                    }

                    return;
                }

                let room = self.model.room.expect("Not in a room").clone();

                self.model.vertex.send_message(msg.to_string(), room);

                let name = self.model.vertex.username();
                self.push_message(&name, &msg);
            }
            VertexMsg::Lifecycle => {
                if let Some(action) = self.model.vertex.handle() {
                    println!("action {:?}", action);
                    self.handle_action(action);
                }
            }
            VertexMsg::Heartbeat => {
                self.model.vertex.dispatch_heartbeat();
            }
            VertexMsg::Quit => gtk::main_quit(),
        }
    }
}

impl Widget for Win {
    type Root = Window;

    fn root(&self) -> Window {
        self.window.clone()
    }

    fn view(relm: &Relm<Win>, model: VertexModel) -> Win {
        let builder = gtk::Builder::new_from_string(GLADE_SRC);

        let window: Window = builder.get_object("window").unwrap();
        let messages: ListBox = builder.get_object("messages").unwrap();
        let entry: Entry = builder.get_object("message_entry").unwrap();
        let rooms: ListBox = builder.get_object("rooms").unwrap();

        connect!(
            relm,
            window,
            connect_delete_event(_, _),
            return (Some(VertexMsg::Quit), Inhibit(false))
        );
        connect!(relm, rooms, connect_row_selected(_, row), {
            row.as_ref()
                .map(|row| VertexMsg::SetRoom(row.get_index() as usize))
        });

        connect!(relm, entry, connect_activate(entry), {
            let msg = entry.get_text().unwrap().trim().to_string();
            entry.set_text("");
            Some(VertexMsg::SendMessage(msg))
        });

        relm::interval(relm.stream(), 16, || VertexMsg::Lifecycle);
        relm::interval(relm.stream(), 2000, || VertexMsg::Heartbeat);

        window.show_all();

        Win {
            model,
            window,
            widgets: Widgets {
                messages,
                entry,
                rooms,
            },
        }
    }
}

struct Widgets {
    messages: ListBox,
    entry: Entry,
    rooms: ListBox,
}

fn main() {
    let matches = App::new(NAME)
        .version(VERSION)
        .author(AUTHORS)
        .arg(
            Arg::with_name("user-id")
                .short("i")
                .long("userid")
                .value_name("UUID")
                .help("Sets the user id to login with")
                .takes_value(true),
        )
        .get_matches();

    let user_id = matches
        .value_of("user-id")
        .and_then(|id| Uuid::parse_str(id).ok());

    let args = VertexArgs { user_id };
    Win::run(args).expect("failed to run window");
}
