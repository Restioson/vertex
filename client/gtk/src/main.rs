#[macro_use]
extern crate serde_derive;

use clap::{App, Arg};
use gtk::prelude::*;
use gtk::{Entry, Label, ListBox, TextView, Window};
use keyring::Keyring;
use relm::{connect, connect_stream, Relm, Update, Widget};
use relm_derive::*;
use url::Url;
use uuid::Uuid;
use vertex_client_backend::*;
use vertex_common::*;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

const GLADE_SRC: &str = include_str!("client.glade");

struct VertexModel {
    vertex: Vertex,
    room: Option<RoomId>,
    community: Option<CommunityId>,
    room_list: Vec<RoomId>,
    keyring: Keyring<'static>,
}

struct VertexArgs {
    ip: Option<String>,
}

#[derive(Msg)]
enum VertexMsg {
    SetRoom(usize),
    SendMessage(String),
    Lifecycle,
    Heartbeat,
    Quit,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredToken {
    device_id: DeviceId,
    token: String,
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
            Action::Error(_error) => {} // TODO handle this? @gegy
            Action::LoggedOut => {
                let text_buffer = self.widgets.messages.get_buffer().unwrap();
                text_buffer.insert(
                    &mut text_buffer.get_end_iter(),
                    &"Logged out -- session invalidated. Please log in again.",
                );
            }
        }
    }
}

impl Update for Win {
    type Model = VertexModel;
    type ModelParam = VertexArgs;
    type Msg = VertexMsg;

    fn model(_relm: &Relm<Win>, args: VertexArgs) -> VertexModel {
        let ip = args.ip.clone().unwrap_or("127.0.0.1:8080".to_string());
        println!("Connecting to {}", ip);

        let model = VertexModel {
            vertex: Vertex::new(Config {
                url: Url::parse(&format!("ws://{}/client/", ip)).unwrap(),
            }),
            room: None,
            community: None,
            room_list: Vec::new(),
            keyring: Keyring::new("vertex_client_gtk", ""), // username = ""
        };

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
                    let v: Vec<&str> = msg.split(' ').collect();

                    match v[0] {
                        //                        "/join" => {
                        //                            if v.len() == 2 {
                        //                                let community = RoomId(Uuid::parse_str(v[1]).expect("Invalid community id"));
                        //                                self.model
                        //                                    .vertex
                        //                                    .join_community(community)
                        //                                    .expect("Error joining community");
                        //                                text_buffer.insert(
                        //                                    &mut text_buffer.get_end_iter(),
                        //                                    &format!("Joined community {}\n", community.0),
                        //                                );
                        //
                        //                                self.model.room = Some(room);
                        //                                let txt: &str = &format!("#{}", room.0);
                        //                                let room_label = Label::new(Some(txt));
                        //                                self.widgets.rooms.insert(&room_label, -1);
                        //                                self.model.room_list.push(community); // TODO lol
                        //                                room_label.show_all();
                        //                            } else {
                        //                                text_buffer
                        //                                    .insert(&mut text_buffer.get_end_iter(), "Room id required");
                        //                            }
                        //                        }
                        "/createroom" => {
                            if v.len() == 3 {
                                text_buffer
                                    .insert(&mut text_buffer.get_end_iter(), "Creating room...\n");

                                let community = CommunityId(
                                    Uuid::parse_str(v[2]).expect("Invalid community id"),
                                );
                                let room = self
                                    .model
                                    .vertex
                                    .create_room(v[1].to_string(), community)
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
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Room name and community id required",
                                );
                            }
                        }
                        "/login" => {
                            if v.len() > 2 {
                                text_buffer
                                    .insert(&mut text_buffer.get_end_iter(), "Logging in...\n");

                                let token = if v.len() == 5 {
                                    let id =
                                        DeviceId(Uuid::parse_str(v[3]).expect("Invalid device id"));
                                    let token = AuthToken(v[4].to_string());

                                    Some((id, token))
                                } else if let Ok(token_ser) = self.model.keyring.get_password() {
                                    let stored_token: StoredToken =
                                        serde_json::from_str(&token_ser)
                                            .expect("Error deserializing token");

                                    Some((stored_token.device_id, AuthToken(stored_token.token)))
                                } else {
                                    None
                                };

                                match self.model.vertex.login(token, v[1], v[2]) {
                                    Ok((device_id, token)) => {
                                        let stored_token = StoredToken {
                                            device_id,
                                            token: token.0.clone(),
                                        };
                                        let token_ser = serde_json::to_string(&stored_token)
                                            .expect("Error serializing token");
                                        self.model
                                            .keyring
                                            .set_password(&token_ser)
                                            .expect("Error storing token");

                                        text_buffer.insert(
                                            &mut text_buffer.get_end_iter(),
                                            &format!(
                                                "Successfully logged in. Device id: {}.\n",
                                                device_id.0,
                                            ),
                                        );
                                    }
                                    Err(e) => text_buffer.insert(
                                        &mut text_buffer.get_end_iter(),
                                        &format!("Error logging in: {:?}\n", e),
                                    ),
                                }
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Username and password required\n",
                                );
                            }
                        }
                        "/forgettoken" => {
                            self.model
                                .keyring
                                .delete_password()
                                .expect("Error forgetting token");
                            text_buffer.insert(&mut text_buffer.get_end_iter(), "Token forgot.\n");
                        }
                        "/refreshtoken" => {
                            if v.len() == 4 {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Refreshing token...\n",
                                );

                                let dev =
                                    DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));

                                self.model
                                    .vertex
                                    .refresh_token(dev, v[2], v[3])
                                    .expect("Error refreshing token");

                                text_buffer
                                    .insert(&mut text_buffer.get_end_iter(), "Token refreshed\n");
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Device ID, username, and password required\n",
                                );
                            }
                        }
                        "/register" => {
                            if v.len() == 3 {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Registering user...\n",
                                );

                                let id = self
                                    .model
                                    .vertex
                                    .create_user(v[1], v[1], v[2])
                                    .expect("Error registering user");

                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    &format!("Registered user with id {}.\n", id.0),
                                );
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Username and password required\n",
                                );
                            }
                        }
                        "/revokecurrent" => {
                            text_buffer
                                .insert(&mut text_buffer.get_end_iter(), "Revoking token...\n");

                            self.model
                                .vertex
                                .revoke_current_token()
                                .expect("Error revoking current token");
                        }
                        "/revoke" => {
                            if v.len() == 3 {
                                text_buffer
                                    .insert(&mut text_buffer.get_end_iter(), "Revoking token...\n");

                                let dev =
                                    DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));
                                self.model
                                    .vertex
                                    .revoke_token(v[2], dev)
                                    .expect("Error revoking token");
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Token and password required\n",
                                );
                            }
                        }
                        "/changeusername" => {
                            if v.len() == 2 {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Changing username...\n",
                                );

                                self.model
                                    .vertex
                                    .change_username(v[1])
                                    .expect("Error changing username");
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "New username required\n",
                                );
                            }
                        }
                        "/changedisplayname" => {
                            if v.len() == 2 {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Changing display name...\n",
                                );

                                self.model
                                    .vertex
                                    .change_display_name(v[1])
                                    .expect("Error changing display name");
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "New display name required\n",
                                );
                            }
                        }
                        "/changepassword" => {
                            if v.len() == 3 {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Changing password...\n",
                                );

                                self.model
                                    .vertex
                                    .change_password(v[1], v[2])
                                    .expect("Error changing password");
                            } else {
                                text_buffer.insert(
                                    &mut text_buffer.get_end_iter(),
                                    "Old password and new password required\n",
                                );
                            }
                        }
                        _ => {
                            text_buffer.insert(&mut text_buffer.get_end_iter(), "Unknown command\n")
                        }
                    }

                    return;
                }

                let room = self.model.room.expect("Not in a room").clone();
                let community = self.model.community.expect("Not in a communtiy").clone();
                self.model
                    .vertex
                    .send_message(msg.to_string(), room, community)
                    .expect("Error sending message"); // todo display error

                let name = self
                    .model
                    .vertex
                    .display_name
                    .as_ref()
                    .expect("Not logged in");

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
            Arg::with_name("ip")
                .short("i")
                .long("ip")
                .value_name("IP")
                .help("Sets the homeserver to connect to")
                .takes_value(true),
        )
        .get_matches();

    let ip = matches.value_of("ip").map(|ip| ip.to_string());
    let args = VertexArgs { ip };

    Win::run(args).expect("failed to run window");
}
