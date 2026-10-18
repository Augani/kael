use crate::{App, SharedString, SharedUri};
use futures::{Future, TryFutureExt};

use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An enum representing
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Resource {
    /// This resource is at a given URI
    Uri(SharedUri),
    /// This resource is at a given path in the file system
    Path(Arc<Path>),
    /// This resource is embedded in the application binary
    Embedded(SharedString),
}

impl From<SharedUri> for Resource {
    fn from(value: SharedUri) -> Self {
        Self::Uri(value)
    }
}

impl From<PathBuf> for Resource {
    fn from(value: PathBuf) -> Self {
        Self::Path(value.into())
    }
}

impl From<Arc<Path>> for Resource {
    fn from(value: Arc<Path>) -> Self {
        Self::Path(value)
    }
}

/// A trait for asynchronous asset loading.
pub trait Asset: 'static {
    /// The source of the asset.
    type Source: Clone + Hash + Send;

    /// The loaded asset
    type Output: Clone + Send;

    /// Load the asset asynchronously
    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static;

    /// Retained output bytes for the completed asset cache's byte budget.
    /// Override this for outputs with heap allocations; the default counts only
    /// the output value itself. Completed entry count is bounded independently.
    fn cache_bytes(output: &Self::Output) -> u64 {
        std::mem::size_of_val(output) as u64
    }

    /// Release cache-owned native residency when a completed output is evicted.
    /// Existing shared output handles remain valid.
    fn on_cache_evict(_output: &Self::Output, _cx: &mut App) {}
}

/// An asset Loader which logs the [`Err`] variant of a [`Result`] during loading
pub enum AssetLogger<T> {
    #[doc(hidden)]
    _Phantom(PhantomData<T>, &'static dyn crate::seal::Sealed),
}

impl<T, R, E> Asset for AssetLogger<T>
where
    T: Asset<Output = Result<R, E>>,
    R: Clone + Send,
    E: Clone + Send + std::fmt::Display,
{
    type Source = T::Source;

    type Output = T::Output;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let load = T::load(source, cx);
        load.inspect_err(|e| log::error!("Failed to load asset: {}", e))
    }

    fn cache_bytes(output: &Self::Output) -> u64 {
        T::cache_bytes(output)
    }

    fn on_cache_evict(output: &Self::Output, cx: &mut App) {
        T::on_cache_evict(output, cx);
    }
}

/// Use a quick, non-cryptographically secure hash function to get an identifier from data
pub fn hash<T: Hash>(data: &T) -> u64 {
    let mut hasher = collections::FxHasher::default();
    data.hash(&mut hasher);
    hasher.finish()
}

// These registries contain weak frame consumers, never pending task owners.
pub(crate) type AssetReadySubscribers =
    crate::SubscriberSet<(), Box<dyn FnMut(&dyn std::any::Any, &Arc<()>, &mut App)>>;
pub(crate) type AssetInvalidationSubscribers =
    crate::SubscriberSet<(std::any::TypeId, u64), Box<dyn FnMut(&mut App)>>;

pub(crate) struct AssetConsumerRequest<A: Asset> {
    pub(crate) task: futures::future::Shared<crate::Task<A::Output>>,
    pub(crate) generation: Arc<()>,
    pub(crate) subscribers: AssetReadySubscribers,
}

/// Active drawing ownership is separate from completed LRU retention. Only a
/// finished Shared task may enter `ready`; pending delivery captures this cell
/// weakly and therefore cannot defeat removal/cancellation of cache-only work.
pub(crate) struct AssetConsumer<A: Asset> {
    state: std::rc::Rc<std::cell::RefCell<AssetConsumerState<A>>>,
    _invalidation: crate::Subscription,
}

struct AssetConsumerState<A: Asset> {
    epoch: std::rc::Rc<()>,
    generation: Option<Arc<()>>,
    ready: Option<futures::future::Shared<crate::Task<A::Output>>>,
    completion: Option<crate::Subscription>,
    wake_view: Option<crate::EntityId>,
}

impl<A: Asset> AssetConsumer<A> {
    pub(crate) fn new(key: (std::any::TypeId, u64), cx: &mut App) -> Self {
        let state = std::rc::Rc::new(std::cell::RefCell::new(AssetConsumerState {
            epoch: std::rc::Rc::new(()),
            generation: None,
            ready: None,
            completion: None,
            wake_view: None,
        }));
        let weak = std::rc::Rc::downgrade(&state);
        let invalidation = cx.observe_asset_consumer_invalidation(key, move |cx| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            let (retired_ready, retired_subscription, wake_view) = {
                let mut state = state.borrow_mut();
                state.epoch = std::rc::Rc::new(());
                state.generation = None;
                (state.ready.take(), state.completion.take(), state.wake_view)
            };
            // Shared output destruction and subscription removal must not run
            // while a consumer borrow is outstanding.
            drop(retired_ready);
            drop(retired_subscription);
            if let Some(view) = wake_view
                && cx.entities.generation(view).is_some()
            {
                cx.notify(view);
            }
        });
        Self {
            state,
            _invalidation: invalidation,
        }
    }

    pub(crate) fn poll(
        &self,
        source: &A::Source,
        key: (std::any::TypeId, u64),
        wake_view: Option<crate::EntityId>,
        initial_request: Option<AssetConsumerRequest<A>>,
        cx: &mut App,
    ) -> anyhow::Result<Option<A::Output>> {
        use futures::FutureExt as _;
        let ready = {
            let mut state = self.state.borrow_mut();
            if wake_view.is_some() {
                state.wake_view = wake_view;
            }
            state.ready.clone()
        };
        if let Some(ready) = ready {
            // This invokes the caller's Output::clone outside the cell borrow.
            return Ok(ready.now_or_never());
        }

        // Asset::load and all cache callbacks may reenter App. No consumer or
        // registry borrow crosses this call or polling the Shared output.
        let AssetConsumerRequest {
            task,
            generation,
            subscribers,
        } = match initial_request {
            Some(request) => request,
            None => cx.fetch_asset_for_consumer::<A>(source, key)?,
        };
        let (epoch, already_subscribed, retired_subscription) = {
            let mut state = self.state.borrow_mut();
            let same = state
                .generation
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &generation));
            let retired = if same { None } else { state.completion.take() };
            state.generation = Some(generation.clone());
            (
                state.epoch.clone(),
                same && state.completion.is_some(),
                retired,
            )
        };
        drop(retired_subscription);
        if let Some(output) = task.clone().now_or_never() {
            Self::publish(&self.state, &epoch, &generation, task, false, cx);
            return Ok(Some(output));
        }
        if already_subscribed {
            return Ok(None);
        }

        let weak = std::rc::Rc::downgrade(&self.state);
        let expected_epoch = epoch.clone();
        let expected_generation = generation.clone();
        let (subscription, activate) = subscribers.insert(
            (),
            Box::new(move |task, current, cx| {
                if !Arc::ptr_eq(current, &expected_generation) {
                    return;
                }
                let Some(state) = weak.upgrade() else {
                    return;
                };
                let Some(task) =
                    task.downcast_ref::<futures::future::Shared<crate::Task<A::Output>>>()
                else {
                    return;
                };
                Self::publish(
                    &state,
                    &expected_epoch,
                    &expected_generation,
                    task.clone(),
                    true,
                    cx,
                );
            }),
        );
        activate();
        let retired = {
            let mut state = self.state.borrow_mut();
            if std::rc::Rc::ptr_eq(&state.epoch, &epoch)
                && state
                    .generation
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &generation))
                && state.ready.is_none()
            {
                state.completion.replace(subscription)
            } else {
                Some(subscription)
            }
        };
        drop(retired);
        Ok(None)
    }

    fn publish(
        state: &std::rc::Rc<std::cell::RefCell<AssetConsumerState<A>>>,
        epoch: &std::rc::Rc<()>,
        generation: &Arc<()>,
        task: futures::future::Shared<crate::Task<A::Output>>,
        notify_owner: bool,
        cx: &mut App,
    ) {
        // The app completion observer has already awaited this task, or poll()
        // itself observed Ready. A pending Shared is never stored in the frame.
        debug_assert!(task.peek().is_some());
        let (retired_ready, retired_subscription, wake_view) = {
            let mut state = state.borrow_mut();
            if !std::rc::Rc::ptr_eq(&state.epoch, epoch)
                || !state
                    .generation
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, generation))
            {
                return;
            }
            (
                state.ready.replace(task),
                state.completion.take(),
                state.wake_view,
            )
        };
        drop(retired_ready);
        drop(retired_subscription);
        if notify_owner
            && let Some(view) = wake_view
            && cx.entities.generation(view).is_some()
        {
            cx.notify(view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryPressureLevel, TestAppContext};
    use futures::FutureExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct Source {
        key: u64,
        bytes: usize,
        calls: Arc<AtomicUsize>,
        pending: bool,
    }
    impl Hash for Source {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.key.hash(state);
        }
    }
    struct ByteAsset;
    impl Asset for ByteAsset {
        type Source = Source;
        type Output = Arc<Vec<u8>>;
        fn load(
            source: Source,
            _: &mut App,
        ) -> impl Future<Output = Self::Output> + Send + 'static {
            source.calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if source.pending {
                    futures::future::pending::<()>().await;
                }
                Arc::new(vec![0; source.bytes])
            }
        }
        fn cache_bytes(output: &Self::Output) -> u64 {
            output.len() as u64
        }
    }

    #[crate::test]
    fn completed_asset_cache_limits_evict_lru_by_bytes_and_preserve_shared_outputs(
        cx: &mut TestAppContext,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let source = |key, bytes| Source {
            key,
            bytes,
            calls: calls.clone(),
            pending: false,
        };
        let a = source(1, 4);
        let b = source(2, 4);
        let c = source(3, 4);
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, 8));
        let a_task = cx.update(|cx| cx.fetch_asset_checked::<ByteAsset>(&a).unwrap().0);
        let b_task = cx.update(|cx| cx.fetch_asset_checked::<ByteAsset>(&b).unwrap().0);
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_bytes(), 8);
            assert!(!cx.fetch_asset::<ByteAsset>(&a).1);
        });
        let c_task = cx.update(|cx| cx.fetch_asset_checked::<ByteAsset>(&c).unwrap().0);
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_bytes(), 8);
            assert_eq!(cx.completed_asset_count(), 2);
            assert!(!cx.fetch_asset::<ByteAsset>(&a).1);
            assert!(cx.fetch_asset::<ByteAsset>(&b).1);
        });
        assert_eq!(b_task.now_or_never().unwrap().len(), 4);
        assert_eq!(a_task.now_or_never().unwrap().len(), 4);
        assert_eq!(c_task.now_or_never().unwrap().len(), 4);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[crate::test]
    fn checked_asset_pending_admission_runs_before_loader_and_keeps_single_flight(
        cx: &mut TestAppContext,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let a = Source {
            key: 1,
            bytes: 4,
            calls: calls.clone(),
            pending: true,
        };
        let b = Source {
            key: 2,
            ..a.clone()
        };
        cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let (task, first) = cx.fetch_asset_checked::<ByteAsset>(&a).unwrap();
            assert!(first);
            let (other, first) = cx.fetch_asset_checked::<ByteAsset>(&a).unwrap();
            assert!(!first);
            assert!(cx.fetch_asset_checked::<ByteAsset>(&b).is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(cx.pending_asset_count(), 1);
            cx.remove_asset::<ByteAsset>(&a);
            assert_eq!(cx.pending_asset_count(), 0);
            drop((task, other));
            assert!(cx.fetch_asset_checked::<ByteAsset>(&b).is_ok());
        });
    }

    #[crate::test]
    fn critical_pressure_releases_completed_assets_without_restoring_stale_completions(
        cx: &mut TestAppContext,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let source = Source {
            key: 1,
            bytes: 4,
            calls,
            pending: false,
        };
        let output = cx.update(|cx| cx.fetch_asset::<ByteAsset>(&source).0);
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 1);
            cx.notify_memory_pressure_checked(MemoryPressureLevel::Critical)
                .unwrap();
            assert_eq!(cx.completed_asset_count(), 0);
            assert_eq!(cx.completed_asset_bytes(), 0);
        });
        cx.run_until_parked();
        assert_eq!(output.now_or_never().unwrap().len(), 4);
        cx.update(|cx| assert_eq!(cx.completed_asset_count(), 0));
    }

    struct EvictionPanicAsset;
    impl Asset for EvictionPanicAsset {
        type Source = u64;
        type Output = ();
        fn load(_: u64, _: &mut App) -> impl Future<Output = ()> + Send + 'static {
            futures::future::ready(())
        }
        fn on_cache_evict(_: &(), _: &mut App) {
            panic!("eviction hook failure");
        }
    }

    struct AccountingPanicAsset;
    impl Asset for AccountingPanicAsset {
        type Source = u64;
        type Output = ();
        fn load(_: u64, _: &mut App) -> impl Future<Output = ()> + Send + 'static {
            futures::future::ready(())
        }
        fn cache_bytes(_: &()) -> u64 {
            panic!("accounting hook failure");
        }
    }

    struct ReentrantEvictionAsset;
    impl Asset for ReentrantEvictionAsset {
        type Source = Source;
        type Output = Source;
        fn load(source: Source, _: &mut App) -> impl Future<Output = Source> + Send + 'static {
            futures::future::ready(source)
        }
        fn on_cache_evict(peer: &Source, cx: &mut App) {
            cx.remove_asset::<ByteAsset>(peer);
            let mut next = peer.clone();
            next.pending = true;
            // The new request is a distinct generation under the same key.
            drop(cx.fetch_asset::<ByteAsset>(&next).0);
        }
    }

    #[crate::test]
    fn reentrant_eviction_cannot_remove_a_replacement_pending_generation(cx: &mut TestAppContext) {
        let peer = Source {
            key: 1,
            bytes: 4,
            calls: Arc::new(AtomicUsize::new(0)),
            pending: false,
        };
        drop(cx.update(|cx| cx.fetch_asset::<ReentrantEvictionAsset>(&peer).0));
        drop(cx.update(|cx| cx.fetch_asset::<ByteAsset>(&peer).0));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 2);
            // The older hook replaces the newer candidate while the LRU trim's
            // candidate snapshot is being walked. That new request must survive.
            cx.set_completed_asset_cache_limits(0, 0);
            assert_eq!(cx.pending_asset_count(), 1);
            assert_eq!(cx.completed_asset_count(), 0);
            assert_eq!(cx.completed_asset_bytes(), 0);
            assert_eq!(cx.clear_completed_asset_cache_checked().unwrap(), 0);
            assert_eq!(cx.pending_asset_count(), 1);
        });
    }

    #[crate::test]
    fn panicking_asset_hooks_do_not_block_pressure_or_leave_unaccounted_entries(
        cx: &mut TestAppContext,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let peer = Source {
            key: 1,
            bytes: 4,
            calls,
            pending: false,
        };
        let peer_output = cx.update(|cx| cx.fetch_asset::<ByteAsset>(&peer).0);
        let bad_output = cx.update(|cx| cx.fetch_asset::<EvictionPanicAsset>(&1).0);
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 2);
            assert!(
                cx.notify_memory_pressure_checked(MemoryPressureLevel::Critical)
                    .is_err()
            );
            assert_eq!(cx.completed_asset_count(), 0);
            assert_eq!(cx.completed_asset_bytes(), 0);
        });
        assert_eq!(peer_output.now_or_never().unwrap().len(), 4);
        assert!(bad_output.now_or_never().is_some());
        let output = cx.update(|cx| cx.fetch_asset::<AccountingPanicAsset>(&1).0);
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.pending_asset_count(), 0);
            assert_eq!(cx.completed_asset_count(), 0);
        });
        assert!(output.now_or_never().is_some());
    }

    #[derive(Clone)]
    struct CancelSource {
        key: u64,
        dropped: Arc<AtomicUsize>,
    }
    impl Hash for CancelSource {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.key.hash(state);
        }
    }
    struct CancelAsset;
    struct DropSignal(Arc<AtomicUsize>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Asset for CancelAsset {
        type Source = CancelSource;
        type Output = ();
        fn load(source: CancelSource, _: &mut App) -> impl Future<Output = ()> + Send + 'static {
            let signal = DropSignal(source.dropped);
            async move {
                let _signal = signal;
                futures::future::pending::<()>().await
            }
        }
    }

    struct OneShotAssetView {
        source: CancelSource,
        requested: bool,
    }
    impl crate::Render for OneShotAssetView {
        fn render(
            &mut self,
            window: &mut crate::Window,
            cx: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            if !self.requested {
                self.requested = true;
                window.use_asset::<CancelAsset>(&self.source, cx);
            }
            crate::div()
        }
    }

    #[crate::test]
    fn window_asset_waiters_do_not_detach_owners_that_prevent_cache_cancellation(
        cx: &mut TestAppContext,
    ) {
        let dropped = Arc::new(AtomicUsize::new(0));
        let source = CancelSource {
            key: 1,
            dropped: dropped.clone(),
        };
        let (_, window) = cx.add_window_view(|_, _| OneShotAssetView {
            source: source.clone(),
            requested: false,
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(cx.pending_asset_count(), 1);
            cx.remove_asset::<CancelAsset>(&source);
        });
        window.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[crate::test]
    fn removing_shared_asset_cancels_only_after_the_final_external_owner_releases(
        cx: &mut TestAppContext,
    ) {
        let dropped = Arc::new(AtomicUsize::new(0));
        let source = CancelSource {
            key: 1,
            dropped: dropped.clone(),
        };
        let external = cx.update(|cx| cx.fetch_asset_checked::<CancelAsset>(&source).unwrap().0);
        cx.run_until_parked();
        cx.update(|cx| cx.remove_asset::<CancelAsset>(&source));
        cx.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        drop(external);
        cx.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        cx.update(|cx| {
            let (task, _) = cx.fetch_asset_checked::<CancelAsset>(&source).unwrap();
            drop(task);
        });
        cx.run_until_parked();
        cx.update(|cx| cx.remove_asset::<CancelAsset>(&source));
        cx.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }

    #[crate::test]
    fn critical_pressure_cancels_cache_only_assets_and_preserves_external_shared_loads(
        cx: &mut TestAppContext,
    ) {
        let dropped = Arc::new(AtomicUsize::new(0));
        let a = CancelSource {
            key: 1,
            dropped: dropped.clone(),
        };
        let b = CancelSource {
            key: 2,
            dropped: dropped.clone(),
        };
        let external = cx.update(|cx| {
            let external = cx.fetch_asset_checked::<CancelAsset>(&a).unwrap().0;
            drop(cx.fetch_asset_checked::<CancelAsset>(&b).unwrap().0);
            external
        });
        cx.run_until_parked();
        cx.update(|cx| {
            cx.notify_memory_pressure_checked(MemoryPressureLevel::Critical)
                .unwrap();
            assert_eq!(cx.pending_asset_count(), 0);
        });
        cx.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        drop(external);
        cx.run_until_parked();
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
    }

    #[crate::test]
    fn stale_asset_completions_cannot_replace_reloaded_outputs(cx: &mut TestAppContext) {
        let calls = Arc::new(AtomicUsize::new(0));
        let source = Source {
            key: 1,
            bytes: 3,
            calls: calls.clone(),
            pending: false,
        };
        let old = cx.update(|cx| cx.fetch_asset::<ByteAsset>(&source).0);
        let replacement = Source {
            bytes: 8,
            ..source.clone()
        };
        let new = cx.update(|cx| {
            cx.remove_asset::<ByteAsset>(&source);
            cx.fetch_asset::<ByteAsset>(&replacement).0
        });
        cx.run_until_parked();
        assert_eq!(old.now_or_never().unwrap().len(), 3);
        assert_eq!(new.now_or_never().unwrap().len(), 8);
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 1);
            assert_eq!(cx.completed_asset_bytes(), 8);
        });
    }
}

#[cfg(test)]
mod active_consumer_regressions {
    use crate::{
        App, Asset, Context, IntoElement, ParentElement, Render, TestAppContext, Window, div,
    };
    use futures::{Future, FutureExt, channel::oneshot};
    use std::{
        collections::VecDeque,
        hash::{Hash, Hasher},
        sync::{
            Arc, Mutex, Weak,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct Payload {
        value: u8,
        bytes: Box<[u8]>,
    }
    fn payload(value: u8) -> (Arc<Payload>, Weak<Payload>) {
        let output = Arc::new(Payload {
            value,
            bytes: vec![value; 4].into_boxed_slice(),
        });
        let weak = Arc::downgrade(&output);
        (output, weak)
    }

    #[derive(Clone)]
    struct Source {
        key: u64,
        receivers: Arc<Mutex<VecDeque<oneshot::Receiver<Arc<Payload>>>>>,
        calls: Arc<AtomicUsize>,
        futures_dropped: Arc<AtomicUsize>,
    }
    impl Hash for Source {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.key.hash(state);
        }
    }
    impl Source {
        fn new() -> Self {
            Self {
                key: 1,
                receivers: Arc::default(),
                calls: Arc::default(),
                futures_dropped: Arc::default(),
            }
        }
        fn next(&self) -> oneshot::Sender<Arc<Payload>> {
            let (sender, receiver) = oneshot::channel();
            self.receivers.lock().unwrap().push_back(receiver);
            sender
        }
    }
    struct FutureDropped(Arc<AtomicUsize>);
    impl Drop for FutureDropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct ProbeAsset;
    impl Asset for ProbeAsset {
        type Source = Source;
        type Output = Arc<Payload>;
        fn load(
            source: Source,
            _: &mut App,
        ) -> impl Future<Output = Self::Output> + Send + 'static {
            source.calls.fetch_add(1, Ordering::SeqCst);
            let receiver = source.receivers.lock().unwrap().pop_front();
            let drop_guard = FutureDropped(source.futures_dropped);
            async move {
                let _guard = drop_guard;
                match receiver {
                    Some(receiver) => match receiver.await {
                        Ok(output) => output,
                        // Cleanup after a deliberately failing baseline must
                        // not introduce an unrelated canceled-sender panic.
                        Err(_) => futures::future::pending().await,
                    },
                    // A baseline reload is observable through calls, not a
                    // fixture panic or an unbounded automatic reload loop.
                    None => futures::future::pending().await,
                }
            }
        }
        fn cache_bytes(output: &Self::Output) -> u64 {
            assert_ne!(output.value, 255, "controlled byte-accounting panic");
            output.bytes.len() as u64
        }
        fn on_cache_evict(output: &Self::Output, _: &mut App) {
            assert_ne!(output.value, 254, "controlled eviction-hook panic");
        }
    }

    struct Consumer {
        source: Source,
        shown: bool,
        last: Option<u8>,
        distinct_deliveries: usize,
    }
    impl Render for Consumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.shown {
                return div();
            }
            // Executes in Render, before child element layout. Repeated use/get
            // of the same source in one namespace must not duplicate a request.
            let output = window.use_asset::<ProbeAsset>(&self.source, cx);
            let repeated = window.get_asset::<ProbeAsset>(&self.source, cx);
            if let Some(output) = output {
                assert!(Arc::ptr_eq(
                    &output,
                    &repeated.expect("same ready consumer")
                ));
                if self.last != Some(output.value) {
                    self.distinct_deliveries += 1;
                    self.last = Some(output.value);
                }
                div().child(format!("payload {}", output.value))
            } else {
                assert!(repeated.is_none());
                div().child("waiting")
            }
        }
    }

    #[::core::prelude::v1::test]
    fn mounted_async_completion_survives_zero_or_undersized_completed_retention() {
        for max_bytes in [0, 3] {
            let mut cx = TestAppContext::single();
            cx.update(|cx| cx.set_completed_asset_cache_limits(8, max_bytes));
            let source = Source::new();
            let sender = source.next();
            let (consumer, window) = cx.add_window_view(|_, _| Consumer {
                source: source.clone(),
                shown: true,
                last: None,
                distinct_deliveries: 0,
            });
            window.update(|window, cx| window.draw(cx).clear());
            window.run_until_parked();
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
            let (output, weak) = payload(17);
            assert!(sender.send(output).is_ok());
            window.run_until_parked();
            // Do not rescue a missing notification with a forced draw. This
            // exercises the normal platform callback plus ordinary test effect
            // flushing, not an isolated production frame-clock measurement.
            window.simulate_platform_frame();
            window.run_until_parked();
            window.update(|_, cx| {
                assert_eq!(consumer.read(cx).last, Some(17));
                assert_eq!(consumer.read(cx).distinct_deliveries, 1);
                assert_eq!(cx.completed_asset_count(), 0);
                assert_eq!(cx.completed_asset_bytes(), 0);
            });
            // Continued active use owns the same output independently of LRU.
            for _ in 0..3 {
                window.update(|window, cx| window.draw(cx).clear());
            }
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
            assert!(weak.upgrade().is_some());
            window.update(|window, cx| {
                consumer.update(cx, |consumer, cx| {
                    consumer.shown = false;
                    cx.notify();
                });
                window.draw(cx).clear();
            });
            window.run_until_parked();
            assert!(
                weak.upgrade().is_none(),
                "no output owner remains after unmount"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn window_teardown_releases_ready_consumer_output() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, 0));
        let source = Source::new();
        let sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: source.clone(),
            shown: true,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (output, weak) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(17));
            assert_eq!(cx.completed_asset_count(), 0);
        });
        drop(consumer);
        window.update(|window, _| window.remove_window());
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
    }

    #[::core::prelude::v1::test]
    fn explicit_remove_reloads_after_the_lru_entry_is_already_gone() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, 0));
        let source = Source::new();
        let first_sender = source.next();
        let next_sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: source.clone(),
            shown: true,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (first, old_weak) = payload(17);
        assert!(first_sender.send(first).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(17));
            assert_eq!(cx.completed_asset_count(), 0);
            cx.remove_asset::<ProbeAsset>(&source);
            window.draw(cx).clear();
        });
        window.run_until_parked();
        assert!(old_weak.upgrade().is_none());
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        let (next, next_weak) = payload(29);
        assert!(next_sender.send(next).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(29));
            assert_eq!(consumer.read(cx).distinct_deliveries, 2);
        });
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        assert!(next_weak.upgrade().is_some());
    }

    #[::core::prelude::v1::test]
    fn dropped_window_does_not_hide_a_pending_shared_owner() {
        let mut cx = TestAppContext::single();
        let source = Source::new();
        let _sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: source.clone(),
            shown: true,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        assert_eq!(source.futures_dropped.load(Ordering::SeqCst), 0);
        drop(consumer);
        window.update(|window, _| window.remove_window());
        cx.run_until_parked();
        // The app intentionally owns pending work. After explicit cache removal
        // no weak consumer callback may keep the Task alive independently.
        cx.update(|cx| cx.remove_asset::<ProbeAsset>(&source));
        cx.run_until_parked();
        assert_eq!(source.futures_dropped.load(Ordering::SeqCst), 1);
    }

    #[::core::prelude::v1::test]
    fn ready_handoff_precedes_accounting_and_eviction_hook_panics() {
        for value in [254, 255] {
            let mut cx = TestAppContext::single();
            cx.update(|cx| cx.set_completed_asset_cache_limits(8, 0));
            let source = Source::new();
            let sender = source.next();
            let (consumer, window) = cx.add_window_view(|_, _| Consumer {
                source: source.clone(),
                shown: true,
                last: None,
                distinct_deliveries: 0,
            });
            window.update(|window, cx| window.draw(cx).clear());
            window.run_until_parked();
            let (output, weak) = payload(value);
            assert!(sender.send(output).is_ok());
            window.run_until_parked();
            window.simulate_platform_frame();
            window.run_until_parked();
            window.update(|_, cx| {
                assert_eq!(consumer.read(cx).last, Some(value));
                assert_eq!(consumer.read(cx).distinct_deliveries, 1);
                assert_eq!(cx.completed_asset_count(), 0);
                assert_eq!(cx.completed_asset_bytes(), 0);
            });
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
            window.update(|window, cx| {
                consumer.update(cx, |consumer, cx| {
                    consumer.shown = false;
                    cx.notify();
                });
                window.draw(cx).clear();
            });
            window.run_until_parked();
            assert!(weak.upgrade().is_none());
        }
    }

    #[::core::prelude::v1::test]
    fn external_old_generation_cannot_publish_into_replacement_consumer() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, 0));
        let source = Source::new();
        let old_sender = source.next();
        let next_sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: source.clone(),
            shown: true,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let external = window.update(|window, cx| {
            let external = cx.fetch_asset_checked::<ProbeAsset>(&source).unwrap().0;
            cx.remove_asset::<ProbeAsset>(&source);
            window.draw(cx).clear();
            external
        });
        window.run_until_parked();
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        let (old, old_weak) = payload(17);
        assert!(old_sender.send(old).is_ok());
        window.run_until_parked();
        assert_eq!(external.clone().now_or_never().unwrap().value, 17);
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, None);
            assert_eq!(consumer.read(cx).distinct_deliveries, 0);
        });
        drop(external);
        window.run_until_parked();
        assert!(old_weak.upgrade().is_none());
        let (next, _) = payload(29);
        assert!(next_sender.send(next).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(29));
            assert_eq!(consumer.read(cx).distinct_deliveries, 1);
        });
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    #[::core::prelude::v1::test]
    fn get_asset_outside_drawing_keeps_legacy_pending_and_ready_polling() {
        let mut cx = TestAppContext::single();
        let source = Source::new();
        let sender = source.next();
        let (_, window) = cx.add_window_view(|_, _| Consumer {
            source: source.clone(),
            shown: false,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| {
            assert!(window.get_asset::<ProbeAsset>(&source, cx).is_none());
        });
        window.run_until_parked();
        let (output, _) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(
                window.get_asset::<ProbeAsset>(&source, cx).unwrap().value,
                17
            );
        });
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }

    struct ErrorProbeAsset;
    impl Asset for ErrorProbeAsset {
        type Source = Source;
        type Output = Result<(), Arc<Payload>>;
        fn load(
            source: Source,
            cx: &mut App,
        ) -> impl Future<Output = Self::Output> + Send + 'static {
            let load = ProbeAsset::load(source, cx);
            async move { Err(load.await) }
        }
        fn cache_bytes(output: &Self::Output) -> u64 {
            output
                .as_ref()
                .err()
                .map_or(0, |error| error.bytes.len() as u64)
        }
    }
    struct ErrorConsumer {
        source: Source,
        last: Option<u8>,
        distinct_deliveries: usize,
    }
    impl Render for ErrorConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if let Some(Err(output)) = window.use_asset::<ErrorProbeAsset>(&self.source, cx) {
                if self.last != Some(output.value) {
                    self.last = Some(output.value);
                    self.distinct_deliveries += 1;
                }
            }
            div()
        }
    }

    #[::core::prelude::v1::test]
    fn active_error_result_survives_lru_and_clear_until_explicit_remove_retries() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let source = Source::new();
        let first_sender = source.next();
        let next_sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| ErrorConsumer {
            source: source.clone(),
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (first_error, first_weak) = payload(17);
        assert!(first_sender.send(first_error).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(consumer.read(cx).last, Some(17));
            assert_eq!(cx.completed_asset_count(), 0);
            cx.clear_completed_asset_cache_checked().unwrap();
        });
        for _ in 0..3 {
            window.update(|window, cx| window.draw(cx).clear());
        }
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        assert!(first_weak.upgrade().is_some());
        window.update(|window, cx| {
            cx.remove_asset::<ErrorProbeAsset>(&source);
            window.draw(cx).clear();
        });
        window.run_until_parked();
        assert!(first_weak.upgrade().is_none());
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
        let (next_error, _) = payload(29);
        assert!(next_sender.send(next_error).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(consumer.read(cx).last, Some(29));
            assert_eq!(consumer.read(cx).distinct_deliveries, 2);
        });
        assert_eq!(source.calls.load(Ordering::SeqCst), 2);
    }

    fn probe_consumer_cells(window: &Window) -> usize {
        [&window.rendered_frame, &window.next_frame]
            .into_iter()
            .flat_map(|frame| frame.element_states.keys())
            .filter(|(_, state_type)| {
                *state_type == std::any::TypeId::of::<super::AssetConsumer<ProbeAsset>>()
            })
            .count()
    }

    struct CountingConsumer {
        source: Source,
        renders: std::rc::Rc<std::cell::Cell<usize>>,
        last: Option<u8>,
    }

    impl Render for CountingConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            use crate::Styled as _;
            self.renders.set(self.renders.get() + 1);
            if let Some(output) = window.use_asset::<ProbeAsset>(&self.source, cx) {
                self.last = Some(output.value);
            }
            div().size_full().child("cached consumer")
        }
    }

    struct CachedConsumerHost {
        child: crate::Entity<CountingConsumer>,
        mounted: bool,
        revision: usize,
    }

    impl Render for CachedConsumerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::Styled as _;
            let mut style = crate::StyleRefinement::default();
            style.size.width = Some(crate::px(200.0).into());
            style.size.height = Some(crate::px(40.0).into());
            let root = div()
                .flex()
                .flex_col()
                .child(format!("revision {}", self.revision));
            if self.mounted {
                root.child(crate::AnyView::from(self.child.clone()).cached(style))
            } else {
                root
            }
        }
    }

    #[::core::prelude::v1::test]
    fn cached_view_skip_keeps_ready_consumer_until_subtree_unmount() {
        use crate::AppContext as _;
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let source = Source::new();
        let sender = source.next();
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let child = cx.new(|_| CountingConsumer {
            source: source.clone(),
            renders: renders.clone(),
            last: None,
        });
        let (host, window) = cx.add_window_view(|_, _| CachedConsumerHost {
            child: child.clone(),
            mounted: true,
            revision: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (output, weak) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(child.read(cx).last, Some(17));
            assert_eq!(probe_consumer_cells(window), 1);
            assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 0, 0));
        });
        let warm_renders = renders.get();
        for revision in 1..=4 {
            window.update(|window, cx| {
                host.update(cx, |host, cx| {
                    host.revision = revision;
                    cx.notify();
                });
                window.draw(cx).clear();
                assert_eq!(
                    renders.get(),
                    warm_renders,
                    "cached child Render must actually be skipped"
                );
                assert_eq!(probe_consumer_cells(window), 1);
                assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 0, 0));
            });
            assert!(weak.upgrade().is_some());
        }
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.mounted = false;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(probe_consumer_cells(window), 0);
            assert_eq!(cx.asset_consumer_subscription_stats(), (0, 0, 0, 0));
        });
        window.run_until_parked();
        assert!(
            weak.upgrade().is_none(),
            "retained Entity handle does not own the frame payload"
        );
    }

    struct NamespacedConsumer {
        source: Source,
        mounted: [bool; 2],
        last: [Option<u8>; 2],
    }

    impl Render for NamespacedConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            for (index, name) in ["left", "right"].into_iter().enumerate() {
                if self.mounted[index] {
                    let output = window.with_element_namespace(name, |window| {
                        window.use_asset::<ProbeAsset>(&self.source, cx)
                    });
                    if let Some(output) = output {
                        self.last[index] = Some(output.value);
                    }
                }
            }
            div().child("namespaced consumers")
        }
    }

    #[::core::prelude::v1::test]
    fn same_source_in_separate_namespaces_has_independent_frame_ownership() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let source = Source::new();
        let sender = source.next();
        let (host, window) = cx.add_window_view(|_, _| NamespacedConsumer {
            source: source.clone(),
            mounted: [true, true],
            last: [None, None],
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(probe_consumer_cells(window), 2);
            assert_eq!(cx.asset_consumer_subscription_stats(), (1, 2, 1, 2));
        });
        window.run_until_parked();
        let (output, weak) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(host.read(cx).last, [Some(17), Some(17)]);
            host.update(cx, |host, cx| {
                host.mounted[0] = false;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(probe_consumer_cells(window), 1);
            assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 0, 0));
        });
        assert!(weak.upgrade().is_some());
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.mounted[1] = false;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(probe_consumer_cells(window), 0);
            assert_eq!(cx.asset_consumer_subscription_stats(), (0, 0, 0, 0));
        });
        window.run_until_parked();
        assert!(weak.upgrade().is_none());
    }

    #[::core::prelude::v1::test]
    fn repeated_pending_and_ready_mounts_retire_frame_and_subscriber_metadata() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let first = Source::new();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: first,
            shown: false,
            last: None,
            distinct_deliveries: 0,
        });
        for cycle in 0..16 {
            let mut source = Source::new();
            source.key = cycle + 100;
            let sender = source.next();
            window.update(|window, cx| {
                consumer.update(cx, |consumer, cx| {
                    consumer.source = source.clone();
                    consumer.shown = true;
                    consumer.last = None;
                    cx.notify();
                });
                for _ in 0..3 {
                    window.draw(cx).clear();
                }
                assert_eq!(probe_consumer_cells(window), 1);
                assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 1, 1));
            });
            window.run_until_parked();
            let weak = if cycle % 2 == 0 {
                let (output, weak) = payload(17);
                assert!(sender.send(output).is_ok());
                window.run_until_parked();
                window.update(|window, cx| {
                    window.draw(cx).clear();
                    assert_eq!(consumer.read(cx).last, Some(17));
                    assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 0, 0));
                });
                Some(weak)
            } else {
                drop(sender);
                None
            };
            window.update(|window, cx| {
                consumer.update(cx, |consumer, cx| {
                    consumer.shown = false;
                    cx.notify();
                });
                window.draw(cx).clear();
                assert_eq!(probe_consumer_cells(window), 0);
                assert_eq!(
                    cx.asset_consumer_subscription_stats(),
                    (0, 0, 0, 0),
                    "retired keys must not accumulate tombstones"
                );
                cx.remove_asset::<ProbeAsset>(&source);
            });
            window.run_until_parked();
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
            assert_eq!(source.futures_dropped.load(Ordering::SeqCst), 1);
            if let Some(weak) = weak {
                assert!(weak.upgrade().is_none());
            }
        }
    }

    struct GetOnlyConsumer {
        source: Source,
        last: Option<u8>,
    }
    impl Render for GetOnlyConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if let Some(output) = window.get_asset::<ProbeAsset>(&self.source, cx) {
                self.last = Some(output.value);
            }
            div().child("get-only consumer")
        }
    }

    #[::core::prelude::v1::test]
    fn get_only_consumer_receives_on_repaint_without_requesting_view_notifications() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let source = Source::new();
        let sender = source.next();
        let (consumer, window) = cx.add_window_view(|_, _| GetOnlyConsumer {
            source: source.clone(),
            last: None,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let _subscription = window.update(|_, cx| {
            let notifications = notifications.clone();
            cx.observe(&consumer, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let (output, weak) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        assert_eq!(
            notifications.get(),
            0,
            "get_asset does not opt into ready notifications"
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(17));
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
            cx.remove_asset::<ProbeAsset>(&source);
        });
        window.run_until_parked();
        assert_eq!(
            notifications.get(),
            0,
            "get-only explicit invalidation also remains opt-out"
        );
        assert!(weak.upgrade().is_none());
    }

    #[::core::prelude::v1::test]
    fn critical_pressure_preserves_active_ready_and_cancels_only_pending_cache_owners() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let ready_source = Source::new();
        let sender = ready_source.next();
        let (consumer, window) = cx.add_window_view(|_, _| Consumer {
            source: ready_source.clone(),
            shown: true,
            last: None,
            distinct_deliveries: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (output, weak) = payload(17);
        assert!(sender.send(output).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            cx.notify_memory_pressure_checked(crate::MemoryPressureLevel::Critical)
                .unwrap();
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(17));
        });
        assert!(weak.upgrade().is_some());
        assert_eq!(ready_source.calls.load(Ordering::SeqCst), 1);
        let mut pending = Source::new();
        pending.key = 2;
        let old_sender = pending.next();
        let replacement_sender = pending.next();
        window.update(|window, cx| {
            consumer.update(cx, |consumer, cx| {
                consumer.source = pending.clone();
                consumer.last = None;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        window.run_until_parked();
        assert!(weak.upgrade().is_none());
        window.update(|_, cx| {
            cx.notify_memory_pressure_checked(crate::MemoryPressureLevel::Critical)
                .unwrap();
        });
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        assert_eq!(
            pending.futures_dropped.load(Ordering::SeqCst),
            1,
            "cache-only retired load must cancel"
        );
        assert_eq!(
            pending.calls.load(Ordering::SeqCst),
            2,
            "current consumer must progress with a fresh request"
        );
        let (old, old_weak) = payload(23);
        assert!(old_sender.send(old).is_err());
        assert!(old_weak.upgrade().is_none());
        let (replacement, replacement_weak) = payload(29);
        assert!(replacement_sender.send(replacement).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(29));
        });
        assert!(replacement_weak.upgrade().is_some());
        window.update(|window, cx| {
            consumer.update(cx, |consumer, cx| {
                consumer.shown = false;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        window.run_until_parked();
        assert!(replacement_weak.upgrade().is_none());
    }

    #[derive(Clone)]
    struct ReentrantSource {
        source: Source,
        evictions: Arc<AtomicUsize>,
    }
    impl Hash for ReentrantSource {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.source.hash(state);
        }
    }
    struct ReentrantAsset;
    impl Asset for ReentrantAsset {
        type Source = ReentrantSource;
        type Output = (Arc<Payload>, ReentrantSource);
        fn load(
            source: Self::Source,
            cx: &mut App,
        ) -> impl Future<Output = Self::Output> + Send + 'static {
            let load = ProbeAsset::load(source.source.clone(), cx);
            async move { (load.await, source) }
        }
        fn cache_bytes(output: &Self::Output) -> u64 {
            output.0.bytes.len() as u64
        }
        fn on_cache_evict(output: &Self::Output, cx: &mut App) {
            if output.1.evictions.fetch_add(1, Ordering::SeqCst) == 0 {
                cx.remove_asset::<Self>(&output.1);
                drop(cx.fetch_asset_checked::<Self>(&output.1).unwrap().0);
            }
        }
    }
    struct ReentrantConsumer {
        source: ReentrantSource,
        last: Option<u8>,
    }
    impl Render for ReentrantConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if let Some((output, _)) = window.use_asset::<ReentrantAsset>(&self.source, cx) {
                self.last = Some(output.value);
            }
            div().child("reentrant consumer")
        }
    }
    #[::core::prelude::v1::test]
    fn reentrant_eviction_explicitly_invalidates_old_ready_and_admits_replacement_generation() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(0, 0));
        let source = ReentrantSource {
            source: Source::new(),
            evictions: Arc::new(AtomicUsize::new(0)),
        };
        let first_sender = source.source.next();
        let second_sender = source.source.next();
        let (consumer, window) = cx.add_window_view(|_, _| ReentrantConsumer {
            source: source.clone(),
            last: None,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        let (first, first_weak) = payload(17);
        assert!(first_sender.send(first).is_ok());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(
                consumer.read(cx).last,
                None,
                "explicit reentrant remove invalidates old ready before repaint"
            );
        });
        assert!(first_weak.upgrade().is_none());
        assert_eq!(source.source.calls.load(Ordering::SeqCst), 2);
        let (second, second_weak) = payload(29);
        assert!(second_sender.send(second).is_ok());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(29));
            assert_eq!(cx.completed_asset_count(), 0);
            assert_eq!(cx.asset_consumer_subscription_stats(), (1, 1, 0, 0));
        });
        assert_eq!(source.evictions.load(Ordering::SeqCst), 2);
        assert!(second_weak.upgrade().is_some());
    }

    #[::core::prelude::v1::test]
    fn preloaded_ready_asset_is_delivered_inline_without_notifying_rendering_owner() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, 8));
        let source = Source::new();
        let sender = source.next();
        let _external = cx.update(|cx| cx.fetch_asset_checked::<ProbeAsset>(&source).unwrap().0);
        cx.run_until_parked();
        let (output, _) = payload(17);
        assert!(sender.send(output).is_ok());
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(cx.completed_asset_count(), 1));
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let renders = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut subscription = None;
        let (consumer, window) = cx.add_window_view(|_, cx| {
            let notifications = notifications.clone();
            subscription = Some(cx.observe_self(move |_, _| {
                notifications.set(notifications.get() + 1);
            }));
            CountingConsumer {
                source: source.clone(),
                renders: renders.clone(),
                last: None,
            }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(consumer.read(cx).last, Some(17));
        });
        window.run_until_parked();
        eprintln!(
            "inline-ready owner-notifications={} renders={}",
            notifications.get(),
            renders.get()
        );
        assert_eq!(
            notifications.get(),
            0,
            "an inline ready result needs no owner invalidation"
        );
        let settled_renders = renders.get();
        for _ in 0..2 {
            window.simulate_platform_frame();
            window.run_until_parked();
        }
        assert_eq!(
            renders.get(),
            settled_renders,
            "a clean ordinary frame must not render again"
        );
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        drop(subscription);
    }
}

// Checked admission must cover synchronous initialization and reentry.
#[cfg(test)]
mod checked_asset_reentrancy_regressions {
    use crate::{App, Asset, TestAppContext};
    use futures::Future;
    use std::{
        hash::{Hash, Hasher},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[derive(Clone, Debug)]
    struct NestedObservation {
        accepted: bool,
        created: Option<bool>,
        pending_before: usize,
        pending_after: usize,
    }

    #[derive(Default)]
    struct Probe {
        outer_loads: AtomicUsize,
        leaf_loads: AtomicUsize,
        same_key_loads: AtomicUsize,
        source_clones: AtomicUsize,
        nested: Mutex<Option<NestedObservation>>,
    }

    struct Source {
        key: u8,
        reenter_same_key: bool,
        panic_on_clone: bool,
        panic_on_load: bool,
        probe: Arc<Probe>,
    }

    impl Source {
        fn new(key: u8, probe: &Arc<Probe>) -> Self {
            Self {
                key,
                reenter_same_key: false,
                panic_on_clone: false,
                panic_on_load: false,
                probe: probe.clone(),
            }
        }
    }

    impl Clone for Source {
        fn clone(&self) -> Self {
            self.probe.source_clones.fetch_add(1, Ordering::SeqCst);
            assert!(!self.panic_on_clone, "controlled source Clone panic");
            Self {
                key: self.key,
                reenter_same_key: self.reenter_same_key,
                panic_on_clone: self.panic_on_clone,
                panic_on_load: self.panic_on_load,
                probe: self.probe.clone(),
            }
        }
    }

    impl Hash for Source {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.key.hash(state);
        }
    }

    // One bounded synchronous reentry. The result is observed before acquiring
    // the Probe mutex, so no callback occurs under a fixture lock.
    fn nested_checked<A: Asset<Source = Source, Output = u8>>(
        cx: &mut App,
        source: &Source,
    ) -> NestedObservation {
        let pending_before = cx.pending_asset_count();
        let result = cx.fetch_asset_checked::<A>(source);
        let observation = NestedObservation {
            accepted: result.is_ok(),
            created: result.as_ref().ok().map(|(_, created)| *created),
            pending_before,
            pending_after: cx.pending_asset_count(),
        };
        drop(result);
        observation
    }

    struct LeafAsset;
    impl Asset for LeafAsset {
        type Source = Source;
        type Output = u8;

        fn load(source: Source, _: &mut App) -> impl Future<Output = u8> + Send + 'static {
            source.probe.leaf_loads.fetch_add(1, Ordering::SeqCst);
            assert!(!source.panic_on_load, "controlled synchronous loader panic");
            futures::future::pending()
        }
    }

    struct OuterAsset;
    impl Asset for OuterAsset {
        type Source = Source;
        type Output = u8;

        fn load(source: Source, cx: &mut App) -> impl Future<Output = u8> + Send + 'static {
            source.probe.outer_loads.fetch_add(1, Ordering::SeqCst);
            let inner = Source::new(1, &source.probe);
            // Distinct keys/types and a non-reentrant leaf: no loader cycle.
            let observation = nested_checked::<LeafAsset>(cx, &inner);
            *source.probe.nested.lock().unwrap() = Some(observation);
            futures::future::pending()
        }
    }

    struct SameKeyAsset;
    impl Asset for SameKeyAsset {
        type Source = Source;
        type Output = u8;

        fn load(source: Source, cx: &mut App) -> impl Future<Output = u8> + Send + 'static {
            source.probe.same_key_loads.fetch_add(1, Ordering::SeqCst);
            if source.reenter_same_key {
                // Explicitly disarm the nested attempt. At most two old-loader
                // invocations can occur; there is no unbounded recursion.
                let inner = Source::new(source.key, &source.probe);
                let observation = nested_checked::<SameKeyAsset>(cx, &inner);
                *source.probe.nested.lock().unwrap() = Some(observation);
            }
            futures::future::pending()
        }
    }

    #[test]
    fn checked_reentrant_different_key_cannot_exceed_one_pending_admission() {
        let cx = TestAppContext::single();
        let probe = Arc::new(Probe::default());
        let outer = Source::new(0, &probe);
        let inner = Source::new(1, &probe);
        assert_ne!(
            crate::asset_cache::hash(&outer),
            crate::asset_cache::hash(&inner)
        );
        cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let (task, created) = cx.fetch_asset_checked::<OuterAsset>(&outer).unwrap();
            let observation = probe.nested.lock().unwrap().clone().unwrap();
            let pending = cx.pending_asset_count();
            let outer_loads = probe.outer_loads.load(Ordering::SeqCst);
            let leaf_loads = probe.leaf_loads.load(Ordering::SeqCst);
            eprintln!(
                "checked-reentrant different-key: limit=1 pending={pending} outer-loads={outer_loads} leaf-loads={leaf_loads} nested-accepted={} nested-created={:?} nested-before={} nested-after={}",
                observation.accepted, observation.created,
                observation.pending_before, observation.pending_after,
            );
            assert!(created);
            assert_eq!(pending, 1, "checked reentrant work must obey the pending cap");
            assert!(!observation.accepted, "outer admission must reserve the only slot before loader callbacks");
            assert_eq!(outer_loads, 1);
            assert_eq!(leaf_loads, 0, "a rejected child must not invoke its loader");
            drop(task);
        });
    }

    #[test]
    fn checked_reentrant_same_key_does_not_start_duplicate_loader_and_later_fetch_shares() {
        let cx = TestAppContext::single();
        let probe = Arc::new(Probe::default());
        let mut source = Source::new(0, &probe);
        source.reenter_same_key = true;
        cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let (task, created) = cx.fetch_asset_checked::<SameKeyAsset>(&source).unwrap();
            let (shared, created_again) = cx.fetch_asset_checked::<SameKeyAsset>(&source).unwrap();
            let observation = probe.nested.lock().unwrap().clone().unwrap();
            let calls = probe.same_key_loads.load(Ordering::SeqCst);
            eprintln!(
                "checked-reentrant same-key: limit=1 pending={} loads={calls} outer-created={created} later-created={created_again} nested-accepted={} nested-created={:?} nested-before={} nested-after={}",
                cx.pending_asset_count(), observation.accepted, observation.created,
                observation.pending_before, observation.pending_after,
            );
            assert_eq!(calls, 1, "initializing the same key must not start a second loader");
            assert_eq!(cx.pending_asset_count(), 1);
            assert!(created);
            assert!(!created_again, "ordinary same-key fetch still shares after publication");
            // Rejecting the initializing-key request is valid; a real shared
            // placeholder is also valid if no duplicate loader was constructed.
            if observation.accepted {
                assert_eq!(observation.created, Some(false));
            }
            drop((task, shared));
        });
    }

    #[test]
    fn checked_reentrant_distinct_key_progresses_when_two_slots_are_available() {
        let cx = TestAppContext::single();
        let probe = Arc::new(Probe::default());
        let outer = Source::new(0, &probe);
        cx.update(|cx| {
            cx.set_pending_asset_limit(2);
            let (task, created) = cx.fetch_asset_checked::<OuterAsset>(&outer).unwrap();
            let (shared, created_again) = cx.fetch_asset_checked::<OuterAsset>(&outer).unwrap();
            let observation = probe.nested.lock().unwrap().clone().unwrap();
            assert!(created && !created_again);
            assert!(observation.accepted);
            assert_eq!(observation.created, Some(true));
            assert_eq!(probe.outer_loads.load(Ordering::SeqCst), 1);
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), 1);
            assert_eq!(cx.pending_asset_count(), 2);
            let third = Source::new(2, &probe);
            assert!(cx.fetch_asset_checked::<LeafAsset>(&third).is_err());
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), 1);
            drop((task, shared));
        });
    }

    fn assert_panicking_initialization_releases_admission(panic_on_clone: bool) {
        let cx = TestAppContext::single();
        let probe = Arc::new(Probe::default());
        let mut source = Source::new(0, &probe);
        source.panic_on_clone = panic_on_clone;
        source.panic_on_load = !panic_on_clone;
        cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let panic = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cx.fetch_asset_checked::<LeafAsset>(&source)
            })) {
                Err(panic) => panic,
                Ok(_) => panic!("controlled source/loader panic did not occur"),
            };
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&'static str>().copied())
                .unwrap();
            assert_eq!(
                message,
                if panic_on_clone {
                    "controlled source Clone panic"
                } else {
                    "controlled synchronous loader panic"
                }
            );
            let expected_calls = usize::from(!panic_on_clone);
            eprintln!(
                "checked-init panic: source-clone={panic_on_clone} pending={} clones={} loads={}",
                cx.pending_asset_count(),
                probe.source_clones.load(Ordering::SeqCst),
                probe.leaf_loads.load(Ordering::SeqCst),
            );
            assert_eq!(cx.pending_asset_count(), 0);
            assert_eq!(probe.source_clones.load(Ordering::SeqCst), 1);
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), expected_calls);

            let retry = Source::new(0, &probe);
            let (task, created) = cx.fetch_asset_checked::<LeafAsset>(&retry).unwrap();
            let (shared, created_again) = cx.fetch_asset_checked::<LeafAsset>(&retry).unwrap();
            assert!(created && !created_again);
            assert_eq!(cx.pending_asset_count(), 1);
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), expected_calls + 1);
            cx.remove_asset::<LeafAsset>(&retry);
            assert_eq!(cx.pending_asset_count(), 0);
            drop((task, shared));
            let different = Source::new(1, &probe);
            let (next, created) = cx.fetch_asset_checked::<LeafAsset>(&different).unwrap();
            assert!(created);
            assert_eq!(cx.pending_asset_count(), 1);
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), expected_calls + 2);
            drop(next);
        });
    }

    #[test]
    fn synchronous_loader_panic_leaves_no_admission_or_initializing_key_reservation() {
        assert_panicking_initialization_releases_admission(false);
    }

    #[test]
    fn source_clone_panic_leaves_no_admission_or_initializing_key_reservation() {
        assert_panicking_initialization_releases_admission(true);
    }

    #[test]
    fn explicit_legacy_fetch_keeps_its_documented_uncapped_pending_contract() {
        let cx = TestAppContext::single();
        let probe = Arc::new(Probe::default());
        let first = Source::new(0, &probe);
        let second = Source::new(1, &probe);
        cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let (one, new_one) = cx.fetch_asset::<LeafAsset>(&first);
            let (two, new_two) = cx.fetch_asset::<LeafAsset>(&second);
            assert!(new_one && new_two);
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), 2);
            assert_eq!(cx.pending_asset_count(), 2);
            // Checked callers still see the legacy-owned pending entries.
            assert!(
                cx.fetch_asset_checked::<LeafAsset>(&Source::new(2, &probe))
                    .is_err()
            );
            assert_eq!(probe.leaf_loads.load(Ordering::SeqCst), 2);
            drop((one, two));
        });
    }
}

// A failed initializer must release admission before waking a deferred view.
#[cfg(test)]
mod checked_asset_initialization_wake_tests {
    use crate::{
        AnyWindowHandle, App, Asset, Context, IntoElement, ParentElement, Render, TestAppContext,
        VisualContext as _, Window, div,
    };
    use futures::Future;
    use std::{
        cell::Cell,
        hash::{Hash, Hasher},
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    #[derive(Clone)]
    struct RetrySource(Arc<AtomicUsize>);
    impl Hash for RetrySource {
        fn hash<H: Hasher>(&self, state: &mut H) {
            1_u8.hash(state);
        }
    }

    struct RetryAsset;
    impl Asset for RetryAsset {
        type Source = RetrySource;
        type Output = u8;

        fn load(source: RetrySource, _: &mut App) -> impl Future<Output = u8> + Send + 'static {
            source.0.fetch_add(1, Ordering::SeqCst);
            futures::future::ready(17)
        }
    }

    struct RetryConsumer {
        source: RetrySource,
        enabled: bool,
        last: Option<u8>,
        renders: usize,
    }
    impl Render for RetryConsumer {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            if self.enabled {
                self.last = window.use_asset::<RetryAsset>(&self.source, cx);
            }
            div().child(if self.last.is_some() {
                "ready"
            } else {
                "waiting"
            })
        }
    }

    #[derive(Clone)]
    struct PanickingSource {
        window: AnyWindowHandle,
        generation_before: u64,
        renders_before: usize,
        blocked_attempts: Arc<AtomicUsize>,
        panic_after_render: bool,
    }
    impl Hash for PanickingSource {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.window.hash(state);
        }
    }

    struct PanickingAsset;
    impl Asset for PanickingAsset {
        type Source = PanickingSource;
        type Output = u8;

        fn load(
            source: PanickingSource,
            cx: &mut App,
        ) -> impl Future<Output = u8> + Send + 'static {
            // This real drawing consumer requests a distinct asset while the
            // outer checked call owns the sole initialization reservation.
            let (renders, last, calls, generation) = source
                .window
                .update(cx, |view, window, cx| {
                    let view = view.downcast::<RetryConsumer>().unwrap();
                    // Deliberately no notify: the cleanup wake must be the only
                    // generation change before the outer App update finishes.
                    view.update(cx, |view, _| view.enabled = true);
                    window.draw(cx).clear();
                    let consumer = view.read(cx);
                    (
                        consumer.renders,
                        consumer.last,
                        consumer.source.0.load(Ordering::SeqCst),
                        cx.entity_generation(view.entity_id()),
                    )
                })
                .unwrap();
            // Restore the Window before assertions or the controlled panic.
            assert!(renders > source.renders_before);
            assert_eq!(last, None);
            assert_eq!(calls, 0);
            assert_eq!(
                generation,
                Some(source.generation_before),
                "the explicit blocked draw must not already notify the owner"
            );
            source.blocked_attempts.fetch_add(1, Ordering::SeqCst);
            assert!(
                !source.panic_after_render,
                "controlled loader panic after a drawing consumer deferred"
            );
            futures::future::pending()
        }
    }

    #[::core::prelude::v1::test]
    fn panicking_initialization_wakes_a_real_deferred_consumer_after_releasing_its_slot() {
        let mut cx = TestAppContext::single();
        let calls = Arc::new(AtomicUsize::new(0));
        let blocked_attempts = Arc::new(AtomicUsize::new(0));
        let (consumer, window) = cx.add_window_view(|_, _| RetryConsumer {
            source: RetrySource(calls.clone()),
            enabled: false,
            last: None,
            renders: 0,
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();
        window.simulate_platform_frame();
        window.run_until_parked();
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let notifications = Rc::new(Cell::new(0));
        let _observer = window.cx.update(|cx| {
            let notifications = notifications.clone();
            cx.observe(&consumer, move |_, _| {
                notifications.set(notifications.get() + 1)
            })
        });
        let (generation_before, renders_before) = window.cx.update(|cx| {
            (
                cx.entity_generation(consumer.entity_id()).unwrap(),
                consumer.read(cx).renders,
            )
        });
        let source = PanickingSource {
            window: window.window_handle(),
            generation_before,
            renders_before,
            blocked_attempts: blocked_attempts.clone(),
            panic_after_render: true,
        };
        // Use the App-only entry point so the loader can draw the owned window;
        // the window is not already borrowed out by VisualTestContext::update.
        window.cx.update(|cx| {
            cx.set_pending_asset_limit(1);
            let panic = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cx.fetch_asset_checked::<PanickingAsset>(&source)
            })) {
                Err(panic) => panic,
                Ok(_) => panic!("controlled loader panic did not occur"),
            };
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&'static str>().copied())
                .unwrap();
            assert_eq!(
                message,
                "controlled loader panic after a drawing consumer deferred"
            );
            assert_eq!(blocked_attempts.load(Ordering::SeqCst), 1);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(cx.pending_asset_count(), 0);
            assert_eq!(
                cx.entity_generation(consumer.entity_id()),
                Some(generation_before.wrapping_add(1)),
                "cleanup must notify the actual deferred owner before effects flush"
            );
        });
        // Do not force a draw after the panic. Normal platform callbacks plus
        // ordinary test effect flushing must admit and deliver the tiny asset.
        window.run_until_parked();
        for _ in 0..2 {
            window.simulate_platform_frame();
            window.run_until_parked();
        }
        window.cx.update(|cx| {
            assert_eq!(consumer.read(cx).last, Some(17));
            assert_eq!(cx.pending_asset_count(), 0);
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(notifications.get() >= 1);
    }
}
