use gtk::prelude::*;
use once_cell::unsync::OnceCell;
use crate::connect::AsConnector;
use crate::config;

thread_local! {
    pub static WINDOW: OnceCell<Window> = OnceCell::new();
}

#[derive(Debug)]
pub struct Window {
    pub window: gtk::ApplicationWindow,
    pub overlay: gtk::Overlay,
}

pub(super) fn init(window: gtk::ApplicationWindow) {
    WINDOW.with(|cell| {
        let overlay = gtk::Overlay::new();
        window.add(&overlay);
        window.show();

        window.connect_size_allocate(|window, _alloc| {
            let config = config::get();
            if !config.maximized && !config.full_screen {
                config::modify(|conf| {
                    conf.resolution = window.get_size();
                })
            }
        });

        window.connect_window_state_event(|_window, state| {
            config::modify(|conf| {
                let state = state.get_new_window_state();
                conf.maximized = state.contains(gdk::WindowState::MAXIMIZED);
                conf.full_screen = state.contains(gdk::WindowState::FULLSCREEN);
            });
            Inhibit(false)
        });

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

pub fn build_dialog_bg() -> gtk::Widget {
    gtk::EventBoxBuilder::new()
        .name("dialog_bg")
        .visible(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build()
        .upcast()
}

pub fn show_dialog<F: FnOnce(&Window) -> gtk::Dialog>(f: F) -> gtk::Dialog {
    WINDOW.with(|window| {
        let window = window.get().expect("window not initialized on this thread");

        let background = build_dialog_bg();
        window.overlay.add_overlay(&background);

        let dialog = f(window);

        dialog.get_content_area().get_style_context().add_class("dialog");

        dialog.set_decorated(false);
        dialog.connect_close(
            (window.overlay.clone(), background).connector()
                .do_sync(|(overlay, bg), dialog: gtk::Dialog| {
                    overlay.remove(&bg);
                    dialog.destroy();
                })
                .build_cloned_consumer()
        );

        dialog.show_all();
        dialog
    })
}

