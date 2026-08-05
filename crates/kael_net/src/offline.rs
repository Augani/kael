use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::client::ApiRequest;

/// A request queued for later execution when connectivity is restored.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueuedRequest {
    /// Unique identifier for this queued request.
    pub id: String,
    /// The original API request to replay.
    pub request: ApiRequest,
    /// Unix timestamp (ms) when the request was originally created.
    pub created_at: u64,
    /// Number of send attempts so far.
    pub attempts: u32,
    /// Priority level (lower number = higher priority).
    pub priority: u32,
}

/// Result of attempting to add a request to an [`OfflineQueue`].
#[derive(Debug)]
#[must_use = "handle whether the offline request was queued or rejected"]
pub enum EnqueueOutcome {
    /// The request was queued with its assigned identifier.
    Queued {
        /// Identifier assigned to the newly queued request.
        id: String,
        /// Lower-priority request removed to make room, if any.
        evicted: Option<QueuedRequest>,
    },
    /// The queue rejected the request because no slot was available.
    Rejected {
        /// The original request, returned so the caller can handle or persist it.
        request: ApiRequest,
    },
}

impl EnqueueOutcome {
    /// Returns the assigned request identifier when the request was queued.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Queued { id, .. } => Some(id),
            Self::Rejected { .. } => None,
        }
    }

    /// Returns the request evicted to make room, if one was displaced.
    pub fn evicted(&self) -> Option<&QueuedRequest> {
        match self {
            Self::Queued { evicted, .. } => evicted.as_ref(),
            Self::Rejected { .. } => None,
        }
    }

    /// Returns true when the incoming request was accepted.
    pub fn is_queued(&self) -> bool {
        matches!(self, Self::Queued { .. })
    }
}

/// A bounded, priority-aware offline request queue.
#[derive(Debug)]
pub struct OfflineQueue {
    queue: VecDeque<QueuedRequest>,
    max_size: usize,
    next_id: u64,
}

impl OfflineQueue {
    /// Create a new offline queue with the given maximum capacity.
    pub fn new(max_size: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            max_size,
            next_id: 1,
        }
    }

    /// Enqueue a request with a given priority.
    ///
    /// If the queue is at capacity and the new request has higher priority, the
    /// lowest-priority existing request is returned as evicted. Otherwise, the
    /// incoming request is returned as rejected rather than silently dropped.
    ///
    /// ```
    /// use kael_net::{ApiRequest, EnqueueOutcome, OfflineQueue};
    ///
    /// let mut queue = OfflineQueue::new(32);
    /// match queue.enqueue(ApiRequest::get("/sync"), 1) {
    ///     EnqueueOutcome::Queued { id, evicted } => {
    ///         assert!(!id.is_empty());
    ///         assert!(evicted.is_none());
    ///     }
    ///     EnqueueOutcome::Rejected { request } => {
    ///         // Persist or execute the returned request another way.
    ///         assert_eq!(request.path, "/sync");
    ///     }
    /// }
    /// ```
    pub fn enqueue(&mut self, request: ApiRequest, priority: u32) -> EnqueueOutcome {
        if self.max_size == 0 {
            return EnqueueOutcome::Rejected { request };
        }

        let mut evicted = None;
        if self.queue.len() >= self.max_size {
            let worst_idx = self
                .queue
                .iter()
                .enumerate()
                .max_by_key(|(_, item)| item.priority)
                .map(|(idx, _)| idx);

            if let Some(idx) = worst_idx {
                if self.queue[idx].priority > priority {
                    evicted = self.queue.remove(idx);
                } else {
                    return EnqueueOutcome::Rejected { request };
                }
            }
        }

        let id = self.allocate_id();
        let queued = QueuedRequest {
            id: id.clone(),
            request,
            created_at: now_unix_millis(),
            attempts: 0,
            priority,
        };

        let insert_pos = self
            .queue
            .iter()
            .position(|item| item.priority > priority)
            .unwrap_or(self.queue.len());

        self.queue.insert(insert_pos, queued);
        EnqueueOutcome::Queued { id, evicted }
    }

    fn allocate_id(&mut self) -> String {
        loop {
            let id = format!("req_{}", self.next_id);
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if self.queue.iter().all(|queued| queued.id != id) {
                return id;
            }
        }
    }

    /// Remove and return the highest-priority request from the front of the queue.
    pub fn dequeue(&mut self) -> Option<QueuedRequest> {
        self.queue.pop_front()
    }

    /// Peek at the highest-priority request without removing it.
    pub fn peek(&self) -> Option<&QueuedRequest> {
        self.queue.front()
    }

    /// Remove a specific request by id, returning true if found.
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.queue.len();
        self.queue.retain(|item| item.id != id);
        self.queue.len() < len_before
    }

    /// Return the number of queued requests.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Return true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Remove all queued requests.
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

fn now_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ApiRequest;

    fn make_request(path: &str) -> ApiRequest {
        ApiRequest::get(path)
    }

    fn require_queued(outcome: EnqueueOutcome) -> (String, Option<QueuedRequest>) {
        match outcome {
            EnqueueOutcome::Queued { id, evicted } => (id, evicted),
            EnqueueOutcome::Rejected { request } => {
                panic!("request was unexpectedly rejected: {}", request.path)
            }
        }
    }

    #[test]
    fn test_new_queue() {
        let queue = OfflineQueue::new(10);
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_enqueue_dequeue() {
        let mut queue = OfflineQueue::new(10);
        let outcome = queue.enqueue(make_request("/a"), 1);
        assert!(outcome.is_queued());
        assert!(outcome.evicted().is_none());
        let (id, _) = require_queued(outcome);
        assert!(!id.is_empty());
        assert_eq!(queue.len(), 1);

        let item = queue.dequeue().unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.request.path, "/a");
        assert!(item.created_at > 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_priority_ordering() {
        let mut queue = OfflineQueue::new(10);
        require_queued(queue.enqueue(make_request("/low"), 10));
        require_queued(queue.enqueue(make_request("/high"), 1));
        require_queued(queue.enqueue(make_request("/mid"), 5));

        assert_eq!(queue.dequeue().unwrap().request.path, "/high");
        assert_eq!(queue.dequeue().unwrap().request.path, "/mid");
        assert_eq!(queue.dequeue().unwrap().request.path, "/low");
    }

    #[test]
    fn test_peek() {
        let mut queue = OfflineQueue::new(10);
        assert!(queue.peek().is_none());

        require_queued(queue.enqueue(make_request("/a"), 1));
        let peeked = queue.peek().unwrap();
        assert_eq!(peeked.request.path, "/a");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut queue = OfflineQueue::new(10);
        let (id1, _) = require_queued(queue.enqueue(make_request("/a"), 1));
        require_queued(queue.enqueue(make_request("/b"), 2));

        assert!(queue.remove(&id1));
        assert_eq!(queue.len(), 1);
        assert!(!queue.remove(&id1));
        assert!(!queue.remove("nonexistent"));
    }

    #[test]
    fn test_clear() {
        let mut queue = OfflineQueue::new(10);
        require_queued(queue.enqueue(make_request("/a"), 1));
        require_queued(queue.enqueue(make_request("/b"), 2));
        queue.clear();
        assert!(queue.is_empty());
    }

    #[test]
    fn test_max_size_eviction() {
        let mut queue = OfflineQueue::new(2);
        require_queued(queue.enqueue(make_request("/a"), 5));
        require_queued(queue.enqueue(make_request("/b"), 3));
        assert_eq!(queue.len(), 2);

        let outcome = queue.enqueue(make_request("/c"), 1);
        assert_eq!(outcome.evicted().unwrap().request.path, "/a");
        assert_eq!(queue.len(), 2);

        let first = queue.dequeue().unwrap();
        assert_eq!(first.request.path, "/c");
        let second = queue.dequeue().unwrap();
        assert_eq!(second.request.path, "/b");
    }

    #[test]
    fn test_max_size_no_eviction_when_lower_priority() {
        let mut queue = OfflineQueue::new(2);
        require_queued(queue.enqueue(make_request("/a"), 1));
        require_queued(queue.enqueue(make_request("/b"), 2));

        let outcome = queue.enqueue(make_request("/c"), 10);
        match outcome {
            EnqueueOutcome::Rejected { request } => assert_eq!(request.path, "/c"),
            EnqueueOutcome::Queued { .. } => panic!("lower-priority request was queued"),
        }
        assert_eq!(queue.len(), 2);

        assert_eq!(queue.dequeue().unwrap().request.path, "/a");
        assert_eq!(queue.dequeue().unwrap().request.path, "/b");
    }

    #[test]
    fn test_unique_ids() {
        let mut queue = OfflineQueue::new(10);
        let (id1, _) = require_queued(queue.enqueue(make_request("/a"), 1));
        let (id2, _) = require_queued(queue.enqueue(make_request("/b"), 1));
        let (id3, _) = require_queued(queue.enqueue(make_request("/c"), 1));
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_zero_capacity_queue_rejects_and_returns_request() {
        let mut queue = OfflineQueue::new(0);
        let outcome = queue.enqueue(make_request("/a"), 1);

        assert!(!outcome.is_queued());
        match outcome {
            EnqueueOutcome::Rejected { request } => assert_eq!(request.path, "/a"),
            EnqueueOutcome::Queued { .. } => panic!("zero-capacity queue accepted a request"),
        }
        assert!(queue.is_empty());
        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn request_ids_wrap_without_panicking_or_colliding() {
        let mut queue = OfflineQueue::new(4);
        queue.next_id = u64::MAX;
        let (max, _) = require_queued(queue.enqueue(make_request("/max"), 1));
        let (wrapped, _) = require_queued(queue.enqueue(make_request("/wrapped"), 1));
        assert_eq!(max, format!("req_{}", u64::MAX));
        assert_eq!(wrapped, "req_1");

        queue.next_id = 1;
        let (id, _) = require_queued(queue.enqueue(make_request("/skip-duplicate"), 1));
        assert_eq!(id, "req_2");
    }
}
