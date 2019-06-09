use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};

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

    window.show_all();
}
