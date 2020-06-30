use crate::{resource};

use super::*;

#[derive(Clone)]
pub struct RoomEntryWidget {
    pub container: gtk::Box,
    label: gtk::Label,
}

impl RoomEntryWidget {
    pub fn build(name: String) -> Self {
        let container = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Horizontal)
            .build();
        let icon = gtk::ImageBuilder::new()
            .name("room_icon")
            .file(&resource("feather/hash.svg"))
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .build();
        let icon_container = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .vexpand(false)
            .build();
        icon_container.add(&icon);

        let label = gtk::LabelBuilder::new()
            .name("room_label")
            .label(&name)
            .halign(gtk::Align::Start)
            .wrap_mode(pango::WrapMode::WordChar)
            .wrap(true)
            .xalign(0.0)
            .build();
        container.add(&icon_container);
        container.add(&label);

        RoomEntryWidget { container, label }
    }
}
