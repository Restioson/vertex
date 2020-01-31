use gtk::prelude::*;

use crate::screen::Screen;

const SCREEN_SRC: &str = include_str!("glade/loading/loading.glade");

pub fn build() -> Screen<()> {
    let builder = gtk::Builder::new_from_string(SCREEN_SRC);
    let viewport: gtk::Viewport = builder.get_object("viewport").unwrap();

    Screen::new(viewport, ())
}
