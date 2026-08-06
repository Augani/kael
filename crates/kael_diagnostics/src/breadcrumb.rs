//! Breadcrumb storage and severity levels.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

const MAX_BREADCRUMBS: usize = 512;
const MAX_CATEGORY_BYTES: usize = 256;
const MAX_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_DATA_ENTRIES: usize = 16;
const MAX_DATA_KEY_BYTES: usize = 256;
const MAX_DATA_VALUE_BYTES: usize = 512;

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
    max_breadcrumbs: usize,
    items: Arc<Mutex<VecDeque<Breadcrumb>>>,
}

impl BreadcrumbBuffer {
    /// Creates a new breadcrumb buffer.
    ///
    /// Retention is capped at 512 entries so generated or untrusted
    /// configuration cannot grow the process indefinitely.
    pub fn new(max_breadcrumbs: usize) -> Self {
        Self {
            max_breadcrumbs: max_breadcrumbs.min(MAX_BREADCRUMBS),
            // Do not eagerly reserve caller-controlled capacity. A very large
            // retention limit should not be able to OOM at setup.
            items: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Appends a bounded breadcrumb and evicts the oldest one when full.
    ///
    /// Oversized text and metadata are truncated before they are retained.
    pub fn push(&self, mut breadcrumb: Breadcrumb) {
        if self.max_breadcrumbs == 0 {
            return;
        }

        breadcrumb.category = truncate_text(breadcrumb.category, MAX_CATEGORY_BYTES);
        breadcrumb.message = truncate_text(breadcrumb.message, MAX_MESSAGE_BYTES);
        breadcrumb.data = breadcrumb
            .data
            .into_iter()
            .take(MAX_DATA_ENTRIES)
            .map(|(key, value)| {
                (
                    truncate_text(key, MAX_DATA_KEY_BYTES),
                    truncate_text(value, MAX_DATA_VALUE_BYTES),
                )
            })
            .collect();

        let mut items = self
            .items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if items.len() >= self.max_breadcrumbs {
            items.pop_front();
        }
        items.push_back(breadcrumb);
    }

    /// Returns a snapshot of the buffered breadcrumbs.
    pub fn snapshot(&self) -> Vec<Breadcrumb> {
        let items = self
            .items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        items.iter().cloned().collect()
    }

    /// Removes all breadcrumbs from the buffer.
    pub fn clear(&self) {
        let mut items = self
            .items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        items.clear();
    }
}

fn truncate_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::SystemTime};

    use super::{
        Breadcrumb, BreadcrumbBuffer, Level, MAX_BREADCRUMBS, MAX_CATEGORY_BYTES, MAX_DATA_ENTRIES,
        MAX_DATA_KEY_BYTES, MAX_DATA_VALUE_BYTES, MAX_MESSAGE_BYTES,
    };

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

    #[test]
    fn bounds_retention_and_payloads_without_splitting_utf8() {
        let buffer = BreadcrumbBuffer::new(usize::MAX);
        let data = (0..MAX_DATA_ENTRIES + 4)
            .map(|index| {
                (
                    format!("{index:04}-{}", "é".repeat(MAX_DATA_KEY_BYTES)),
                    "é".repeat(MAX_DATA_VALUE_BYTES),
                )
            })
            .collect();
        let breadcrumb = Breadcrumb {
            category: "é".repeat(MAX_CATEGORY_BYTES),
            message: "é".repeat(MAX_MESSAGE_BYTES),
            level: Level::Warning,
            timestamp: SystemTime::UNIX_EPOCH,
            data,
        };

        for _ in 0..MAX_BREADCRUMBS + 1 {
            buffer.push(breadcrumb.clone());
        }

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), MAX_BREADCRUMBS);
        let retained = &snapshot[0];
        assert!(retained.category.len() <= MAX_CATEGORY_BYTES);
        assert!(retained.message.len() <= MAX_MESSAGE_BYTES);
        assert_eq!(retained.data.len(), MAX_DATA_ENTRIES);
        assert!(
            retained
                .data
                .keys()
                .all(|key| key.len() <= MAX_DATA_KEY_BYTES)
        );
        assert!(
            retained
                .data
                .values()
                .all(|value| value.len() <= MAX_DATA_VALUE_BYTES)
        );
    }
}
