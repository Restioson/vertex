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
use std::collections::HashMap;

pub struct AdminsList {
    list: gtk::ListStore,
    view: gtk::TreeView,
    username_to_id: Rc<Mutex<BiMap<String, UserId>>>,
    id_to_perm: Rc<Mutex<HashMap<UserId, AdminPermissionFlags>>>,
    client: Client<screen::active::Ui>,
}

impl AdminsList {
    pub fn build(builder: gtk::Builder, client: Client<screen::active::Ui>) {
        let demote_button: gtk::Button = builder.get_object("demote_button").unwrap();

        let this = Rc::new(AdminsList {
            list: Self::create_model(),
            view: builder.get_object("admins_list").unwrap(),
            username_to_id: Rc::new(Mutex::new(BiMap::new())),
            id_to_perm: Rc::new(Mutex::new(HashMap::new())),
            client,
        });
        this.create_and_setup_view();

        demote_button.connect_clicked(
            this.connector()
                .do_async(move |this, _| async move {
                    this.clone().perform_action(Action::Demote).await;
                    this.refresh().await;
                })
                .build_cloned_consumer()
        );

        scheduler::spawn(this.refresh());
    }

    fn create_model() -> gtk::ListStore {
        let types: Vec<glib::Type> = Some(bool::static_type())
            .into_iter()
            .chain(Some(String::static_type()).into_iter())
            .chain(iter::repeat(bool::static_type()).take(5))
            .chain(Some(String::static_type()).into_iter()) // Dummy
            .collect();
        gtk::ListStore::new(&types)
    }

    fn create_and_setup_view(&self) {
        super::append_checkbutton_column("Selected", &self.list, &self.view, 0);
        super::append_text_column("Username", &self.view, 1);

        let headers = [
            "All Permissions",
            "Basic Permissions",
            "Ban/unban",
            "Promote/demote",
            "Set accounts compromised",
        ];

        for (i, header) in headers.iter().enumerate() {
            let column = gtk::TreeViewColumn::new();
            let cell = gtk::CellRendererToggle::new();
            cell.set_activatable(true);
            let id = i as i32 + 2;

            cell.connect_toggled(
                (
                    self.list.clone(),
                    self.client.clone(),
                    self.username_to_id.clone(),
                    self.id_to_perm.clone(),
                ).connector()
                    .do_async(move |(store, client, name_to_id, map), (_cell, path): (gtk::CellRendererToggle, gtk::TreePath)| {
                        let row = store.get_iter(&path).unwrap();
                        let toggled = store.get_value(&row, id).get::<bool>().unwrap().unwrap();

                        async move {
                            let perm = match i {
                                0 => AdminPermissionFlags::ALL,
                                1 => AdminPermissionFlags::IS_ADMIN,
                                2 => AdminPermissionFlags::BAN,
                                3 => AdminPermissionFlags::PROMOTE,
                                4 => AdminPermissionFlags::SET_ACCOUNTS_COMPROMISED,
                                _ => panic!("Invalid col #"),
                            };
                            let name = store.get_value(&row, 1).get::<String>().unwrap().unwrap();
                            let name_to_id = name_to_id.lock().unwrap();
                            let user_id = name_to_id.get_by_left(&name).unwrap();
                            let mut map = map.lock().unwrap();
                            let perms = map.get_mut(user_id).unwrap();
                            let old_perms = *perms;

                            perms.set(perm, !toggled);

                            let old_toggled = toggled;
                            let res = client.promote_users(vec![*user_id], *perms).await;
                            let toggled = match res {
                                Ok(mut v) if v.len() > 0 => {
                                    dialog::show_generic_error(&v.pop().unwrap().1);
                                    toggled
                                },
                                Err(e) => {
                                    dialog::show_generic_error(&e);
                                    toggled
                                },
                                _ => {
                                    !toggled
                                }
                            };

                            // Action failed, revert perms
                            if old_toggled == toggled {
                                *perms = old_perms;
                            }

                            store.set_value(&row, id as u32, &toggled.to_value())
                        }
                    })
                    .build_widget_and_owned_listener()
            );

            column.pack_start(&cell, true);
            column.add_attribute(&cell, "active", id);
            column.set_title(header);
            self.view.append_column(&column);
        }

        // Dummy for alignment of checkbutton
        super::append_text_column("", &self.view, 7);

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
        let mut name_to_perm = self.id_to_perm.lock().unwrap();
        self.list.clear();
        map.clear();
        name_to_perm.clear();

        for user in users {
            map.insert(user.username.clone(), user.id);
            name_to_perm.insert(user.id, user.permissions);
            self.insert_user(user);
        }

        self.view.set_model(Some(&self.list));
        self.view.show_all();
    }

    fn insert_user(&self, user: Admin) {
        // +----------+----------+-----+--------+-----+---------+
        // | Selected | Username | All | Basic  | Ban | Promote |
        // +----------+----------+-----+--------+-----+---------+

        let arr: &[&dyn glib::ToValue] = &[
            &false,
            &user.username,
            &user.permissions.contains(AdminPermissionFlags::ALL),
            &user.permissions.contains(AdminPermissionFlags::IS_ADMIN),
            &user.permissions.contains(AdminPermissionFlags::BAN),
            &user.permissions.contains(AdminPermissionFlags::PROMOTE),
            &user.permissions.contains(AdminPermissionFlags::SET_ACCOUNTS_COMPROMISED),
        ];

        let cols: Vec<_> = (0..7).collect();
        self.list.insert_with_values(None, &cols, arr);
    }

    async fn refresh(self: Rc<Self>) {
        match self.client.list_all_admins().await {
            Ok(users) => self.insert_users(users),
            Err(err) => dialog::show_generic_error(&err),
        }
    }
}
