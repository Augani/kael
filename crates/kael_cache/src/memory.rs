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
    access_counter: u64,
    hits: u64,
    misses: u64,
}

impl<V: Clone> MemoryCache<V> {
    /// Creates a new memory cache limited to `max_entries` items.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            eviction_order: BTreeSet::new(),
            max_entries,
            access_counter: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Retrieves a cached value by key, updating its access recency.
    pub fn get(&mut self, key: &str) -> Option<V> {
        if let Some(entry) = self.entries.get_mut(key) {
            self.eviction_order.remove(&entry.eviction);
            self.access_counter += 1;
            entry.eviction.access_order = self.access_counter;
            self.eviction_order.insert(entry.eviction.clone());
            self.hits += 1;
            Some(entry.value.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    /// Inserts a value with the given priority, evicting the lowest-priority
    /// least-recently-used entry if the cache is full.
    pub fn insert(&mut self, key: String, value: V, priority: CachePriority) {
        if self.max_entries == 0 {
            return;
        }

        if self.entries.contains_key(&key) {
            self.access_counter += 1;
            let entry = self.entries.get_mut(&key).expect("key confirmed present");
            self.eviction_order.remove(&entry.eviction);
            entry.value = value;
            entry.eviction.priority = priority;
            entry.eviction.access_order = self.access_counter;
            self.eviction_order.insert(entry.eviction.clone());
            return;
        }

        if self.entries.len() >= self.max_entries {
            self.evict_one();
        }

        self.access_counter += 1;
        let eviction = EvictionKey::new(key.clone(), priority, self.access_counter);
        self.entries.insert(
            key,
            Entry {
                value,
                eviction: eviction.clone(),
            },
        );
        self.eviction_order.insert(eviction);
    }

    /// Removes and returns the value for `key`, if present.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.entries.remove(key).map(|entry| {
            self.eviction_order.remove(&entry.eviction);
            entry.value
        })
    }

    /// Removes all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.eviction_order.clear();
    }

    /// Returns the number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
            self.entries.remove(victim.key.as_str());
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
}
