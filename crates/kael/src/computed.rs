use crate::{App, AppContext, Context, Entity, EntityId, Result, Subscription};
use anyhow::anyhow;
use collections::FxHashSet;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

const MAX_COMPUTED_DEPENDENCIES: usize = 4_096;

/// Records the entities read during a [`Computed`] recomputation.
///
/// Dependency tracking is explicit: a computation reads its inputs through
/// [`Tracker::read`] so the resulting [`Computed`] can subscribe to exactly
/// those entities. The tracker dereferences to [`App`], so any other context
/// operation is available, but only reads routed through [`Tracker::read`]
/// (or [`Tracker::observe`]) establish a dependency.
pub struct Tracker<'a> {
    app: &'a mut App,
    dependencies: FxHashSet<EntityId>,
    dependency_overflowed: bool,
}

impl<'a> Tracker<'a> {
    fn new(app: &'a mut App) -> Self {
        Self {
            app,
            dependencies: FxHashSet::default(),
            dependency_overflowed: false,
        }
    }

    /// Reads an entity and records it as a dependency of the current
    /// computation.
    pub fn read<'b, T: 'static>(&'b mut self, entity: &Entity<T>) -> &'b T {
        self.record_dependency(entity.entity_id());
        entity.read(self.app)
    }

    /// Reads an entity and rejects computations that exceed the dependency cap.
    pub fn try_read<'b, T: 'static>(&'b mut self, entity: &Entity<T>) -> Result<&'b T> {
        anyhow::ensure!(
            self.record_dependency(entity.entity_id()),
            "computed dependency limit exceeded"
        );
        Ok(entity.read(self.app))
    }

    /// Records `entity` as a dependency without reading it, for cases where the
    /// computation depends on a notify but not the current value.
    pub fn observe<T: 'static>(&mut self, entity: &Entity<T>) {
        self.record_dependency(entity.entity_id());
    }

    /// Records a dependency and reports when the computation exceeds its cap.
    pub fn try_observe<T: 'static>(&mut self, entity: &Entity<T>) -> Result<()> {
        anyhow::ensure!(
            self.record_dependency(entity.entity_id()),
            "computed dependency limit exceeded"
        );
        Ok(())
    }

    /// The underlying app context.
    pub fn app(&mut self) -> &mut App {
        self.app
    }

    fn record_dependency(&mut self, entity_id: EntityId) -> bool {
        if self.dependencies.contains(&entity_id) {
            return true;
        }
        if self.dependencies.len() >= MAX_COMPUTED_DEPENDENCIES {
            self.dependency_overflowed = true;
            return false;
        }
        self.dependencies.insert(entity_id);
        true
    }
}

impl Deref for Tracker<'_> {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}

impl DerefMut for Tracker<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.app
    }
}

type ComputeFn<T> = Rc<dyn Fn(&mut Tracker) -> T + 'static>;

/// Backing state for a [`Computed`] value, owned by an [`Entity`].
///
/// Views may `cx.observe` the wrapping entity to re-render whenever the cached
/// value is invalidated by one of its tracked dependencies.
pub struct ComputedState<T: 'static> {
    compute: ComputeFn<T>,
    cached: Option<T>,
    dirty: bool,
    computing: bool,
    dependency_overflowed: bool,
    dependencies: FxHashSet<EntityId>,
    subscriptions: Vec<Subscription>,
}

impl<T: 'static> ComputedState<T> {
    /// Returns true while the cached value is stale and a read would recompute.
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.cached.is_none()
    }

    /// Returns the entity ids the most recent computation depended upon.
    pub fn dependencies(&self) -> &FxHashSet<EntityId> {
        &self.dependencies
    }

    /// Whether the latest legacy computation exceeded the dependency cap.
    pub fn dependency_overflowed(&self) -> bool {
        self.dependency_overflowed
    }
}

/// A memoized value derived from one or more entities, recomputed only when a
/// tracked dependency changes.
///
/// The computation closure is run lazily on the first read. It receives a
/// [`Tracker`]; every entity read through [`Tracker::read`] is recorded as a
/// dependency. The result is cached and the [`Computed`] subscribes to each
/// dependency. A `notify` on any dependency marks the cache dirty and notifies
/// the wrapping entity, so observers re-render and the next read recomputes.
/// Dependencies are re-tracked on every recomputation, so a closure whose
/// accessed entities change between runs is handled correctly.
pub struct Computed<T: 'static> {
    state: Entity<ComputedState<T>>,
}

impl<T: 'static> Clone for Computed<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T: 'static> Computed<T> {
    /// Creates a new computed value backed by an entity in the given context.
    ///
    /// The closure is not run until the first [`Computed::read`] or
    /// [`Computed::get`].
    pub fn new(cx: &mut App, compute: impl Fn(&mut Tracker) -> T + 'static) -> Self {
        let compute: ComputeFn<T> = Rc::new(compute);
        let state = cx.new(|_| ComputedState {
            compute,
            cached: None,
            dirty: true,
            computing: false,
            dependency_overflowed: false,
            dependencies: FxHashSet::default(),
            subscriptions: Vec::new(),
        });
        Self { state }
    }

    /// The entity id of the backing [`ComputedState`], for use with
    /// `cx.observe`.
    pub fn entity_id(&self) -> EntityId {
        self.state.entity_id()
    }

    /// The backing entity, so views can `cx.observe(computed.entity())`.
    pub fn entity(&self) -> Entity<ComputedState<T>> {
        self.state.clone()
    }

    /// Recomputes the value if stale, then returns a reference to the cached
    /// result.
    pub fn read<'a>(&self, cx: &'a mut App) -> &'a T {
        self.try_read(cx)
            .expect("computed value recomputation failed")
    }

    /// Recomputes if stale and reports callback failures or dependency cycles.
    pub fn try_read<'a>(&self, cx: &'a mut App) -> Result<&'a T> {
        self.recompute_if_dirty(cx)?;
        self.state
            .read(cx)
            .cached
            .as_ref()
            .ok_or_else(|| anyhow!("computed value is unavailable"))
    }

    fn recompute_if_dirty(&self, cx: &mut App) -> Result<()> {
        let state = self.state.read(cx);
        let needs_recompute = state.is_dirty();
        anyhow::ensure!(!state.computing, "computed dependency cycle detected");
        if !needs_recompute {
            return Ok(());
        }

        let compute = state.compute.clone();
        self.state.update(cx, |state, _| state.computing = true);
        let mut tracker = Tracker::new(cx);
        let value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            compute(&mut tracker)
        })) {
            Ok(value) => value,
            Err(_) => {
                drop(tracker);
                self.state.update(cx, |state, _| state.computing = false);
                return Err(anyhow!("computed callback panicked"));
            }
        };
        let dependencies = tracker.dependencies;
        let dependency_overflowed = tracker.dependency_overflowed;

        let weak = self.state.downgrade();
        let subscriptions = dependencies
            .iter()
            .map(|dependency| {
                let weak = weak.clone();
                cx.new_observer(
                    *dependency,
                    Box::new(move |cx| {
                        if let Some(state) = weak.upgrade() {
                            state.update(cx, |state, cx| {
                                if !state.dirty {
                                    state.dirty = true;
                                    cx.notify();
                                }
                            });
                            true
                        } else {
                            false
                        }
                    }),
                )
            })
            .collect::<Vec<_>>();

        self.state.update(cx, |state, _| {
            state.cached = Some(value);
            state.dirty = dependency_overflowed;
            state.computing = false;
            state.dependency_overflowed = dependency_overflowed;
            state.dependencies = dependencies;
            state.subscriptions = subscriptions;
        });
        Ok(())
    }
}

impl<T: Clone + 'static> Computed<T> {
    /// Recomputes if stale, then returns a clone of the cached value.
    pub fn get(&self, cx: &mut App) -> T {
        self.read(cx).clone()
    }

    /// Recomputes if stale and returns a cloned value through the checked path.
    pub fn try_get(&self, cx: &mut App) -> Result<T> {
        self.try_read(cx).cloned()
    }
}

/// Extension that adds [`AppContextExt::computed`] for ergonomic creation.
pub trait AppContextExt {
    /// Creates a [`Computed`] value tracking the entities read by `compute`.
    fn computed<T: 'static>(
        &mut self,
        compute: impl Fn(&mut Tracker) -> T + 'static,
    ) -> Computed<T>;
}

impl AppContextExt for App {
    fn computed<T: 'static>(
        &mut self,
        compute: impl Fn(&mut Tracker) -> T + 'static,
    ) -> Computed<T> {
        Computed::new(self, compute)
    }
}

impl<T: 'static> AppContextExt for Context<'_, T> {
    fn computed<U: 'static>(
        &mut self,
        compute: impl Fn(&mut Tracker) -> U + 'static,
    ) -> Computed<U> {
        Computed::new(self, compute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestAppContext;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct Counter(usize);

    #[kael::test]
    fn computed_caches_until_dependency_changes(cx: &mut TestAppContext) {
        let runs = Rc::new(Cell::new(0));
        let source = cx.new(|_| Counter(2));

        let computed = cx.update(|cx| {
            let runs = runs.clone();
            let source = source.clone();
            Computed::new(cx, move |tracker| {
                runs.set(runs.get() + 1);
                tracker.read(&source).0 * 10
            })
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 20);
        assert_eq!(runs.get(), 1);

        assert_eq!(cx.update(|cx| computed.get(cx)), 20);
        assert_eq!(runs.get(), 1);

        source.update(cx, |counter, cx| {
            counter.0 = 5;
            cx.notify();
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 50);
        assert_eq!(runs.get(), 2);

        assert_eq!(cx.update(|cx| computed.get(cx)), 50);
        assert_eq!(runs.get(), 2);
    }

    #[kael::test]
    fn computed_invalidates_on_dependency_notify(cx: &mut TestAppContext) {
        let source = cx.new(|_| Counter(0));
        let computed = cx.update(|cx| {
            let source = source.clone();
            Computed::new(cx, move |tracker| tracker.read(&source).0 + 1)
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 1);

        source.update(cx, |counter, cx| {
            counter.0 = 41;
            cx.notify();
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 42);
    }

    #[kael::test]
    fn computed_switches_dependencies_between_runs(cx: &mut TestAppContext) {
        let runs = Rc::new(Cell::new(0));
        let toggle = cx.new(|_| true);
        let left = cx.new(|_| Counter(1));
        let right = cx.new(|_| Counter(100));

        let computed = cx.update(|cx| {
            let runs = runs.clone();
            let toggle = toggle.clone();
            let left = left.clone();
            let right = right.clone();
            Computed::new(cx, move |tracker| {
                runs.set(runs.get() + 1);
                if *tracker.read(&toggle) {
                    tracker.read(&left).0
                } else {
                    tracker.read(&right).0
                }
            })
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 1);
        assert_eq!(runs.get(), 1);

        right.update(cx, |counter, cx| {
            counter.0 = 200;
            cx.notify();
        });
        assert_eq!(cx.update(|cx| computed.get(cx)), 1);
        assert_eq!(runs.get(), 1);

        toggle.update(cx, |value, cx| {
            *value = false;
            cx.notify();
        });
        assert_eq!(cx.update(|cx| computed.get(cx)), 200);
        assert_eq!(runs.get(), 2);

        left.update(cx, |counter, cx| {
            counter.0 = 9;
            cx.notify();
        });
        assert_eq!(cx.update(|cx| computed.get(cx)), 200);
        assert_eq!(runs.get(), 2);

        right.update(cx, |counter, cx| {
            counter.0 = 300;
            cx.notify();
        });
        assert_eq!(cx.update(|cx| computed.get(cx)), 300);
        assert_eq!(runs.get(), 3);
    }

    #[kael::test]
    fn computed_is_observable(cx: &mut TestAppContext) {
        let observed = Rc::new(Cell::new(0));
        let source = cx.new(|_| Counter(0));

        let computed = cx.update(|cx| {
            let source = source.clone();
            Computed::new(cx, move |tracker| tracker.read(&source).0 * 2)
        });

        cx.update(|cx| computed.get(cx));

        cx.update(|cx| {
            let observed = observed.clone();
            cx.observe(&computed.entity(), move |_, _| {
                observed.set(observed.get() + 1);
            })
            .detach();
        });

        source.update(cx, |counter, cx| {
            counter.0 = 7;
            cx.notify();
        });

        assert_eq!(observed.get(), 1);
        assert_eq!(cx.update(|cx| computed.get(cx)), 14);
    }

    #[kael::test]
    fn computed_via_cx_extension(cx: &mut TestAppContext) {
        let source = cx.new(|_| Counter(3));
        let computed = cx.update(|cx| {
            let source = source.clone();
            cx.computed(move |tracker| tracker.read(&source).0 + 100)
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 103);
    }

    #[kael::test]
    fn computed_contains_callback_panics_and_recovers(cx: &mut TestAppContext) {
        let calls = Rc::new(Cell::new(0));
        let computed = cx.update(|cx| {
            let calls = calls.clone();
            Computed::new(cx, move |_| {
                let call = calls.get();
                calls.set(call + 1);
                assert!(call > 0, "private computed panic");
                7
            })
        });

        assert_eq!(
            cx.update(|cx| computed.try_get(cx).unwrap_err().to_string()),
            "computed callback panicked"
        );
        assert_eq!(cx.update(|cx| computed.try_get(cx).unwrap()), 7);
        assert_eq!(calls.get(), 2);
    }

    #[kael::test]
    fn computed_detects_recursive_reads_without_recursing(cx: &mut TestAppContext) {
        let slot = Rc::new(RefCell::new(None::<Computed<usize>>));
        let cycle_detected = Rc::new(Cell::new(false));
        let computed = cx.update(|cx| {
            let slot = slot.clone();
            let cycle_detected = cycle_detected.clone();
            Computed::new(cx, move |tracker| {
                let recursive = slot
                    .borrow()
                    .as_ref()
                    .expect("computed installed before first lazy read")
                    .try_get(tracker.app());
                cycle_detected.set(recursive.is_err());
                11
            })
        });
        *slot.borrow_mut() = Some(computed.clone());

        assert_eq!(cx.update(|cx| computed.try_get(cx).unwrap()), 11);
        assert!(cycle_detected.get());
    }

    #[kael::test]
    fn computed_bounds_legacy_dependency_tracking(cx: &mut TestAppContext) {
        let sources = (0..=MAX_COMPUTED_DEPENDENCIES)
            .map(|_| cx.new(|_| ()))
            .collect::<Vec<_>>();
        let runs = Rc::new(Cell::new(0));
        let computed = cx.update(|cx| {
            let runs = runs.clone();
            Computed::new(cx, move |tracker| {
                runs.set(runs.get() + 1);
                for source in &sources {
                    tracker.observe(source);
                }
                5
            })
        });

        assert_eq!(cx.update(|cx| computed.get(cx)), 5);
        cx.read(|cx| {
            let state = computed.entity();
            let state = state.read(cx);
            assert!(state.dependency_overflowed());
            assert_eq!(state.dependencies().len(), MAX_COMPUTED_DEPENDENCIES);
            assert!(state.is_dirty());
        });
        assert_eq!(cx.update(|cx| computed.get(cx)), 5);
        assert_eq!(runs.get(), 2);
    }
}
