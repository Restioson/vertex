use gtk::prelude::*;

use crate::UiEntity;

pub fn build() -> UiEntity<gtk::Viewport> {
    let builder = gtk::Builder::new_from_file("res/glade/loading/loading.glade");
    UiEntity::new(builder.get_object("viewport").unwrap())
}
