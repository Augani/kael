//! Breadcrumb storage and severity levels.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

/// The severity associated with a breadcrumb or error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Verbose trace information.
    Trace,
    /// Debug-level information.
    Debug,
    /// Informational events.
    Info,
    /// Warning-level events.
    Warning,
    /// Error-level events.
    Error,
    /// Fatal events.
    Fatal,
}

/// A single breadcrumb event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breadcrumb {
    /// The subsystem that emitted the breadcrumb.
    pub category: String,
    /// The human-readable breadcrumb message.
    pub message: String,
    /// The severity for the breadcrumb.
    pub level: Level,
    /// When the breadcrumb was recorded.
    pub timestamp: SystemTime,
    /// Additional structured metadata.
    pub data: HashMap<String, String>,
}

/// An in-memory ring buffer for breadcrumbs.
#[derive(Debug, Clone)]
pub struct BreadcrumbBuffer {
    inner: Arc<Mutex<BreadcrumbBufferState>>,
}

#[derive(Debug)]
struct BreadcrumbBufferState {
    max_breadcrumbs: usize,
    items: VecDeque<Breadcrumb>,
}

impl BreadcrumbBuffer {
    /// Creates a new breadcrumb buffer.
    pub fn new(max_breadcrumbs: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BreadcrumbBufferState {
                max_breadcrumbs,
                // Do not eagerly reserve caller-controlled capacity. A very
                // large retention limit should not be able to OOM at setup.
                items: VecDeque::new(),
            })),
        }
    }

    /// Appends a breadcrumb and evicts the oldest one when the buffer is full.
    pub fn push(&self, breadcrumb: Breadcrumb) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if inner.max_breadcrumbs == 0 {
            return;
        }
        if inner.items.len() >= inner.max_breadcrumbs {
            inner.items.pop_front();
        }

        inner.items.push_back(breadcrumb);
    }

    /// Returns a snapshot of the buffered breadcrumbs.
    pub fn snapshot(&self) -> Vec<Breadcrumb> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.items.iter().cloned().collect()
    }

    /// Removes all breadcrumbs from the buffer.
    pub fn clear(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::SystemTime};

    use super::{Breadcrumb, BreadcrumbBuffer, Level};

    #[test]
    fn evicts_oldest_breadcrumbs_on_overflow() {
        let buffer = BreadcrumbBuffer::new(2);
        buffer.push(Breadcrumb {
            category: "test".to_string(),
            message: "one".to_string(),
            level: Level::Info,
            timestamp: SystemTime::UNIX_EPOCH,
            data: HashMap::new(),
        });
        buffer.push(Breadcrumb {
            category: "test".to_string(),
            message: "two".to_string(),
            level: Level::Info,
            timestamp: SystemTime::UNIX_EPOCH,
            data: HashMap::new(),
        });
        buffer.push(Breadcrumb {
            category: "test".to_string(),
            message: "three".to_string(),
            level: Level::Info,
            timestamp: SystemTime::UNIX_EPOCH,
            data: HashMap::new(),
        });

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].message, "two");
        assert_eq!(snapshot[1].message, "three");
    }

    #[test]
    fn zero_capacity_discards_breadcrumbs() {
        let buffer = BreadcrumbBuffer::new(0);
        buffer.push(Breadcrumb {
            category: "test".to_string(),
            message: "discarded".to_string(),
            level: Level::Info,
            timestamp: SystemTime::UNIX_EPOCH,
            data: HashMap::new(),
        });
        assert!(buffer.snapshot().is_empty());
    }
}
