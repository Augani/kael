//! Subscription handle used by observable storage APIs.

/// A handle that unregisters a callback when it is dropped.
#[must_use]
pub struct Subscription {
    unsubscribe: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl Subscription {
    /// Creates a new subscription.
    pub fn new(unsubscribe: impl FnOnce() + Send + 'static) -> Self {
        Self {
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }

    /// Detaches the callback from this handle without unsubscribing it.
    pub fn detach(mut self) {
        self.unsubscribe.take();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").finish()
    }
}