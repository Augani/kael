use std::collections::HashMap;

/// Current status of a collaborator.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PresenceStatus {
    /// User is actively engaged.
    Active,
    /// User has been inactive for a short period.
    Idle,
    /// User has stepped away.
    Away,
    /// User is not connected.
    Offline,
}

/// Presence information for a single collaborator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Presence {
    /// Unique user identifier.
    pub user_id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Assigned color for cursors/highlights (e.g. "#ff6b35").
    pub color: String,
    /// Optional cursor position as (x, y).
    pub cursor_position: Option<(f64, f64)>,
    /// Unix timestamp (ms) of the last activity.
    pub last_seen: u64,
    /// Current presence status.
    pub status: PresenceStatus,
}

/// Tracks presence state for all collaborators in a session.
#[derive(Debug)]
pub struct PresenceTracker {
    peers: HashMap<String, Presence>,
    idle_timeout_ms: u64,
}

impl PresenceTracker {
    /// Create a new tracker with the given idle timeout in milliseconds.
    pub fn new(idle_timeout_ms: u64) -> Self {
        Self {
            peers: HashMap::new(),
            idle_timeout_ms,
        }
    }

    /// Update or insert presence for a user.
    pub fn update(&mut self, presence: Presence) {
        self.peers.insert(presence.user_id.clone(), presence);
    }

    /// Remove a user from the tracker.
    pub fn remove(&mut self, user_id: &str) {
        self.peers.remove(user_id);
    }

    /// Return all peers with `Active` status.
    pub fn active_peers(&self) -> Vec<&Presence> {
        self.peers
            .values()
            .filter(|p| p.status == PresenceStatus::Active)
            .collect()
    }

    /// Return all tracked peers.
    pub fn all_peers(&self) -> Vec<&Presence> {
        self.peers.values().collect()
    }

    /// Look up a specific peer by user id.
    pub fn peer(&self, user_id: &str) -> Option<&Presence> {
        self.peers.get(user_id)
    }

    /// Return the total number of tracked peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Remove peers whose `last_seen` timestamp is older than `now - idle_timeout_ms`.
    pub fn prune_stale(&mut self, now: u64) {
        let cutoff = now.saturating_sub(self.idle_timeout_ms);
        self.peers.retain(|_, p| p.last_seen >= cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_presence(user_id: &str, status: PresenceStatus, last_seen: u64) -> Presence {
        Presence {
            user_id: user_id.to_string(),
            display_name: user_id.to_string(),
            color: "#ff0000".to_string(),
            cursor_position: None,
            last_seen,
            status,
        }
    }

    #[test]
    fn test_new_tracker() {
        let tracker = PresenceTracker::new(5000);
        assert_eq!(tracker.peer_count(), 0);
        assert!(tracker.all_peers().is_empty());
        assert!(tracker.active_peers().is_empty());
    }

    #[test]
    fn test_update_and_get() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));

        assert_eq!(tracker.peer_count(), 1);
        let peer = tracker.peer("alice").unwrap();
        assert_eq!(peer.display_name, "alice");
        assert_eq!(peer.status, PresenceStatus::Active);
    }

    #[test]
    fn test_update_overwrites() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));
        tracker.update(make_presence("alice", PresenceStatus::Idle, 2000));

        assert_eq!(tracker.peer_count(), 1);
        assert_eq!(tracker.peer("alice").unwrap().status, PresenceStatus::Idle);
        assert_eq!(tracker.peer("alice").unwrap().last_seen, 2000);
    }

    #[test]
    fn test_remove() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));
        tracker.remove("alice");
        assert_eq!(tracker.peer_count(), 0);
        assert!(tracker.peer("alice").is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.remove("nobody");
        assert_eq!(tracker.peer_count(), 0);
    }

    #[test]
    fn test_active_peers() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));
        tracker.update(make_presence("bob", PresenceStatus::Idle, 1000));
        tracker.update(make_presence("carol", PresenceStatus::Active, 1000));
        tracker.update(make_presence("dave", PresenceStatus::Offline, 1000));

        let active = tracker.active_peers();
        assert_eq!(active.len(), 2);
        let ids: Vec<&str> = active.iter().map(|p| p.user_id.as_str()).collect();
        assert!(ids.contains(&"alice"));
        assert!(ids.contains(&"carol"));
    }

    #[test]
    fn test_all_peers() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));
        tracker.update(make_presence("bob", PresenceStatus::Away, 1000));
        assert_eq!(tracker.all_peers().len(), 2);
    }

    #[test]
    fn test_peer_not_found() {
        let tracker = PresenceTracker::new(5000);
        assert!(tracker.peer("nobody").is_none());
    }

    #[test]
    fn test_prune_stale() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 1000));
        tracker.update(make_presence("bob", PresenceStatus::Active, 4000));
        tracker.update(make_presence("carol", PresenceStatus::Active, 6000));

        tracker.prune_stale(8000);

        assert_eq!(tracker.peer_count(), 2);
        assert!(tracker.peer("alice").is_none());
        assert!(tracker.peer("bob").is_some());
        assert!(tracker.peer("carol").is_some());
    }

    #[test]
    fn test_prune_stale_boundary() {
        let mut tracker = PresenceTracker::new(5000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 3000));

        tracker.prune_stale(8000);
        assert_eq!(tracker.peer_count(), 1);

        tracker.prune_stale(8001);
        assert_eq!(tracker.peer_count(), 0);
    }

    #[test]
    fn test_prune_stale_underflow() {
        let mut tracker = PresenceTracker::new(10000);
        tracker.update(make_presence("alice", PresenceStatus::Active, 0));

        tracker.prune_stale(5000);
        assert_eq!(tracker.peer_count(), 1);
    }

    #[test]
    fn test_cursor_position() {
        let mut tracker = PresenceTracker::new(5000);
        let mut presence = make_presence("alice", PresenceStatus::Active, 1000);
        presence.cursor_position = Some((100.5, 200.3));
        tracker.update(presence);

        let peer = tracker.peer("alice").unwrap();
        let (x, y) = peer.cursor_position.unwrap();
        assert!((x - 100.5).abs() < f64::EPSILON);
        assert!((y - 200.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_presence_status_variants() {
        assert_ne!(PresenceStatus::Active, PresenceStatus::Idle);
        assert_ne!(PresenceStatus::Idle, PresenceStatus::Away);
        assert_ne!(PresenceStatus::Away, PresenceStatus::Offline);
        assert_eq!(PresenceStatus::Active, PresenceStatus::Active);
    }
}
