#[macro_use]
extern crate serde_derive;

use clap::{App, Arg};
use gtk::prelude::*;
use gtk::{Entry, Grid, Label, ListBox, Orientation, Separator, Window};
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
            Action::LoggedIn { device_id, token } => {
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

                self.push_message("Info", &format!(
                    "Successfully logged in. Device id: {}.",
                    device_id.0,
                ));
            }
            Action::UserCreated(id) => self.push_message("Info", &format!("Registered user with id {}.\n", id.0)),
            Action::TokenRefreshed => self.push_message("Info", "Token refreshed."),
            Action::TokenRevoked => self.push_message("Info", "Token revoked."),
            Action::UsernameChanged(_) => self.push_message("Info", "Username changed successfully."),
            Action::DisplayNameChanged(_) => self.push_message("Info", "Display name changed successfully."),
            Action::PasswordChanged => self.push_message("Info", "Password changed successfully."),
            Action::Error(error) => panic!("error: {:?}", error), // TODO handle this? @gegy
            Action::LoggedOut => {
                self.push_message("Info", "Logged out -- session invalidated. Please log in again.");
            }
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
        let ip = args.ip.clone().unwrap_or("127.0.0.1:8080".to_string());
        println!("Connecting to {}", ip);

        let net = net::connect(Url::parse(&format!("ws://{}/client/", ip)).unwrap());

        let vertex = Vertex::new(net);

        VertexModel {
            vertex,
            room: None,
            room_list: Vec::new(),
            keyring: Keyring::new("vertex_client_gtk", ""), // username = ""
        }
    }

    fn update(&mut self, event: VertexMsg) {
        match event {
            VertexMsg::SetRoom(idx) => {
                let room = self.model.room_list[idx];
                self.model.room = Some(room);
            }
            VertexMsg::SendMessage(msg) => {
                if msg.starts_with("/") {
                    let v: Vec<&str> = msg.split(' ').collect();

                    match v[0] {
                        "/join" => {
                            if v.len() == 2 {
                                let room = RoomId(Uuid::parse_str(v[1]).expect("Invalid room id"));
                                self.model.vertex.join_room(room).expect("Error joining room");
                            } else {
                                self.push_message("Info", "Room id required");
                            }
                        }
                        "/createroom" => {
                            self.model.vertex.create_room().expect("Error creating room");
                        }
                        "/login" => {
                            if v.len() > 2 {
                                self.push_message("Info", "Logging in...");

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

                                self.model.vertex.login(token, v[1].to_string(), v[2].to_string()).expect("Error logging in");
                            } else {
                                self.push_message("Info", "Username and password required");
                            }
                        }
                        "/forgettoken" => {
                            self.model
                                .keyring
                                .delete_password()
                                .expect("Error forgetting token");
                            self.push_message("Info", "Token forgot.");
                        }
                        "/refreshtoken" => {
                            if v.len() == 4 {
                                self.push_message("Info", "Refreshing token...");

                                let dev =
                                    DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));

                                self.model
                                    .vertex
                                    .refresh_token(dev, v[2].to_string(), v[3].to_string());
                            } else {
                                self.push_message("Info", "Device ID, username, and password required");
                            }
                        }
                        "/register" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Registering user...");

                                self
                                    .model
                                    .vertex
                                    .create_user(v[1].to_string(), v[1].to_string(), v[2].to_string());
                            } else {
                                self.push_message("Info", "Username and password required");
                            }
                        }
                        "/revokecurrent" => {
                            self.push_message("Info", "Revoking token...");

                            self.model
                                .vertex
                                .revoke_current_token()
                                .expect("Error revoking current token");
                        }
                        "/revoke" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Revoking token...");

                                let dev =
                                    DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));
                                self.model
                                    .vertex
                                    .revoke_token(dev, v[2].to_string())
                                    .expect("Error revoking token");
                            } else {
                                self.push_message("Info", "Token and password required.");
                            }
                        }
                        "/changeusername" => {
                            if v.len() == 2 {
                                self.push_message("Info", "Changing username...");

                                self.model
                                    .vertex
                                    .change_username(v[1].to_string())
                                    .expect("Error changing username");
                            } else {
                                self.push_message("Info", "New username required.");
                            }
                        }
                        "/changedisplayname" => {
                            if v.len() == 2 {
                                self.push_message("Info", "Changing display name...");

                                self.model
                                    .vertex
                                    .change_display_name(v[1].to_string())
                                    .expect("Error changing display name");
                            } else {
                                self.push_message("Info", "New display name required.");
                            }
                        }
                        "/changepassword" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Changing password...");

                                self.model
                                    .vertex
                                    .change_password(v[1].to_string(), v[2].to_string())
                                    .expect("Error changing password");
                            } else {
                                self.push_message("Info", "Old password and new password required.");
                            }
                        }
                        _ => {
                            self.push_message("Info", "Unknown command.");
                        }
                    }

                    return;
                }

                let room = self.model.room.expect("Not in a room").clone();

                self.model.vertex.send_message(msg.to_string(), room).expect("Error sending message");

                let name = self
                    .model
                    .vertex
                    .display_name
                    .as_ref()
                    .expect("Not logged in");

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
