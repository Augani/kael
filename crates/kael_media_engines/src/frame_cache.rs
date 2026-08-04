//! Decoded-frame cache with byte-budget LRU eviction.
//!
//! The tiered cache the editor relies on at 4K/8K: decoded frames are kept in a
//! bounded pool and the least-recently-used frames are evicted when the byte
//! budget is exceeded, so memory stays flat over long sessions. The budget is
//! intended to be driven by the renderer's GPU-memory budget.

use std::collections::HashMap;
use std::sync::Arc;

/// Key identifying a cached decoded frame within a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey {
    /// Identifier of the clip the frame belongs to.
    pub clip_id: u64,
    /// Frame index / presentation order within the clip.
    pub frame: u64,
}

impl FrameKey {
    /// Create a frame key.
    pub fn new(clip_id: u64, frame: u64) -> Self {
        Self { clip_id, frame }
    }
}

struct Entry {
    data: Arc<[u8]>,
    last_used: u64,
}

/// Hit/miss/eviction statistics for a [`FrameCache`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameCacheStats {
    /// Lookups that found a cached frame.
    pub hits: u64,
    /// Lookups that missed.
    pub misses: u64,
    /// Frames evicted to stay within budget.
    pub evictions: u64,
}

/// A decoded-frame cache bounded by a byte budget, evicting least-recently-used
/// frames when the budget would be exceeded.
pub struct FrameCache {
    budget_bytes: u64,
    used_bytes: u64,
    tick: u64,
    entries: HashMap<FrameKey, Entry>,
    stats: FrameCacheStats,
}

impl FrameCache {
    /// Create a cache with the given byte budget.
    pub fn new(budget_bytes: u64) -> Self {
        Self {
            budget_bytes,
            used_bytes: 0,
            tick: 0,
            entries: HashMap::new(),
            stats: FrameCacheStats::default(),
        }
    }

    /// The byte budget.
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Bytes currently held.
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Number of cached frames.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hit/miss/eviction statistics.
    pub fn stats(&self) -> FrameCacheStats {
        self.stats
    }

    /// Fraction of lookups that hit, in `0.0..=1.0`.
    pub fn hit_rate(&self) -> f64 {
        if self.stats.hits == 0 && self.stats.misses == 0 {
            0.0
        } else {
            self.stats.hits as f64 / (self.stats.hits as f64 + self.stats.misses as f64)
        }
    }

    /// Insert a frame, evicting least-recently-used frames to stay within budget.
    /// Returns `false` (and does not insert) if the frame alone exceeds the budget.
    pub fn insert(&mut self, key: FrameKey, data: Arc<[u8]>) -> bool {
        let Ok(bytes) = u64::try_from(data.len()) else {
            return false;
        };
        if bytes > self.budget_bytes {
            return false;
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.used_bytes = self
                .used_bytes
                .saturating_sub(u64::try_from(previous.data.len()).unwrap_or(u64::MAX));
        }
        while self.used_bytes > self.budget_bytes.saturating_sub(bytes) {
            if !self.evict_one() {
                return false;
            }
        }
        let tick = self.advance_tick();
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.entries.insert(
            key,
            Entry {
                data,
                last_used: tick,
            },
        );
        true
    }

    /// Look up a frame, marking it most-recently-used and updating statistics.
    pub fn get(&mut self, key: &FrameKey) -> Option<Arc<[u8]>> {
        let tick = self.advance_tick();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = tick;
            self.stats.hits = self.stats.hits.saturating_add(1);
            Some(entry.data.clone())
        } else {
            self.stats.misses = self.stats.misses.saturating_add(1);
            None
        }
    }

    /// Drop all cached frames (statistics are retained).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    fn evict_one(&mut self) -> bool {
        let lru = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| *key);
        if let Some(key) = lru
            && let Some(entry) = self.entries.remove(&key)
        {
            self.used_bytes = self
                .used_bytes
                .saturating_sub(u64::try_from(entry.data.len()).unwrap_or(u64::MAX));
            self.stats.evictions = self.stats.evictions.saturating_add(1);
            return true;
        }
        false
    }

    fn advance_tick(&mut self) -> u64 {
        if self.tick == u64::MAX {
            let mut by_age: Vec<_> = self
                .entries
                .iter()
                .map(|(key, entry)| (*key, entry.last_used))
                .collect();
            by_age.sort_unstable_by_key(|(_, last_used)| *last_used);
            for (index, (key, _)) in by_age.into_iter().enumerate() {
                if let Some(entry) = self.entries.get_mut(&key) {
                    entry.last_used = u64::try_from(index).unwrap_or(u64::MAX - 1) + 1;
                }
            }
            self.tick = u64::try_from(self.entries.len()).unwrap_or(u64::MAX - 1);
        }
        self.tick = self.tick.saturating_add(1);
        self.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: usize) -> Arc<[u8]> {
        vec![0u8; bytes].into()
    }

    #[test]
    fn hit_and_miss() {
        let mut cache = FrameCache::new(1000);
        let key = FrameKey::new(1, 0);
        assert!(cache.get(&key).is_none());
        cache.insert(key, frame(100));
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
        assert!((cache.hit_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn evicts_least_recently_used_over_budget() {
        let mut cache = FrameCache::new(100);
        cache.insert(FrameKey::new(1, 0), frame(40));
        cache.insert(FrameKey::new(1, 1), frame(40));
        // Touch frame 0 so frame 1 becomes least-recently-used.
        assert!(cache.get(&FrameKey::new(1, 0)).is_some());
        cache.insert(FrameKey::new(1, 2), frame(40));

        assert!(cache.used_bytes() <= 100);
        assert_eq!(cache.stats().evictions, 1);
        assert!(cache.get(&FrameKey::new(1, 0)).is_some());
        assert!(
            cache.get(&FrameKey::new(1, 1)).is_none(),
            "LRU frame evicted"
        );
        assert!(cache.get(&FrameKey::new(1, 2)).is_some());
    }

    #[test]
    fn rejects_frame_larger_than_budget() {
        let mut cache = FrameCache::new(100);
        assert!(!cache.insert(FrameKey::new(1, 0), frame(200)));
        assert!(cache.is_empty());
    }

    #[test]
    fn replacing_a_key_updates_byte_usage() {
        let mut cache = FrameCache::new(1000);
        cache.insert(FrameKey::new(1, 0), frame(100));
        cache.insert(FrameKey::new(1, 0), frame(50));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.used_bytes(), 50);
    }

    #[test]
    fn clear_drops_frames_keeps_stats() {
        let mut cache = FrameCache::new(1000);
        cache.insert(FrameKey::new(1, 0), frame(100));
        let _ = cache.get(&FrameKey::new(1, 0));
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.used_bytes(), 0);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn counter_saturation_preserves_lru_order() {
        let mut cache = FrameCache::new(2);
        let old = FrameKey::new(1, 0);
        let recent = FrameKey::new(1, 1);
        cache.insert(old, frame(1));
        cache.insert(recent, frame(1));
        cache.tick = u64::MAX;

        assert!(cache.get(&recent).is_some());
        cache.insert(FrameKey::new(1, 2), frame(1));

        assert!(cache.get(&old).is_none());
        assert!(cache.get(&recent).is_some());
    }

    #[test]
    fn saturated_statistics_still_have_a_finite_hit_rate() {
        let cache = FrameCache {
            budget_bytes: 0,
            used_bytes: 0,
            tick: 0,
            entries: HashMap::new(),
            stats: FrameCacheStats {
                hits: u64::MAX,
                misses: u64::MAX,
                evictions: 0,
            },
        };
        assert_eq!(cache.hit_rate(), 0.5);
    }
}
