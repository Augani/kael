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

    /// Enqueue a request with a given priority, returning its assigned id.
    ///
    /// If the queue is at capacity, the lowest-priority (highest number) item is dropped.
    pub fn enqueue(&mut self, request: ApiRequest, priority: u32) -> String {
        let id = format!("req_{}", self.next_id);
        self.next_id += 1;

        if self.max_size == 0 {
            return id;
        }

        let queued = QueuedRequest {
            id: id.clone(),
            request,
            created_at: now_unix_millis(),
            attempts: 0,
            priority,
        };

        if self.queue.len() >= self.max_size {
            let worst_idx = self
                .queue
                .iter()
                .enumerate()
                .max_by_key(|(_, item)| item.priority)
                .map(|(idx, _)| idx);

            if let Some(idx) = worst_idx {
                if self.queue[idx].priority > priority {
                    self.queue.remove(idx);
                } else {
                    return id;
                }
            }
        }

        let insert_pos = self
            .queue
            .iter()
            .position(|item| item.priority > priority)
            .unwrap_or(self.queue.len());

        self.queue.insert(insert_pos, queued);
        id
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
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ApiRequest;

    fn make_request(path: &str) -> ApiRequest {
        ApiRequest::get(path)
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
        let id = queue.enqueue(make_request("/a"), 1);
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
        queue.enqueue(make_request("/low"), 10);
        queue.enqueue(make_request("/high"), 1);
        queue.enqueue(make_request("/mid"), 5);

        assert_eq!(queue.dequeue().unwrap().request.path, "/high");
        assert_eq!(queue.dequeue().unwrap().request.path, "/mid");
        assert_eq!(queue.dequeue().unwrap().request.path, "/low");
    }

    #[test]
    fn test_peek() {
        let mut queue = OfflineQueue::new(10);
        assert!(queue.peek().is_none());

        queue.enqueue(make_request("/a"), 1);
        let peeked = queue.peek().unwrap();
        assert_eq!(peeked.request.path, "/a");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut queue = OfflineQueue::new(10);
        let id1 = queue.enqueue(make_request("/a"), 1);
        let _id2 = queue.enqueue(make_request("/b"), 2);

        assert!(queue.remove(&id1));
        assert_eq!(queue.len(), 1);
        assert!(!queue.remove(&id1));
        assert!(!queue.remove("nonexistent"));
    }

    #[test]
    fn test_clear() {
        let mut queue = OfflineQueue::new(10);
        queue.enqueue(make_request("/a"), 1);
        queue.enqueue(make_request("/b"), 2);
        queue.clear();
        assert!(queue.is_empty());
    }

    #[test]
    fn test_max_size_eviction() {
        let mut queue = OfflineQueue::new(2);
        queue.enqueue(make_request("/a"), 5);
        queue.enqueue(make_request("/b"), 3);
        assert_eq!(queue.len(), 2);

        queue.enqueue(make_request("/c"), 1);
        assert_eq!(queue.len(), 2);

        let first = queue.dequeue().unwrap();
        assert_eq!(first.request.path, "/c");
        let second = queue.dequeue().unwrap();
        assert_eq!(second.request.path, "/b");
    }

    #[test]
    fn test_max_size_no_eviction_when_lower_priority() {
        let mut queue = OfflineQueue::new(2);
        queue.enqueue(make_request("/a"), 1);
        queue.enqueue(make_request("/b"), 2);

        queue.enqueue(make_request("/c"), 10);
        assert_eq!(queue.len(), 2);

        assert_eq!(queue.dequeue().unwrap().request.path, "/a");
        assert_eq!(queue.dequeue().unwrap().request.path, "/b");
    }

    #[test]
    fn test_unique_ids() {
        let mut queue = OfflineQueue::new(10);
        let id1 = queue.enqueue(make_request("/a"), 1);
        let id2 = queue.enqueue(make_request("/b"), 1);
        let id3 = queue.enqueue(make_request("/c"), 1);
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_zero_capacity_queue_drops_everything() {
        let mut queue = OfflineQueue::new(0);
        let id = queue.enqueue(make_request("/a"), 1);

        assert!(!id.is_empty());
        assert!(queue.is_empty());
        assert!(queue.dequeue().is_none());
    }
}
