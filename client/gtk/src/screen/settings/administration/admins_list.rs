use std::sync::Mutex;
use gtk::prelude::*;
use vertex::prelude::*;
use crate::{screen, Client, scheduler};
use std::rc::Rc;
use crate::connect::AsConnector;
use bimap::BiMap;
use crate::screen::settings::administration::Action;
use std::iter;
use crate::screen::active::dialog;

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

        scheduler::spawn(async move {
            match this.client.list_all_admins().await {
                Ok(users) => this.insert_users(users),
                Err(err) => dialog::show_generic_error(&err),
            }
        });
    }

    fn create_model() -> gtk::ListStore {
        let types: Vec<glib::Type> = Some(bool::static_type())
            .into_iter()
            .chain(Some(String::static_type()).into_iter())
            .chain(iter::repeat(bool::static_type()).take(3))
            .collect();
        gtk::ListStore::new(&types)
    }

    fn create_and_setup_view(&self) {
        super::append_checkbutton_column("Selected", &self.list, &self.view, 0);
        super::append_text_column("Username", &self.view, 1);

        let headers = [
            "Basic Permissions",
            "Ban/unban",
            "Promote/demote",
        ];

        for (i, header) in headers.iter().enumerate() {
            super::append_checkbutton_column(header, &self.list, &self.view, i as i32 + 2);
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

    fn insert_users(&self, users: Vec<Admin>) {
        let mut map = self.username_to_id.lock().unwrap();
        self.list.clear();
        map.clear();

        for user in users {
            map.insert(user.username.clone(), user.id);
            self.insert_user(user);
        }

        self.view.set_model(Some(&self.list));
        self.view.show_all();
    }

    fn insert_user(&self, user: Admin) {
        // +----------+----------+-------------------+-----------+----------------+
        // | Selected | Username | Basic Permissions | Ban/unban | Promote/demote |
        // +----------+----------+-------------------+-----------+----------------+

        let arr: &[&dyn glib::ToValue] = &[
            &false,
            &user.username,
            &user.permissions.contains(AdminPermissionFlags::IS_ADMIN),
            &user.permissions.contains(AdminPermissionFlags::BAN),
            &user.permissions.contains(AdminPermissionFlags::PROMOTE),
        ];

        let cols: Vec<_> = (0..5).collect();
        self.list.insert_with_values(None, &cols, arr);
    }
}