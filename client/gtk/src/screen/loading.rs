use futures::Future;
use gtk::prelude::*;

use crate::{screen, window};
use crate::connect::AsConnector;

pub fn build() -> gtk::Viewport {
    let builder = gtk::Builder::new_from_file("res/glade/loading/loading.glade");
    builder.get_object("viewport").unwrap()
}

pub fn build_error<F, Fut>(error: String, retry: F) -> gtk::Viewport
    where F: Fn() -> Fut + 'static,
          Fut: Future<Output = ()> + 'static
{
    let builder = gtk::Builder::new_from_file("res/glade/loading/error.glade");
    let viewport = builder.get_object("viewport").unwrap();

    let error_label: gtk::Label = builder.get_object("error_label").unwrap();
    error_label.set_text(&error);

    let login_button: gtk::Button = builder.get_object("login_button").unwrap();
    login_button.connect_button_release_event(
        ().connector()
            .do_async(|_, (_, _)| async move {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
            })
            .build_widget_event()
    );

    let retry_button: gtk::Button = builder.get_object("connect_button").unwrap();
    retry_button.connect_button_release_event(
        ().connector()
            .do_async(move |_, (_, _)| retry())
            .build_widget_event()
    );

    viewport
}
