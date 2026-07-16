//! GPU memory budgeting and eviction.
//!
//! [`GpuMemoryBudget::query`] reports the device's working-set budget and
//! current usage, and [`GpuMemoryManager`] tracks evictable GPU resources and
//! sheds the least-recently-used ones to stay within a byte budget. Together
//! they are the foundation the tiered frame cache builds on: register every
//! cached GPU texture/buffer with the manager, `touch` it on use, and call
//! [`GpuMemoryManager::ensure_available`] before a large allocation.

use crate::{App, BorrowAppContext, Global, SubscriberSet, Subscription};
use anyhow::Result;

const MAX_TRACKED_GPU_RESOURCES: usize = 65_536;

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
    /// `on_evict` callback is invoked if the resource is later evicted. Returns
    /// the reserved zero identifier when validation fails; use
    /// [`Self::register_checked`] to inspect the error.
    pub fn register(
        &mut self,
        bytes: u64,
        on_evict: impl FnMut() + Send + 'static,
    ) -> GpuResourceId {
        self.register_checked(bytes, on_evict).unwrap_or(0)
    }

    /// Register a resource while validating capacity and byte accounting.
    pub fn register_checked(
        &mut self,
        bytes: u64,
        on_evict: impl FnMut() + Send + 'static,
    ) -> Result<GpuResourceId> {
        anyhow::ensure!(
            self.tracked.len() < MAX_TRACKED_GPU_RESOURCES,
            "GPU memory manager cannot exceed {MAX_TRACKED_GPU_RESOURCES} resources"
        );
        let used_bytes = self
            .used_bytes
            .checked_add(bytes)
            .ok_or_else(|| anyhow::anyhow!("tracked GPU byte count overflowed"))?;
        let id = self.allocate_id()?;
        let tick = self.next_tick();
        self.used_bytes = used_bytes;
        self.tracked.push(Tracked {
            id,
            bytes,
            last_used: tick,
            on_evict: Box::new(on_evict),
        });
        Ok(id)
    }

    /// Mark a resource as most-recently-used. Returns `false` if unknown.
    pub fn touch(&mut self, id: GpuResourceId) -> bool {
        let Some(index) = self.tracked.iter().position(|resource| resource.id == id) else {
            return false;
        };
        let tick = self.next_tick();
        self.tracked[index].last_used = tick;
        true
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
        self.evict_until(self.budget_bytes).0
    }

    /// Evict to budget and report if any owner callback panicked.
    pub fn evict_to_budget_checked(&mut self) -> Result<usize> {
        let (evicted, callback_panicked) = self.evict_until(self.budget_bytes);
        anyhow::ensure!(!callback_panicked, "GPU eviction callback panicked");
        Ok(evicted)
    }

    /// Evict least-recently-used resources until at least `bytes` are free within
    /// the budget. Returns the number of resources evicted.
    pub fn ensure_available(&mut self, bytes: u64) -> usize {
        let target = self.budget_bytes.saturating_sub(bytes);
        self.evict_until(target).0
    }

    /// Evict until enough room is available and report callback failure.
    pub fn ensure_available_checked(&mut self, bytes: u64) -> Result<usize> {
        let target = self.budget_bytes.saturating_sub(bytes);
        let (evicted, callback_panicked) = self.evict_until(target);
        anyhow::ensure!(!callback_panicked, "GPU eviction callback panicked");
        Ok(evicted)
    }

    fn evict_until(&mut self, target_used: u64) -> (usize, bool) {
        let mut evicted = 0;
        let mut callback_panicked = false;
        while self.used_bytes > target_used {
            let Some(index) = self.least_recently_used_index() else {
                break;
            };
            let mut resource = self.tracked.remove(index);
            self.used_bytes = self.used_bytes.saturating_sub(resource.bytes);
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (resource.on_evict)()))
                .is_err()
            {
                callback_panicked = true;
            }
            evicted += 1;
        }
        (evicted, callback_panicked)
    }

    fn least_recently_used_index(&self) -> Option<usize> {
        self.tracked
            .iter()
            .enumerate()
            .min_by_key(|(_, resource)| resource.last_used)
            .map(|(index, _)| index)
    }

    fn allocate_id(&mut self) -> Result<GpuResourceId> {
        for _ in 0..=self.tracked.len() {
            let id = self.next_id.max(1);
            self.next_id = id.wrapping_add(1).max(1);
            if self.tracked.iter().all(|resource| resource.id != id) {
                return Ok(id);
            }
        }
        anyhow::bail!("GPU resource identifier space is exhausted")
    }

    fn next_tick(&mut self) -> u64 {
        if self.tick == u64::MAX {
            let mut order = (0..self.tracked.len()).collect::<Vec<_>>();
            order.sort_by_key(|index| self.tracked[*index].last_used);
            for (rank, index) in order.into_iter().enumerate() {
                self.tracked[index].last_used = rank as u64 + 1;
            }
            self.tick = self.tracked.len() as u64;
        }
        self.tick += 1;
        self.tick
    }
}

/// Coarse GPU-memory pressure level derived from device budget utilization.
///
/// Apps subscribe via [`App::on_memory_pressure`] and shed their own caches when
/// the level rises; the framework also evicts everything registered with the
/// app's [`GpuMemoryManager`] down to budget on [`MemoryPressureLevel::Critical`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryPressureLevel {
    /// Comfortable headroom; no action needed.
    Normal,
    /// Approaching the budget; shed non-essential caches.
    Warning,
    /// At or over the budget; shed aggressively to avoid eviction by the OS.
    Critical,
}

impl MemoryPressureLevel {
    /// Map a `[0.0, 1.0+]` budget utilization onto a pressure level
    /// (`>= 0.90` critical, `>= 0.75` warning, else normal).
    pub fn from_utilization(utilization: f64) -> Self {
        if !utilization.is_finite() || utilization < 0.0 {
            return Self::Critical;
        }
        if utilization >= 0.90 {
            Self::Critical
        } else if utilization >= 0.75 {
            Self::Warning
        } else {
            Self::Normal
        }
    }

    /// Derive the pressure level from a device memory budget snapshot.
    pub fn from_budget(budget: &GpuMemoryBudget) -> Self {
        Self::from_utilization(budget.utilization())
    }
}

type MemoryPressureSubscriber = Box<dyn FnMut(MemoryPressureLevel, &mut App) + 'static>;

struct GpuMemoryRuntime {
    manager: GpuMemoryManager,
    last_level: MemoryPressureLevel,
    subscribers: SubscriberSet<(), MemoryPressureSubscriber>,
}

impl Default for GpuMemoryRuntime {
    fn default() -> Self {
        Self {
            manager: GpuMemoryManager::new(u64::MAX),
            last_level: MemoryPressureLevel::Normal,
            subscribers: SubscriberSet::new(),
        }
    }
}

impl Global for GpuMemoryRuntime {}

impl App {
    /// Set the soft GPU-memory budget the app's [`GpuMemoryManager`] enforces over
    /// resources registered through [`App::with_gpu_memory_manager`]. Does not evict
    /// on its own; eviction happens on [`App::notify_memory_pressure`]
    /// ([`MemoryPressureLevel::Critical`]) or an explicit
    /// [`GpuMemoryManager::evict_to_budget`].
    pub fn set_gpu_budget(&mut self, budget_bytes: u64) {
        self.update_default_global::<GpuMemoryRuntime, _>(|runtime, _| {
            runtime.manager.set_budget(budget_bytes);
        });
    }

    /// Set a soft glyph/sprite-atlas byte budget across all currently open windows. When
    /// set, each window's renderer evicts least-recently-used atlas tiles down to the budget
    /// at the end of every frame, bounding glyph-atlas memory growth on long-running,
    /// text-churning UIs. Size it against the device with [`App::gpu_memory_budget`]; `None`
    /// disables eviction (the default). Currently honored on the Metal backend.
    pub fn set_atlas_byte_budget(&mut self, budget: Option<u64>) {
        for window in self.windows.values().flatten() {
            window.set_atlas_byte_budget(budget);
        }
    }

    /// Run `f` against the app-wide [`GpuMemoryManager`] to register, touch, release,
    /// or evict GPU resources. This is the single point subsystems and apps use to put
    /// their evictable GPU caches under the shared budget.
    pub fn with_gpu_memory_manager<R>(&mut self, f: impl FnOnce(&mut GpuMemoryManager) -> R) -> R {
        self.update_default_global::<GpuMemoryRuntime, _>(|runtime, _| f(&mut runtime.manager))
    }

    /// Subscribe to GPU-memory pressure transitions. The callback fires when the level
    /// changes (via [`App::poll_memory_pressure`] or [`App::notify_memory_pressure`]),
    /// letting the app shed its own caches. Drop the returned [`Subscription`] to
    /// unsubscribe, or call [`Subscription::detach`] to keep it for the app's lifetime.
    pub fn on_memory_pressure(
        &mut self,
        callback: impl FnMut(MemoryPressureLevel, &mut App) + 'static,
    ) -> Subscription {
        self.update_default_global::<GpuMemoryRuntime, _>(|runtime, _| {
            let (subscription, activate) = runtime.subscribers.insert((), Box::new(callback));
            activate();
            subscription
        })
    }

    /// Dispatch a pressure level to all subscribers, recording it as the current level.
    /// A [`MemoryPressureLevel::Critical`] notification also evicts every registered GPU
    /// resource down to budget before the subscribers run.
    pub fn notify_memory_pressure(&mut self, level: MemoryPressureLevel) {
        let _ = self.notify_memory_pressure_checked(level);
    }

    /// Dispatch pressure while containing and reporting owner callback panics.
    pub fn notify_memory_pressure_checked(&mut self, level: MemoryPressureLevel) -> Result<()> {
        let (subscribers, eviction_failed) =
            self.update_default_global::<GpuMemoryRuntime, _>(|runtime, _| {
                runtime.last_level = level;
                let eviction_failed = matches!(level, MemoryPressureLevel::Critical)
                    && runtime.manager.evict_to_budget_checked().is_err();
                (runtime.subscribers.clone(), eviction_failed)
            });
        let mut subscriber_panicked = false;
        subscribers.retain(&(), |subscriber| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| subscriber(level, self)))
                .is_err()
            {
                subscriber_panicked = true;
            }
            true
        });
        anyhow::ensure!(!eviction_failed, "GPU eviction callback panicked");
        anyhow::ensure!(!subscriber_panicked, "memory-pressure subscriber panicked");
        Ok(())
    }

    /// Query the device GPU-memory budget, map it to a [`MemoryPressureLevel`], and
    /// dispatch to subscribers if the level changed since the last poll. Returns the
    /// current level (`Normal` when no budget is available). Call once per frame or on a
    /// timer to drive automatic cache shedding.
    pub fn poll_memory_pressure(&mut self) -> MemoryPressureLevel {
        let level = self
            .gpu_memory_budget()
            .map(|budget| MemoryPressureLevel::from_budget(&budget))
            .unwrap_or(MemoryPressureLevel::Normal);
        let last_level =
            self.update_default_global::<GpuMemoryRuntime, _>(|runtime, _| runtime.last_level);
        if level != last_level {
            self.notify_memory_pressure(level);
        }
        level
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

    #[test]
    fn memory_pressure_level_thresholds() {
        assert_eq!(
            MemoryPressureLevel::from_utilization(0.50),
            MemoryPressureLevel::Normal
        );
        assert_eq!(
            MemoryPressureLevel::from_utilization(0.80),
            MemoryPressureLevel::Warning
        );
        assert_eq!(
            MemoryPressureLevel::from_utilization(0.95),
            MemoryPressureLevel::Critical
        );
        assert_eq!(
            MemoryPressureLevel::from_utilization(1.20),
            MemoryPressureLevel::Critical
        );
        assert_eq!(
            MemoryPressureLevel::from_utilization(f64::NAN),
            MemoryPressureLevel::Critical
        );
        assert_eq!(
            MemoryPressureLevel::from_utilization(-0.1),
            MemoryPressureLevel::Critical
        );
    }

    #[test]
    fn manager_contains_counter_rollover_and_byte_overflow() {
        let mut manager = GpuMemoryManager::new(u64::MAX);
        let first = manager.register_checked(1, || {}).unwrap();
        manager.next_id = u64::MAX;
        let near_wrap = manager.register_checked(1, || {}).unwrap();
        let after_wrap = manager.register_checked(1, || {}).unwrap();
        assert_ne!(first, near_wrap);
        assert_ne!(first, after_wrap);
        assert_ne!(near_wrap, after_wrap);

        manager.tick = u64::MAX;
        assert!(manager.touch(first));
        assert!(manager.tick < u64::MAX);
        assert!(!manager.touch(0));

        let mut overflow = GpuMemoryManager::new(u64::MAX);
        overflow.register_checked(u64::MAX, || {}).unwrap();
        assert!(overflow.register_checked(1, || {}).is_err());
        assert_eq!(overflow.used_bytes(), u64::MAX);
        assert_eq!(overflow.tracked_count(), 1);
    }

    #[test]
    fn eviction_continues_after_owner_callback_panics() {
        let calls = Arc::new(AtomicU64::new(0));
        let mut manager = GpuMemoryManager::new(0);
        manager
            .register_checked(10, || panic!("private eviction panic"))
            .unwrap();
        let calls_for_callback = calls.clone();
        manager
            .register_checked(10, move || {
                calls_for_callback.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();

        assert_eq!(
            manager.evict_to_budget_checked().unwrap_err().to_string(),
            "GPU eviction callback panicked"
        );
        assert_eq!(manager.used_bytes(), 0);
        assert_eq!(manager.tracked_count(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[kael::test]
    fn set_gpu_budget_registers_and_evicts(cx: &mut crate::TestAppContext) {
        cx.update(|cx| {
            cx.set_gpu_budget(100);
            cx.with_gpu_memory_manager(|manager| {
                manager.register(40, || {});
                manager.register(40, || {});
                manager.register(40, || {});
            });
            let used = cx.with_gpu_memory_manager(|manager| manager.used_bytes());
            assert_eq!(
                used, 120,
                "all three resources are tracked under the budget"
            );

            let evicted = cx.with_gpu_memory_manager(|manager| manager.evict_to_budget());
            assert_eq!(
                evicted, 1,
                "one resource must be shed to fit a 100-byte budget"
            );
            let used = cx.with_gpu_memory_manager(|manager| manager.used_bytes());
            assert!(
                used <= 100,
                "tracked bytes must fit the budget after eviction"
            );
        });
    }

    #[kael::test]
    fn on_memory_pressure_fires_and_critical_evicts(cx: &mut crate::TestAppContext) {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        let seen: Rc<RefCell<Vec<MemoryPressureLevel>>> = Rc::new(RefCell::new(Vec::new()));
        let evictions = Arc::new(AtomicU64::new(0));

        cx.update(|cx| {
            cx.set_gpu_budget(50);
            let evict_counter = evictions.clone();
            cx.with_gpu_memory_manager(move |manager| {
                let counter = evict_counter.clone();
                manager.register(40, move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                });
                let counter = evict_counter.clone();
                manager.register(40, move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                });
            });

            let sink = seen.clone();
            cx.on_memory_pressure(move |level, _| sink.borrow_mut().push(level))
                .detach();

            cx.notify_memory_pressure(MemoryPressureLevel::Warning);
            cx.notify_memory_pressure(MemoryPressureLevel::Critical);
        });

        assert_eq!(
            *seen.borrow(),
            vec![MemoryPressureLevel::Warning, MemoryPressureLevel::Critical],
            "subscribers must receive each dispatched pressure level in order"
        );
        assert!(
            evictions.load(Ordering::SeqCst) >= 1,
            "a Critical notification must evict registered resources down to budget"
        );
    }

    #[kael::test]
    fn memory_pressure_contains_subscriber_panics(cx: &mut crate::TestAppContext) {
        use std::cell::Cell;
        use std::rc::Rc;

        let later_ran = Rc::new(Cell::new(false));
        cx.update(|cx| {
            cx.on_memory_pressure(|_, _| panic!("private pressure panic"))
                .detach();
            let later_ran = later_ran.clone();
            cx.on_memory_pressure(move |_, _| later_ran.set(true))
                .detach();
            assert_eq!(
                cx.notify_memory_pressure_checked(MemoryPressureLevel::Warning)
                    .unwrap_err()
                    .to_string(),
                "memory-pressure subscriber panicked"
            );
        });
        assert!(later_ran.get());
    }
}
