use chrono::{DateTime, Utc, Duration, Datelike, Local};
use gtk::prelude::*;

use vertex::prelude::*;

use crate::client::{self, ChatSide, InviteEmbed, MessageEmbed, MessageStatus, OpenGraphEmbed};
use crate::Glade;

use super::*;
use pango::WrapMode;
use ordinal::Ordinal;

#[derive(Clone, Eq, PartialEq)]
pub struct MessageGroupWidget {
    pub author: UserId,
    pub origin_time: DateTime<Utc>,
    pub widget: gtk::Box,
    pub entry_list: gtk::ListBox,
}

impl MessageGroupWidget {
    pub fn build(author: UserId, profile: Profile, origin_time: DateTime<Utc>) -> MessageGroupWidget {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/message_entry.glade").unwrap();
        }

        let builder: gtk::Builder = GLADE.builder();

        let widget: gtk::Box = builder.get_object("message_group").unwrap();
        let entry_list: gtk::ListBox = builder.get_object("entry_list").unwrap();

        let author_name: gtk::Label = builder.get_object("author_name").unwrap();
        author_name.set_text(&profile.display_name);
        author_name.set_can_focus(false);

        let timestamp: gtk::Label = builder.get_object("timestamp").unwrap();

        let time_text = pretty_date(origin_time);
        timestamp.set_text(&time_text);

        MessageGroupWidget { author, origin_time, widget, entry_list }
    }

    pub fn can_combine(&self, user: UserId, time: DateTime<Utc>) -> bool {
        self.author == user && (time - self.origin_time).num_minutes().abs() < 10
    }

    pub fn add_message(&self, content: Option<String>, side: ChatSide) -> MessageEntryWidget {
        let entry = MessageEntryWidget {
            group: self.clone(),
            content: MessageContentWidget::build(content),
        };

        match side {
            ChatSide::Front => self.entry_list.add(&entry.content.widget),
            ChatSide::Back => self.entry_list.insert(&entry.content.widget, 0),
        }

        entry
    }

    fn is_empty(&self) -> bool {
        // TODO: this is quite expensive to allocate a vec; is there another way?
        self.entry_list.get_children().is_empty()
    }

    pub fn remove_from(&self, list: &gtk::ListBox) {
        if let Some(row) = self.widget.get_parent() {
            list.remove(&row);
        }
    }
}

#[derive(Clone)]
struct MessageContentWidget {
    widget: gtk::Box,
    text: gtk::Label,
}

impl MessageContentWidget {
    pub fn build(text: Option<String>) -> MessageContentWidget {
        let widget = gtk::BoxBuilder::new()
            .name("message")
            .orientation(gtk::Orientation::Vertical)
            .build();

        let text = gtk::LabelBuilder::new()
            .name("message_text")
            .label(text.unwrap_or_else(|| "<Deleted>".to_string()).trim()) // TODO deletion
            .halign(gtk::Align::Start)
            .selectable(true)
            .can_focus(false)
            .wrap_mode(WrapMode::WordChar)
            .wrap(true)
            .build();

        widget.add(&text);

        MessageContentWidget { widget, text }
    }
}

#[derive(Clone)]
pub struct MessageEntryWidget {
    group: MessageGroupWidget,
    content: MessageContentWidget,
}

impl MessageEntryWidget {
    pub fn remove(&self) -> Option<&MessageGroupWidget> {
        if let Some(row) = self.content.widget.get_parent() {
            self.group.entry_list.remove(&row);

            if self.group.is_empty() {
                return Some(&self.group);
            }
        }

        None
    }
}

impl client::MessageEntryWidget<Ui> for MessageEntryWidget {
    fn set_status(&self, status: client::MessageStatus) {
        let style = self.content.text.get_style_context();
        style.remove_class("pending");
        style.remove_class("error");

        match status {
            MessageStatus::Pending => style.add_class("pending"),
            MessageStatus::Err => style.add_class("error"),
            _ => (),
        }
    }

    fn push_embed(&self, client: &Client<Ui>, embed: MessageEmbed) {
        let embed = build_embed(client, embed);
        self.content.widget.add(&embed);
    }
}

fn build_embed(client: &Client<Ui>, embed: MessageEmbed) -> gtk::Widget {
    match embed {
        MessageEmbed::OpenGraph(og) => build_opengraph_embed(og),
        MessageEmbed::Invite(invite) => build_invite_embed(client, invite),
        // TODO: custom embed for errors
        MessageEmbed::Error(error) => build_opengraph_embed(OpenGraphEmbed {
            url: error.url,
            title: error.title,
            description: error.error,
        }),
    }
}

fn build_opengraph_embed(embed: OpenGraphEmbed) -> gtk::Widget {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/embed/opengraph.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let opengraph: gtk::Box = builder.get_object("opengraph").unwrap();

    let title_label: gtk::Label = builder.get_object("title").unwrap();
    title_label.set_text(&embed.title);

    let description_buffer: gtk::TextBuffer = builder.get_object("description_buffer").unwrap();
    description_buffer.set_text(&embed.description);

    opengraph.upcast()
}

fn build_invite_embed(client: &Client<Ui>, embed: InviteEmbed) -> gtk::Widget {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("active/embed/invite.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let invite: gtk::Box = builder.get_object("invite").unwrap();

    let name_label: gtk::Label = builder.get_object("community_name").unwrap();
    name_label.set_text(&embed.name);

    let motd_label: gtk::Label = builder.get_object("invite_community_description").unwrap();
    motd_label.set_text(&embed.description);

    let join_button: gtk::Button = builder.get_object("join_button").unwrap();
    join_button.connect_clicked(
        client.connector()
            .do_async(move |client, _| {
                let code = embed.code.clone();
                async move {
                    // TODO: report error
                    let _ = client.join_community(code).await;
                }
            })
            .build_cloned_consumer()
    );

    invite.upcast()
}

fn pretty_date(msg: DateTime<Utc>) -> String {
    let now = Local::now();
    let msg: DateTime<Local> = msg.into();

    if msg.date() == now.date() {
        msg.format("%H:%M").to_string() // e.g 13:34
    } else if msg.date() + Duration::days(1) == now.date() {
        msg.format("%H:%M, Yesterday").to_string() // e.g 13:34, Yesterday
    } else if msg.year() == now.year() {
        if msg.month() == now.month() {
            let msg_week = msg.iso_week().week() as i32;
            let week = now.iso_week().week() as i32;

            if msg_week == week {
                msg.format("%H:%M, %A").to_string() // e.g 13:34, Sunday
            } else if msg_week - week == 1 {
                msg.format("%H:%M, %A, last week").to_string() // e.g 13:34, Sunday, last week
            } else {
                let day = Ordinal(msg.day());
                msg.format(&format!("%H:%M, %A the {}", day)).to_string() // 13:34, Sunday the 7th
            }
        } else {
            msg.format("%H:%M, %B %d").to_string() // e.g 13:34, July 8
        }
    } else {
        msg.format("%H:%M, %d %B %Y").to_string() // e.g 13:34, 8 July 2018
    }
}
