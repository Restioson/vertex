use super::*;

type SyncFn<Model, Args> = Box<dyn Fn(Screen<Model>, Args)>;
type AsyncFn<Model, Args> = Box<dyn Fn(Screen<Model>, Args) -> Pin<Box<dyn Future<Output = ()>>>>;

pub struct Connector<Model, Args: Clone> {
    screen: Screen<Model>,
    do_async: Vec<AsyncFn<Model, Args>>,
    do_sync: Vec<SyncFn<Model, Args>>,
    inhibit: bool,
}

impl<Model, Args: Clone> Connector<Model, Args> {
    pub fn new(screen: Screen<Model>) -> Self {
        Connector {
            screen,
            do_async: Vec::new(),
            do_sync: Vec::new(),
            inhibit: false,
        }
    }

    #[inline]
    pub fn do_async<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(Screen<Model>, Args) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        self.do_async.push(Box::new(move |screen, args| {
            let future = f(screen, args);
            Box::pin(future)
        }));
        self
    }

    #[inline]
    pub fn do_sync<F>(mut self, f: F) -> Self
    where
        F: Fn(Screen<Model>, Args) + 'static,
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
            do_sync(self.screen.clone(), args.clone());
        }
    }

    #[inline]
    fn execute_async(&self, args: &Args) {
        if self.do_async.is_empty() {
            return;
        }

        let context = glib::MainContext::ref_thread_default();
        for do_async in &self.do_async {
            let future = do_async(self.screen.clone(), args.clone());
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
        move |args| {
            self.execute(args);
        }
    }
}

impl<Model, Arg: Clone> Connector<Model, Arg> {
    #[inline]
    pub fn build_cloned_consumer(self) -> impl Fn(&Arg) {
        move |arg| {
            self.execute(arg.clone());
        }
    }
}

impl<Model, Widget: Clone, Event: Clone> Connector<Model, (Widget, Event)> {
    #[inline]
    pub fn build_widget_event(self) -> impl Fn(&Widget, &Event) -> gtk::Inhibit {
        move |widget, event| self.execute((widget.clone(), event.clone()))
    }
}

impl<Model, Widget: Clone, Opt: Clone> Connector<Model, (Widget, Option<Opt>)> {
    #[inline]
    pub fn build_widget_and_option_consumer(self) -> impl Fn(&Widget, Option<&Opt>) {
        move |widget, opt| {
            self.execute((widget.clone(), opt.cloned()));
        }
    }
}
