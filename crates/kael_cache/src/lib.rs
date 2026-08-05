#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod disk;
mod manager;
mod memory;

pub use disk::DiskCache;
pub use manager::{CacheConfig, CacheManager, CacheStats};
pub use memory::{CachePriority, MemoryCache};
