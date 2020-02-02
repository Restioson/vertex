use gtk::prelude::*;

use crate::screen::Screen;

pub fn build() -> Screen<()> {
    let builder = gtk::Builder::new_from_file("res/glade/loading/loading.glade");
    let viewport: gtk::Viewport = builder.get_object("viewport").unwrap();

    Screen::new(viewport, ())
}
