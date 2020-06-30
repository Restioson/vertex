use std::sync::Mutex;
use std::rc::Rc;
use std::{iter};
use std::cell::RefCell;
use bimap::BiMap;
use gtk::prelude::*;
use vertex::prelude::*;
use crate::connect::AsConnector;
use crate::screen::active::dialog;
use crate::{Client, window, TryGetText};
use super::Action;

pub struct UsersSearch {
    list: gtk::ListStore,
    view: gtk::TreeView,
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>,
    client: Client,
}

impl UsersSearch {
    pub fn build(builder: gtk::Builder, client: Client) {
        let users_search: gtk::SearchEntry = builder.get_object("users_search_entry").unwrap();
        let list_all_button: gtk::Button = builder.get_object("list_users_button").unwrap();
        let ban_button: gtk::Button = builder.get_object("ban_button").unwrap();
        let unban_button: gtk::Button = builder.get_object("unban_button").unwrap();
        let unlock_button: gtk::Button = builder.get_object("unlock_button").unwrap();
        let promote_button: gtk::Button = builder.get_object("promote_button").unwrap();

        let this = Rc::new(UsersSearch {
            list:  Self::create_model(),
            view: builder.get_object("users_search_list").unwrap(),
            username_to_id: Rc::new(Mutex::new(BiMap::new())),
            client,
        });
        this.create_and_setup_view();

        users_search.connect_activate(
            this.connector()
                .do_async(|this, entry: gtk::SearchEntry| async move {
                    let txt = entry.try_get_text().unwrap_or_else(|_| String::new());
                    match this.client.search_users(txt).await {
                        Ok(users) => this.insert_users(users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                })
                .build_cloned_consumer()
        );

        list_all_button.connect_clicked(
            this.connector()
                .do_async(|this, _| async move {
                    match this.client.list_all_server_users().await {
                        Ok(users) => this.insert_users(users),
                        Err(err) => dialog::show_generic_error(&err),
                    }
                })
                .build_cloned_consumer()
        );

        ban_button.connect_clicked(
            this.connector()
                .do_async(move |this, _| this.perform_action(Action::Ban))
                .build_cloned_consumer()
        );

        unban_button.connect_clicked(
            this.connector()
                .do_async(move |this, _| this.perform_action(Action::Unban))
                .build_cloned_consumer()
        );

        unlock_button.connect_clicked(
            this.connector()
                .do_async(move |this, _| this.perform_action(Action::Unlock))
                .build_cloned_consumer()
        );

        promote_button.connect_clicked(move |_| this.clone().show_promote());
    }

    fn create_model() -> gtk::ListStore {
        let types: Vec<glib::Type> = Some(bool::static_type())
            .into_iter()
            .chain(iter::repeat(String::static_type()))
            .take(7)
            .collect();
        gtk::ListStore::new(&types)
    }

    fn create_and_setup_view(&self) {
        super::append_checkbutton_column("Selected", &self.list, &self.view, 0);

        let headers = [
            "Username",
            "Display name",
            "Banned",
            "Compromised",
            "Locked",
            "Latest hash scheme"
        ];

        for (i, header) in headers.iter().enumerate() {
            super::append_text_column(header, &self.view, i as i32 + 1);
        }

        self.view.set_model(Some(&self.list));
    }

    async fn perform_action(self: Rc<Self>, action: Action) {
        super::perform_action(
            action,
            self.list.clone(),
            self.username_to_id.clone(),
            &self.client
        ).await
    }

    fn insert_users(&self, users: Vec<ServerUser>) {
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

    fn insert_user(&self, user: ServerUser) {
        // +----------+----------+--------------+--------+-------------+--------+-------------+
        // | Selected | Username | Display name | Banned | Compromised | Locked | Latest HSV  |
        // +----------+----------+--------------+--------+-------------+--------+-------------+

        let arr: &[&dyn glib::ToValue] = &[
            &false,
            &user.username,
            &user.display_name,
            &label_for_bool(user.banned),
            &label_for_bool(user.compromised),
            &label_for_bool(user.locked),
            &label_for_bool(user.latest_hash_scheme),
        ];

        let cols: Vec<_> = (0..7).collect();
        self.list.insert_with_values(None, &cols, arr);
    }

    fn show_promote(self: Rc<Self>) {
        let flags = Rc::new(RefCell::new(AdminPermissionFlags::from_bits_truncate(0)));
        let all: gtk::CheckButton = gtk::CheckButton::new_with_label("All permissions");
        let ban: gtk::CheckButton = gtk::CheckButton::new_with_label("Ban/unban users");
        let basic: gtk::CheckButton = gtk::CheckButton::new_with_label("Basic");
        let promote: gtk::CheckButton = gtk::CheckButton::new_with_label("Promote/demote admins");

        all.connect_toggled(
            flags.connector()
                .do_sync(|flags, _| flags.clone().borrow_mut().toggle(AdminPermissionFlags::ALL))
                .build_cloned_consumer()
        );

        ban.connect_toggled(
            flags.connector()
                .do_sync(|flags, _| flags.clone().borrow_mut().toggle(AdminPermissionFlags::BAN))
                .build_cloned_consumer()
        );

        promote.connect_toggled(
            flags.connector()
                .do_sync(|flags, _| flags.clone().borrow_mut().toggle(AdminPermissionFlags::PROMOTE))
                .build_cloned_consumer()
        );

        basic.connect_toggled(
            flags.connector()
                .do_sync(|flags, _| flags.clone().borrow_mut().toggle(AdminPermissionFlags::IS_ADMIN))
                .build_cloned_consumer()
        );

        let permissions = [basic, ban, promote, all];

        window::show_dialog(|window| {
            let dialog = gtk::Dialog::new_with_buttons(
                None,
                Some(&window.window),
                gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
                &[("Ok", gtk::ResponseType::Ok)],
            );
            let content = dialog.get_content_area();

            let label = gtk::Label::new(Some("Select Permissions"));
            label.get_style_context().add_class("title");
            let title_box = gtk::BoxBuilder::new()
                .orientation(gtk::Orientation::Horizontal)
                .hexpand(true)
                .child(&label)
                .build();
            content.add(&title_box);

            for permission in &permissions {
                content.add(permission);
            }

            dialog.connect_response(
                (self, flags).connector()
                    .do_async(|(this, flags), (dialog, response): (gtk::Dialog, gtk::ResponseType)| {
                        dialog.emit_close();
                        async move {
                            if response == gtk::ResponseType::Ok {
                                let action = Action::Promote { permissions: *flags.borrow() };
                                this.perform_action(action).await;
                            } else {
                                dialog.emit_close();
                            }
                        }
                    })
                    .build_widget_and_owned_listener()
            );

            (dialog, title_box)
        });
    }
}

fn label_for_bool(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}
