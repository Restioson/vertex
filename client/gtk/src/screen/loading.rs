use futures::Future;
use gtk::prelude::*;

use lazy_static::lazy_static;

use crate::{screen, window};
use crate::connect::AsConnector;
use crate::Glade;

pub fn build() -> gtk::Viewport {
    lazy_static! {
        static ref GLADE: Glade = Glade::open("loading/loading.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    builder.get_object("viewport").unwrap()
}

pub fn build_error<F, Fut>(error: String, retry: F) -> gtk::Viewport
    where F: Fn() -> Fut + 'static,
          Fut: Future<Output = ()> + 'static
{
    lazy_static! {
        static ref GLADE: Glade = Glade::open("loading/error.glade").unwrap();
    }

    let builder: gtk::Builder = GLADE.builder();
    let viewport = builder.get_object("viewport").unwrap();

    let error_label: gtk::Label = builder.get_object("error_label").unwrap();
    error_label.set_text(&error);

    let login_button: gtk::Button = builder.get_object("login_button").unwrap();
    login_button.connect_clicked(
        ().connector()
            .do_async(|_, _| async move {
                let screen = screen::login::build().await;
                window::set_screen(&screen.main);
            })
            .build_cloned_consumer()
    );

    let retry_button: gtk::Button = builder.get_object("connect_button").unwrap();
    retry_button.connect_clicked(
        ().connector()
            .do_async(move |_, _| retry())
            .build_cloned_consumer()
    );

    viewport
}
