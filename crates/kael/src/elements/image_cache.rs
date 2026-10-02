use crate::{
    AnyElement, AnyEntity, App, AppContext, Asset, AssetLogger, Bounds, Element, ElementId, Entity,
    GlobalElementId, ImageAssetLoader, ImageCacheError, InspectorElementId, IntoElement, LayoutId,
    MemoryPressureLevel, ParentElement, Pixels, RenderImage, Resource, Style, StyleRefinement,
    Styled, Subscription, Task, WeakEntity, Window, hash,
};

use futures::{
    FutureExt,
    future::{AbortHandle, Abortable, Shared},
};
use refineable::Refineable;
use smallvec::SmallVec;
use std::{collections::HashMap, fmt, sync::Arc};

/// An image cache element, all its child img elements will use the cache specified by this element.
/// Note that this could as simple as passing an `Entity<T: ImageCache>`
pub fn image_cache(image_cache_provider: impl ImageCacheProvider) -> ImageCacheElement {
    ImageCacheElement {
        image_cache_provider: Box::new(image_cache_provider),
        style: StyleRefinement::default(),
        children: SmallVec::default(),
    }
}

/// A dynamically typed image cache, which can be used to store any image cache
#[derive(Clone)]
pub struct AnyImageCache {
    image_cache: AnyEntity,
    load_fn: fn(
        image_cache: &AnyEntity,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>>,
}

impl<I: ImageCache> From<Entity<I>> for AnyImageCache {
    fn from(image_cache: Entity<I>) -> Self {
        Self {
            image_cache: image_cache.into_any(),
            load_fn: any_image_cache::load::<I>,
        }
    }
}

impl AnyImageCache {
    /// Load an image given a resource
    /// returns the result of loading the image if it has finished loading, or None if it is still loading
    pub fn load(
        &self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        (self.load_fn)(&self.image_cache, resource, window, cx)
    }
}

mod any_image_cache {
    use super::*;

    pub(crate) fn load<I: 'static + ImageCache>(
        image_cache: &AnyEntity,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let image_cache = image_cache.clone().downcast::<I>().unwrap();
        image_cache.update(cx, |image_cache, cx| image_cache.load(resource, window, cx))
    }
}

/// An image cache element.
pub struct ImageCacheElement {
    image_cache_provider: Box<dyn ImageCacheProvider>,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 2]>,
}

impl ImageCacheElement {
    /// Number of child elements covered by this image cache scope.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!("image_cache_element(child_count={})", self.child_count())
    }
}

impl ParentElement for ImageCacheElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl Styled for ImageCacheElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for ImageCacheElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ImageCacheElement {
    type RequestLayoutState = SmallVec<[LayoutId; 4]>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let image_cache = self.image_cache_provider.provide(window, cx);
        window.with_image_cache(Some(image_cache), |window| {
            let child_layout_ids = self
                .children
                .iter_mut()
                .map(|child| child.request_layout(window, cx))
                .collect::<SmallVec<_>>();
            let mut style = Style::default();
            style.refine(&self.style);
            let layout_id = window.request_layout(style, child_layout_ids.iter().copied(), cx);
            (layout_id, child_layout_ids)
        })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        for child in &mut self.children {
            child.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let image_cache = self.image_cache_provider.provide(window, cx);
        window.with_image_cache(Some(image_cache), |window| {
            for child in &mut self.children {
                child.paint(window, cx);
            }
        })
    }
}

/// An image loading task associated with an image cache.
pub type ImageLoadingTask = Shared<Task<Result<Arc<RenderImage>, ImageCacheError>>>;

/// An image cache item
pub enum ImageCacheItem {
    /// The associated image is currently loading
    Loading(ImageLoadingTask),
    /// This item has loaded an image.
    Loaded(Result<Arc<RenderImage>, ImageCacheError>),
}

impl std::fmt::Debug for ImageCacheItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self {
            ImageCacheItem::Loading(_) => &"Loading...".to_string(),
            ImageCacheItem::Loaded(render_image) => &format!("{:?}", render_image),
        };
        f.debug_struct("ImageCacheItem")
            .field("status", status)
            .finish()
    }
}

impl ImageCacheItem {
    /// Stable status key for diagnostics and generated tests.
    pub fn status_key(&self) -> &'static str {
        match self {
            Self::Loading(_) => "loading",
            Self::Loaded(Ok(_)) => "loaded",
            Self::Loaded(Err(_)) => "error",
        }
    }

    /// Returns true when the image is still loading.
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }

    /// Returns true when the image has loaded successfully.
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(Ok(_)))
    }

    /// Returns true when loading resolved to an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Loaded(Err(_)))
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!("image_cache_item(status={})", self.status_key())
    }

    /// Attempt to get the image from the cache item.
    pub fn get(&mut self) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match self {
            ImageCacheItem::Loading(task) => {
                let res = task.now_or_never()?;
                *self = ImageCacheItem::Loaded(res.clone());
                Some(res)
            }
            ImageCacheItem::Loaded(res) => Some(res.clone()),
        }
    }
}

/// An object that can handle the caching and unloading of images.
/// Implementations of this trait should ensure that images are removed from all windows when they are no longer needed.
pub trait ImageCache: 'static {
    /// Load an image given a resource
    /// returns the result of loading the image if it has finished loading, or None if it is still loading
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>>;
}

/// An object that can create an ImageCache during the render phase.
/// See the ImageCache trait for more information.
pub trait ImageCacheProvider: 'static {
    /// Called during the request_layout phase to create an ImageCache.
    fn provide(&mut self, _window: &mut Window, _cx: &mut App) -> AnyImageCache;
}

impl<T: ImageCache> ImageCacheProvider for Entity<T> {
    fn provide(&mut self, _window: &mut Window, _cx: &mut App) -> AnyImageCache {
        self.clone().into()
    }
}

/// An [`ImageCache`] that retains every decoded image for the lifetime of the cache
/// Images are released together on critical memory pressure, when the cache entity
/// is dropped, or when [`RetainAllImageCache::clear`] is called. Pending work is
/// bounded to eight loads. Use this explicit retention policy when the working set is
/// bounded; for unbounded or churning image sets, scope the cache to a smaller subtree
/// (a shorter-lived element id) so it is dropped and reclaimed more often.
pub struct RetainAllImageCache {
    items: HashMap<u64, ImageCacheItem>,
    observers: HashMap<u64, Task<()>>,
    cancellations: HashMap<u64, AbortHandle>,
    weak_self: WeakEntity<Self>,
    pressure_subscription: Option<Subscription>,
}

impl fmt::Debug for RetainAllImageCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HashMapImageCache")
            .field("num_images", &self.items.len())
            .finish()
    }
}

impl Drop for RetainAllImageCache {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

impl RetainAllImageCache {
    /// Create a new image cache.
    #[inline]
    pub fn new(cx: &mut App) -> Entity<Self> {
        let e = cx.new(|cx| RetainAllImageCache {
            items: HashMap::new(),
            observers: HashMap::new(),
            cancellations: HashMap::new(),
            weak_self: cx.weak_entity(),
            pressure_subscription: None,
        });
        let weak = e.downgrade();
        let subscription = cx.on_memory_pressure(move |level, cx| {
            if level == MemoryPressureLevel::Critical {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |cache, cx| {
                        cache.cancel_pending();
                        cache.observers.clear();
                        for (_, mut item) in std::mem::take(&mut cache.items) {
                            if let Some(Ok(image)) = item.get() {
                                cx.drop_image(image, None);
                            }
                        }
                        cx.resume_deferred_asset_requests();
                    });
                });
            }
        });
        e.update(cx, |cache, _| {
            cache.pressure_subscription = Some(subscription)
        });
        cx.observe_release(&e, |image_cache, cx| {
            image_cache.cancel_pending();
            image_cache.observers.clear();
            for (_, mut item) in std::mem::take(&mut image_cache.items) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        e
    }

    /// Load an image from the given source.
    ///
    /// Returns `None` if the image is loading.
    pub fn load(
        &mut self,
        source: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let hash = hash(source);

        if let Some(item) = self.items.get_mut(&hash) {
            return item.get();
        }
        for item in self.items.values_mut() {
            if item.is_loading() {
                item.get();
            }
        }
        if self.loading_count() >= 8 {
            cx.defer_asset_retry(window.current_view());
            return Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "image cache pending-load limit reached; retry after a load completes"
            )))));
        }

        let fut = AssetLogger::<ImageAssetLoader>::load(source.clone(), cx);
        let (cancel, registration) = AbortHandle::new_pair();
        let fut = async move {
            Abortable::new(fut, registration).await.unwrap_or_else(|_| {
                Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                    "image cache load cancelled"
                ))))
            })
        };
        let task = cx.background_executor().spawn(fut).shared();
        self.cancellations.insert(hash, cancel);
        self.items
            .insert(hash, ImageCacheItem::Loading(task.clone()));

        let entity = window.current_view();
        let weak = self.weak_self.clone();
        let observer = cx.spawn(async move |cx| {
            let _ = task.await;
            let _ = cx.update(|cx| {
                let _ = weak.update(cx, |cache, _| {
                    if let Some(item) = cache.items.get_mut(&hash) {
                        item.get();
                    }
                    cache.observers.remove(&hash);
                    cache.cancellations.remove(&hash);
                });
                cx.notify(entity);
                cx.resume_deferred_asset_requests();
            });
        });
        self.observers.insert(hash, observer);

        None
    }

    /// Clear the image cache.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.cancel_pending();
        self.observers.clear();
        for (_, mut item) in std::mem::take(&mut self.items) {
            if let Some(Ok(image)) = item.get() {
                cx.drop_image(image, Some(window));
            }
        }
        cx.resume_deferred_asset_requests();
    }

    /// Remove the image from the cache by the given source.
    pub fn remove(&mut self, source: &Resource, window: &mut Window, cx: &mut App) {
        let hash = hash(source);
        if let Some(cancel) = self.cancellations.remove(&hash) {
            cancel.abort();
        }
        self.observers.remove(&hash);
        if let Some(mut item) = self.items.remove(&hash)
            && let Some(Ok(image)) = item.get()
        {
            cx.drop_image(image, Some(window));
        }
        cx.resume_deferred_asset_requests();
    }

    fn cancel_pending(&mut self) {
        // Mark the whole batch before releasing any permit. Otherwise a queued
        // sibling can start I/O when an active task is dropped during clearing.
        for (_, cancel) in self.cancellations.drain() {
            cancel.abort();
        }
    }

    /// Returns the number of images in the cache.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of entries that are still loading.
    pub fn loading_count(&self) -> usize {
        self.items.values().filter(|item| item.is_loading()).count()
    }

    /// Number of entries that have loaded successfully.
    pub fn loaded_count(&self) -> usize {
        self.items.values().filter(|item| item.is_loaded()).count()
    }

    /// Number of entries that resolved to an error.
    pub fn error_count(&self) -> usize {
        self.items.values().filter(|item| item.is_error()).count()
    }

    /// Stable policy key.
    pub fn policy_key(&self) -> &'static str {
        "retain_all"
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "retain_all_image_cache(policy={}, entry_count={}, loading_count={}, loaded_count={}, error_count={}, empty={})",
            self.policy_key(),
            self.len(),
            self.loading_count(),
            self.loaded_count(),
            self.error_count(),
            self.is_empty()
        )
    }
}

impl ImageCache for RetainAllImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        RetainAllImageCache::load(self, resource, window, cx)
    }
}

/// Constructs a retain-all image cache that uses the element state associated with the given ID.
pub fn retain_all(id: impl Into<ElementId>) -> RetainAllImageCacheProvider {
    RetainAllImageCacheProvider { id: id.into() }
}

/// A provider struct for creating a retain-all image cache inline
pub struct RetainAllImageCacheProvider {
    id: ElementId,
}

impl RetainAllImageCacheProvider {
    /// Stable policy key.
    pub fn policy_key(&self) -> &'static str {
        "retain_all"
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "retain_all_image_cache_provider(policy={})",
            self.policy_key()
        )
    }
}

impl ImageCacheProvider for RetainAllImageCacheProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> AnyImageCache {
        window
            .with_global_id(self.id.clone(), |global_id, window| {
                window.with_element_state::<Entity<RetainAllImageCache>, _>(
                    global_id,
                    |cache, _window| {
                        let mut cache = cache.unwrap_or_else(|| RetainAllImageCache::new(cx));
                        (cache.clone(), cache)
                    },
                )
            })
            .into()
    }
}

struct LruImageEntry {
    item: ImageCacheItem,
    last_used: u64,
    observer: Option<Task<()>>,
    cancellation: Option<AbortHandle>,
}

impl Drop for LruImageEntry {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancellation {
            cancel.abort();
        }
    }
}

/// Retention and background-work limits for an image cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageCacheLimits {
    /// Maximum completed entries, including errors; normalized to at least one.
    pub max_images: usize,
    /// Maximum bytes across all decoded frames. Zero rejects image decoding.
    pub max_decoded_bytes: u64,
    /// Maximum cache-owned in-flight image loads; normalized to 1..=64.
    pub max_pending_loads: usize,
}

impl ImageCacheLimits {
    /// A count-limited cache with a 64 MiB decoded budget and eight pending loads.
    pub fn new(max_images: usize) -> Self {
        Self {
            max_images: max_images.max(1),
            max_decoded_bytes: 64 * 1024 * 1024,
            max_pending_loads: 8,
        }
    }

    fn normalized(self) -> Self {
        Self {
            max_images: self.max_images.max(1),
            max_pending_loads: self.max_pending_loads.clamp(1, 64),
            ..self
        }
    }
}

/// Choose loaded entries to evict, least-recently-used first. Pending loads do
/// not consume decoded-image capacity, and the image requested by the caller
/// must remain available for the current frame.
#[cfg(test)]
fn select_lru_victims(
    entries: impl Iterator<Item = (u64, bool, u64)>,
    max_images: usize,
    protected_key: Option<u64>,
) -> Vec<u64> {
    let mut loaded: Vec<(u64, u64)> = entries
        .filter_map(|(key, is_loaded, last_used)| is_loaded.then_some((key, last_used)))
        .collect();
    let evictable = loaded.len().saturating_sub(max_images);
    if evictable == 0 {
        return Vec::new();
    }
    loaded.sort_by_key(|(_, last_used)| *last_used);
    loaded
        .into_iter()
        .filter(|(key, _)| Some(*key) != protected_key)
        .take(evictable)
        .map(|(key, _)| key)
        .collect()
}

/// An [`ImageCache`] that retains at most `max_images` decoded images, evicting the
/// least-recently-used entries (releasing their GPU textures via `drop_image`) once the
/// cap is exceeded. Use this for churning or unbounded image working sets — an infinite
/// feed, gallery, or map — where [`RetainAllImageCache`] would grow without bound.
///
/// Decoded frames also have a byte budget, and pending loads have a separate limit.
/// Clearing, removing, releasing, or critical memory pressure cancels cache-owned
/// loading tasks. Excess pending requests return a retryable load error. Completed
/// failures count toward the entry cap. Oversized images fail before decoding.
pub struct LruImageCache {
    items: HashMap<u64, LruImageEntry>,
    tick: u64,
    max_images: usize,
    max_decoded_bytes: u64,
    max_pending_loads: usize,
    weak_self: WeakEntity<Self>,
    pressure_subscription: Option<Subscription>,
}

impl fmt::Debug for LruImageCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LruImageCache")
            .field("num_images", &self.items.len())
            .field("max_images", &self.max_images)
            .finish()
    }
}

impl Drop for LruImageCache {
    fn drop(&mut self) {
        self.cancel_pending();
    }
}

impl LruImageCache {
    /// Create a new bounded image cache holding at most `max_images` decoded images
    /// (clamped to at least 1).
    pub fn new(max_images: usize, cx: &mut App) -> Entity<Self> {
        Self::with_limits(ImageCacheLimits::new(max_images), cx)
    }

    /// Create a cache with explicit decoded-memory and pending-load limits.
    pub fn with_limits(limits: ImageCacheLimits, cx: &mut App) -> Entity<Self> {
        let limits = limits.normalized();
        let e = cx.new(|cx| LruImageCache {
            items: HashMap::new(),
            tick: 0,
            max_images: limits.max_images,
            max_decoded_bytes: limits.max_decoded_bytes,
            max_pending_loads: limits.max_pending_loads,
            weak_self: cx.weak_entity(),
            pressure_subscription: None,
        });
        let weak = e.downgrade();
        let subscription = cx.on_memory_pressure(move |level, cx| {
            if level == MemoryPressureLevel::Critical {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |cache, cx| cache.clear_global(cx));
                });
            }
        });
        e.update(cx, |cache, _| {
            cache.pressure_subscription = Some(subscription)
        });
        cx.observe_release(&e, |image_cache, cx| {
            image_cache.cancel_pending();
            for (_, mut entry) in std::mem::replace(&mut image_cache.items, HashMap::new()) {
                if let Some(Ok(image)) = entry.item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        e
    }

    fn next_tick(&mut self) -> u64 {
        if self.tick == u64::MAX {
            let mut by_age: Vec<_> = self
                .items
                .iter()
                .map(|(key, entry)| (*key, entry.last_used))
                .collect();
            by_age.sort_unstable_by_key(|(key, last_used)| (*last_used, *key));
            for (index, (key, _)) in by_age.into_iter().enumerate() {
                if let Some(entry) = self.items.get_mut(&key) {
                    entry.last_used = index as u64;
                }
            }
            self.tick = self.items.len() as u64;
        }
        self.tick += 1;
        self.tick
    }

    /// Load an image from the given source, marking it most-recently-used.
    ///
    /// Returns `None` if the image is loading.
    pub fn load(
        &mut self,
        source: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let hash = hash(source);
        let tick = self.next_tick();

        self.evict_over_cap(Some(hash), window, cx);

        if let Some(entry) = self.items.get_mut(&hash) {
            entry.last_used = tick;
            let result = entry.item.get();
            self.evict_over_cap(Some(hash), window, cx);
            return result;
        }

        if self.loading_count() >= self.max_pending_loads {
            cx.defer_asset_retry(window.current_view());
            return Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "image cache pending-load limit reached; retry after a load completes"
            )))));
        }
        if self.max_decoded_bytes == 0 {
            return Some(Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                "image cache decoded-byte budget is zero"
            )))));
        }
        let fut =
            ImageAssetLoader::load_with_decoded_budget(source.clone(), cx, self.max_decoded_bytes);
        let (cancel, registration) = AbortHandle::new_pair();
        let fut = async move {
            Abortable::new(fut, registration).await.unwrap_or_else(|_| {
                Err(ImageCacheError::Other(Arc::new(anyhow::anyhow!(
                    "image cache load cancelled"
                ))))
            })
        };
        let task = cx.background_executor().spawn(fut).shared();
        self.items.insert(
            hash,
            LruImageEntry {
                item: ImageCacheItem::Loading(task.clone()),
                last_used: tick,
                observer: None,
                cancellation: Some(cancel),
            },
        );

        let entity = window.current_view();
        let cache = self.weak_self.clone();
        let observer = cx.spawn(async move |cx| {
            let _ = task.await;
            let _ = cx.update(move |cx| {
                // Resolve current entries rather than reinserting this
                // task's result: removed or replaced loads stay removed.
                let _ = cache.update(cx, |cache, cx| {
                    cache.evict_over_limits(None, None, cx);
                });
                cx.notify(entity);
                cx.resume_deferred_asset_requests();
            });
        });
        self.items.get_mut(&hash).unwrap().observer = Some(observer);

        self.evict_over_cap(None, window, cx);

        None
    }

    fn evict_over_cap(&mut self, protected_key: Option<u64>, window: &mut Window, cx: &mut App) {
        self.evict_over_limits(protected_key, Some(window), cx);
    }

    fn evict_over_limits(
        &mut self,
        protected_key: Option<u64>,
        mut window: Option<&mut Window>,
        cx: &mut App,
    ) {
        // Shared tasks may have completed without another request for their
        // keys. Promote every ready result before evaluating decoded capacity.
        for entry in self.items.values_mut() {
            if entry.item.is_loading() {
                entry.item.get();
            }
        }
        let mut loaded: Vec<_> = self
            .items
            .iter()
            .filter_map(|(key, entry)| {
                if let ImageCacheItem::Loaded(result) = &entry.item {
                    Some((
                        *key,
                        entry.last_used,
                        result.as_ref().map_or(0, |image| image.decoded_bytes()),
                    ))
                } else {
                    None
                }
            })
            .collect();
        loaded.sort_by_key(|(key, used, _)| (*used, *key));
        let mut count = loaded.len();
        let mut bytes: u128 = loaded.iter().map(|(_, _, bytes)| u128::from(*bytes)).sum();
        for (key, _, weight) in loaded {
            if count <= self.max_images && bytes <= u128::from(self.max_decoded_bytes) {
                break;
            }
            if Some(key) == protected_key && weight <= self.max_decoded_bytes {
                continue;
            }
            if let Some(mut entry) = self.items.remove(&key)
                && let Some(Ok(image)) = entry.item.get()
            {
                cx.drop_image(image, window.as_deref_mut());
            }
            count -= 1;
            bytes = bytes.saturating_sub(u128::from(weight));
        }
    }

    fn clear_global(&mut self, cx: &mut App) {
        self.cancel_pending();
        for (_, mut entry) in std::mem::take(&mut self.items) {
            if let Some(Ok(image)) = entry.item.get() {
                cx.drop_image(image, None);
            }
        }
        cx.resume_deferred_asset_requests();
    }

    /// Clear the image cache, releasing every retained image.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        self.cancel_pending();
        for (_, mut entry) in std::mem::replace(&mut self.items, HashMap::new()) {
            if let Some(Ok(image)) = entry.item.get() {
                cx.drop_image(image, Some(window));
            }
        }
        cx.resume_deferred_asset_requests();
    }

    /// Remove a single image from the cache by its source.
    pub fn remove(&mut self, source: &Resource, window: &mut Window, cx: &mut App) {
        let hash = hash(source);
        if let Some(entry) = self.items.get(&hash)
            && let Some(cancel) = &entry.cancellation
        {
            cancel.abort();
        }
        if let Some(mut entry) = self.items.remove(&hash)
            && let Some(Ok(image)) = entry.item.get()
        {
            cx.drop_image(image, Some(window));
        }
        cx.resume_deferred_asset_requests();
    }

    fn cancel_pending(&self) {
        for entry in self.items.values() {
            if let Some(cancel) = &entry.cancellation {
                cancel.abort();
            }
        }
    }

    /// The maximum number of decoded images this cache retains.
    pub fn capacity(&self) -> usize {
        self.max_images
    }

    /// Configured decoded-frame byte budget.
    pub fn byte_capacity(&self) -> u64 {
        self.max_decoded_bytes
    }

    /// Decoded bytes retained across every frame of loaded images.
    pub fn decoded_bytes(&self) -> u64 {
        self.items
            .values()
            .filter_map(|entry| match &entry.item {
                ImageCacheItem::Loaded(Ok(image)) => Some(image.decoded_bytes()),
                _ => None,
            })
            .fold(0, u64::saturating_add)
    }

    /// Maximum cache-owned concurrent loads.
    pub fn pending_capacity(&self) -> usize {
        self.max_pending_loads
    }

    /// Update retention/admission limits. Existing pending tasks keep their
    /// original decode limits; new requests use the updated limits immediately.
    pub fn set_limits(&mut self, limits: ImageCacheLimits, window: &mut Window, cx: &mut App) {
        let limits = limits.normalized();
        self.max_images = limits.max_images;
        self.max_decoded_bytes = limits.max_decoded_bytes;
        self.max_pending_loads = limits.max_pending_loads;
        self.evict_over_cap(None, window, cx);
    }

    /// The number of entries (loaded or loading) currently held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of entries that are still loading.
    pub fn loading_count(&self) -> usize {
        self.items
            .values()
            .filter(|entry| entry.item.is_loading())
            .count()
    }

    /// Number of entries that have loaded successfully.
    pub fn loaded_count(&self) -> usize {
        self.items
            .values()
            .filter(|entry| entry.item.is_loaded())
            .count()
    }

    /// Number of entries that resolved to an error.
    pub fn error_count(&self) -> usize {
        self.items
            .values()
            .filter(|entry| entry.item.is_error())
            .count()
    }

    /// Stable policy key.
    pub fn policy_key(&self) -> &'static str {
        "lru"
    }

    /// Coarse capacity class for content-safe diagnostics.
    pub fn capacity_class(&self) -> &'static str {
        capacity_class(self.max_images)
    }

    /// Returns true when the cache is at or above its configured capacity.
    pub fn is_at_capacity(&self) -> bool {
        self.len() >= self.max_images
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "lru_image_cache(policy={}, entry_count={}, loading_count={}, loaded_count={}, error_count={}, capacity={}, decoded_bytes={}, byte_capacity={}, pending_capacity={}, capacity_class={}, at_capacity={}, empty={})",
            self.policy_key(),
            self.len(),
            self.loading_count(),
            self.loaded_count(),
            self.error_count(),
            self.capacity(),
            self.decoded_bytes(),
            self.byte_capacity(),
            self.pending_capacity(),
            self.capacity_class(),
            self.is_at_capacity(),
            self.is_empty()
        )
    }
}

impl ImageCache for LruImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        LruImageCache::load(self, resource, window, cx)
    }
}

/// Constructs a bounded LRU image cache (holding at most `max_images` decoded images)
/// keyed to the element state for the given ID.
pub fn lru(id: impl Into<ElementId>, max_images: usize) -> LruImageCacheProvider {
    LruImageCacheProvider {
        id: id.into(),
        limits: ImageCacheLimits::new(max_images),
    }
}

/// Construct an inline cache with explicit memory and background-work limits.
pub fn lru_with_limits(
    id: impl Into<ElementId>,
    limits: ImageCacheLimits,
) -> LruImageCacheProvider {
    LruImageCacheProvider {
        id: id.into(),
        limits: limits.normalized(),
    }
}

/// A provider struct for creating a bounded LRU image cache inline.
pub struct LruImageCacheProvider {
    id: ElementId,
    limits: ImageCacheLimits,
}

impl LruImageCacheProvider {
    /// The configured image capacity, clamped to at least 1.
    pub fn capacity(&self) -> usize {
        self.limits.max_images.max(1)
    }

    /// Stable policy key.
    pub fn policy_key(&self) -> &'static str {
        "lru"
    }

    /// Coarse capacity class for content-safe diagnostics.
    pub fn capacity_class(&self) -> &'static str {
        capacity_class(self.capacity())
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "lru_image_cache_provider(policy={}, capacity={}, capacity_class={})",
            self.policy_key(),
            self.capacity(),
            self.capacity_class()
        )
    }
}

impl ImageCacheProvider for LruImageCacheProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> AnyImageCache {
        let limits = self.limits;
        window
            .with_global_id(self.id.clone(), |global_id, window| {
                window.with_element_state::<Entity<LruImageCache>, _>(global_id, |cache, window| {
                    let mut cache = cache.unwrap_or_else(|| LruImageCache::with_limits(limits, cx));
                    cache.update(cx, |cache, cx| cache.set_limits(limits, window, cx));
                    (cache.clone(), cache)
                })
            })
            .into()
    }
}

fn capacity_class(capacity: usize) -> &'static str {
    match capacity {
        0 | 1 => "single",
        2..=16 => "small",
        17..=128 => "medium",
        _ => "large",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ImageCacheElement, ImageCacheItem, ImageCacheLimits, LruImageCache, LruImageEntry,
        RetainAllImageCache, capacity_class, lru, retain_all, select_lru_victims,
    };
    use crate::{
        Context, Entity, IntoElement, Render, RenderImage, RequestFrameOptions, Resource, Task,
        TestAppContext, VisualContext, Window, div, hash,
    };
    use futures::FutureExt;
    use smallvec::SmallVec;
    use std::sync::Arc;

    #[crate::test]
    fn lru_byte_budget_counts_every_animation_frame_and_preserves_requested_image(
        cx: &mut TestAppContext,
    ) {
        let window = cx.add_empty_window();
        let images = [
            Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                image::RgbaImage::new(2, 2)
            )])),
            Arc::new(RenderImage::new(smallvec::smallvec![
                image::Frame::new(image::RgbaImage::new(2, 2)),
                image::Frame::new(image::RgbaImage::new(2, 2))
            ])),
            test_image(),
        ];
        let sources = [
            Resource::Embedded("a".into()),
            Resource::Embedded("b".into()),
            Resource::Embedded("c".into()),
        ];
        window.update(|window, cx| {
            let cache = LruImageCache::with_limits(
                ImageCacheLimits {
                    max_images: 10,
                    max_decoded_bytes: 36,
                    max_pending_loads: 2,
                },
                cx,
            );
            cache.update(cx, |cache, cx| {
                for (index, (source, image)) in sources.iter().zip(&images).enumerate() {
                    cache.items.insert(
                        hash(source),
                        LruImageEntry {
                            item: ImageCacheItem::Loaded(Ok(image.clone())),
                            last_used: index as u64,
                            observer: None,
                            cancellation: None,
                        },
                    );
                }
                cache.tick = 3;
                assert_eq!(
                    cache.load(&sources[2], window, cx).unwrap().unwrap().id,
                    images[2].id
                );
                assert_eq!(cache.decoded_bytes(), 36);
                assert_eq!(cache.len(), 2);
                assert!(!cache.items.contains_key(&hash(&sources[0])));
                assert_eq!(Arc::strong_count(&images[0]), 1);
            });
        });
    }

    struct PendingImageRequests {
        cache: Option<crate::AnyImageCache>,
        requested: bool,
        overflow: usize,
        count: usize,
    }
    impl Render for PendingImageRequests {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.requested {
                self.requested = true;
                for index in 0..self.count {
                    let source =
                        Resource::Uri(format!("https://test.example/image-{index}").into());
                    if let Some(Err(_)) = self.cache.as_ref().unwrap().load(&source, window, cx) {
                        self.overflow += 1;
                    }
                }
            }
            div()
        }
    }

    struct DropSignal(Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn pending_http(
        cx: &mut TestAppContext,
    ) -> (
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancelled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let client = http_client::FakeHttpClient::create({
            let started = started.clone();
            let cancelled = cancelled.clone();
            move |_| {
                started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let signal = DropSignal(cancelled.clone());
                async move {
                    let _signal = signal;
                    futures::future::pending::<
                        anyhow::Result<http_client::Response<http_client::AsyncBody>>,
                    >()
                    .await
                }
            }
        });
        cx.update(|cx| cx.set_http_client(client));
        (started, cancelled)
    }

    #[crate::test]
    fn pending_image_admission_is_bounded_and_remove_and_pressure_cancel_actual_http_work(
        cx: &mut TestAppContext,
    ) {
        let (started, cancelled) = pending_http(cx);
        let cache = cx.update(|cx| {
            LruImageCache::with_limits(
                ImageCacheLimits {
                    max_images: 10,
                    max_decoded_bytes: 1024,
                    max_pending_loads: 2,
                },
                cx,
            )
        });
        let (view, window) = cx.add_window_view(|_, _| PendingImageRequests {
            cache: Some(cache.clone().into()),
            requested: false,
            overflow: 0,
            count: 4,
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        window.update(|window, cx| {
            assert_eq!(view.read(cx).overflow, 2);
            assert_eq!(cache.read(cx).loading_count(), 2);
            assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 2);
            cache.update(cx, |cache, cx| {
                cache.remove(
                    &Resource::Uri("https://test.example/image-0".into()),
                    window,
                    cx,
                )
            });
        });
        window.run_until_parked();
        assert_eq!(cancelled.load(std::sync::atomic::Ordering::SeqCst), 1);
        window.update(|_, cx| cx.notify_memory_pressure(crate::MemoryPressureLevel::Critical));
        window.run_until_parked();
        assert_eq!(cancelled.load(std::sync::atomic::Ordering::SeqCst), 2);
        window.update(|_, cx| assert!(cache.read(cx).is_empty()));
    }

    #[crate::test]
    fn retain_all_clear_and_entity_release_cancel_owned_http_loads(cx: &mut TestAppContext) {
        let (started, cancelled) = pending_http(cx);
        let cache = cx.update(RetainAllImageCache::new);
        let (view, window) = cx.add_window_view(|_, _| PendingImageRequests {
            cache: Some(cache.clone().into()),
            requested: false,
            overflow: 0,
            count: 10,
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert_eq!(view.read(cx).overflow, 2);
            assert_eq!(cache.read(cx).loading_count(), 8);
        });
        // Across all caches, only four built-in image loads may perform I/O or decode work.
        assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 4);
        window.update(|window, cx| cache.update(cx, |cache, cx| cache.clear(window, cx)));
        window.run_until_parked();
        assert_eq!(cancelled.load(std::sync::atomic::Ordering::SeqCst), 4);
        window.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.requested = false;
                cx.notify();
            })
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 8);
        window.update(|_, cx| view.update(cx, |view, _| view.cache = None));
        drop(cache);
        window.update(|_, _| {});
        window.run_until_parked();
        assert_eq!(cancelled.load(std::sync::atomic::Ordering::SeqCst), 8);
    }

    #[test]
    fn lru_victims_within_cap_is_a_noop() {
        let entries = [(1u64, true, 1u64), (2, true, 2)];
        assert!(select_lru_victims(entries.into_iter(), 2, None).is_empty());
    }

    #[test]
    fn lru_victims_evicts_least_recently_used_first() {
        // Three loaded entries, cap of two → the oldest (lowest last_used) is shed.
        let entries = [(10u64, true, 1u64), (20, true, 3), (30, true, 2)];
        assert_eq!(select_lru_victims(entries.into_iter(), 2, None), vec![10]);
    }

    #[test]
    fn lru_victims_never_evicts_loading_entries() {
        // Pending work must not evict the one decoded image that fits the cap.
        let entries = [(1u64, false, 5u64), (2, false, 6), (3, true, 7)];
        assert!(select_lru_victims(entries.into_iter(), 1, None).is_empty());
    }

    #[test]
    fn lru_victims_sheds_multiple_when_far_over_cap() {
        let entries = [(1u64, true, 1u64), (2, true, 2), (3, true, 3), (4, true, 4)];
        let mut victims = select_lru_victims(entries.into_iter(), 2, None);
        victims.sort_unstable();
        assert_eq!(victims, vec![1, 2]);
    }

    #[test]
    fn lru_victims_preserves_the_requested_image() {
        let entries = [(1, true, 1), (2, true, 2), (3, false, 3)];
        assert_eq!(select_lru_victims(entries.into_iter(), 1, Some(1)), vec![2]);
    }

    fn test_image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            image::RgbaImage::new(1, 1)
        )]))
    }

    #[crate::test]
    fn lru_existing_lookup_prunes_ready_loads_without_new_requests(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        window.update(|window, cx| {
            let entity = LruImageCache::new(1, cx);
            let images = [test_image(), test_image(), test_image()];
            let sources = [
                Resource::Embedded("old".into()),
                Resource::Embedded("middle".into()),
                Resource::Embedded("requested".into()),
            ];
            entity.update(cx, |cache, cx| {
                for (index, (source, image)) in sources.iter().zip(&images).enumerate() {
                    cache.items.insert(
                        hash(source),
                        LruImageEntry {
                            item: ImageCacheItem::Loading(Task::ready(Ok(image.clone())).shared()),
                            last_used: index as u64,
                            observer: None,
                            cancellation: None,
                        },
                    );
                }
                cache.tick = 3;
                let result = cache.load(&sources[2], window, cx).unwrap().unwrap();
                assert_eq!(result.id, images[2].id);
                assert_eq!(cache.len(), 1);
                assert_eq!(cache.loaded_count(), 1);
                assert_eq!(cache.loading_count(), 0);
                assert_eq!(Arc::strong_count(&images[0]), 1);
                assert_eq!(Arc::strong_count(&images[1]), 1);
                assert!(cache.items.contains_key(&hash(&sources[2])));
            });
        });
    }

    #[crate::test]
    fn lru_recency_wrap_preserves_eviction_order(cx: &mut TestAppContext) {
        let entity = cx.update(|cx| LruImageCache::new(1, cx));
        entity.update(cx, |cache, _| {
            for (key, last_used) in [(1, 10), (2, 20), (3, 30)] {
                cache.items.insert(
                    key,
                    LruImageEntry {
                        item: ImageCacheItem::Loaded(Ok(test_image())),
                        last_used,
                        observer: None,
                        cancellation: None,
                    },
                );
            }
            cache.tick = u64::MAX;
            let tick = cache.next_tick();
            cache.items.get_mut(&1).unwrap().last_used = tick;
            let victims = select_lru_victims(
                cache
                    .items
                    .iter()
                    .map(|(key, entry)| (*key, true, entry.last_used)),
                1,
                None,
            );
            assert_eq!(victims, vec![2, 3]);
        });
    }

    struct BurstImageRequests {
        cache: Entity<LruImageCache>,
        requested: bool,
    }

    impl Render for BurstImageRequests {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.requested {
                self.requested = true;
                self.cache.update(cx, |cache, cx| {
                    for index in 0..4 {
                        // The default test asset source returns None. Completed
                        // failures exercise the same shared-task eviction path.
                        let source = Resource::Embedded(format!("missing-{index}").into());
                        assert!(cache.load(&source, window, cx).is_none());
                    }
                });
            }
            div()
        }
    }

    #[crate::test]
    fn lru_completion_enforces_capacity_on_the_next_frame(cx: &mut TestAppContext) {
        let (view, window) = cx.add_window_view(|_, cx| BurstImageRequests {
            cache: LruImageCache::new(1, cx),
            requested: false,
        });
        let cache = window.update(|window, cx| {
            window.draw(cx).clear();
            view.read(cx).cache.clone()
        });
        window.run_until_parked();
        let test_window = window.test_window(window.window_handle());
        test_window.run_request_frame(RequestFrameOptions::default());
        window.update(|_, cx| {
            let cache = cache.read(cx);
            assert_eq!(cache.len(), 1);
            assert_eq!(cache.error_count(), 1);
            assert_eq!(cache.loading_count(), 0);
        });
    }

    #[crate::test]
    fn lru_stale_completions_do_not_restore_cleared_entries(cx: &mut TestAppContext) {
        let (view, window) = cx.add_window_view(|_, cx| BurstImageRequests {
            cache: LruImageCache::new(1, cx),
            requested: false,
        });
        let cache = window.update(|window, cx| {
            window.draw(cx).clear();
            let cache = view.read(cx).cache.clone();
            cache.update(cx, |cache, cx| cache.clear(window, cx));
            cache
        });
        window.run_until_parked();
        let test_window = window.test_window(window.window_handle());
        test_window.run_request_frame(RequestFrameOptions::default());
        window.update(|_, cx| assert!(cache.read(cx).is_empty()));
    }

    #[crate::test]
    fn image_cache_summary_is_content_safe(cx: &mut TestAppContext) {
        let error = ImageCacheItem::Loaded(Err(crate::ImageCacheError::Other(Arc::new(
            anyhow::anyhow!("secret resource failed"),
        ))));
        assert_eq!(error.status_key(), "error");
        assert!(error.is_error());
        assert_eq!(error.to_text(), "image_cache_item(status=error)");
        assert!(!error.to_text().contains("secret resource"));

        let retain_all_cache = cx.update(RetainAllImageCache::new);
        let retain_all_summary = retain_all_cache.update(cx, |cache, _| {
            cache.items.insert(42, error);
            assert_eq!(cache.policy_key(), "retain_all");
            assert_eq!(cache.len(), 1);
            assert_eq!(cache.error_count(), 1);
            cache.to_text()
        });
        assert!(retain_all_summary.contains("policy=retain_all"));
        assert!(retain_all_summary.contains("error_count=1"));
        assert!(!retain_all_summary.contains("42"));
        assert!(!retain_all_summary.contains("secret resource"));

        let cache = cx.update(|cx| LruImageCache::new(12, cx));
        let lru_summary = cache.update(cx, |lru_cache, _| {
            lru_cache.tick = 99;
            assert_eq!(lru_cache.policy_key(), "lru");
            assert_eq!(lru_cache.capacity_class(), "small");
            assert!(!lru_cache.is_at_capacity());
            lru_cache.to_text()
        });
        assert!(lru_summary.contains("capacity=12"));
        assert!(lru_summary.contains("capacity_class=small"));
        assert!(!lru_summary.contains("99"));
    }

    #[test]
    fn image_cache_provider_summary_is_content_safe() {
        let retain_all_provider = retain_all("private-gallery-cache");
        assert_eq!(retain_all_provider.policy_key(), "retain_all");
        let retain_all_summary = retain_all_provider.to_text();
        assert!(retain_all_summary.contains("policy=retain_all"));
        assert!(!retain_all_summary.contains("private-gallery-cache"));

        let lru_provider = lru("private-feed-cache", 0);
        assert_eq!(lru_provider.capacity(), 1);
        assert_eq!(lru_provider.capacity_class(), "single");
        let lru_summary = lru_provider.to_text();
        assert!(lru_summary.contains("capacity=1"));
        assert!(!lru_summary.contains("private-feed-cache"));

        assert_eq!(capacity_class(1), "single");
        assert_eq!(capacity_class(16), "small");
        assert_eq!(capacity_class(128), "medium");
        assert_eq!(capacity_class(129), "large");
    }

    #[test]
    fn image_cache_element_summary_is_content_safe() {
        let mut element = ImageCacheElement {
            image_cache_provider: Box::new(retain_all("private-cache")),
            style: Default::default(),
            children: SmallVec::new(),
        };
        element.children.push(div().into_any_element());
        assert_eq!(element.child_count(), 1);
        let summary = element.to_text();
        assert_eq!(summary, "image_cache_element(child_count=1)");
        assert!(!summary.contains("private-cache"));
    }
}
