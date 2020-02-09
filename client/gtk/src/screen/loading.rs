use gtk::prelude::*;

pub fn build() -> gtk::Viewport {
    let builder = gtk::Builder::new_from_file("res/glade/loading/loading.glade");
    builder.get_object("viewport").unwrap()
}
