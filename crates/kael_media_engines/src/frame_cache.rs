//! Decoded-frame cache with byte-budget LRU eviction.
//!
//! The tiered cache the editor relies on at 4K/8K: decoded frames are kept in a
//! bounded pool and the least-recently-used frames are evicted when the byte
//! budget is exceeded, so memory stays flat over long sessions. The budget is
//! intended to be driven by the renderer's GPU-memory budget. An entry limit
//! also bounds metadata for small frames; payload bytes exclude caller-held
//! clones and cache metadata.

use std::collections::HashMap;
use std::sync::Arc;

const DEFAULT_MAX_ENTRIES: usize = 4096;

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
    key: FrameKey,
    data: Arc<[u8]>,
    previous: Option<usize>,
    next: Option<usize>,
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
///
/// An intrusive LRU list makes lookups and each eviction expected O(1), so
/// inserting a large frame evicts many small frames without repeatedly scanning
/// the cache. Entry limits bound metadata; external `Arc` clones can outlive
/// eviction and are outside the cache's retained-payload accounting.
pub struct FrameCache {
    budget_bytes: u64,
    used_bytes: u64,
    max_entries: usize,
    entries: HashMap<FrameKey, usize>,
    slots: Vec<Option<Entry>>,
    free_slots: Vec<usize>,
    least_recent: Option<usize>,
    most_recent: Option<usize>,
    stats: FrameCacheStats,
}

impl FrameCache {
    /// Create a cache with the given byte budget and a limit of 4,096 entries.
    ///
    /// Use [`Self::with_entry_limit`] to choose a different metadata bound.
    pub fn new(budget_bytes: u64) -> Self {
        Self::with_entry_limit(budget_bytes, DEFAULT_MAX_ENTRIES)
    }

    /// Create a cache with both payload-byte and entry limits.
    ///
    /// Empty frames are rejected. A zero byte or entry limit disables retention;
    /// even very large limits do not allocate eagerly.
    pub fn with_entry_limit(budget_bytes: u64, max_entries: usize) -> Self {
        Self {
            budget_bytes,
            used_bytes: 0,
            max_entries,
            entries: HashMap::new(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            least_recent: None,
            most_recent: None,
            stats: FrameCacheStats::default(),
        }
    }

    /// The byte budget.
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Change the byte budget, evicting least-recently-used frames immediately.
    ///
    /// Releases unused metadata capacity after eviction. Statistics are retained.
    pub fn set_budget_bytes(&mut self, budget_bytes: u64) {
        self.budget_bytes = budget_bytes;
        while self.used_bytes > budget_bytes {
            self.evict_one();
        }
        self.compact_storage();
        self.entries.shrink_to_fit();
    }

    /// Maximum number of retained frames, also bounding cache metadata.
    pub fn max_entries(&self) -> usize {
        self.max_entries
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
    /// Returns `false`, preserving existing entries, if the frame is empty,
    /// exceeds the budget, or the entry limit is zero.
    pub fn insert(&mut self, key: FrameKey, data: Arc<[u8]>) -> bool {
        let Ok(bytes) = u64::try_from(data.len()) else {
            return false;
        };
        if bytes == 0 || bytes > self.budget_bytes || self.max_entries == 0 {
            return false;
        }
        self.remove(&key);
        while self.used_bytes > self.budget_bytes - bytes || self.entries.len() >= self.max_entries
        {
            self.evict_one();
        }
        self.used_bytes += bytes;
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() - 1
        });
        self.slots[slot] = Some(Entry {
            key,
            data,
            previous: None,
            next: None,
        });
        self.entries.insert(key, slot);
        self.append_recent(slot);
        true
    }

    /// Look up a frame, marking it most-recently-used and updating statistics.
    pub fn get(&mut self, key: &FrameKey) -> Option<Arc<[u8]>> {
        if let Some(slot) = self.entries.get(key).copied() {
            if self.most_recent != Some(slot) {
                self.unlink(slot);
                self.append_recent(slot);
            }
            self.stats.hits = self.stats.hits.saturating_add(1);
            Some(self.entry(slot).data.clone())
        } else {
            self.stats.misses = self.stats.misses.saturating_add(1);
            None
        }
    }

    /// Remove one frame without recording an eviction or lookup.
    pub fn remove(&mut self, key: &FrameKey) -> Option<Arc<[u8]>> {
        let slot = self.entries.remove(key)?;
        Some(self.remove_slot(slot))
    }

    /// Drop all cached frames and metadata capacity (statistics are retained).
    pub fn clear(&mut self) {
        self.entries = HashMap::new();
        self.slots = Vec::new();
        self.free_slots = Vec::new();
        self.used_bytes = 0;
        self.least_recent = None;
        self.most_recent = None;
    }

    fn evict_one(&mut self) {
        if let Some(slot) = self.least_recent {
            let key = self.entry(slot).key;
            self.entries.remove(&key);
            self.remove_slot(slot);
            self.stats.evictions = self.stats.evictions.saturating_add(1);
        }
    }

    fn remove_slot(&mut self, slot: usize) -> Arc<[u8]> {
        self.unlink(slot);
        let entry = self.slots[slot].take().expect("LRU entry exists");
        self.used_bytes -= entry.data.len() as u64;
        self.free_slots.push(slot);
        entry.data
    }

    fn entry(&self, slot: usize) -> &Entry {
        self.slots[slot].as_ref().expect("LRU entry exists")
    }

    fn entry_mut(&mut self, slot: usize) -> &mut Entry {
        self.slots[slot].as_mut().expect("LRU entry exists")
    }

    fn unlink(&mut self, slot: usize) {
        let entry = self.entry_mut(slot);
        let previous = entry.previous.take();
        let next = entry.next.take();
        if let Some(previous) = previous {
            self.entry_mut(previous).next = next;
        } else {
            self.least_recent = next;
        }
        if let Some(next) = next {
            self.entry_mut(next).previous = previous;
        } else {
            self.most_recent = previous;
        }
    }

    fn append_recent(&mut self, slot: usize) {
        let previous = self.most_recent;
        let entry = self.entry_mut(slot);
        entry.previous = previous;
        entry.next = None;
        if let Some(previous) = self.most_recent {
            self.entry_mut(previous).next = Some(slot);
        } else {
            self.least_recent = Some(slot);
        }
        self.most_recent = Some(slot);
    }

    fn compact_storage(&mut self) {
        if self.free_slots.is_empty() {
            return;
        }
        let mut slots = Vec::with_capacity(self.entries.len());
        let mut next = self.least_recent;
        while let Some(slot) = next {
            let mut entry = self.slots[slot].take().expect("LRU entry exists");
            next = entry.next;
            let index = slots.len();
            entry.previous = index.checked_sub(1);
            entry.next = next.map(|_| index + 1);
            *self.entries.get_mut(&entry.key).expect("LRU key exists") = index;
            slots.push(Some(entry));
        }
        self.slots = slots;
        self.free_slots = Vec::new();
        self.least_recent = (!self.slots.is_empty()).then_some(0);
        self.most_recent = self.slots.len().checked_sub(1);
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
    fn saturated_statistics_preserve_lru_order() {
        let mut cache = FrameCache::new(2);
        let old = FrameKey::new(1, 0);
        let recent = FrameKey::new(1, 1);
        cache.insert(old, frame(1));
        cache.insert(recent, frame(1));
        cache.stats.hits = u64::MAX;
        cache.stats.misses = u64::MAX;
        cache.stats.evictions = u64::MAX;

        assert!(cache.get(&recent).is_some());
        assert_eq!(cache.stats.hits, u64::MAX);
        assert_eq!(cache.stats.misses, u64::MAX);
        assert_eq!(cache.stats.evictions, u64::MAX);
        cache.insert(FrameKey::new(1, 2), frame(1));

        assert!(cache.get(&old).is_none());
        assert!(cache.get(&recent).is_some());
        assert_eq!(cache.stats.evictions, u64::MAX);
    }

    #[test]
    fn saturated_statistics_still_have_a_finite_hit_rate() {
        let mut cache = FrameCache::new(0);
        cache.stats = FrameCacheStats {
            hits: u64::MAX,
            misses: u64::MAX,
            evictions: 0,
        };
        assert_eq!(cache.hit_rate(), 0.5);
    }

    fn assert_order(cache: &FrameCache, expected: &[FrameKey]) {
        let mut order = Vec::new();
        let mut key = cache.least_recent;
        let mut previous = None;
        while let Some(current) = key {
            let entry = cache.entry(current);
            assert_eq!(entry.previous, previous);
            assert_eq!(cache.entries.get(&entry.key), Some(&current));
            order.push(entry.key);
            assert!(
                order.len() <= cache.entries.len(),
                "LRU list contains a cycle"
            );
            previous = key;
            key = entry.next;
        }
        assert_eq!(cache.most_recent, previous);
        assert_eq!(order, expected);
        assert_eq!(order.len(), cache.entries.len());
        assert_eq!(
            cache.used_bytes,
            cache
                .slots
                .iter()
                .flatten()
                .map(|entry| entry.data.len() as u64)
                .sum::<u64>()
        );
        assert!(cache.used_bytes <= cache.budget_bytes);
        assert!(cache.len() <= cache.max_entries);
        assert!(cache.slots.len() <= cache.max_entries);
        assert_eq!(cache.free_slots.len() + cache.len(), cache.slots.len());
        let free: std::collections::HashSet<_> = cache.free_slots.iter().copied().collect();
        assert_eq!(free.len(), cache.free_slots.len());
        for slot in free {
            assert!(cache.slots[slot].is_none());
        }
    }

    #[test]
    fn empty_frames_and_zero_limits_do_not_retain_metadata() {
        let mut cache = FrameCache::new(100);
        for index in 0..1000 {
            assert!(!cache.insert(FrameKey::new(1, index), frame(0)));
        }
        assert_order(&cache, &[]);
        assert_eq!(cache.entries.capacity(), 0);
        assert_eq!(cache.slots.capacity(), 0);
        let mut disabled = FrameCache::with_entry_limit(100, 0);
        assert!(!disabled.insert(FrameKey::new(1, 0), frame(1)));
        let mut no_bytes = FrameCache::new(0);
        assert!(!no_bytes.insert(FrameKey::new(1, 0), frame(1)));
        assert!(!no_bytes.insert(FrameKey::new(1, 0), frame(0)));
    }

    #[test]
    fn entry_limit_bounds_metadata_independently_of_payload_budget() {
        let mut cache = FrameCache::with_entry_limit(u64::MAX, 2);
        for index in 0..3 {
            assert!(cache.insert(FrameKey::new(1, index), frame(1)));
        }
        assert_order(&cache, &[FrameKey::new(1, 1), FrameKey::new(1, 2)]);
        assert_eq!(cache.stats.evictions, 1);

        let mut default = FrameCache::new(u64::MAX);
        assert_eq!(default.max_entries(), 4096);
        for index in 0..=DEFAULT_MAX_ENTRIES {
            assert!(default.insert(FrameKey::new(1, index as u64), frame(1)));
        }
        assert_eq!(default.len(), DEFAULT_MAX_ENTRIES);
        assert_eq!(default.stats.evictions, 1);
        assert!(default.get(&FrameKey::new(1, 0)).is_none());
    }

    #[test]
    fn removals_and_replacements_keep_the_list_linked() {
        let mut cache = FrameCache::new(100);
        let keys = (0..5)
            .map(|index| FrameKey::new(1, index))
            .collect::<Vec<_>>();
        for key in &keys {
            cache.insert(*key, frame(1));
        }
        cache.remove(&keys[2]); // Middle.
        cache.remove(&keys[0]); // Oldest.
        cache.remove(&keys[4]); // Newest.
        assert_order(&cache, &[keys[1], keys[3]]);
        cache.insert(keys[1], frame(2)); // Existing key becomes newest.
        assert_order(&cache, &[keys[3], keys[1]]);
        cache.remove(&keys[3]);
        cache.remove(&keys[1]);
        assert_order(&cache, &[]);
        assert_eq!(cache.remove(&keys[0]), None);
        cache.insert(keys[2], frame(1));
        assert_order(&cache, &[keys[2]]);
        assert_eq!(cache.stats.evictions, 0);
    }

    #[test]
    fn rejected_replacements_preserve_existing_frame_and_recency() {
        let mut cache = FrameCache::new(4);
        let first = FrameKey::new(1, 0);
        let second = FrameKey::new(1, 1);
        let original: Arc<[u8]> = vec![1; 2].into();
        cache.insert(first, original.clone());
        cache.insert(second, frame(2));
        assert!(!cache.insert(first, frame(0)));
        assert!(!cache.insert(first, frame(5)));
        assert_order(&cache, &[first, second]);
        assert!(Arc::ptr_eq(&cache.get(&first).unwrap(), &original));
    }

    #[test]
    fn bulk_eviction_and_budget_changes_preserve_recent_frames() {
        let mut cache = FrameCache::new(10);
        let keys = (0..10)
            .map(|index| FrameKey::new(1, index))
            .collect::<Vec<_>>();
        for key in &keys {
            cache.insert(*key, frame(1));
        }
        cache.get(&keys[0]);
        let large = FrameKey::new(2, 0);
        cache.insert(large, frame(8));
        assert_order(&cache, &[keys[9], keys[0], large]);
        assert_eq!(cache.stats.evictions, 8);
        cache.set_budget_bytes(8);
        assert_order(&cache, &[large]);
        assert_eq!(cache.stats.evictions, 10);
        cache.set_budget_bytes(0);
        assert_order(&cache, &[]);
        assert_eq!(cache.stats.evictions, 11);
        assert_eq!(cache.entries.capacity(), 0);
        assert_eq!(cache.slots.capacity(), 0);
        assert_eq!(cache.free_slots.capacity(), 0);
    }

    #[test]
    fn clear_releases_metadata_and_cache_can_be_reused() {
        let mut cache = FrameCache::new(10);
        cache.insert(FrameKey::new(1, 0), frame(1));
        cache.get(&FrameKey::new(1, 0));
        assert!(cache.entries.capacity() > 0);
        cache.clear();
        assert_eq!(cache.entries.capacity(), 0);
        assert_eq!(cache.slots.capacity(), 0);
        assert_eq!(cache.free_slots.capacity(), 0);
        assert_order(&cache, &[]);
        cache.insert(FrameKey::new(2, 0), frame(1));
        assert_order(&cache, &[FrameKey::new(2, 0)]);
        assert_eq!(cache.stats.hits, 1);
    }

    #[test]
    fn cache_matches_reference_lru_under_mixed_operations() {
        use std::collections::VecDeque;
        let mut cache = FrameCache::with_entry_limit(64, 7);
        let mut reference: VecDeque<(FrameKey, Arc<[u8]>)> = VecDeque::new();
        let mut expected_stats = FrameCacheStats::default();
        let mut state = 42_u64;
        for _ in 0..10_000 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let key = FrameKey::new((state >> 8) % 2, (state >> 16) % 12);
            match state % 8 {
                0..=3 => {
                    let data = frame(((state >> 32) % 80) as usize);
                    let bytes = data.len() as u64;
                    let admitted = bytes > 0 && bytes <= cache.budget_bytes;
                    if admitted {
                        reference.retain(|(entry, _)| *entry != key);
                        while reference.len() >= cache.max_entries
                            || reference
                                .iter()
                                .map(|(_, data)| data.len() as u64)
                                .sum::<u64>()
                                > cache.budget_bytes - bytes
                        {
                            reference.pop_front();
                            expected_stats.evictions += 1;
                        }
                        reference.push_back((key, data.clone()));
                    }
                    assert_eq!(cache.insert(key, data), admitted);
                }
                4 => {
                    let expected =
                        reference
                            .iter()
                            .position(|(entry, _)| *entry == key)
                            .map(|position| {
                                let (key, data) = reference.remove(position).unwrap();
                                reference.push_back((key, data.clone()));
                                data
                            });
                    if expected.is_some() {
                        expected_stats.hits += 1;
                    } else {
                        expected_stats.misses += 1;
                    }
                    assert_eq!(cache.get(&key), expected);
                }
                5 => {
                    let expected = reference
                        .iter()
                        .position(|(entry, _)| *entry == key)
                        .map(|position| reference.remove(position).unwrap().1);
                    assert_eq!(cache.remove(&key), expected);
                }
                6 => {
                    let budget = (state >> 24) % 100;
                    while reference
                        .iter()
                        .map(|(_, data)| data.len() as u64)
                        .sum::<u64>()
                        > budget
                    {
                        reference.pop_front();
                        expected_stats.evictions += 1;
                    }
                    cache.set_budget_bytes(budget);
                }
                _ => {
                    reference.clear();
                    cache.clear();
                }
            }
            assert_eq!(cache.stats, expected_stats);
            assert_order(
                &cache,
                &reference.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
            );
            for (key, data) in &reference {
                assert!(Arc::ptr_eq(
                    &cache.entry(*cache.entries.get(key).unwrap()).data,
                    data
                ));
            }
        }
    }
}
