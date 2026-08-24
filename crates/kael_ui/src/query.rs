//! Async data-loading helpers: a [`Loadable`](crate::query::Loadable) state enum,
//! an entity-friendly [`QueryState`](crate::query::QueryState) that manages fetch
//! lifecycles with stale-response dropping and optional debounce, and an in-memory
//! [`QueryCache`](crate::query::QueryCache) for deduplicating identical queries
//! within a TTL.

use kael::{Context, SharedString};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use web_time::Instant;

/// The lifecycle state of an asynchronously loaded value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Loadable<T> {
    /// No fetch has started.
    #[default]
    Idle,
    /// A fetch is in flight.
    Loading,
    /// A fetch completed successfully.
    Loaded(T),
    /// A fetch failed with the given message.
    Error(SharedString),
}

impl<T> Loadable<T> {
    /// Whether a fetch is currently in flight.
    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    /// Whether no fetch has started yet.
    pub fn is_idle(&self) -> bool {
        matches!(self, Loadable::Idle)
    }

    /// Whether the last fetch failed.
    pub fn is_error(&self) -> bool {
        matches!(self, Loadable::Error(_))
    }

    /// The loaded value, if the last fetch succeeded.
    pub fn as_loaded(&self) -> Option<&T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    /// The error message, if the last fetch failed.
    pub fn as_error(&self) -> Option<&SharedString> {
        match self {
            Loadable::Error(message) => Some(message),
            _ => None,
        }
    }

    /// Transforms a loaded value, preserving every other state.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Loadable<U> {
        match self {
            Loadable::Idle => Loadable::Idle,
            Loadable::Loading => Loadable::Loading,
            Loadable::Loaded(value) => Loadable::Loaded(f(value)),
            Loadable::Error(message) => Loadable::Error(message),
        }
    }
}

/// A monotonically increasing token identifying the most recent fetch.
///
/// Each [`QueryState::run`] bumps the generation; a response is only applied if
/// its captured generation still matches, which drops responses from runs that
/// were superseded by a newer fetch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation(u64);

impl Generation {
    fn next(self) -> Generation {
        Generation(self.0.wrapping_add(1))
    }
}

type LastQuery<T> = Box<dyn Fn() -> BoxFetch<T> + 'static>;

type BoxFetch<T> = std::pin::Pin<Box<dyn Future<Output = Result<T, SharedString>> + 'static>>;

/// Manages the fetch lifecycle for a single asynchronously loaded value.
///
/// Hold it in an entity (`cx.new(|_| QueryState::new())`) and drive it from a
/// view; the state writes `Loading`, then `Loaded`/`Error` back through the
/// entity with `cx.notify`, dropping any response from a run that was
/// superseded by a newer [`QueryState::run`].
pub struct QueryState<T: 'static> {
    state: Loadable<T>,
    generation: Generation,
    debounce: Option<Duration>,
    last_query: Option<LastQuery<T>>,
}

impl<T: 'static> Default for QueryState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> QueryState<T> {
    /// Creates an idle query state.
    pub fn new() -> Self {
        Self {
            state: Loadable::Idle,
            generation: Generation::default(),
            debounce: None,
            last_query: None,
        }
    }

    /// Sets a debounce delay applied before each fetch begins.
    pub fn debounce(mut self, delay: Duration) -> Self {
        self.debounce = Some(delay);
        self
    }

    /// The current loadable state.
    pub fn state(&self) -> &Loadable<T> {
        &self.state
    }

    /// Whether a fetch is currently in flight.
    pub fn is_loading(&self) -> bool {
        self.state.is_loading()
    }

    /// The loaded value, if the last fetch succeeded.
    pub fn as_loaded(&self) -> Option<&T> {
        self.state.as_loaded()
    }

    /// The current generation token.
    pub fn generation(&self) -> Generation {
        self.generation
    }

    /// Whether a response captured at `generation` is still the most recent run
    /// and should be applied.
    pub fn accepts(&self, generation: Generation) -> bool {
        self.generation == generation
    }

    /// Starts a fetch, transitioning to [`Loadable::Loading`] and writing the
    /// result back when it resolves.
    ///
    /// The factory is stored so [`QueryState::refetch`] can re-run the same
    /// query. Each call bumps the generation, so a slow response from an earlier
    /// call is dropped once a newer call has started.
    pub fn run<Fut>(&mut self, cx: &mut Context<Self>, fetch: impl Fn() -> Fut + 'static)
    where
        Fut: Future<Output = Result<T, SharedString>> + 'static,
    {
        self.last_query = Some(Box::new(move || Box::pin(fetch())));
        self.spawn_current(cx);
    }

    /// Re-runs the most recent query, if any has been started.
    pub fn refetch(&mut self, cx: &mut Context<Self>) {
        if self.last_query.is_some() {
            self.spawn_current(cx);
        }
    }

    fn spawn_current(&mut self, cx: &mut Context<Self>) {
        let Some(query) = self.last_query.as_ref() else {
            return;
        };

        self.generation = self.generation.next();
        let generation = self.generation;
        self.state = Loadable::Loading;
        cx.notify();

        let future = query();
        let debounce = self.debounce;

        cx.spawn(async move |this, cx| {
            if let Some(delay) = debounce {
                cx.background_executor().timer(delay).await;
            }

            let outcome = future.await;

            _ = this.update(cx, |this, cx| {
                if !this.accepts(generation) {
                    return;
                }
                this.state = match outcome {
                    Ok(value) => Loadable::Loaded(value),
                    Err(message) => Loadable::Error(message),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

struct CacheEntry<T> {
    value: T,
    inserted_at: Instant,
}

/// A small in-memory cache keyed by query string, with per-entry TTL, for
/// deduplicating identical queries.
pub struct QueryCache<T> {
    ttl: Duration,
    entries: HashMap<SharedString, CacheEntry<T>>,
}

impl<T: Clone> QueryCache<T> {
    /// Creates a cache whose entries expire after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    /// Returns a cached value if it is present and not expired.
    pub fn get(&self, key: &str) -> Option<T> {
        self.get_at(key, Instant::now())
    }

    fn get_at(&self, key: &str, now: Instant) -> Option<T> {
        self.entries.get(key).and_then(|entry| {
            if now.duration_since(entry.inserted_at) < self.ttl {
                Some(entry.value.clone())
            } else {
                None
            }
        })
    }

    /// Inserts or replaces a cached value, resetting its TTL.
    pub fn insert(&mut self, key: impl Into<SharedString>, value: T) {
        self.insert_at(key, value, Instant::now());
    }

    fn insert_at(&mut self, key: impl Into<SharedString>, value: T, now: Instant) {
        self.entries.insert(
            key.into(),
            CacheEntry {
                value,
                inserted_at: now,
            },
        );
    }

    /// Removes a single cached entry.
    pub fn invalidate(&mut self, key: &str) {
        self.entries.remove(key);
    }

    /// Drops every cached entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The number of entries currently held, including any that are expired but
    /// not yet evicted.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn loadable_helpers_cover_every_state() {
        let idle: Loadable<i32> = Loadable::Idle;
        assert!(idle.is_idle());
        assert!(!idle.is_loading());
        assert_eq!(idle.as_loaded(), None);

        let loading: Loadable<i32> = Loadable::Loading;
        assert!(loading.is_loading());

        let loaded = Loadable::Loaded(7);
        assert_eq!(loaded.as_loaded(), Some(&7));
        assert!(!loaded.is_loading());

        let error: Loadable<i32> = Loadable::Error("boom".into());
        assert!(error.is_error());
        assert_eq!(error.as_error().map(|m| m.to_string()), Some("boom".into()));
    }

    #[::core::prelude::v1::test]
    fn loadable_map_only_transforms_loaded() {
        assert_eq!(Loadable::Loaded(2).map(|v| v * 10), Loadable::Loaded(20));
        assert_eq!(Loadable::<i32>::Loading.map(|v| v * 10), Loadable::Loading);
        assert_eq!(Loadable::<i32>::Idle.map(|v| v * 10), Loadable::Idle);
        assert_eq!(
            Loadable::<i32>::Error("e".into()).map(|v| v * 10),
            Loadable::Error("e".into())
        );
    }

    #[::core::prelude::v1::test]
    fn generation_gating_drops_stale_responses() {
        let mut state: QueryState<i32> = QueryState::new();
        let first = state.generation();

        state.generation = state.generation.next();
        let second = state.generation();

        assert!(!state.accepts(first));
        assert!(state.accepts(second));

        state.generation = state.generation.next();
        assert!(!state.accepts(second));
        assert!(state.accepts(state.generation()));
    }

    #[::core::prelude::v1::test]
    fn cache_returns_value_within_ttl_and_expires_after() {
        let mut cache: QueryCache<i32> = QueryCache::new(Duration::from_secs(10));
        let start = Instant::now();

        cache.insert_at("users", 42, start);

        assert_eq!(cache.get_at("users", start), Some(42));
        assert_eq!(
            cache.get_at("users", start + Duration::from_secs(9)),
            Some(42)
        );
        assert_eq!(cache.get_at("users", start + Duration::from_secs(11)), None);
        assert_eq!(cache.get_at("missing", start), None);
    }

    #[::core::prelude::v1::test]
    fn cache_invalidate_and_clear() {
        let mut cache: QueryCache<i32> = QueryCache::new(Duration::from_secs(60));
        cache.insert("a", 1);
        cache.insert("b", 2);
        assert_eq!(cache.len(), 2);

        cache.invalidate("a");
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(2));

        cache.clear();
        assert!(cache.is_empty());
    }
}
