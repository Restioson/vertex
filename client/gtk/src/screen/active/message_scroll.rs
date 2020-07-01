use std::cell::Cell;
use std::rc::Rc;

use crate::{Client, RoomId};
use gtk::prelude::*;
use crate::scheduler;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum Position {
    Top,
    Bottom,
}

pub struct ScrollWidget {
    upper: Rc<Cell<f64>>,
    value: Rc<Cell<f64>>, // FIXME: is it really used anywhere?
    balance: Rc<Cell<Option<Position>>>,
    autoscroll: Rc<Cell<bool>>,
    client: Client,
    /* whether a request for more messages has been send or not */
    request_sent: Rc<Cell<bool>>,
    widgets: Widgets,
}

pub struct Widgets {
    view: gtk::ScrolledWindow,
    listbox: gtk::ListBox,
}

impl Widgets {
    pub fn new(messages: gtk::ListBox, view: gtk::ScrolledWindow) -> Widgets {
        // TODO ?
        // if let Some(adj) = view.get_vadjustment() {
        //     if let Some(child) = view.get_child() {
        //         if let Some(container) = child.downcast_ref::<gtk::Container>() {
        //             container.set_focus_vadjustment(&adj);
        //         }
        //     }
        // }
        //
        Widgets {
            view,
            listbox: messages,
        }
    }
}

impl ScrollWidget {
    pub fn new(
        room_id: RoomId,
        client: Client,
        messages: gtk::ListBox,
        view: gtk::ScrolledWindow,
    ) -> ScrollWidget {
        let widgets = Widgets::new(messages, view);
        let upper = widgets
            .view
            .get_vadjustment()
            .map(|adj| adj.get_upper())
            .unwrap_or_default();
        let value = widgets
            .view
            .get_vadjustment()
            .map(|adj| adj.get_value())
            .unwrap_or_default();

        let mut scroll = ScrollWidget {
            widgets,
            client,
            value: Rc::new(Cell::new(value)),
            upper: Rc::new(Cell::new(upper)),
            autoscroll: Rc::new(Cell::new(false)),
            request_sent: Rc::new(Cell::new(false)),
            balance: Rc::new(Cell::new(None)),
        };
        scroll.connect(room_id);
        scroll
    }

    /* Keep the same position if new messages are added */
    pub fn connect(&mut self, room_id: RoomId) -> Option<()> {
        let adj = self.widgets.view.get_vadjustment()?;
        let upper = Rc::downgrade(&self.upper);
        let balance = Rc::downgrade(&self.balance);
        let autoscroll = Rc::downgrade(&self.autoscroll);
        let view = self.widgets.view.downgrade();
        adj.connect_property_upper_notify(move |adj| {
            let check = || -> Option<()> {
                let view = view.upgrade()?;
                let upper = upper.upgrade()?;
                let balance = balance.upgrade()?;
                let autoscroll = autoscroll.upgrade()?;
                let new_upper = adj.get_upper();
                let diff = new_upper - upper.get();
                /* Don't do anything if upper didn't change */
                if diff != 0.0 {
                    upper.set(new_upper);
                    /* Stay at the end of the room history when autoscroll is on */
                    if autoscroll.get() {
                        adj.set_value(adj.get_upper() - adj.get_page_size());
                    } else if balance.take().map_or(false, |x| x == Position::Top) {
                        adj.set_value(adj.get_value() + diff);
                        view.set_kinetic_scrolling(true);
                    }
                }
                Some(())
            }();
            debug_assert!(
                check.is_some(),
                "Upper notify callback couldn't acquire a strong pointer"
            );
        });

        let autoscroll = Rc::downgrade(&self.autoscroll);
        let request_sent = Rc::downgrade(&self.request_sent);
        let client = self.client.clone();

        adj.connect_value_changed(move |adj| {
            let check = || -> Option<()> {
                let autoscroll = autoscroll.upgrade()?;

                let bottom = adj.get_upper() - adj.get_page_size();
                if (adj.get_value() - bottom).abs() < std::f64::EPSILON {
                    autoscroll.set(true);
                } else {
                    autoscroll.set(false);
                }
                Some(())
            }();
            debug_assert!(
                check.is_some(),
                "Value changed callback couldn't acquire a strong pointer"
            );

            let check = || -> Option<()> {
                let request_sent = request_sent.upgrade()?;
                if !request_sent.get() {
                    /* the page size twice to detect if the user gets close the edge */
                    if adj.get_value() < adj.get_page_size() * 2.0 {
                        // TODO
                        // gtk::PositionType::Top => {
                        let req_sent = request_sent.clone();
                        let client = client.clone();
                        scheduler::spawn(async move {
                            if let Some(chat) = client.chat().await {
                                chat.extend_older().await.expect("Error extending older!");
                                req_sent.set(false);
                            }
                        });
                        // },
                        // gtk::PositionType::Bottom => {
                        //     drop(state);
                        //     chat.extend_newer().await
                        // },

                        /* Load more messages once the user is nearly at the end of the history */
                        request_sent.set(true);
                    }
                }

                Some(())
            }();
            debug_assert!(check.is_some(), "Can't request more messages");
        });

        None
    }
}
