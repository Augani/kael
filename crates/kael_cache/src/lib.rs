#![deny(missing_docs)]

//! General-purpose application cache layer for Kael.
//!
//! Provides a two-tier caching system with an in-memory LRU cache backed by
//! content-addressed disk storage. Use [`CacheManager`] for the combined
//! experience, or the individual [`MemoryCache`] and [`DiskCache`] types
//! when only one tier is needed.

mod disk;
mod manager;
mod memory;

pub use disk::DiskCache;
pub use manager::{CacheConfig, CacheManager, CacheStats};
pub use memory::{CachePriority, MemoryCache};
