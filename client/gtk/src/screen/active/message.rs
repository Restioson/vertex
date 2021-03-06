use chrono::{DateTime, Utc, Duration, Datelike, Local};
use gtk::prelude::*;

use vertex::prelude::*;

use crate::client::{ChatSide, InviteEmbed, MessageEmbed, MessageStatus, OpenGraphEmbed};
use crate::{Glade, resource};

use super::*;
use pango::WrapMode;
use ordinal::Ordinal;
use atk::AtkObjectExt;

#[derive(Clone, PartialEq, Eq)]
pub struct MessageGroupWidget {
    author: UserId,
    origin_time: DateTime<Utc>,
    interactable: bool,
    flavour: MessageGroupFlavour,
    messages: Vec<MessageId>,
}

#[derive(Clone)]
enum MessageGroupFlavour {
    Widget {
        widget: gtk::Box,
        entry_list: gtk::ListBox,
    },
    Inline {
        title: gtk::Label,
        messages: Vec<MessageEntryWidget>,
    },
}

impl PartialEq for MessageGroupFlavour {
    fn eq(&self, other: &Self) -> bool {
        use MessageGroupFlavour::*;
        match (other, self) {
            (Widget { widget, .. }, Widget { widget: other, .. }) => {
                widget == other
            },
            (Inline { title, .. }, Inline { title: other, .. }) => {
                title == other
            },
            _ => false,
        }
    }
}

impl Eq for MessageGroupFlavour {}

impl MessageGroupWidget {
    pub fn build(
        author: UserId,
        profile: Profile,
        origin_time: DateTime<Utc>,
        interactable: bool,
        is_inline: bool,
    ) -> MessageGroupWidget {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/message_entry.glade").unwrap();
        }

        if is_inline {
            let title = format!(
                "{} at {} said",
                profile.display_name,
                pretty_date(origin_time),
            );
            let title = gtk::LabelBuilder::new()
                .label(&title)
                .xalign(0.0)
                .build();
            let flavour = MessageGroupFlavour::Inline {
                title,
                messages: Vec::with_capacity(1),
            };

            MessageGroupWidget {
                author,
                origin_time,
                flavour,
                messages: Vec::new(),
                interactable
            }
        } else {
            let builder: gtk::Builder = GLADE.builder();

            let widget: gtk::Box = builder.get_object("message_group").unwrap();
            let entry_list: gtk::ListBox = builder.get_object("entry_list").unwrap();

            let author_name: gtk::Label = builder.get_object("author_name").unwrap();
            author_name.set_text(&profile.display_name);
            author_name.set_can_focus(false);

            let timestamp: gtk::Label = builder.get_object("timestamp").unwrap();

            let time_text = pretty_date(origin_time);
            timestamp.set_text(&time_text);
            widget.hide();

            let flavour = MessageGroupFlavour::Widget {
                widget,
                entry_list
            };

            MessageGroupWidget {
                author,
                origin_time,
                flavour,
                messages: Vec::new(),
                interactable,
            }
        }
    }

    pub fn can_combine(&self, user: UserId, time: DateTime<Utc>) -> bool {
        self.author == user && (time - self.origin_time).num_minutes().abs() < 10
    }

    pub fn add_message(
        &mut self,
        content: Option<String>,
        id: MessageId,
        side: ChatSide,
        list: &gtk::ListBox,
        client: Client,
    ) -> MessageEntryWidget {
        let entry = MessageEntryWidget::build(client, content, id, self.interactable);

        match &mut self.flavour {
            MessageGroupFlavour::Inline { title, messages } => {
                match side {
                    ChatSide::Front => {
                        self.messages.push(id);
                        messages.push(entry.clone());

                        list.add(&entry.widget);
                    },
                    ChatSide::Back => {
                        self.messages.insert(0, id);
                        messages.insert(0, entry.clone());

                        if let Some(row) = title.get_parent() {
                            list.remove(&row);
                        }
                        list.insert(&entry.widget, 0);
                        list.insert(title, 0);
                    },
                }
            },
            MessageGroupFlavour::Widget { entry_list, .. } => {
                match side {
                    ChatSide::Front => {
                        self.messages.push(id);
                        entry_list.add(&entry.widget)
                    },
                    ChatSide::Back => {
                        self.messages.insert(0, id);
                        entry_list.insert(&entry.widget, 0)
                    },
                }
            }
        }

        entry
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn position_of(&self, id: &MessageId) -> Option<usize> {
        self.messages.iter().position(|i| i == id)
    }

    pub fn remove_from(&self, list: &gtk::ListBox) {
        match &self.flavour {
            MessageGroupFlavour::Inline { title, messages } => {
                messages
                    .iter()
                    .map(|w| w.widget.clone().upcast())
                    .chain(Some(title.clone().upcast::<gtk::Widget>()))
                    .filter_map(|widget| widget.get_parent())
                    .for_each(|row| list.remove(&row));
            },
            MessageGroupFlavour::Widget { widget, .. } => {
                if let Some(row) = widget.get_parent() {
                    list.remove(&row);
                }
            }
        }
    }
    
    pub fn add_to(&self, list: &gtk::ListBox, side: ChatSide) {
        match &self.flavour {
            MessageGroupFlavour::Inline { title, .. } => {
                match side {
                    ChatSide::Front => list.add(title),
                    ChatSide::Back => list.insert(title, 0),
                }
            }
            MessageGroupFlavour::Widget { widget, .. } => {
                match side {
                    ChatSide::Front => list.add(widget),
                    ChatSide::Back => list.insert(widget, 0),
                }
            }
        }
    }

    pub fn add_report_message(
        &self,
        b: &gtk::Box,
        content: Option<String>,
        id: MessageId,
        client: Client,
    ) {
        let entry = MessageEntryWidget::build(client, content, id, self.interactable);

        match &self.flavour {
            MessageGroupFlavour::Inline { title, .. } => {
                b.add(title);
                b.add(&entry.widget);
            }
            MessageGroupFlavour::Widget { widget, .. } => {
                b.add(widget);
            }
        }
    }

    pub fn remove_message(
        &mut self,
        pos: usize,
    ) -> Option<&MessageGroupWidget> {
        let widget: gtk::Widget = match &self.flavour {
            MessageGroupFlavour::Inline { messages, .. } => {
                messages[pos].widget.clone().upcast()
            },
            MessageGroupFlavour::Widget { entry_list, .. } => {
                entry_list.get_row_at_index(pos as i32).unwrap().upcast()
            },
        };

        self.messages.remove(pos);

        match &mut self.flavour {
            MessageGroupFlavour::Inline { messages, .. } => {
                messages.remove(pos);
                let row: gtk::ListBoxRow = widget.get_parent().unwrap().downcast().unwrap();
                let list: gtk::ListBox = row.get_parent().unwrap().downcast().unwrap();
                list.remove(&row);
            },
            MessageGroupFlavour::Widget { entry_list, .. } => entry_list.remove(&widget),
        }

        if self.is_empty() {
            return Some(self);
        }

        None
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MessageEntryWidget {
    widget: gtk::Box,
    text: gtk::Label,
}

impl MessageEntryWidget {
    pub fn build(
        client: Client,
        text: Option<String>,
        id: MessageId,
        interactable: bool,
    ) -> Self {
        thread_local! {
            static ICON: gdk_pixbuf::Pixbuf = gdk_pixbuf::Pixbuf::new_from_file_at_size(
                &resource("feather/more-horizontal-cropped.svg"),
                15,
                10,
            ).expect("Error loading more-horizontal-cropped.svg!");
        }

        let vbox = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Vertical)
            .name("message")
            .build();

        let hbox = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .build();

        let text = gtk::LabelBuilder::new()
            .name("message_text")
            .label(text.unwrap_or_else(|| "<Deleted>".to_string()).trim()) // TODO deletion
            .halign(gtk::Align::Start)
            .hexpand(true)
            .selectable(true)
            .can_focus(true)
            .wrap_mode(WrapMode::WordChar)
            .wrap(true)
            .build();

        let settings_vbox = gtk::BoxBuilder::new()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::End)
            .build();

        let icon = ICON.with(|icon| gtk::Image::new_from_pixbuf(Some(&icon)));

        if interactable {
            let settings_button = gtk::ButtonBuilder::new()
                .child(&icon)
                .name("message_settings")
                .valign(gtk::Align::Start)
                .build();

            settings_button.get_accessible().unwrap().set_name("Message menu");

            settings_button.connect_clicked(
                client.connector()
                    .do_sync(move |client, button: gtk::Button| {
                        button.get_style_context().add_class("active");
                        let menu = Self::build_menu(client, id);
                        menu.set_relative_to(Some(&button));
                        menu.show();

                        let button = button.clone();
                        menu.connect_hide(move |popover| {
                            // weird gtk behavior: if we don't do this, it messes with dialog rendering order
                            popover.set_relative_to::<gtk::Widget>(None);
                            button.get_style_context().remove_class("active");
                        });
                    })
                    .build_cloned_consumer()
            );

            settings_vbox.add(&settings_button);
        }

        hbox.add(&text);
        hbox.add(&settings_vbox);
        vbox.add(&hbox);

        MessageEntryWidget { widget: vbox, text }
    }

    fn build_menu(client: Client, msg: MessageId) -> gtk::Popover {
        lazy_static! {
            static ref GLADE: Glade = Glade::open("active/message_menu.glade").unwrap();
        }
        thread_local! {
            static ICON: gdk_pixbuf::Pixbuf = gdk_pixbuf::Pixbuf::new_from_file_at_size(
                &resource("feather/flag.svg"),
                18,
                18,
            ).expect("Error loading flag.svg!");
        }

        let builder: gtk::Builder = GLADE.builder();
        let menu: gtk::Popover = builder.get_object("message_menu").unwrap();
        let report_button: gtk::Button = builder.get_object("report_button").unwrap();
        let img: gtk::Image = builder.get_object("report_icon").unwrap();

        ICON.with(|icon| img.set_from_pixbuf(Some(&icon)));

        report_button.connect_clicked(
            (menu.clone(), client).connector()
                .do_sync(move |(menu, client), _| {
                    dialog::show_report_message(client, msg);
                    menu.hide();
                })
                .build_cloned_consumer()
        );

        menu
    }

    pub fn push_embed(&self, client: &Client, embed: MessageEmbed) {
        let embed = build_embed(client, embed);
        if let Some(embed) = embed {
            self.widget.add(&embed);
        }
    }

    pub fn set_status(&self, status: MessageStatus) {
        let style = self.text.get_style_context();
        style.remove_class("pending");
        style.remove_class("error");

        match status {
            MessageStatus::Pending => style.add_class("pending"),
            MessageStatus::Err => style.add_class("error"),
            _ => (),
        }
    }
}

fn build_embed(client: &Client, embed: MessageEmbed) -> Option<gtk::Widget> {
    match embed {
        MessageEmbed::OpenGraph(og) => Some(build_opengraph_embed(og)),
        MessageEmbed::Invite(invite) => Some(build_invite_embed(client, invite)),
        MessageEmbed::Error(error) => {
            log::debug!("Error loading embed: {:?}", error);
            None
        },
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

    let description: gtk::TextView = builder.get_object("description").unwrap();
    let description_buffer: gtk::TextBuffer = builder.get_object("description_buffer").unwrap();
    description_buffer.set_text(&embed.description);

    let image_box: gtk::Box = builder.get_object("image_box").unwrap();

    if embed.description == "" {
        opengraph.remove(&description);
    }

    if let Some(image_info) = embed.image {
        let image = gtk::Image::new_from_pixbuf(Some(&image_info.pixbuf));
        if let Some(alt) = image_info.alt {
            image.get_accessible().unwrap().set_description(&alt);
            image.set_tooltip_text(Some(&alt));
        }

        image_box.add(&image);
        opengraph.show_all();
    }




    opengraph.upcast()
}

fn build_invite_embed(client: &Client, embed: InviteEmbed) -> gtk::Widget {
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
                    if let Err(err) = client.join_community(code).await {
                        show_generic_error(&err);
                    }
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
