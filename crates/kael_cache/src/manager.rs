use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::disk::DiskCache;
use crate::memory::{CachePriority, MemoryCache};

/// Configuration for the [`CacheManager`].
pub struct CacheConfig {
    /// Maximum number of entries in the in-memory cache.
    pub memory_max_entries: usize,
    /// Root directory for the disk cache.
    pub disk_root: PathBuf,
    /// Maximum total bytes for the disk cache.
    pub disk_max_bytes: u64,
}

/// Aggregated hit/miss statistics for both cache tiers.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of in-memory cache hits.
    pub memory_hits: u64,
    /// Number of in-memory cache misses.
    pub memory_misses: u64,
    /// Number of disk cache hits.
    pub disk_hits: u64,
    /// Number of disk cache misses.
    pub disk_misses: u64,
    /// Current number of entries in the memory cache.
    pub memory_entries: usize,
    /// Current total bytes used by the disk cache.
    pub disk_bytes: u64,
}

impl CacheStats {
    /// Returns the combined hit rate across both tiers as a value in `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when no lookups have been recorded.
    pub fn hit_rate(&self) -> f64 {
        let total = self.memory_hits + self.memory_misses + self.disk_hits + self.disk_misses;
        if total == 0 {
            return 0.0;
        }
        (self.memory_hits + self.disk_hits) as f64 / total as f64
    }
}

/// A two-tier cache that checks memory first, then falls back to disk.
///
/// Values are serialized via `serde_json` for disk storage and kept as raw
/// bytes in the memory tier (keyed by `namespace:key`).
pub struct CacheManager {
    memory: MemoryCache<Arc<[u8]>>,
    disk: DiskCache,
    disk_hits: AtomicU64,
    disk_misses: AtomicU64,
}

impl CacheManager {
    /// Creates a new two-tier cache from the given configuration.
    pub fn new(config: CacheConfig) -> Result<Self> {
        let memory = MemoryCache::new(config.memory_max_entries);
        let disk = DiskCache::new(config.disk_root, config.disk_max_bytes)?;
        Ok(Self {
            memory,
            disk,
            disk_hits: AtomicU64::new(0),
            disk_misses: AtomicU64::new(0),
        })
    }

    /// Retrieves a value, checking memory first, then disk.
    ///
    /// On a disk hit the value is promoted into the memory cache.
    pub fn get<V: DeserializeOwned>(&mut self, namespace: &str, key: &str) -> Result<Option<V>> {
        let mem_key = format!("{namespace}:{key}");

        if let Some(bytes) = self.memory.get(&mem_key) {
            let value: V = serde_json::from_slice(bytes.as_ref())?;
            return Ok(Some(value));
        }

        if let Some(bytes) = self.disk.get(namespace, key)? {
            self.disk_hits.fetch_add(1, Ordering::Relaxed);
            let bytes = Arc::<[u8]>::from(bytes);
            self.memory
                .insert(mem_key, bytes.clone(), CachePriority::Normal);
            let value: V = serde_json::from_slice(bytes.as_ref())?;
            return Ok(Some(value));
        }

        self.disk_misses.fetch_add(1, Ordering::Relaxed);
        Ok(None)
    }

    /// Stores a value in both the memory and disk caches.
    pub fn put<V: Serialize>(
        &mut self,
        namespace: &str,
        key: &str,
        value: &V,
        priority: CachePriority,
    ) -> Result<()> {
        let bytes = Arc::<[u8]>::from(serde_json::to_vec(value)?);
        let mem_key = format!("{namespace}:{key}");

        self.memory.insert(mem_key, bytes.clone(), priority);
        self.disk.put(namespace, key, bytes.as_ref())?;
        Ok(())
    }

    /// Removes a single entry from both tiers.
    pub fn invalidate(&mut self, namespace: &str, key: &str) -> Result<()> {
        let mem_key = format!("{namespace}:{key}");
        self.memory.remove(&mem_key);
        self.disk.remove(namespace, key)?;
        Ok(())
    }

    /// Removes all entries for a namespace from disk and clears the memory cache.
    pub fn invalidate_namespace(&mut self, namespace: &str) -> Result<()> {
        self.disk.clear_namespace(namespace)?;
        self.memory.clear();
        Ok(())
    }

    /// Returns current cache statistics.
    pub fn stats(&self) -> CacheStats {
        let disk_bytes = self.disk.total_size().unwrap_or(0);
        CacheStats {
            memory_hits: self.memory.hits(),
            memory_misses: self.memory.misses(),
            disk_hits: self.disk_hits.load(Ordering::Relaxed),
            disk_misses: self.disk_misses.load(Ordering::Relaxed),
            memory_entries: self.memory.len(),
            disk_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> CacheConfig {
        CacheConfig {
            memory_max_entries: 100,
            disk_root: tmp.path().join("cache"),
            disk_max_bytes: 1024 * 1024,
        }
    }

    #[test]
    fn put_and_get_string() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        mgr.put(
            "test",
            "greeting",
            &"hello world".to_string(),
            CachePriority::Normal,
        )
        .unwrap();
        let val: Option<String> = mgr.get("test", "greeting").unwrap();
        assert_eq!(val, Some("hello world".to_string()));
    }

    #[test]
    fn get_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();
        let val: Option<String> = mgr.get("ns", "nope").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn invalidate_removes_from_both_tiers() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        mgr.put("ns", "k", &42i32, CachePriority::Normal).unwrap();
        mgr.invalidate("ns", "k").unwrap();
        let val: Option<i32> = mgr.get("ns", "k").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn invalidate_namespace_clears_all() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        mgr.put("ns", "a", &1i32, CachePriority::Normal).unwrap();
        mgr.put("ns", "b", &2i32, CachePriority::Normal).unwrap();
        mgr.invalidate_namespace("ns").unwrap();

        let a: Option<i32> = mgr.get("ns", "a").unwrap();
        let b: Option<i32> = mgr.get("ns", "b").unwrap();
        assert_eq!(a, None);
        assert_eq!(b, None);
    }

    #[test]
    fn disk_fallback_on_memory_miss() {
        let tmp = TempDir::new().unwrap();
        let config = CacheConfig {
            memory_max_entries: 1,
            disk_root: tmp.path().join("cache"),
            disk_max_bytes: 1024 * 1024,
        };
        let mut mgr = CacheManager::new(config).unwrap();

        mgr.put("ns", "first", &"one".to_string(), CachePriority::Normal)
            .unwrap();
        mgr.put("ns", "second", &"two".to_string(), CachePriority::Normal)
            .unwrap();

        let val: Option<String> = mgr.get("ns", "first").unwrap();
        assert_eq!(val, Some("one".to_string()));
    }

    #[test]
    fn stats_tracking() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        mgr.put("ns", "k", &"v".to_string(), CachePriority::Normal)
            .unwrap();
        let _: Option<String> = mgr.get("ns", "k").unwrap();
        let _: Option<String> = mgr.get("ns", "miss").unwrap();

        let stats = mgr.stats();
        assert_eq!(stats.memory_hits, 1);
        assert!(stats.memory_misses >= 1);
        assert_eq!(stats.memory_entries, 1);
        assert!(stats.disk_bytes > 0);
    }

    #[test]
    fn hit_rate_calculation() {
        let stats = CacheStats {
            memory_hits: 3,
            memory_misses: 1,
            disk_hits: 1,
            disk_misses: 0,
            memory_entries: 0,
            disk_bytes: 0,
        };
        let rate = stats.hit_rate();
        assert!((rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn hit_rate_zero_when_no_lookups() {
        let stats = CacheStats {
            memory_hits: 0,
            memory_misses: 0,
            disk_hits: 0,
            disk_misses: 0,
            memory_entries: 0,
            disk_bytes: 0,
        };
        assert!((stats.hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn put_with_priority() {
        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        mgr.put("ns", "high", &"important".to_string(), CachePriority::High)
            .unwrap();
        mgr.put("ns", "low", &"disposable".to_string(), CachePriority::Low)
            .unwrap();

        let h: Option<String> = mgr.get("ns", "high").unwrap();
        let l: Option<String> = mgr.get("ns", "low").unwrap();
        assert_eq!(h, Some("important".to_string()));
        assert_eq!(l, Some("disposable".to_string()));
    }

    #[test]
    fn serialization_round_trip_complex_type() {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct Widget {
            name: String,
            count: u32,
            tags: Vec<String>,
        }

        let tmp = TempDir::new().unwrap();
        let mut mgr = CacheManager::new(test_config(&tmp)).unwrap();

        let widget = Widget {
            name: "gear".into(),
            count: 42,
            tags: vec!["metal".into(), "round".into()],
        };

        mgr.put("widgets", "gear", &widget, CachePriority::Normal)
            .unwrap();
        let restored: Option<Widget> = mgr.get("widgets", "gear").unwrap();
        assert_eq!(restored, Some(widget));
    }
}
