use gtk::prelude::*;
use vertex::prelude::*;
use crate::{screen, Glade, Client, scheduler, TryGetText, config};
use std::rc::Rc;
use crate::connect::AsConnector;
use crate::screen::active::dialog;
use crate::screen::active::message::MessageGroupWidget;
use super::parse_search;

pub struct ReportsList {
    list: gtk::Box,
    client: Client<screen::active::Ui>,
}

impl ReportsList {
    pub fn build(builder: gtk::Builder, client: Client<screen::active::Ui>) {
        let list_all: gtk::Button = builder.get_object("list_reports").unwrap();
        let search: gtk::SearchEntry = builder.get_object("reports_search_entry").unwrap();

        let this = Rc::new(ReportsList {
            list: builder.get_object("reports_list").unwrap(),
            client,
        });

        list_all.connect_clicked(
            this.connector()
                .do_async(move |this, _| this.search(SearchCriteria::default()))
                .build_cloned_consumer()
        );

        search.connect_activate(
            this.connector()
                .do_async(move |this, search: gtk::SearchEntry| async move {
                    let txt = search.try_get_text().unwrap_or_else(|_| "".to_string());
                    let parsed = match parse_search::do_parse(&txt) {
                        Ok((_, parsed)) => parsed,
                        Err(e) => return dialog::show_generic_error(&e),
                    };

                    this.search(parsed).await;
                })
                .build_cloned_consumer()
        );

        let open = SearchCriteria {
            status: Some(ReportStatus::Opened),
            ..Default::default()
        };
        scheduler::spawn(this.search(open));
    }

    fn insert_reports(&self, reports: Vec<Report>) {
        lazy_static::lazy_static! {
            static ref GLADE: Glade = Glade::open("settings/report.glade").unwrap();
        };

        self.list.foreach(|w| self.list.remove(w));

        for report in reports {
            let builder = GLADE.builder();
            let main: gtk::Box = builder.get_object("report").unwrap();

            let date: gtk::Label = builder.get_object("date").unwrap();
            date.set_text(&format!("Reported on {}", report.datetime.date().format("%F")));

            let by_user: gtk::Label = builder.get_object("by_user").unwrap();
            let name = report.reporter.map(|x| x.username)
                .unwrap_or_else(|| "<Deleted User>".to_string());
            by_user.set_text(&format!("Reported by \"{}\"", name));

            let title: gtk::Label = builder.get_object("title").unwrap();
            title.set_text(&format!("Title: \"{}\"", report.short_desc));

            let desc: gtk::Label = builder.get_object("description").unwrap();
            desc.set_text(&format!("Description: \"{}\"", &report.extended_desc));

            let loc: gtk::Label = builder.get_object("in").unwrap();
            loc.set_text(&format!(
                "In: channel \"{}\" in community \"{}\"",
                report.room.map(|r| r.name).unwrap_or_else(|| "<Deleted>".to_string()),
                report.community.map(|c| c.name).unwrap_or_else(|| "<Deleted>".to_string()),
            ));

            let status: gtk::Label = builder.get_object("status").unwrap();
            status.set_text(&format!("Status: {}", &report.status));

            let profile = Profile {
                version: ProfileVersion(0), // doesn't matter
                username: report.reported.username.clone(),
                display_name: report.reported.username, // its fine
            };
            let msg = MessageGroupWidget::build(
                report.reported.id,
                profile,
                report.message.sent_at,
                false,
                config::get().screen_reader_message_list,
            );
            msg.add_report_message(
                &main,
                Some(report.message.text),
                MessageId::default(), // doesn't matter
                self.client.clone(),
            );

            let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            main.add(&buttons);
            build_buttons(
                self.client.clone(),
                buttons,
                status,
                report.status,
                report.id,
                report.reported.id,
            );

            main.show_all();

            self.list.add(&main);
        }
        self.list.show_all();
    }

    async fn search(self: Rc<Self>, criteria: SearchCriteria) {
        match self.client.search_reports( criteria).await {
            Ok(reports) => self.insert_reports(reports),
            Err(err) => dialog::show_generic_error(&err),
        }
    }
}

fn build_buttons(
    client: Client<screen::active::Ui>,
    buttons: gtk::Box,
    status_label: gtk::Label,
    status: ReportStatus,
    id: i32,
    user: UserId,
) {
    buttons.foreach(|w| buttons.remove(w));
    status_label.set_text(&format!("Status: {}", &status));

    if status == ReportStatus::Opened {
        let accept = gtk::Button::new_with_label("Accept (choose an action...)");
        accept.connect_clicked(
            (client.clone(), buttons.clone(), status_label.clone()).connector()
                .do_async(move |(client, buttons, status_label), _| async move {
                    let status = ReportStatus::Accepted;
                    match client.set_report_status(id, status).await {
                        Err(e) => dialog::show_generic_error(&e),
                        Ok(_) => {
                            dialog::show_choose_report_action(client.clone(), user);
                            build_buttons(client, buttons, status_label, status, id, user);
                        }
                    }
                })
                .build_cloned_consumer()
        );

        let deny = gtk::Button::new_with_label("Deny (do not take action...)");
        deny.connect_clicked(
            (client.clone(), buttons.clone(), status_label).connector()
                .do_async(move |(client, buttons, status_label), _| async move {
                    let status = ReportStatus::Denied;
                    match client.set_report_status(id, status).await {
                        Err(e) => dialog::show_generic_error(&e),
                        Ok(_) => build_buttons(client, buttons, status_label, status, id, user),
                    }

                })
                .build_cloned_consumer()
        );

        buttons.add(&accept);
        buttons.add(&deny);
    } else {
        let reopen = gtk::Button::new_with_label("Re-open report");
        reopen.connect_clicked(
            (client.clone(), buttons.clone(), status_label).connector()
                .do_async(move |(client, buttons, status_label), _| async move {
                    let status = ReportStatus::Opened;
                    match client.set_report_status(id, status).await {
                        Err(e) => dialog::show_generic_error(&e),
                        Ok(_) => build_buttons(client, buttons, status_label, status, id, user),
                    }
                })
                .build_cloned_consumer()
        );

        buttons.add(&reopen);
    }

    buttons.show_all();
}
