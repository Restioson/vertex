use crate::client;

use super::*;

#[derive(Clone)]
pub struct RoomEntryWidget {
    pub label: gtk::Label,
}

impl RoomEntryWidget {
    pub fn build(name: String) -> Self {
        RoomEntryWidget {
            label: gtk::LabelBuilder::new()
                .name("room_label")
                .label(&name)
                .halign(gtk::Align::Start)
                .build()
        }
    }
}

impl client::RoomEntryWidget<Ui> for RoomEntryWidget {
    fn bind_events(&self, _room: &client::RoomEntry<Ui>) {}
}
