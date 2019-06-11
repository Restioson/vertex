use gtk::prelude::*;
use gtk::{Entry, Label, ListBox, TextView, Window};
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
                let text_buffer = self.widgets.messages.get_buffer().unwrap();
                let message = format!("{}: {}\n", msg.author, msg.content);
                text_buffer.insert(&mut text_buffer.get_end_iter(), &message);
            }
            Action::Error(_error) => {}
        }
    }
}

impl Update for Win {
    type Model = VertexModel;
    type ModelParam = VertexArgs;
    type Msg = VertexMsg;

    fn model(relm: &Relm<Win>, args: VertexArgs) -> VertexModel {
        let mut model = VertexModel {
            vertex: Vertex::new(Config {
                url: Url::parse("ws://127.0.0.1:8080/client/").unwrap(),
                client_id: UserId(args.user_id.unwrap_or_else(|| Uuid::new_v4())),
            }),
            room: None,
            room_list: Vec::new(),
        };

        // TODO: Where should this go?
        model.vertex.login().expect("Error logging in");

        model
    }

    fn update(&mut self, event: VertexMsg) {
        match event {
            VertexMsg::SetRoom(idx) => {
                let room = self.model.room_list[idx];
                self.model.room = Some(room);
            }
            VertexMsg::SendMessage(msg) => {
                let text_buffer = self.widgets.messages.get_buffer().unwrap();

                if msg.starts_with("/") {
                    let v: Vec<&str> = msg.splitn(2, ' ').collect();

                    match v[0] {
                        "/join" => {
                            if v.len() == 2 {
                                let room = RoomId(Uuid::parse_str(v[1]).expect("Invalid room id"));
                                self.model
                                    .vertex
                                    .join_room(room)
                                    .expect("Error joining room");
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    &format!("Joined room {}\n", room.0),
                                );

                                self.model.room = Some(room);
                                let txt: &str = &format!("#{}", room.0);
                                let room_label = Label::new(Some(txt));
                                self.widgets.rooms.insert(&room_label, -1);
                                self.model.room_list.push(room);
                                room_label.show_all();
                            } else {
                                text_buffer
                                    .insert(&mut text_buffer.get_end_iter(), "Room id required");
                            }
                        }
                        "/createroom" => {
                            text_buffer
                                .insert(&mut text_buffer.get_end_iter(), "Creating room...\n");
                            let room = self
                                .model
                                .vertex
                                .create_room()
                                .expect("Error creating room");
                            text_buffer.insert(
                                &mut text_buffer.get_end_iter(),
                                &format!("Joined room {}\n", room.0),
                            );

                            self.model.room = Some(room);
                            let txt: &str = &format!("#{}", room.0);
                            let room_label = Label::new(Some(txt));
                            self.widgets.rooms.insert(&room_label, -1);
                            self.model.room_list.push(room);
                            room_label.show_all();
                        }
                        _ => {
                            text_buffer.insert(&mut text_buffer.get_end_iter(), "Unknown command\n")
                        }
                    }

                    return;
                }

                let room = self.model.room.expect("Not in a room").clone();
                self.model
                    .vertex
                    .send_message(msg.to_string(), room)
                    .expect("Error sending message"); // todo display error

                let name = self.model.vertex.username();

                // TODO: Unify
                text_buffer.insert(
                    &mut text_buffer.get_end_iter(),
                    &format!("{}: {}\n", name, msg),
                );
            }
            VertexMsg::Lifecycle => {
                if let Some(action) = self.model.vertex.handle() {
                    println!("action {:?}", action);
                    self.handle_action(action);
                }
            }
            VertexMsg::Heartbeat => {
                if let Err(_) = self.model.vertex.heartbeat() {
                    eprintln!("Server timed out");
                    std::process::exit(1);
                }
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
        let messages: TextView = builder.get_object("messages").unwrap();
        let entry: Entry = builder.get_object("message_entry").unwrap();
        let rooms: ListBox = builder.get_object("channels").unwrap();

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
    messages: TextView,
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
