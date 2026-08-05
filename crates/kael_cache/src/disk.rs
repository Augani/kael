use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};
use tempfile::Builder;

const MAX_NAMESPACE_BYTES: usize = 128;
const MAX_KEY_BYTES: usize = 16 * 1024;

/// A key-addressed disk cache organized by namespace.
///
/// Files are stored at `{root}/{namespace}/{hash_prefix}/{hash}` where the hash
/// is derived from the key using SHA-256.
///
/// A cache root must have one live [`DiskCache`] owner. Instances and processes
/// that independently manage the same root do not coordinate their indexes.
pub struct DiskCache {
    root: PathBuf,
    max_bytes: u64,
    index: Mutex<DiskIndex>,
    operations: Mutex<()>,
}

#[derive(Debug, Clone, Copy)]
struct DiskEntry {
    size: u64,
    modified: SystemTime,
    insertion_order: u128,
}

#[derive(Debug, Default)]
struct DiskIndex {
    total_bytes: u64,
    entries: HashMap<PathBuf, DiskEntry>,
    max_entries_per_namespace: Option<usize>,
    next_insertion_order: u128,
}

impl DiskCache {
    /// Creates a new disk cache rooted at `root` with a size budget of `max_bytes`.
    pub fn new(root: PathBuf, max_bytes: u64) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create cache root: {}", root.display()))?;
        let cache = Self {
            index: Mutex::new(build_index(&root)?),
            operations: Mutex::new(()),
            root,
            max_bytes,
        };
        cache.enforce_startup_limits()?;
        Ok(cache)
    }

    /// Creates a new disk cache with both a byte budget and a per-namespace entry count limit.
    pub fn with_namespace_limit(
        root: PathBuf,
        max_bytes: u64,
        max_entries_per_namespace: usize,
    ) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create cache root: {}", root.display()))?;
        let mut index = build_index(&root)?;
        index.max_entries_per_namespace = Some(max_entries_per_namespace);
        let cache = Self {
            index: Mutex::new(index),
            operations: Mutex::new(()),
            root,
            max_bytes,
        };
        cache.enforce_startup_limits()?;
        Ok(cache)
    }

    /// Retrieves cached data for the given namespace and key.
    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let _operation = self.lock_operations();
        let path = self.entry_path(namespace, key)?;
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.remove_index_entry(&path);
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read cache entry: {}", path.display()));
            }
        };

        let metadata = file
            .metadata()
            .with_context(|| format!("failed to stat cache entry: {}", path.display()))?;
        if metadata.len() > self.max_bytes {
            self.discard_entry(&path)?;
            return Ok(None);
        }

        let mut data = Vec::new();
        file.take(self.max_bytes.saturating_add(1))
            .read_to_end(&mut data)
            .with_context(|| format!("failed to read cache entry: {}", path.display()))?;
        if u64::try_from(data.len()).unwrap_or(u64::MAX) > self.max_bytes {
            self.discard_entry(&path)?;
            return Ok(None);
        }

        let index_changed = self.reconcile_index_entry(
            path,
            u64::try_from(data.len()).unwrap_or(u64::MAX),
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        );
        if index_changed {
            self.enforce_limits_for_namespace(namespace)?;
        }

        Ok(Some(data))
    }

    /// Stores data under the given namespace and key, evicting old entries if
    /// the size budget would be exceeded.
    pub fn put(&self, namespace: &str, key: &str, data: &[u8]) -> Result<()> {
        let _operation = self.lock_operations();
        let data_len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if data_len > self.max_bytes {
            anyhow::bail!(
                "data size ({data_len} bytes) exceeds max cache size ({} bytes)",
                self.max_bytes
            );
        }

        let path = self.entry_path(namespace, key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create cache dir: {}", parent.display()))?;
        }
        write_atomically(&path, data)?;

        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to stat cache entry: {}", path.display()))?;
        self.record_index_entry(
            path,
            metadata.len(),
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        );
        self.enforce_limits_for_namespace(namespace)
    }

    /// Removes a single cached entry.
    pub fn remove(&self, namespace: &str, key: &str) -> Result<()> {
        let _operation = self.lock_operations();
        let path = self.entry_path(namespace, key)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                self.remove_index_entry(&path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.remove_index_entry(&path);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to remove cache entry: {}", path.display()));
            }
        }
        Ok(())
    }

    /// Removes all entries within a namespace.
    pub fn clear_namespace(&self, namespace: &str) -> Result<()> {
        let _operation = self.lock_operations();
        let ns_path = self.namespace_path(namespace)?;
        if ns_path.exists() {
            fs::remove_dir_all(&ns_path)
                .with_context(|| format!("failed to clear namespace: {}", ns_path.display()))?;
        }

        let mut index = self.lock_index();
        let removed_paths = index
            .entries
            .keys()
            .filter(|path| path.starts_with(&ns_path))
            .cloned()
            .collect::<Vec<_>>();
        for path in removed_paths {
            remove_index_entry_from(&mut index, &path);
        }
        Ok(())
    }

    /// Returns the total size in bytes of all cached files.
    pub fn total_size(&self) -> u64 {
        self.lock_index().total_bytes
    }

    /// Returns the configured maximum size in bytes.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Evicts all entries older than `max_age`, returning bytes freed.
    pub fn evict_by_age(&self, max_age: Duration) -> Result<u64> {
        let _operation = self.lock_operations();
        let cutoff = SystemTime::now()
            .checked_sub(max_age)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let candidates = {
            let index = self.lock_index();
            index
                .entries
                .iter()
                .filter(|(_, entry)| entry.modified < cutoff)
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>()
        };
        let freed = self.remove_paths(&candidates)?;
        self.cleanup_empty_dirs()?;
        Ok(freed)
    }

    /// Evicts oldest entries until total size is at or below `target_bytes`,
    /// returning bytes freed.
    pub fn evict_by_size(&self, target_bytes: u64) -> Result<u64> {
        let _operation = self.lock_operations();
        let candidates = {
            let index = self.lock_index();
            eviction_candidates(&index, target_bytes)
        };
        let freed = self.remove_paths(&candidates)?;
        self.cleanup_empty_dirs()?;
        Ok(freed)
    }

    fn entry_path(&self, namespace: &str, key: &str) -> Result<PathBuf> {
        validate_cache_address(namespace, key)?;
        let namespace_path = self.root.join(namespace);
        let hash = hash_key(key);
        let prefix = &hash[..2];
        Ok(namespace_path.join(prefix).join(&hash))
    }

    fn namespace_path(&self, namespace: &str) -> Result<PathBuf> {
        validate_namespace(namespace)?;
        Ok(self.root.join(namespace))
    }

    fn cleanup_empty_dirs(&self) -> Result<()> {
        remove_empty_dirs(&self.root)?;
        Ok(())
    }

    fn enforce_startup_limits(&self) -> Result<()> {
        self.evict_by_size(self.max_bytes)?;

        let Some(limit) = self.lock_index().max_entries_per_namespace else {
            return Ok(());
        };
        let _operation = self.lock_operations();
        let candidates = {
            let index = self.lock_index();
            let mut namespaces = index
                .entries
                .keys()
                .filter_map(|path| path.strip_prefix(&self.root).ok())
                .filter_map(|path| path.components().next())
                .filter_map(|component| match component {
                    std::path::Component::Normal(component) => Some(self.root.join(component)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            namespaces.sort_unstable();
            namespaces.dedup();
            namespaces
                .iter()
                .flat_map(|namespace| namespace_eviction_candidates(&index, namespace, limit))
                .collect::<Vec<_>>()
        };
        self.remove_paths(&candidates)?;
        self.cleanup_empty_dirs()
    }

    fn lock_index(&self) -> MutexGuard<'_, DiskIndex> {
        self.index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_operations(&self) -> MutexGuard<'_, ()> {
        self.operations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn record_index_entry(&self, path: PathBuf, size: u64, modified: SystemTime) {
        let mut index = self.lock_index();
        let insertion_order = index.next_insertion_order;
        index.next_insertion_order = index.next_insertion_order.saturating_add(1);
        if let Some(previous) = index.entries.insert(
            path,
            DiskEntry {
                size,
                modified,
                insertion_order,
            },
        ) {
            index.total_bytes = index.total_bytes.saturating_sub(previous.size);
        }
        index.total_bytes = index.total_bytes.saturating_add(size);
    }

    fn reconcile_index_entry(&self, path: PathBuf, size: u64, modified: SystemTime) -> bool {
        let mut index = self.lock_index();
        if let Some((previous_size, changed)) = index.entries.get_mut(&path).map(|entry| {
            let previous_size = entry.size;
            let changed = entry.size != size || entry.modified != modified;
            entry.size = size;
            entry.modified = modified;
            (previous_size, changed)
        }) {
            index.total_bytes = index.total_bytes.saturating_sub(previous_size);
            index.total_bytes = index.total_bytes.saturating_add(size);
            return changed;
        }

        let insertion_order = index.next_insertion_order;
        index.next_insertion_order = index.next_insertion_order.saturating_add(1);
        index.entries.insert(
            path,
            DiskEntry {
                size,
                modified,
                insertion_order,
            },
        );
        index.total_bytes = index.total_bytes.saturating_add(size);
        true
    }

    fn enforce_limits_for_namespace(&self, namespace: &str) -> Result<()> {
        let byte_candidates = {
            let index = self.lock_index();
            eviction_candidates(&index, self.max_bytes)
        };
        if !byte_candidates.is_empty() {
            self.remove_paths(&byte_candidates)?;
            self.cleanup_empty_dirs()?;
        }

        let namespace_candidates = {
            let index = self.lock_index();
            if let Some(limit) = index.max_entries_per_namespace {
                let namespace_path = self.namespace_path(namespace)?;
                namespace_eviction_candidates(&index, &namespace_path, limit)
            } else {
                Vec::new()
            }
        };
        if !namespace_candidates.is_empty() {
            self.remove_paths(&namespace_candidates)?;
            self.cleanup_empty_dirs()?;
        }

        Ok(())
    }

    fn remove_index_entry(&self, path: &Path) {
        let mut index = self.lock_index();
        remove_index_entry_from(&mut index, path);
    }

    fn remove_paths(&self, paths: &[PathBuf]) -> Result<u64> {
        let mut freed = 0u64;

        for path in paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to remove cache entry: {}", path.display())
                    });
                }
            }

            let mut index = self.lock_index();
            if let Some(entry) = remove_index_entry_from(&mut index, path) {
                freed = freed.saturating_add(entry.size);
            }
        }

        Ok(freed)
    }

    fn discard_entry(&self, path: &PathBuf) -> Result<()> {
        self.remove_paths(std::slice::from_ref(path))?;
        self.cleanup_empty_dirs()
    }
}

fn write_atomically(path: &Path, data: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .context("cache entry has no parent directory")?;
    let mut file = Builder::new()
        .prefix(".kael-cache.tmp-")
        .tempfile_in(directory)
        .with_context(|| format!("failed to create cache entry: {}", path.display()))?;
    file.write_all(data)
        .and_then(|()| file.flush())
        .with_context(|| format!("failed to write cache entry: {}", path.display()))?;
    file.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace cache entry: {}", path.display()))?;
    Ok(())
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn validate_namespace(namespace: &str) -> Result<()> {
    let bytes = namespace.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= MAX_NAMESPACE_BYTES
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && namespace != "."
        && namespace != ".."
        && !matches!(bytes.last(), Some(b'.'))
        && !is_windows_reserved_namespace(namespace)
        && std::path::Path::new(namespace)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    if !valid {
        anyhow::bail!(
            "cache namespace must be a portable ASCII path component of at most {MAX_NAMESPACE_BYTES} bytes"
        );
    }

    Ok(())
}

fn is_windows_reserved_namespace(namespace: &str) -> bool {
    let base = namespace
        .split('.')
        .next()
        .unwrap_or(namespace)
        .trim_end_matches([' ', '.']);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| base.eq_ignore_ascii_case(reserved))
    {
        return true;
    }

    let bytes = base.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

pub(crate) fn validate_cache_address(namespace: &str, key: &str) -> Result<()> {
    validate_namespace(namespace)?;
    anyhow::ensure!(
        !key.is_empty() && key.len() <= MAX_KEY_BYTES,
        "cache key must be between 1 and {MAX_KEY_BYTES} bytes"
    );
    Ok(())
}

fn build_index(root: &Path) -> Result<DiskIndex> {
    let mut index = DiskIndex::default();
    let mut files = Vec::new();

    for entry in walk_files(root)? {
        if !is_canonical_entry_path(root, &entry) {
            continue;
        }
        let metadata = fs::metadata(&entry)?;
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((entry, size, modified));
    }

    files.sort_by(
        |(left_path, _, left_modified), (right_path, _, right_modified)| {
            left_modified
                .cmp(right_modified)
                .then_with(|| left_path.cmp(right_path))
        },
    );

    for (entry, size, modified) in files {
        let insertion_order = index.next_insertion_order;
        index.next_insertion_order = index.next_insertion_order.saturating_add(1);
        index.total_bytes = index.total_bytes.saturating_add(size);
        index.entries.insert(
            entry,
            DiskEntry {
                size,
                modified,
                insertion_order,
            },
        );
    }

    Ok(index)
}

fn is_canonical_entry_path(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(namespace)) = components.next() else {
        return false;
    };
    let Some(std::path::Component::Normal(prefix)) = components.next() else {
        return false;
    };
    let Some(std::path::Component::Normal(hash)) = components.next() else {
        return false;
    };
    if components.next().is_some() {
        return false;
    }

    let (Some(namespace), Some(prefix), Some(hash)) =
        (namespace.to_str(), prefix.to_str(), hash.to_str())
    else {
        return false;
    };
    validate_namespace(namespace).is_ok()
        && prefix.len() == 2
        && prefix
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        && hash.len() == 64
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        && hash.starts_with(prefix)
}

fn walk_files(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).with_context(|| format!("failed to read dir: {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let ft = entry.file_type()?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".kael-cache.tmp-"))
            {
                let _ = fs::remove_file(entry.path());
                continue;
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

fn eviction_candidates(index: &DiskIndex, target_bytes: u64) -> Vec<PathBuf> {
    if index.total_bytes <= target_bytes {
        return Vec::new();
    }

    let mut entries = index
        .entries
        .iter()
        .map(|(path, entry)| {
            (
                path.clone(),
                entry.modified,
                entry.insertion_order,
                entry.size,
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(
        |(left_path, left_modified, left_order, _),
         (right_path, right_modified, right_order, _)| {
            left_modified
                .cmp(right_modified)
                .then_with(|| left_order.cmp(right_order))
                .then_with(|| left_path.cmp(right_path))
        },
    );

    let mut current_size = index.total_bytes;
    let mut candidates = Vec::new();
    for (path, _, _, size) in entries {
        if current_size <= target_bytes {
            break;
        }
        current_size = current_size.saturating_sub(size);
        candidates.push(path);
    }

    candidates
}

fn namespace_eviction_candidates(index: &DiskIndex, ns_path: &Path, limit: usize) -> Vec<PathBuf> {
    let mut ns_entries: Vec<(PathBuf, SystemTime, u128)> = index
        .entries
        .iter()
        .filter(|(path, _)| path.starts_with(ns_path))
        .map(|(path, entry)| (path.clone(), entry.modified, entry.insertion_order))
        .collect();

    if ns_entries.len() <= limit {
        return vec![];
    }

    ns_entries.sort_by(
        |(left_path, left_modified, left_order), (right_path, right_modified, right_order)| {
            left_modified
                .cmp(right_modified)
                .then_with(|| left_order.cmp(right_order))
                .then_with(|| left_path.cmp(right_path))
        },
    );
    let excess = ns_entries.len() - limit;
    ns_entries
        .into_iter()
        .take(excess)
        .map(|(path, _, _)| path)
        .collect()
}

fn remove_index_entry_from(index: &mut DiskIndex, path: &Path) -> Option<DiskEntry> {
    let removed = index.entries.remove(path);
    if let Some(entry) = removed {
        index.total_bytes = index.total_bytes.saturating_sub(entry.size);
        Some(entry)
    } else {
        None
    }
}

fn remove_empty_dirs(root: &std::path::Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_empty_dirs(&entry.path())?;
            if fs::read_dir(entry.path())?.next().is_none() {
                let _ = fs::remove_dir(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup() -> (TempDir, DiskCache) {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path().to_path_buf(), 1024 * 1024).unwrap();
        (tmp, cache)
    }

    #[test]
    fn put_and_get() {
        let (_tmp, cache) = setup();
        cache.put("ns", "key1", b"hello").unwrap();
        let data = cache.get("ns", "key1").unwrap();
        assert_eq!(data, Some(b"hello".to_vec()));
    }

    #[test]
    fn get_missing_returns_none() {
        let (_tmp, cache) = setup();
        assert_eq!(cache.get("ns", "missing").unwrap(), None);
    }

    #[test]
    fn remove_entry() {
        let (_tmp, cache) = setup();
        cache.put("ns", "key1", b"data").unwrap();
        cache.remove("ns", "key1").unwrap();
        assert_eq!(cache.get("ns", "key1").unwrap(), None);
    }

    #[test]
    fn remove_missing_is_ok() {
        let (_tmp, cache) = setup();
        cache.remove("ns", "nope").unwrap();
    }

    #[test]
    fn clear_namespace() {
        let (_tmp, cache) = setup();
        cache.put("ns1", "a", b"1").unwrap();
        cache.put("ns1", "b", b"2").unwrap();
        cache.put("ns2", "c", b"3").unwrap();

        cache.clear_namespace("ns1").unwrap();
        assert_eq!(cache.get("ns1", "a").unwrap(), None);
        assert_eq!(cache.get("ns1", "b").unwrap(), None);
        assert_eq!(cache.get("ns2", "c").unwrap(), Some(b"3".to_vec()));
    }

    #[test]
    fn total_size_reflects_stored_data() {
        let (_tmp, cache) = setup();
        cache.put("ns", "k1", b"12345").unwrap();
        cache.put("ns", "k2", b"67890").unwrap();
        assert_eq!(cache.total_size(), 10);
    }

    #[test]
    fn evict_by_age() {
        let (_tmp, cache) = setup();
        cache.put("ns", "old", b"data").unwrap();
        let freed = cache.evict_by_age(Duration::from_secs(0)).unwrap();
        assert!(freed > 0);
        assert_eq!(cache.get("ns", "old").unwrap(), None);
    }

    #[test]
    fn evict_by_size() {
        let (_tmp, cache) = setup();
        cache.put("ns", "a", b"aaaa").unwrap();
        cache.put("ns", "b", b"bbbb").unwrap();
        let freed = cache.evict_by_size(4).unwrap();
        assert!(freed >= 4);
        assert!(cache.total_size() <= 4);
    }

    #[test]
    fn rejects_data_exceeding_max_bytes() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path().to_path_buf(), 5).unwrap();
        let result = cache.put("ns", "big", b"toolarge");
        assert!(result.is_err());
    }

    #[test]
    fn auto_evicts_on_put_when_full() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path().to_path_buf(), 10).unwrap();
        cache.put("ns", "a", b"12345").unwrap();
        cache.put("ns", "b", b"67890").unwrap();
        cache.put("ns", "c", b"abcde").unwrap();
        assert!(cache.total_size() <= 10);
    }

    #[test]
    fn content_addressed_path_is_deterministic() {
        let (_tmp, cache) = setup();
        cache.put("ns", "key", b"v1").unwrap();
        cache.put("ns", "key", b"v2").unwrap();
        assert_eq!(cache.get("ns", "key").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn namespace_capacity_evicts_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            DiskCache::with_namespace_limit(dir.path().to_path_buf(), 1_000_000, 3).unwrap();
        cache.put("ns", "key1", b"data1").unwrap();
        cache.put("ns", "key2", b"data2").unwrap();
        cache.put("ns", "key3", b"data3").unwrap();
        cache.put("ns", "key4", b"data4").unwrap();
        assert!(cache.get("ns", "key1").unwrap().is_none());
        assert!(cache.get("ns", "key4").unwrap().is_some());
    }

    #[test]
    fn namespace_capacity_breaks_timestamp_ties_by_insertion_order() {
        let namespace = PathBuf::from("cache/ns");
        let first = namespace.join("first");
        let second = namespace.join("second");
        let mut index = DiskIndex::default();
        index.entries.insert(
            first.clone(),
            DiskEntry {
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
                insertion_order: 0,
            },
        );
        index.entries.insert(
            second,
            DiskEntry {
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
                insertion_order: 1,
            },
        );

        assert_eq!(
            namespace_eviction_candidates(&index, &namespace, 1),
            vec![first]
        );
    }

    #[test]
    fn concurrent_writers_do_not_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path().to_path_buf(), 10_000_000).unwrap());
        let handles: Vec<_> = (0..8)
            .map(|thread_id| {
                let cache = cache.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let key = format!("t{thread_id}-k{i}");
                        let data = format!("data-{thread_id}-{i}");
                        cache.put("stress", &key, data.as_bytes()).unwrap();
                        let read = cache.get("stress", &key).unwrap();
                        assert!(read.is_some(), "missing key {key}");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn namespace_cannot_escape_root() {
        let (tmp, cache) = setup();
        let sibling = tmp.path().parent().unwrap().join("kael-cache-escape-check");
        fs::write(&sibling, b"keep").unwrap();

        assert!(
            cache
                .put("../kael-cache-escape-check", "key", b"bad")
                .is_err()
        );
        assert!(cache.clear_namespace("../kael-cache-escape-check").is_err());
        assert_eq!(fs::read(&sibling).unwrap(), b"keep");

        fs::remove_file(sibling).ok();
    }

    #[test]
    fn cache_addresses_are_portable_and_bounded() {
        let (_tmp, cache) = setup();
        for namespace in [
            "",
            ".",
            "trailing.",
            "../escape",
            "with space",
            "windows:drive",
            "CON",
            "con.txt",
            "COM1",
            "lpt9.cache",
        ] {
            assert!(cache.put(namespace, "key", b"value").is_err());
        }
        for namespace in ["cache-v1", ".hidden", "COM10", "LPT0"] {
            assert!(validate_namespace(namespace).is_ok());
        }
        assert!(
            cache
                .put(&"n".repeat(MAX_NAMESPACE_BYTES + 1), "key", b"value")
                .is_err()
        );
        assert!(cache.put("ns", "", b"value").is_err());
        assert!(
            cache
                .put("ns", &"k".repeat(MAX_KEY_BYTES + 1), b"value")
                .is_err()
        );
    }

    #[test]
    fn reopening_enforces_existing_byte_and_namespace_budgets() {
        let directory = tempfile::tempdir().unwrap();
        {
            let cache = DiskCache::new(directory.path().to_path_buf(), 1_000_000).unwrap();
            cache.put("one", "a", b"aaaaa").unwrap();
            cache.put("one", "b", b"bbbbb").unwrap();
            cache.put("two", "a", b"ccccc").unwrap();
            cache.put("two", "b", b"ddddd").unwrap();
        }

        let cache = DiskCache::with_namespace_limit(directory.path().to_path_buf(), 15, 1).unwrap();
        assert!(cache.total_size() <= 10);
        for namespace in ["one", "two"] {
            let remaining = ["a", "b"]
                .into_iter()
                .filter(|key| cache.get(namespace, key).unwrap().is_some())
                .count();
            assert!(remaining <= 1);
        }
    }

    #[test]
    fn stale_temporary_files_are_not_indexed() {
        let tmp = TempDir::new().unwrap();
        let namespace = tmp.path().join("ns");
        fs::create_dir_all(&namespace).unwrap();
        fs::write(namespace.join(".kael-cache.tmp-stale"), b"partial").unwrap();

        let cache = DiskCache::new(tmp.path().to_path_buf(), 1024).unwrap();
        assert_eq!(cache.total_size(), 0);
        assert!(!namespace.exists() || fs::read_dir(namespace).unwrap().next().is_none());
    }

    #[test]
    fn startup_indexes_only_canonical_cache_entries() {
        let tmp = TempDir::new().unwrap();
        let hash = hash_key("kept");
        let canonical = tmp.path().join("ns").join(&hash[..2]).join(&hash);
        fs::create_dir_all(canonical.parent().unwrap()).unwrap();
        fs::write(&canonical, b"kept").unwrap();

        let root_junk = tmp.path().join("README.txt");
        let wrong_prefix_name = if &hash[..2] == "ff" { "00" } else { "ff" };
        let wrong_prefix = tmp.path().join("ns").join(wrong_prefix_name).join(&hash);
        let nested_junk = tmp.path().join("ns").join(&hash[..2]).join("extra/file");
        fs::write(&root_junk, b"ignore").unwrap();
        fs::create_dir_all(wrong_prefix.parent().unwrap()).unwrap();
        fs::write(&wrong_prefix, b"ignore").unwrap();
        fs::create_dir_all(nested_junk.parent().unwrap()).unwrap();
        fs::write(&nested_junk, b"ignore").unwrap();

        let cache = DiskCache::new(tmp.path().to_path_buf(), 1024).unwrap();
        assert_eq!(cache.total_size(), 4);
        assert_eq!(cache.get("ns", "kept").unwrap(), Some(b"kept".to_vec()));
        assert!(root_junk.exists());
        assert!(wrong_prefix.exists());
        assert!(nested_junk.exists());
    }

    #[test]
    fn oversized_external_replacements_are_discarded_without_loading() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path().to_path_buf(), 8).unwrap();
        cache.put("ns", "key", b"small").unwrap();
        let path = cache.entry_path("ns", "key").unwrap();
        fs::write(&path, vec![b'x'; 1024]).unwrap();

        assert_eq!(cache.get("ns", "key").unwrap(), None);
        assert!(!path.exists());
        assert_eq!(cache.total_size(), 0);
    }

    #[test]
    fn external_size_changes_are_reconciled_with_the_budget() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path().to_path_buf(), 10).unwrap();
        cache.put("ns", "old", b"1234").unwrap();
        cache.put("ns", "changed", b"5678").unwrap();
        let changed_path = cache.entry_path("ns", "changed").unwrap();
        fs::write(&changed_path, b"12345678").unwrap();

        assert_eq!(
            cache.get("ns", "changed").unwrap(),
            Some(b"12345678".to_vec())
        );
        assert!(cache.total_size() <= cache.max_bytes());
        assert_eq!(cache.get("ns", "old").unwrap(), None);
    }
}
