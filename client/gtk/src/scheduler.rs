use std::future::Future;

#[inline]
pub fn spawn<F: Future<Output = ()> + 'static>(f: F) {
    glib::MainContext::ref_thread_default().spawn_local(f);
}
