# kael_cache

Bounded caching for data that applications can recreate.

`CacheManager` combines a priority-aware in-memory LRU with a key-addressed
disk tier. `MemoryCache` and `DiskCache` are also available
independently when an application needs only one tier. The crate has no UI
dependency and works with Kael's runtime primitives or another interface layer.

## Quick start

```rust,no_run
use kael_cache::{CacheConfig, CacheManager, CachePriority};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cache = CacheManager::new(CacheConfig {
        memory_max_entries: 256,
        disk_root: std::env::temp_dir().join("kael-thumbnail-cache"),
        disk_max_bytes: 512 * 1024 * 1024,
    })?;

    cache.put("thumbnails", "document-42", &vec![1_u8, 2, 3], CachePriority::Normal)?;
    let thumbnail = cache.get::<Vec<u8>>("thumbnails", "document-42")?;
    assert_eq!(thumbnail, Some(vec![1, 2, 3]));
    Ok(())
}
```

Namespaces are restricted to short portable path components, and logical keys
are bounded before hashing. Disk keys are SHA-256-addressed, replacements are
atomic on supported platforms, and byte and per-namespace entry budgets evict
the oldest entries deterministically, including when an existing cache is
opened. Cached values use JSON serialization; secrets and irreplaceable user
data belong in their dedicated storage systems instead.

See the [Kael guide](https://augani.github.io/kael/) for framework-level usage.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
