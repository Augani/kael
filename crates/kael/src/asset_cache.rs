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
