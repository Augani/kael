use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// A content-addressed disk cache organized by namespace.
///
/// Files are stored at `{root}/{namespace}/{hash_prefix}/{hash}` where the hash
/// is derived from the key using SHA-256.
pub struct DiskCache {
    root: PathBuf,
    max_bytes: u64,
}

impl DiskCache {
    /// Creates a new disk cache rooted at `root` with a size budget of `max_bytes`.
    pub fn new(root: PathBuf, max_bytes: u64) -> Result<Self> {
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create cache root: {}", root.display()))?;
        Ok(Self { root, max_bytes })
    }

    /// Retrieves cached data for the given namespace and key.
    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.entry_path(namespace, key);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(&path)
            .with_context(|| format!("failed to read cache entry: {}", path.display()))?;
        Ok(Some(data))
    }

    /// Stores data under the given namespace and key, evicting old entries if
    /// the size budget would be exceeded.
    pub fn put(&self, namespace: &str, key: &str, data: &[u8]) -> Result<()> {
        let data_len = data.len() as u64;
        if data_len > self.max_bytes {
            anyhow::bail!(
                "data size ({data_len} bytes) exceeds max cache size ({} bytes)",
                self.max_bytes
            );
        }

        let current = self.total_size()?;
        if current + data_len > self.max_bytes {
            let need_to_free = (current + data_len).saturating_sub(self.max_bytes);
            self.evict_by_size(current.saturating_sub(need_to_free))?;
        }

        let path = self.entry_path(namespace, key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create cache dir: {}", parent.display()))?;
        }
        fs::write(&path, data)
            .with_context(|| format!("failed to write cache entry: {}", path.display()))?;
        Ok(())
    }

    /// Removes a single cached entry.
    pub fn remove(&self, namespace: &str, key: &str) -> Result<()> {
        let path = self.entry_path(namespace, key);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove cache entry: {}", path.display()))?;
        }
        Ok(())
    }

    /// Removes all entries within a namespace.
    pub fn clear_namespace(&self, namespace: &str) -> Result<()> {
        let ns_path = self.root.join(namespace);
        if ns_path.exists() {
            fs::remove_dir_all(&ns_path)
                .with_context(|| format!("failed to clear namespace: {}", ns_path.display()))?;
        }
        Ok(())
    }

    /// Returns the total size in bytes of all cached files.
    pub fn total_size(&self) -> Result<u64> {
        if !self.root.exists() {
            return Ok(0);
        }
        dir_size(&self.root)
    }

    /// Returns the configured maximum size in bytes.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Evicts all entries older than `max_age`, returning bytes freed.
    pub fn evict_by_age(&self, max_age: Duration) -> Result<u64> {
        let cutoff = SystemTime::now()
            .checked_sub(max_age)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mut freed = 0u64;

        for entry in walk_files(&self.root)? {
            let metadata = fs::metadata(&entry)?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            if modified < cutoff {
                freed += metadata.len();
                fs::remove_file(&entry)?;
            }
        }
        self.cleanup_empty_dirs()?;
        Ok(freed)
    }

    /// Evicts oldest entries until total size is at or below `target_bytes`,
    /// returning bytes freed.
    pub fn evict_by_size(&self, target_bytes: u64) -> Result<u64> {
        let mut files = Vec::new();
        for path in walk_files(&self.root)? {
            let metadata = fs::metadata(&path)?;
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((path, metadata.len(), modified));
        }
        files.sort_by(|a, b| a.2.cmp(&b.2));

        let mut current_size = self.total_size()?;
        let mut freed = 0u64;

        for (path, size, _) in &files {
            if current_size <= target_bytes {
                break;
            }
            fs::remove_file(path)?;
            current_size = current_size.saturating_sub(*size);
            freed += size;
        }
        self.cleanup_empty_dirs()?;
        Ok(freed)
    }

    fn entry_path(&self, namespace: &str, key: &str) -> PathBuf {
        let hash = hash_key(key);
        let prefix = &hash[..2];
        self.root.join(namespace).join(prefix).join(&hash)
    }

    fn cleanup_empty_dirs(&self) -> Result<()> {
        remove_empty_dirs(&self.root)?;
        Ok(())
    }
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn dir_size(path: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in walk_files(path)? {
        total += fs::metadata(&entry)?.len();
    }
    Ok(total)
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
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
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
        assert_eq!(cache.total_size().unwrap(), 10);
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
        assert!(cache.total_size().unwrap() <= 4);
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
        assert!(cache.total_size().unwrap() <= 10);
    }

    #[test]
    fn content_addressed_path_is_deterministic() {
        let (_tmp, cache) = setup();
        cache.put("ns", "key", b"v1").unwrap();
        cache.put("ns", "key", b"v2").unwrap();
        assert_eq!(cache.get("ns", "key").unwrap(), Some(b"v2".to_vec()));
    }
}
