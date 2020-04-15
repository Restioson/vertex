use crate::{client, resource};

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
            .build();

        let label = gtk::LabelBuilder::new()
            .name("room_label")
            .label(&name)
            .halign(gtk::Align::Start)
            .build();
        container.add(&icon);
        container.add(&label);

        RoomEntryWidget { container, label }
    }
}

impl client::RoomEntryWidget<Ui> for RoomEntryWidget {
    fn bind_events(&self, _room: &client::RoomEntry<Ui>) {}
}
