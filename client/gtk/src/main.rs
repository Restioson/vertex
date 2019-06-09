use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, TextView, Entry};
use url::Url;
use uuid::Uuid;
use lazy_static::lazy_static;
use vertex_client_backend::*;

lazy_static ! {
    static ref VERTEX: Mutex<Vertex> = Mutex::new(Vertex::new(Config {
        url: Url::parse("ws://127.0.0.1:8080/client/").unwrap(),
        client_id: Uuid::new_v4(),
    }));
}

fn main() {
    let app = Application::new("com.github.restioson.vertex", Default::default())
        .expect("Error initializing GTK application");

    app.connect_activate(|app| create(app));
    app.run(&std::env::args().collect::<Vec<_>>());
}

fn create(app: &Application) {
    let glade_src = include_str!("client.glade");
    let builder = gtk::Builder::new_from_string(glade_src);

    let window: ApplicationWindow = builder.get_object("window").unwrap();
    window.set_application(app);
    window.set_title("Vertex client");
    window.set_default_size(640, 480);

    let (action_tx, action_rx) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
    VERTEX.lock().unwrap().login().expect("Error logging in");

    // TODO
    let room = VERTEX.lock().unwrap().create_room().expect("Error creating room");

    thread::spawn(move || {
        loop {
            let action = VERTEX.lock().unwrap().handle();
            if let Some(action) = action {
                println!("action {:?}", action);
                action_tx.send(action).expect("Error sending action over channel");
            }

            thread::sleep(Duration::from_millis(16)); // Read once every frame
        }
    });

    let messages: TextView = builder.get_object("messages").unwrap();
    let text_buffer = messages.get_buffer().unwrap();

    action_rx.attach(None, move |action| {
        match action {
            Action::AddMessage(msg) => {
                text_buffer.set_text(&msg.content);
            },
            _ => unimplemented!(),
        }

        glib::Continue(true)
    });

    let entry: Entry = builder.get_object("message_entry").unwrap();
    entry.connect_activate(move |entry| {
        VERTEX
            .lock()
            .unwrap()
            .send_message(entry.get_text().unwrap().to_string(), room)
            .expect("Error sending message"); // todo display

        entry.set_text("");
    });

    window.show_all();
}
