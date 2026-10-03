//! Measures bookkeeping independently of native GPU work and owner callbacks.
use kael::GpuMemoryManager;
use std::{hint::black_box, time::Instant};

fn main() {
    const RESOURCE_COUNT: usize = 30_000;
    const SAMPLES: usize = 9;
    let mut times = [Vec::new(), Vec::new(), Vec::new()];
    for _ in 0..SAMPLES {
        let mut manager = GpuMemoryManager::new(u64::MAX);
        let start = Instant::now();
        let ids: Vec<_> = (0..RESOURCE_COUNT)
            .map(|_| manager.register_checked(1, || {}).unwrap())
            .collect();
        times[0].push(start.elapsed());

        let start = Instant::now();
        for id in &ids {
            assert!(black_box(&mut manager).touch(*id));
        }
        times[1].push(start.elapsed());

        let start = Instant::now();
        manager.set_budget(0);
        assert_eq!(manager.evict_to_budget(), RESOURCE_COUNT);
        assert_eq!(manager.used_bytes(), 0);
        assert_eq!(manager.tracked_count(), 0);
        times[2].push(start.elapsed());
        black_box(manager);
    }
    for (name, samples) in ["register", "touch", "evict"].into_iter().zip(&mut times) {
        samples.sort_unstable();
        println!(
            "{name}: resources={RESOURCE_COUNT} median_ms={:.3} min_ms={:.3}",
            samples[SAMPLES / 2].as_secs_f64() * 1000.0,
            samples[0].as_secs_f64() * 1000.0,
        );
    }
}
