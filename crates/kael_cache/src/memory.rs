use std::collections::{BTreeSet, HashMap};

/// Priority level for cached entries, influencing eviction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePriority {
    /// Evicted first when capacity is exceeded.
    Low,
    /// Default priority level.
    Normal,
    /// Evicted last; retained as long as possible.
    High,
}

#[derive(Debug, Clone)]
struct Entry<V: Clone> {
    value: V,
    eviction: EvictionKey,
    bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EvictionKey {
    priority: CachePriority,
    access_order: u64,
    key: String,
}

impl EvictionKey {
    fn new(key: String, priority: CachePriority, access_order: u64) -> Self {
        Self {
            priority,
            access_order,
            key,
        }
    }
}

/// An in-memory LRU cache with priority-aware eviction.
///
/// Entries are evicted in order of ascending priority, then by least-recent access.
#[derive(Debug)]
pub struct MemoryCache<V: Clone> {
    entries: HashMap<String, Entry<V>>,
    eviction_order: BTreeSet<EvictionKey>,
    max_entries: usize,
    max_bytes: Option<u64>,
    used_bytes: u64,
    weigh: fn(&V) -> u64,
    access_counter: u64,
    hits: u64,
    misses: u64,
}

impl<V: Clone> MemoryCache<V> {
    /// Creates a new memory cache limited to `max_entries` items.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            eviction_order: BTreeSet::new(),
            max_entries,
            max_bytes: None,
            used_bytes: 0,
            weigh: |_| 0,
            access_counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Creates a cache bounded by both entry count and caller-reported payload bytes.
    ///
    /// `weigh` is evaluated on insertion; report the bytes retained by each value.
    /// Reinsert a value if its retained size changes through interior mutability.
    /// Keys, cache metadata, and clones held by callers are outside this budget.
    /// A zero byte budget admits only zero-weight values, subject to `max_entries`.
    pub fn with_byte_budget(max_entries: usize, max_bytes: u64, weigh: fn(&V) -> u64) -> Self {
        Self {
            max_bytes: Some(max_bytes),
            weigh,
            ..Self::new(max_entries)
        }
    }

    /// Retrieves a cached value by key, updating its access recency.
    pub fn get(&mut self, key: &str) -> Option<V> {
        if let Some(previous_eviction) = self.entries.get(key).map(|entry| entry.eviction.clone()) {
            self.eviction_order.remove(&previous_eviction);
            let access_order = self.next_access_order();
            let entry = self.entries.get_mut(key)?;
            entry.eviction.access_order = access_order;
            self.eviction_order.insert(entry.eviction.clone());
            self.hits = self.hits.saturating_add(1);
            Some(entry.value.clone())
        } else {
            self.misses = self.misses.saturating_add(1);
            None
        }
    }

    /// Inserts a value with the given priority, evicting the lowest-priority
    /// least-recently-used entries if the cache is full.
    ///
    /// Values exceeding the byte budget are ignored, preserving an existing value
    /// at the same key. Use [`Self::try_insert`] to check whether a value was admitted.
    pub fn insert(&mut self, key: String, value: V, priority: CachePriority) {
        self.try_insert(key, value, priority);
    }

    /// Inserts a value if it fits the configured limits, returning whether it was admitted.
    ///
    /// Rejection preserves existing entries. Successful replacements update both
    /// their priority and byte accounting before eviction.
    pub fn try_insert(&mut self, key: String, value: V, priority: CachePriority) -> bool {
        if self.max_entries == 0 {
            return false;
        }
        let bytes = (self.weigh)(&value);
        let max_bytes = self.max_bytes.unwrap_or(u64::MAX);
        if bytes > max_bytes {
            return false;
        }
        self.remove(&key);
        while self.entries.len() >= self.max_entries || self.used_bytes > max_bytes - bytes {
            self.evict_one();
        }

        let access_order = self.next_access_order();
        let eviction = EvictionKey::new(key.clone(), priority, access_order);
        self.entries.insert(
            key,
            Entry {
                value,
                eviction: eviction.clone(),
                bytes,
            },
        );
        self.used_bytes += bytes;
        self.eviction_order.insert(eviction);
        true
    }

    /// Removes and returns the value for `key`, if present.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.entries.remove(key).map(|entry| {
            self.eviction_order.remove(&entry.eviction);
            self.used_bytes -= entry.bytes;
            entry.value
        })
    }

    /// Removes all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.eviction_order.clear();
        self.used_bytes = 0;
    }

    /// Returns the number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the payload-byte limit, or `None` for an entry-count-only cache.
    pub fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Returns the sum of retained payload weights (zero for an entry-count-only cache).
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Returns the number of cache hits recorded.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Returns the number of cache misses recorded.
    pub fn misses(&self) -> u64 {
        self.misses
    }

    fn evict_one(&mut self) {
        if let Some(victim) = self.eviction_order.pop_first() {
            if let Some(entry) = self.entries.remove(victim.key.as_str()) {
                self.used_bytes -= entry.bytes;
            }
        }
    }

    fn next_access_order(&mut self) -> u64 {
        if self.access_counter == u64::MAX {
            let keys = self
                .eviction_order
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>();
            self.eviction_order.clear();
            for (index, key) in keys.into_iter().enumerate() {
                if let Some(entry) = self.entries.get_mut(&key) {
                    entry.eviction.access_order = u64::try_from(index)
                        .unwrap_or(u64::MAX - 1)
                        .saturating_add(1);
                    self.eviction_order.insert(entry.eviction.clone());
                }
            }
            self.access_counter = u64::try_from(self.entries.len()).unwrap_or(u64::MAX - 1);
        }
        self.access_counter = self.access_counter.saturating_add(1);
        self.access_counter
    }

    pub(crate) fn remove_matching(&mut self, predicate: impl Fn(&str) -> bool) {
        let keys = self
            .entries
            .keys()
            .filter(|key| predicate(key))
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_and_get() {
        let mut cache = MemoryCache::new(10);
        cache.insert("a".into(), 1, CachePriority::Normal);
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.get("missing"), None);
    }

    #[test]
    fn len_and_is_empty() {
        let mut cache = MemoryCache::<i32>::new(10);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.insert("x".into(), 42, CachePriority::Normal);
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn remove_entry() {
        let mut cache = MemoryCache::new(10);
        cache.insert("k".into(), "val", CachePriority::Normal);
        assert_eq!(cache.remove("k"), Some("val"));
        assert!(cache.is_empty());
        assert_eq!(cache.remove("k"), None);
    }

    #[test]
    fn clear_cache() {
        let mut cache = MemoryCache::new(10);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("b".into(), 2, CachePriority::Normal);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn evicts_lru_when_full() {
        let mut cache = MemoryCache::new(2);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("b".into(), 2, CachePriority::Normal);
        cache.insert("c".into(), 3, CachePriority::Normal);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(2));
        assert_eq!(cache.get("c"), Some(3));
    }

    #[test]
    fn evicts_low_priority_first() {
        let mut cache = MemoryCache::new(2);
        cache.insert("low".into(), 1, CachePriority::Low);
        cache.insert("high".into(), 2, CachePriority::High);
        cache.insert("new".into(), 3, CachePriority::Normal);

        assert_eq!(cache.get("low"), None);
        assert_eq!(cache.get("high"), Some(2));
        assert_eq!(cache.get("new"), Some(3));
    }

    #[test]
    fn update_existing_key() {
        let mut cache = MemoryCache::new(2);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("a".into(), 99, CachePriority::High);
        assert_eq!(cache.get("a"), Some(99));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn hit_miss_tracking() {
        let mut cache = MemoryCache::new(10);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.get("a");
        cache.get("a");
        cache.get("missing");

        assert_eq!(cache.hits(), 2);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn access_refreshes_lru_order() {
        let mut cache = MemoryCache::new(2);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("b".into(), 2, CachePriority::Normal);
        cache.get("a");
        cache.insert("c".into(), 3, CachePriority::Normal);

        assert_eq!(cache.get("b"), None);
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.get("c"), Some(3));
    }

    #[test]
    fn zero_capacity_never_stores_entries() {
        let mut cache = MemoryCache::new(0);
        cache.insert("a".into(), 1, CachePriority::High);

        assert!(cache.is_empty());
        assert_eq!(cache.get("a"), None);
    }

    #[test]
    fn counters_and_access_order_do_not_overflow() {
        let mut cache = MemoryCache::new(2);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("b".into(), 2, CachePriority::Normal);
        cache.access_counter = u64::MAX;
        cache.hits = u64::MAX;
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.hits(), u64::MAX);
        cache.insert("c".into(), 3, CachePriority::Normal);
        assert_eq!(cache.get("b"), None);
    }

    #[test]
    fn large_limits_do_not_allocate_eagerly() {
        let cache = MemoryCache::<i32>::new(usize::MAX);
        assert!(cache.is_empty());
    }

    #[test]
    fn payload_budget_evicts_multiple_entries_by_priority_then_recency() {
        let mut cache = MemoryCache::with_byte_budget(10, 10, |bytes: &Vec<u8>| bytes.len() as u64);
        cache.insert("high".into(), vec![1; 3], CachePriority::High);
        cache.insert("old-low".into(), vec![2; 2], CachePriority::Low);
        cache.insert("new-low".into(), vec![3; 2], CachePriority::Low);
        cache.insert("normal".into(), vec![4; 2], CachePriority::Normal);
        cache.get("old-low");

        assert!(cache.try_insert("large".into(), vec![5; 5], CachePriority::Normal));

        assert_eq!(cache.used_bytes(), 10);
        assert_eq!(cache.get("new-low"), None);
        assert_eq!(cache.get("old-low"), None);
        assert_eq!(cache.get("high"), Some(vec![1; 3]));
        assert_eq!(cache.get("normal"), Some(vec![4; 2]));
        assert_eq!(cache.get("large"), Some(vec![5; 5]));
    }

    #[test]
    fn replacement_releases_its_weight_before_evicting_other_entries() {
        let mut cache = MemoryCache::with_byte_budget(2, 10, |bytes: &Vec<u8>| bytes.len() as u64);
        cache.insert("a".into(), vec![1; 8], CachePriority::Normal);
        cache.insert("b".into(), vec![2; 2], CachePriority::Normal);
        cache.insert("a".into(), vec![3; 4], CachePriority::High);
        assert_eq!(cache.used_bytes(), 6);
        assert_eq!(cache.get("b"), Some(vec![2; 2]));
        cache.insert("b".into(), vec![4; 7], CachePriority::Normal);
        assert_eq!(cache.used_bytes(), 7);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("a"), None);
    }

    #[test]
    fn oversized_insert_preserves_the_cache_and_previous_value() {
        let mut cache = MemoryCache::with_byte_budget(2, 4, |bytes: &Vec<u8>| bytes.len() as u64);
        cache.insert("a".into(), vec![1; 4], CachePriority::Normal);
        assert!(!cache.try_insert("a".into(), vec![2; 5], CachePriority::High));
        assert!(!cache.try_insert("b".into(), vec![3; 5], CachePriority::High));
        assert_eq!(cache.used_bytes(), 4);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("a"), Some(vec![1; 4]));
    }

    #[test]
    fn payload_accounting_tracks_removal_namespace_invalidation_and_clear() {
        let mut cache = MemoryCache::with_byte_budget(4, 10, |bytes: &Vec<u8>| bytes.len() as u64);
        cache.insert("ns:a".into(), vec![1; 3], CachePriority::Normal);
        cache.insert("ns:b".into(), vec![2; 2], CachePriority::Normal);
        cache.insert("other:c".into(), vec![3; 4], CachePriority::Normal);
        assert_eq!(cache.remove("ns:a"), Some(vec![1; 3]));
        assert_eq!(cache.used_bytes(), 6);
        cache.remove_matching(|key| key.starts_with("ns:"));
        assert_eq!(cache.used_bytes(), 4);
        cache.clear();
        assert_eq!(cache.used_bytes(), 0);
        assert!(cache.is_empty());
        assert!(cache.eviction_order.is_empty());
    }

    #[test]
    fn zero_byte_budget_and_zero_entry_budget_remain_bounded() {
        let mut cache = MemoryCache::with_byte_budget(1, 0, |bytes: &Vec<u8>| bytes.len() as u64);
        assert!(!cache.try_insert("large".into(), vec![1], CachePriority::Normal));
        assert!(cache.try_insert("empty".into(), vec![], CachePriority::Normal));
        assert!(cache.try_insert("new-empty".into(), vec![], CachePriority::Normal));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.used_bytes(), 0);
        assert_eq!(cache.get("empty"), None);

        let mut disabled = MemoryCache::with_byte_budget(0, u64::MAX, |weight: &u64| *weight);
        assert!(!disabled.try_insert("empty".into(), 0, CachePriority::Normal));
        assert!(disabled.is_empty());
    }

    #[test]
    fn byte_accounting_does_not_overflow_with_maximum_weights() {
        let mut cache = MemoryCache::with_byte_budget(4, u64::MAX, |weight: &u64| *weight);
        assert!(cache.try_insert("full".into(), u64::MAX, CachePriority::High));
        assert!(cache.try_insert("one".into(), 1, CachePriority::Low));
        assert_eq!(cache.used_bytes(), 1);
        assert_eq!(cache.get("full"), None);
        assert!(cache.try_insert("full".into(), u64::MAX, CachePriority::Normal));
        assert_eq!(cache.used_bytes(), u64::MAX);
        assert_eq!(cache.remove("full"), Some(u64::MAX));
        assert_eq!(cache.used_bytes(), 0);
    }

    #[test]
    fn payload_budget_preserves_recency_when_access_counter_wraps() {
        let mut cache = MemoryCache::with_byte_budget(3, 3, |weight: &u64| *weight);
        cache.insert("a".into(), 1, CachePriority::Normal);
        cache.insert("b".into(), 1, CachePriority::Normal);
        cache.insert("c".into(), 1, CachePriority::Normal);
        cache.access_counter = u64::MAX;
        cache.get("a");
        cache.insert("d".into(), 2, CachePriority::Normal);
        assert_eq!(cache.get("b"), None);
        assert_eq!(cache.get("c"), None);
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.used_bytes(), 3);
    }
}
