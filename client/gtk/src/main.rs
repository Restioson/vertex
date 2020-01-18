use clap::{App, Arg};
use gtk::prelude::*;
use gtk::{Entry, Label, ListBox, Window, Separator, Orientation, Grid};
use keyring::Keyring;
use relm::{connect, Relm, Update, Widget};
use relm_derive::*;
use url::Url;
use uuid::Uuid;
use vertex_client_backend::*;
use vertex_common::*;

use serde::{Serialize, Deserialize};
use std::rc::Rc;
use futures::executor::{LocalPool, LocalSpawner};
use futures::task::LocalSpawnExt;
use futures::stream::StreamExt;

use tokio::sync::mpsc;

const NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

const GLADE_SRC: &str = include_str!("client.glade");

struct VertexModel {
    executor: LocalPool,
    spawner: LocalSpawner,
    action_recv: mpsc::UnboundedReceiver<Action>,
    vertex: Rc<Vertex>,
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
    device: DeviceId,
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
            Action::Error(_error) => {} // TODO handle this? @gegy
            Action::LoggedOut => {
                self.push_message("Info", "Logged out -- session invalidated. Please log in again.");
            }
        }
    }

    fn push_message(&self, author: &str, content: &str) {
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

        let separator = Separator::new(Orientation::Horizontal);

        separator.show_all();
        grid.show_all();

        self.widgets.messages.insert(&separator, -1);
        self.widgets.messages.insert(&grid, -1);
    }
}

impl Update for Win {
    type Model = VertexModel;
    type ModelParam = VertexArgs;
    type Msg = VertexMsg;

    fn model(_relm: &Relm<Win>, args: VertexArgs) -> VertexModel {
        let ip = args.ip.clone().unwrap_or("127.0.0.1:8080".to_string());
        println!("Connecting to {}", ip);

        let executor = LocalPool::new();

        let (action_send, action_recv) = mpsc::unbounded_channel();

        let vertex = futures::executor::block_on(async {
            let url = Url::parse(&format!("wss://{}/client/", ip)).unwrap();
            Vertex::connect(url).await.expect("failed to connect")
        });
        let vertex = Rc::new(vertex);

        let spawner = executor.spawner();
        spawner.spawn_local({
            use futures::pin_mut;

            let vertex = vertex.clone();
            async move {
                let stream = vertex.action_stream()
                    .expect("action stream already consumed");

                pin_mut!(stream);
                while let Some(action) = stream.next().await {
                    if action_send.send(action).is_err() {
                        break;
                    }
                }
            }
        }).unwrap();

        let model = VertexModel {
            executor,
            spawner,
            action_recv,
            vertex,
            room: None,
            community: None,
            room_list: Vec::new(),
            keyring: Keyring::new("vertex_client_gtk", ""), // username = ""
        };

        model
    }

    fn update(&mut self, event: VertexMsg) {
        // TODO: Currently blocking on actions: make async!

        match event {
            VertexMsg::SetRoom(idx) => {
                let room = self.model.room_list[idx];
                self.model.room = Some(room);
            }
            VertexMsg::SendMessage(msg) => {
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
                                self.push_message("Info", "Creating room...");

                                let community = CommunityId(
                                    Uuid::parse_str(v[2]).expect("Invalid community id"),
                                );

                                let vertex = self.model.vertex.clone();
                                let name = v[1].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.create_room(name, community).await.expect("Error creating room");
                                }).unwrap();

                                // TODO
//                                self.model.room = Some(room);
//                                let txt: &str = &format!("#{}", room.0);
//                                let room_label = Label::new(Some(txt));
//                                self.widgets.rooms.insert(&room_label, -1);
//                                self.model.room_list.push(room);
//                                room_label.show_all();
                            } else {
                                self.push_message("Error", &format!("Room name and community id required"));
                            }
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

                                    Some((stored_token.device, AuthToken(stored_token.token)))
                                } else {
                                    None
                                };

                                let vertex = self.model.vertex.clone();
                                let username = v[1].to_owned();
                                let password = v[2].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    let _ = vertex.login(token, username, password).await;
                                }).unwrap();

                                // TODO
//                                match result {
//                                    Ok((device, token)) => {
//                                        let stored_token = StoredToken {
//                                            device,
//                                            token: token.0.clone(),
//                                        };
//                                        let token_ser = serde_json::to_string(&stored_token)
//                                            .expect("Error serializing token");
//                                        self.model
//                                            .keyring
//                                            .set_password(&token_ser)
//                                            .expect("Error storing token");
//
//                                        self.push_message("Info", &format!("Successfully logged in. Device id: {}", device.0));
//                                    }
//                                    Err(e) => self.push_message("Error", &format!("Error logging in: {:?}", e)),
//                                }
                            } else {
                                self.push_message("Error", "Username and password required");
                            }
                        }
                        "/forgettoken" => {
                            self.model
                                .keyring
                                .delete_password()
                                .expect("Error forgetting token");
                            self.push_message("Info", "Token forgot");
                        }
                        "/refreshtoken" => {
                            if v.len() == 4 {
                                self.push_message("Info", "Refreshing token...");

                                let dev = DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));

                                let vertex = self.model.vertex.clone();
                                let username = v[2].to_owned();
                                let password = v[3].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.refresh_token(dev, username, password).await.expect("Error refreshing token")
                                }).unwrap();
                            } else {
                                self.push_message("Error", "Device ID, username, and password required");
                            }
                        }
                        "/register" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Registering user...");

                                let vertex = self.model.vertex.clone();
                                let username = v[1].to_owned();
                                let password = v[2].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.create_user(username.clone(), username, password).await.expect("Error registering user");
                                }).unwrap();
                            } else {
                                self.push_message("Error", "Username and password required");
                            }
                        }
                        "/revokecurrent" => {
                            self.push_message("Info", "Revoking token...");

                            let vertex = self.model.vertex.clone();
                            self.model.spawner.spawn_local(async move {
                                vertex.revoke_current_token().await.expect("Error revoking current token")
                            }).unwrap();
                        }
                        "/revoke" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Revoking token...");

                                let dev = DeviceId(Uuid::parse_str(v[1]).expect("Invalid device id"));

                                let vertex = self.model.vertex.clone();
                                let password = v[2].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.revoke_token(dev, password).await.expect("Error revoking token")
                                }).unwrap();
                            } else {
                                self.push_message("Error", "Token and password required");
                            }
                        }
                        "/changeusername" => {
                            if v.len() == 2 {
                                self.push_message("Info", "Changing username...");

                                let vertex = self.model.vertex.clone();
                                let username = v[1].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.change_username(username).await.expect("Error changing username")
                                }).unwrap();
                            } else {
                                self.push_message("Error", "New username required");
                            }
                        }
                        "/changedisplayname" => {
                            if v.len() == 2 {
                                self.push_message("Info", "Changing display name...");

                                let vertex = self.model.vertex.clone();
                                let display_name = v[1].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.change_display_name(display_name).await.expect("Error changing display name")
                                }).unwrap();
                            } else {
                                self.push_message("Error", "New display name required");
                            }
                        }
                        "/changepassword" => {
                            if v.len() == 3 {
                                self.push_message("Info", "Changing password...");

                                let vertex = self.model.vertex.clone();
                                let old_password = v[1].to_owned();
                                let new_password = v[2].to_owned();

                                self.model.spawner.spawn_local(async move {
                                    vertex.change_password(old_password, new_password).await.expect("Error changing password")
                                }).unwrap();
                            } else {
                                self.push_message("Error", "Old password and new password required");
                            }
                        }
                        _ => {
                            self.push_message("Error", "Unknown command");
                        }
                    }

                    return;
                }

                let room = self.model.room.expect("Not in a room").clone();
                let community = self.model.community.expect("Not in a communtiy").clone();

                let vertex = self.model.vertex.clone();
                self.model.spawner.spawn_local({
                    let msg = msg.clone();
                    async move {
                        vertex.send_message(msg, community, room).await.expect("Error sending message")
                    }
                }).unwrap();

                let name = self.model.vertex.identity.borrow().as_ref()
                    .map(|ident| ident.display_name.clone())
                    .expect("Not logged in");

                self.push_message(&name, &msg);
            }
            VertexMsg::Lifecycle => {
                while let Some(action) = self.model.action_recv.try_recv().ok() {
                    println!("action {:?}", action);
                    self.handle_action(action);
                }
            }
            VertexMsg::Heartbeat => {
                let vertex = self.model.vertex.clone();
                self.model.spawner.spawn_local(async move {
                    vertex.dispatch_heartbeat().await
                        .expect("failed to dispatch heartbeat");
                }).unwrap();
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

// TODO: can we get rid of need for this? (do we need to use tokio-tungstenite or can we just use tungstenite?)
#[tokio::main]
async fn main() {
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
