use gtk::prelude::*;

use std::rc::Rc;
use std::cell::{self, RefCell};
use std::future::Future;
use std::pin::Pin;

use vertex_client_backend as vertex;

pub mod active;
pub mod login;
pub mod register;

mod connect;

// TODO: Ideally don't have to use an enum and can rather use a Box with dyn type
pub enum DynamicScreen {
    Active(Screen<active::Model>),
    Login(Screen<login::Model>),
    Register(Screen<register::Model>),
}

impl DynamicScreen {
    #[inline]
    pub fn viewport(&self) -> &gtk::Viewport {
        match self {
            DynamicScreen::Active(screen) => screen.viewport(),
            DynamicScreen::Login(screen) => screen.viewport(),
            DynamicScreen::Register(screen) => screen.viewport(),
        }
    }
}

pub struct Screen<M> {
    viewport: gtk::Viewport,
    model: Rc<RefCell<M>>,
}

impl<M> Clone for Screen<M> {
    fn clone(&self) -> Self {
        Screen {
            viewport: self.viewport.clone(),
            model: self.model.clone(),
        }
    }
}

impl<M> Screen<M> {
    pub fn new(viewport: gtk::Viewport, model: M) -> Screen<M> {
        Screen {
            viewport,
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
    pub fn viewport(&self) -> &gtk::Viewport { &self.viewport }
}
