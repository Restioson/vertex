use gtk::prelude::*;

use crate::UiShared;

pub fn build() -> UiShared<gtk::Viewport> {
    let builder = gtk::Builder::new_from_file("res/glade/loading/loading.glade");
    UiShared::new(builder.get_object("viewport").unwrap())
}
