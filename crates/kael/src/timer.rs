use std::time::Duration;

/// A portable one-shot timer for asynchronous UI work.
pub struct Timer;

impl Timer {
    /// Resolve after at least `duration` has elapsed on the browser event loop.
    pub fn after(duration: Duration) -> WebTimer {
        WebTimer::new(duration)
    }
}

use futures::channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use wasm_bindgen::{JsCast as _, closure::Closure};

/// Future returned by [`Timer::after`] in browser builds.
pub struct WebTimer {
    receiver: oneshot::Receiver<()>,
    window: web_sys::Window,
    timeout_handle: i32,
    _callback: Closure<dyn FnMut()>,
}

impl WebTimer {
    fn new(duration: Duration) -> Self {
        let window = web_sys::window().expect("Timer requires a browser Window");
        let (sender, receiver) = oneshot::channel();
        let mut sender = Some(sender);
        let callback = Closure::wrap(Box::new(move || {
            if let Some(sender) = sender.take() {
                let _ = sender.send(());
            }
        }) as Box<dyn FnMut()>);
        let delay = duration.as_millis().min(i32::MAX as u128) as i32;
        let timeout_handle = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                delay,
            )
            .expect("failed to schedule browser timer");
        Self {
            receiver,
            window,
            timeout_handle,
            _callback: callback,
        }
    }
}

impl Future for WebTimer {
    type Output = web_time::Instant;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver)
            .poll(cx)
            .map(|_| web_time::Instant::now())
    }
}

impl Drop for WebTimer {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.timeout_handle);
    }
}
