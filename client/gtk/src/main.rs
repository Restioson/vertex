use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Entry, TextView};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use url::Url;
use uuid::Uuid;
use vertex_client_backend::*;

macro_rules! clone {
    (@param _) => ( _ );
    (@param $x:ident) => ( $x );
    ($($n:ident),+ => move || $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move || $body
        }
    );
    ($($n:ident),+ => move |$($p:tt),+| $body:expr) => (
        {
            $( let $n = $n.clone(); )+
            move |$(clone!(@param $p),)+| $body
        }
    );
}

struct VertexApp {
    vertex: Vertex,
    room: Option<Uuid>,
}

fn main() {
    let app = Application::new("com.github.restioson.vertex", Default::default())
        .expect("Error initializing GTK application");

    app.connect_activate(move |app| create(app));

    app.run(&std::env::args().collect::<Vec<_>>());
}

fn create(gtk_app: &Application) {
    let app = Arc::new(Mutex::new(VertexApp {
        vertex: Vertex::new(Config {
            url: Url::parse("ws://127.0.0.1:8080/client/").unwrap(),
            client_id: Uuid::new_v4(),
        }),
        room: None,
    }));

    let glade_src = include_str!("client.glade");
    let builder = gtk::Builder::new_from_string(glade_src);

    let window: ApplicationWindow = builder.get_object("window").unwrap();
    window.set_application(gtk_app);
    window.set_title("Vertex client");
    window.set_default_size(640, 480);

    let messages: TextView = builder.get_object("messages").unwrap();
    let text_buffer = messages.get_buffer().unwrap();

    let (action_tx, action_rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
    app.lock()
        .unwrap()
        .vertex
        .login()
        .expect("Error logging in");

    thread::spawn(clone!(app => move || {
        loop {
            let action = app.lock().unwrap().vertex.handle();
            if let Some(action) = action {
                println!("action {:?}", action);
                action_tx.send(action).expect("Error sending action over channel");
            }

            thread::sleep(Duration::from_millis(16)); // Read once every frame
        }
    }));

    action_rx.attach(
        None,
        clone!(text_buffer => move |action| {
            match action {
                Action::AddMessage(msg) => {
                    text_buffer.insert(
                        &mut text_buffer.get_end_iter(),
                        &format!("{}: {}\n", msg.author, msg.content)
                    );
                },
                _ => panic!("unimplemented"),
            }

            glib::Continue(true)
        }),
    );

    let entry: Entry = builder.get_object("message_entry").unwrap();
    entry.connect_activate(move |entry| {
        let mut app = app.lock().unwrap();
        let msg = entry.get_text().unwrap().to_string();

        if msg.trim().starts_with("/") {
            let v: Vec<&str> = msg.splitn(2, ' ').collect();

            match v[0] {
                "/join" => {
                    if v.len() == 2 {
                        let room = Uuid::parse_str(v[1]).expect("Invalid room id");
                        app.vertex
                            .join_room(room.clone())
                            .expect("Error joining room");
                        text_buffer.insert(
                            &mut text_buffer.get_end_iter(),
                            &format!("Joined room {}\n", room),
                        );

                        app.room = Some(room)
                    } else {
                        text_buffer.insert(&mut text_buffer.get_end_iter(), "Room id required");
                    }
                }
                "/createroom" => {
                    text_buffer.insert(&mut text_buffer.get_end_iter(), "Creating room...\n");
                    let room = app.vertex.create_room().expect("Error creating room");
                    text_buffer.insert(
                        &mut text_buffer.get_end_iter(),
                        &format!("Joined room {}\n", room),
                    );

                    app.room = Some(room)
                }
                _ => {
                    text_buffer.insert(&mut text_buffer.get_end_iter(), "Unknown command\n");
                }
            }

            entry.set_text("");
            return;
        }

        let room = app.room.expect("Not in a room").clone();
        app.vertex
            .send_message(msg.to_string(), room)
            .expect("Error sending message"); // todo display error

        let name = app.vertex.username();
        text_buffer.insert(
            &mut text_buffer.get_end_iter(),
            &format!("{}: {}\n", name, msg),
        );
        entry.set_text("");
    });

    window.show_all();
}
