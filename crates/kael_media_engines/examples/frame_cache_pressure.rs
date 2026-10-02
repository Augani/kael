//! Measures bulk decoded-frame eviction with growing working sets.
//!
//! Run with `cargo run --release -p kael_media_engines --example frame_cache_pressure`.
//! This measures cache bookkeeping, not media decoding, GPU memory, or end-to-end frame time.

use kael_media_engines::frame_cache::{FrameCache, FrameKey};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    for count in [4096, 16_384, 65_536] {
        let mut samples = Vec::new();
        for _ in 0..7 {
            let mut cache = FrameCache::with_entry_limit(count as u64, count);
            let small = Arc::<[u8]>::from([0]);
            for index in 0..count {
                assert!(cache.insert(FrameKey::new(1, index as u64), small.clone()));
            }
            let large: Arc<[u8]> = vec![0; count].into();
            let started = Instant::now();
            assert!(cache.insert(FrameKey::new(2, 0), large));
            samples.push(started.elapsed());
            assert_eq!(cache.len(), 1);
            assert_eq!(cache.used_bytes(), count as u64);
            assert_eq!(cache.stats().evictions, count as u64);
            black_box(cache);
        }
        samples.sort_unstable();
        println!(
            "{count} evictions: median {:.3} ms, {:.1} ns/eviction",
            samples[3].as_secs_f64() * 1000.0,
            samples[3].as_secs_f64() * 1_000_000_000.0 / count as f64,
        );
    }
}
