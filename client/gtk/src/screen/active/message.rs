use gtk::prelude::*;

use vertex::*;

use crate::client::{self, InviteEmbed, MessageEmbed, MessageStatus, OpenGraphEmbed};

use super::*;

pub struct MessageListWidget {
    pub scroll: gtk::ScrolledWindow,
    pub list: gtk::ListBox,
    pub last_group: Option<GroupedMessageWidget>,
}

impl MessageListWidget {
    fn next_group(&mut self, author: UserId, profile: UserProfile) -> &GroupedMessageWidget {
        match &self.last_group {
            Some(group) if group.author == author => {}
            _ => {
                let group = GroupedMessageWidget::build(author, profile);
                self.list.insert(&group.widget, -1);
                self.last_group = Some(group);
            }
        }

        self.last_group.as_ref().unwrap()
    }
}

impl client::MessageListWidget<Ui> for MessageListWidget {
    fn clear(&mut self) {
        for child in self.list.get_children() {
            self.list.remove(&child);
        }
        self.last_group = None;
    }

    fn push_message(&mut self, author: UserId, author_profile: UserProfile, content: String) -> MessageEntryWidget {
        let group = self.next_group(author, author_profile);
        let widget = group.push_message(content);

        widget
    }

    fn bind_events(&self, list: &client::MessageList<Ui>) {
        let adjustment = self.scroll.get_vadjustment().unwrap();

        adjustment.connect_value_changed(
            list.connector()
                .do_async(|list, adjustment: gtk::Adjustment| async move {
                    let upper = adjustment.get_upper() - adjustment.get_page_size();
                    let reading_new = adjustment.get_value() + 10.0 >= upper;
                    list.set_reading_new(reading_new).await;
                })
                .build_cloned_consumer()
        );

        self.list.connect_size_allocate(
            (list.clone(), adjustment).connector()
                .do_async(|(list, adjustment), (_, _)| async move {
                    if list.reading_new().await {
                        adjustment.set_value(adjustment.get_upper() - adjustment.get_page_size());
                    }
                })
                .build_widget_listener()
        );
    }
}

pub struct GroupedMessageWidget {
    author: UserId,
    widget: gtk::Box,
    entry_list: gtk::ListBox,
}

impl GroupedMessageWidget {
    fn build(author: UserId, profile: UserProfile) -> GroupedMessageWidget {
        let builder = gtk::Builder::new_from_file("res/glade/active/message_entry.glade");

        let widget: gtk::Box = builder.get_object("message_group").unwrap();
        let entry_list: gtk::ListBox = builder.get_object("entry_list").unwrap();

        let author_name: gtk::Label = builder.get_object("author_name").unwrap();
        author_name.set_text(&profile.display_name);
        author_name.set_can_focus(false);

        widget.show_all();

        GroupedMessageWidget { author, widget, entry_list }
    }

    fn push_message(&self, content: String) -> MessageEntryWidget {
        let entry = MessageEntryWidget::build(content);
        self.entry_list.add(&entry.widget);
        self.entry_list.show_all();

        entry
    }
}

#[derive(Clone)]
pub struct MessageEntryWidget {
    widget: gtk::Box,
    text: gtk::Label,
}

impl MessageEntryWidget {
    pub fn build(text: String) -> MessageEntryWidget {
        let widget = gtk::BoxBuilder::new()
            .name("message")
            .orientation(gtk::Orientation::Vertical)
            .build();

        let text = gtk::LabelBuilder::new()
            .name("message_text")
            .label(text.trim())
            .halign(gtk::Align::Start)
            .selectable(true)
            .can_focus(false)
            .build();

        widget.add(&text);

        MessageEntryWidget { widget, text }
    }
}

impl client::MessageEntryWidget<Ui> for MessageEntryWidget {
    fn set_status(&mut self, status: client::MessageStatus) {
        let style = self.text.get_style_context();
        style.remove_class("pending");
        style.remove_class("error");

        match status {
            MessageStatus::Pending => style.add_class("pending"),
            MessageStatus::Err => style.add_class("error"),
            _ => (),
        }
    }

    fn push_embed(&mut self, client: &Client<Ui>, embed: MessageEmbed) {
        let embed = build_embed(client, embed);
        self.widget.add(&embed);
    }
}

// TODO: cache glade source in memory so it doesn't have to be reloaded every time
fn build_embed(client: &Client<Ui>, embed: MessageEmbed) -> gtk::Widget {
    match embed {
        MessageEmbed::OpenGraph(og) => build_opengraph_embed(og),
        MessageEmbed::Invite(invite) => build_invite_embed(client, invite),
        // TODO: Own embed for errors
        MessageEmbed::Error(error) => build_opengraph_embed(OpenGraphEmbed {
            url: error.url,
            title: error.title,
            description: error.error,
        }),
    }
}

fn build_opengraph_embed(embed: OpenGraphEmbed) -> gtk::Widget {
    let builder = gtk::Builder::new_from_file("res/glade/active/embed/opengraph.glade");
    let opengraph: gtk::Box = builder.get_object("opengraph").unwrap();

    let title_label: gtk::Label = builder.get_object("title").unwrap();
    title_label.set_text(&embed.title);

    let description_label: gtk::Label = builder.get_object("description").unwrap();
    description_label.set_text(&embed.description);

    opengraph.upcast()
}

fn build_invite_embed(client: &Client<Ui>, embed: InviteEmbed) -> gtk::Widget {
    let builder = gtk::Builder::new_from_file("res/glade/active/embed/invite.glade");
    let invite: gtk::Box = builder.get_object("invite").unwrap();

    let name_label: gtk::Label = builder.get_object("community_name").unwrap();
    name_label.set_text(&embed.name);

    let motd_label: gtk::Label = builder.get_object("community_motd").unwrap();
    motd_label.set_text("5 members");

    let join_button: gtk::Button = builder.get_object("join_button").unwrap();
    join_button.connect_button_press_event(
        client.connector()
            .do_async(move |client, (_, _)| {
                let code = embed.code.clone();
                async move {
                    // TODO: report error
                    let _ = client.join_community(code).await;
                }
            })
            .build_widget_event()
    );

    invite.upcast()
}
