use std::sync::Mutex;
use gtk::prelude::*;
use vertex::prelude::*;
use crate::{screen, Client};
use std::rc::Rc;
use crate::connect::AsConnector;
use bimap::BiMap;
use crate::screen::settings::administration::Action;
use std::iter;

pub struct AdminsList {
    list: gtk::ListStore,
    view: gtk::TreeView,
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>,
    client: Client<screen::active::Ui>,
}

impl AdminsList {
    pub fn build(builder: gtk::Builder, client: Client<screen::active::Ui>) {
        let demote_button: gtk::Button = builder.get_object("demote_button").unwrap();

        let this = Rc::new(AdminsList {
            list: Self::create_model(),
            view: builder.get_object("admins_list").unwrap(),
            username_to_id: Rc::new(Mutex::new(BiMap::new())),
            client,
        });
        this.create_and_setup_view();

        demote_button.connect_clicked(
            this.connector()
                .do_async(move |this, _| this.perform_action(Action::Demote))
                .build_cloned_consumer()
        );
    }

    fn create_model() -> gtk::ListStore {
        let types: Vec<glib::Type> = iter::repeat(bool::static_type())
            .take(5)
            .collect();
        gtk::ListStore::new(&types)
    }

    fn create_and_setup_view(&self) {
        let headers = [
            "Selected",
            "Username",
            "Basic Permissions",
            "Ban/unban",
            "Promote/demote",
        ];

        for (i, header) in headers.iter().enumerate() {
            super::append_checkbutton_column(header, &self.list, &self.view, i as i32);
        }

        self.view.set_model(Some(&self.list));
    }

    async fn perform_action(self: Rc<Self>, action: Action) {
        super::perform_action(
            action,
            self.list.clone(),
            self.username_to_id.clone(),
            self.client.clone()
        ).await
    }
}