//! GPU memory budgeting and eviction.
//!
//! [`GpuMemoryBudget::query`] reports the device's working-set budget and
//! current usage, and [`GpuMemoryManager`] tracks evictable GPU resources and
//! sheds the least-recently-used ones to stay within a byte budget. Together
//! they are the foundation the tiered frame cache builds on: register every
//! cached GPU texture/buffer with the manager, `touch` it on use, and call
//! [`GpuMemoryManager::ensure_available`] before a large allocation.

/// A snapshot of GPU memory budget and usage for the default device, with a real
/// query on every backend (Metal / DXGI / Vulkan) via [`kael_gpu_budget`].
pub use kael_gpu_budget::GpuMemoryBudget;

/// Identifier for a resource registered with a [`GpuMemoryManager`].
pub type GpuResourceId = u64;

struct Tracked {
    id: GpuResourceId,
    bytes: u64,
    last_used: u64,
    on_evict: Box<dyn FnMut() + Send + 'static>,
}

/// Tracks evictable GPU resources and sheds the least-recently-used ones to keep
/// total tracked bytes within a soft budget.
///
/// The manager never frees memory itself; it invokes each resource's eviction
/// callback so the owner can drop the underlying GPU object.
pub struct GpuMemoryManager {
    budget_bytes: u64,
    used_bytes: u64,
    tick: u64,
    next_id: GpuResourceId,
    tracked: Vec<Tracked>,
}

impl GpuMemoryManager {
    /// Create a manager with the given soft byte budget.
    pub fn new(budget_bytes: u64) -> Self {
        Self {
            budget_bytes,
            used_bytes: 0,
            tick: 0,
            next_id: 1,
            tracked: Vec::new(),
        }
    }

    /// The current soft byte budget.
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Update the soft byte budget. Does not evict on its own; call
    /// [`Self::evict_to_budget`] afterwards if desired.
    pub fn set_budget(&mut self, budget_bytes: u64) {
        self.budget_bytes = budget_bytes;
    }

    /// Total bytes currently tracked.
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Bytes available before exceeding the budget (saturating at zero).
    pub fn available_bytes(&self) -> u64 {
        self.budget_bytes.saturating_sub(self.used_bytes)
    }

    /// Number of resources currently tracked.
    pub fn tracked_count(&self) -> usize {
        self.tracked.len()
    }

    /// Register an evictable resource costing `bytes`, returning its id. The
    /// `on_evict` callback is invoked if the resource is later evicted.
    pub fn register(
        &mut self,
        bytes: u64,
        on_evict: impl FnMut() + Send + 'static,
    ) -> GpuResourceId {
        let id = self.next_id;
        self.next_id += 1;
        self.tick += 1;
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.tracked.push(Tracked {
            id,
            bytes,
            last_used: self.tick,
            on_evict: Box::new(on_evict),
        });
        id
    }

    /// Mark a resource as most-recently-used. Returns `false` if unknown.
    pub fn touch(&mut self, id: GpuResourceId) -> bool {
        self.tick += 1;
        let tick = self.tick;
        if let Some(resource) = self.tracked.iter_mut().find(|resource| resource.id == id) {
            resource.last_used = tick;
            true
        } else {
            false
        }
    }

    /// Stop tracking a resource without invoking its eviction callback (the owner
    /// is freeing it directly). Returns `false` if unknown.
    pub fn release(&mut self, id: GpuResourceId) -> bool {
        if let Some(index) = self.tracked.iter().position(|resource| resource.id == id) {
            let resource = self.tracked.remove(index);
            self.used_bytes = self.used_bytes.saturating_sub(resource.bytes);
            true
        } else {
            false
        }
    }

    /// Evict least-recently-used resources until tracked bytes fit the budget.
    /// Returns the number of resources evicted.
    pub fn evict_to_budget(&mut self) -> usize {
        self.evict_until(self.budget_bytes)
    }

    /// Evict least-recently-used resources until at least `bytes` are free within
    /// the budget. Returns the number of resources evicted.
    pub fn ensure_available(&mut self, bytes: u64) -> usize {
        let target = self.budget_bytes.saturating_sub(bytes);
        self.evict_until(target)
    }

    fn evict_until(&mut self, target_used: u64) -> usize {
        let mut evicted = 0;
        while self.used_bytes > target_used {
            let Some(index) = self.least_recently_used_index() else {
                break;
            };
            let mut resource = self.tracked.remove(index);
            self.used_bytes = self.used_bytes.saturating_sub(resource.bytes);
            (resource.on_evict)();
            evicted += 1;
        }
        evicted
    }

    fn least_recently_used_index(&self) -> Option<usize> {
        self.tracked
            .iter()
            .enumerate()
            .min_by_key(|(_, resource)| resource.last_used)
            .map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn counting_callback() -> (Arc<AtomicU64>, impl FnMut() + Send + 'static) {
        let counter = Arc::new(AtomicU64::new(0));
        let handle = counter.clone();
        (counter, move || {
            handle.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn evicts_least_recently_used_first() {
        let mut manager = GpuMemoryManager::new(100);
        let (count_a, evict_a) = counting_callback();
        let (count_b, evict_b) = counting_callback();
        let (_count_c, evict_c) = counting_callback();

        let a = manager.register(40, evict_a);
        let _b = manager.register(40, evict_b);
        manager.register(40, evict_c);

        manager.touch(a);

        let evicted = manager.evict_to_budget();
        assert_eq!(evicted, 1);
        assert_eq!(
            count_b.load(Ordering::SeqCst),
            1,
            "B was least recently used"
        );
        assert_eq!(count_a.load(Ordering::SeqCst), 0, "A was touched, kept");
        assert_eq!(manager.used_bytes(), 80);
    }

    #[test]
    fn ensure_available_frees_enough_space() {
        let mut manager = GpuMemoryManager::new(100);
        manager.register(30, || {});
        manager.register(30, || {});
        manager.register(30, || {});
        assert_eq!(manager.used_bytes(), 90);

        let evicted = manager.ensure_available(50);
        assert!(manager.used_bytes() <= 50, "should free room for 50 bytes");
        assert!(evicted >= 1);
    }

    #[test]
    fn release_does_not_invoke_eviction_callback() {
        let mut manager = GpuMemoryManager::new(100);
        let (count, evict) = counting_callback();
        let id = manager.register(40, evict);

        assert!(manager.release(id));
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(manager.used_bytes(), 0);
        assert!(!manager.release(id));
    }

    #[test]
    fn evict_to_budget_is_noop_within_budget() {
        let mut manager = GpuMemoryManager::new(100);
        manager.register(30, || panic!("should not evict"));
        manager.register(30, || panic!("should not evict"));
        assert_eq!(manager.evict_to_budget(), 0);
        assert_eq!(manager.used_bytes(), 60);
    }

    #[test]
    fn budget_snapshot_math() {
        let budget = GpuMemoryBudget {
            total_bytes: 1000,
            used_bytes: 250,
            has_unified_memory: true,
        };
        assert_eq!(budget.available_bytes(), 750);
        assert!((budget.utilization() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn query_is_callable() {
        let _ = GpuMemoryBudget::query();
    }
}
