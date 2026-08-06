//! Listener subscription lifetime management.

/// A listener subscription that unregisters its callback when dropped.
#[must_use = "dropping the subscription immediately unregisters the listener"]
pub struct Subscription {
    unsubscribe: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl Subscription {
    pub(crate) fn new(unsubscribe: impl FnOnce() + Send + 'static) -> Self {
        Self {
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }

    /// Keeps the listener registered after this handle is dropped.
    pub fn detach(mut self) {
        self.unsubscribe.take();
    }

    /// Unregisters the listener immediately.
    pub fn unsubscribe(mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Subscription").finish()
    }
}
