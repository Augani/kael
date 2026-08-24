use crate::{PlatformDispatcher, TaskLabel, Timer};
use async_task::Runnable;
use parking::{Parker, Unparker};
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use web_time::Instant;

/// Schedules Kael futures on the browser's single JavaScript event-loop thread.
/// Serializable CPU work can be moved to a dedicated Web Worker through
/// `BackgroundExecutor::spawn_worker_request`; opaque Rust futures cannot be
/// transferred between independently instantiated WebAssembly heaps.
pub(crate) struct WebDispatcher {
    parker: Arc<Mutex<Parker>>,
    unparker: Unparker,
}

impl WebDispatcher {
    pub(crate) fn new() -> Arc<Self> {
        let (parker, unparker) = parking::pair();
        Arc::new(Self {
            parker: Arc::new(Mutex::new(parker)),
            unparker,
        })
    }
}

impl PlatformDispatcher for WebDispatcher {
    fn is_main_thread(&self) -> bool {
        true
    }

    fn dispatch(&self, runnable: Runnable, _label: Option<TaskLabel>) {
        wasm_bindgen_futures::spawn_local(async move {
            runnable.run();
        });
    }

    fn dispatch_on_main_thread(&self, runnable: Runnable) {
        wasm_bindgen_futures::spawn_local(async move {
            runnable.run();
        });
    }

    fn dispatch_after(&self, duration: Duration, runnable: Runnable) {
        wasm_bindgen_futures::spawn_local(async move {
            Timer::after(duration).await;
            runnable.run();
        });
    }

    fn park(&self, _timeout: Option<Duration>) -> bool {
        // Blocking the JavaScript event loop would also prevent the future that
        // wakes us from running. Browser applications must remain asynchronous.
        false
    }

    fn unparker(&self) -> Unparker {
        let _ = &self.parker;
        self.unparker.clone()
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}
