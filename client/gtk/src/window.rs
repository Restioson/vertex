use gtk::prelude::*;
use once_cell::unsync::OnceCell;

thread_local! {
    static WINDOW: OnceCell<Window> = OnceCell::new();
}

#[derive(Debug)]
struct Window {
    window: gtk::ApplicationWindow,
    overlay: gtk::Overlay,
}

pub(super) fn init(window: gtk::ApplicationWindow) {
    WINDOW.with(|cell| {
        let overlay = gtk::Overlay::new();
        window.add(&overlay);
        window.show();

        let window = Window { window, overlay };

        cell.set(window).expect("double window initialization");
    });
}

pub fn is_focused() -> bool {
    WINDOW.with(|window| {
        let window = window.get().expect("window not initialized on this thread");
        window.window.is_active()
    })
}

pub fn set_screen<W>(screen: &W)
    where W: glib::IsA<gtk::Widget>
{
    WINDOW.with(|window| {
        let window = window.get().expect("window not initialized on this thread");
        let overlay = &window.overlay;

        for child in overlay.get_children() {
            overlay.remove(&child);
        }
        overlay.add(screen);

        overlay.show_all();
    });
}

fn build_dialog_bg() -> gtk::Widget {
    gtk::EventBoxBuilder::new()
        .name("dialog_bg")
        .visible(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build()
        .upcast()
}

pub fn show_dialog<W: glib::IsA<gtk::Widget>>(widget: W) -> Dialog {
    WINDOW.with(|window| {
        let window = window.get().expect("window not initialized on this thread");

        let background = build_dialog_bg();

        let widget = widget.upcast();
        widget.get_style_context().add_class("dialog");

        window.overlay.add_overlay(&background);
        window.overlay.add_overlay(&widget);

        let dialog = Dialog {
            overlay: window.overlay.clone(),
            background,
            widget,
        };

        dialog.background.connect_button_release_event({
            let dialog = dialog.clone();
            move |_, _| {
                dialog.close();
                gtk::Inhibit(false)
            }
        });

        dialog
    })
}

#[derive(Clone)]
pub struct Dialog {
    overlay: gtk::Overlay,
    background: gtk::Widget,
    widget: gtk::Widget,
}

impl Dialog {
    pub fn close(&self) {
        self.overlay.remove(&self.background);
        self.overlay.remove(&self.widget);
    }
}
