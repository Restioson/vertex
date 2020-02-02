use std::cell::{self, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use gtk::prelude::*;

pub mod active;
pub mod login;
pub mod register;
pub mod settings;
pub mod loading;

mod connect;

pub fn dialog_bg() -> gtk::Widget {
    gtk::EventBoxBuilder::new()
        .name("dialog_bg")
        .visible(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build()
        .upcast()
}

pub fn show_dialog<W: glib::IsA<gtk::Widget>>(overlay: &gtk::Overlay, dialog: W) -> Dialog {
    let background = dialog_bg();

    let dialog = dialog.upcast();
    dialog.get_style_context().add_class("dialog");

    overlay.add_overlay(&background);
    overlay.add_overlay(&dialog);

    let dialog = Dialog {
        overlay: overlay.clone(),
        background: background.clone(),
        dialog: dialog.clone(),
    };

    background.connect_button_release_event({
        let dialog = dialog.clone();
        move |_, _| {
            dialog.close();
            gtk::Inhibit(false)
        }
    });

    dialog
}

#[derive(Clone)]
pub struct Dialog {
    overlay: gtk::Overlay,
    background: gtk::Widget,
    dialog: gtk::Widget,
}

impl Dialog {
    pub fn close(&self) {
        self.overlay.remove(&self.background);
        self.overlay.remove(&self.dialog);
    }
}

pub struct Screen<M> {
    widget: gtk::Widget,
    model: Rc<RefCell<M>>,
}

impl<M> Clone for Screen<M> {
    fn clone(&self) -> Self {
        Screen {
            widget: self.widget.clone(),
            model: self.model.clone(),
        }
    }
}

impl<M> Screen<M> {
    pub fn new<W: glib::IsA<gtk::Widget>>(widget: W, model: M) -> Screen<M> {
        Screen {
            widget: widget.upcast(),
            model: Rc::new(RefCell::new(model)),
        }
    }

    #[inline]
    pub fn connector<Args: Clone>(&self) -> connect::Connector<M, Args> {
        connect::Connector::new(self.clone())
    }

    #[inline]
    pub fn model(&self) -> cell::Ref<M> { self.model.borrow() }

    #[inline]
    pub fn model_mut(&self) -> cell::RefMut<M> { self.model.borrow_mut() }

    #[inline]
    pub fn widget(&self) -> &gtk::Widget { &self.widget }
}

pub trait TryGetText {
    fn try_get_text(&self) -> Result<String, ()>;
}

impl<E: gtk::EntryExt> TryGetText for E {
    fn try_get_text(&self) -> Result<String, ()> {
        self.get_text().map(|s| s.as_str().to_owned()).ok_or(())
    }
}
