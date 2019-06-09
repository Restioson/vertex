use gtk::prelude::*;
use gtk::Window;

fn main() {
    gtk::init().expect("Error initialising gtk");
    let glade_src = include_str!("client.glade");
    let builder = gtk::Builder::new_from_string(glade_src);

    let window: Window = builder.get_object("window").unwrap();
    window.set_title("Vertex client");
    window.set_default_size(350, 70);

    window.show_all();

    gtk::main();
}