use std::pin::Pin;

use futures::Future;

type AsyncFn<Shared, Args> = dyn Fn(Shared, Args) -> Pin<Box<dyn Future<Output = ()>>>;

pub trait AsConnector: Clone {
    fn connector<Args: Clone>(&self) -> Connector<Self, Args>;
}

impl<T: Clone> AsConnector for T {
    fn connector<Args: Clone>(&self) -> Connector<Self, Args> {
        Connector::new(self.clone())
    }
}

pub struct Connector<Shared: Clone, Args: Clone> {
    shared: Shared,
    do_async: Vec<Box<AsyncFn<Shared, Args>>>,
    do_sync: Vec<Box<dyn Fn(Shared, Args)>>,
    inhibit: bool,
}

impl<Shared: Clone, Args: Clone> Connector<Shared, Args> {
    pub fn new(shared: Shared) -> Self {
        Connector {
            shared,
            do_async: Vec::new(),
            do_sync: Vec::new(),
            inhibit: false,
        }
    }

    #[inline]
    pub fn do_async<F, Fut>(mut self, f: F) -> Self
        where F: Fn(Shared, Args) -> Fut + 'static,
              Fut: Future<Output = ()> + 'static,
    {
        self.do_async.push(Box::new(move |shared, args| {
            let future = f(shared, args);
            Box::pin(future)
        }));
        self
    }

    #[inline]
    pub fn do_sync<F>(mut self, f: F) -> Self
        where F: Fn(Shared, Args) + 'static
    {
        self.do_sync.push(Box::new(f));
        self
    }

    #[inline]
    pub fn inhibit(mut self, inhibit: bool) -> Self {
        self.inhibit = inhibit;
        self
    }

    #[inline]
    fn execute_sync(&self, args: &Args) {
        for do_sync in &self.do_sync {
            do_sync(self.shared.clone(), args.clone());
        }
    }

    #[inline]
    fn execute_async(&self, args: &Args) {
        if self.do_async.is_empty() { return; }

        let context = glib::MainContext::ref_thread_default();
        for do_async in &self.do_async {
            let future = do_async(self.shared.clone(), args.clone());
            context.spawn_local(future);
        }
    }

    #[inline]
    fn execute(&self, args: Args) -> gtk::Inhibit {
        self.execute_sync(&args);
        self.execute_async(&args);
        gtk::Inhibit(self.inhibit)
    }

    #[inline]
    pub fn build(self) -> impl Fn(Args) {
        move |args| { self.execute(args); }
    }
}

impl<Shared: Clone, Arg: Clone> Connector<Shared, Arg> {
    #[inline]
    pub fn build_cloned_consumer(self) -> impl Fn(&Arg) {
        move |arg| { self.execute(arg.clone()); }
    }
}

impl<Shared: Clone, Widget: Clone, Event: Clone> Connector<Shared, (Widget, Event)> {
    #[inline]
    pub fn build_widget_event(self) -> impl Fn(&Widget, &Event) -> gtk::Inhibit {
        move |widget, event| {
            self.execute((widget.clone(), event.clone()))
        }
    }
}

impl<Shared: Clone, Widget: Clone, Listener: Clone> Connector<Shared, (Widget, Listener)> {
    #[inline]
    pub fn build_widget_listener(self) -> impl Fn(&Widget, &Listener) {
        move |widget, event| {
            self.execute((widget.clone(), event.clone()));
        }
    }
}

impl<Shared: Clone, Widget: Clone, Opt: Clone> Connector<Shared, (Widget, Option<Opt>)> {
    #[inline]
    pub fn build_widget_and_option_consumer(self) -> impl Fn(&Widget, Option<&Opt>) {
        move |widget, opt| {
            self.execute((widget.clone(), opt.cloned()));
        }
    }
}
