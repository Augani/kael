use crate::{
    AnyElement, AnyEntity, App, AppContext, Asset, AssetLogger, Bounds, Element, ElementId, Entity,
    GlobalElementId, ImageAssetLoader, ImageCacheError, InspectorElementId, IntoElement, LayoutId,
    ParentElement, Pixels, RenderImage, Resource, Style, StyleRefinement, Styled, Task, Window,
    hash,
};

use futures::{FutureExt, future::Shared};
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
/// (no eviction). Images are released together when the cache entity is dropped or
/// [`RetainAllImageCache::clear`] is called. Use this when the working set of images is
/// bounded; for unbounded or churning image sets, scope the cache to a smaller subtree
/// (a shorter-lived element id) so it is dropped and reclaimed more often.
pub struct RetainAllImageCache(HashMap<u64, ImageCacheItem>);

impl fmt::Debug for RetainAllImageCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HashMapImageCache")
            .field("num_images", &self.0.len())
            .finish()
    }
}

impl RetainAllImageCache {
    /// Create a new image cache.
    #[inline]
    pub fn new(cx: &mut App) -> Entity<Self> {
        let e = cx.new(|_cx| RetainAllImageCache(HashMap::new()));
        cx.observe_release(&e, |image_cache, cx| {
            for (_, mut item) in std::mem::replace(&mut image_cache.0, HashMap::new()) {
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

        if let Some(item) = self.0.get_mut(&hash) {
            return item.get();
        }

        let fut = AssetLogger::<ImageAssetLoader>::load(source.clone(), cx);
        let task = cx.background_executor().spawn(fut).shared();
        self.0.insert(hash, ImageCacheItem::Loading(task.clone()));

        let entity = window.current_view();
        window
            .spawn(cx, {
                async move |cx| {
                    _ = task.await;
                    cx.on_next_frame(move |_, cx| {
                        cx.notify(entity);
                    });
                }
            })
            .detach();

        None
    }

    /// Clear the image cache.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        for (_, mut item) in std::mem::replace(&mut self.0, HashMap::new()) {
            if let Some(Ok(image)) = item.get() {
                cx.drop_image(image, Some(window));
            }
        }
    }

    /// Remove the image from the cache by the given source.
    pub fn remove(&mut self, source: &Resource, window: &mut Window, cx: &mut App) {
        let hash = hash(source);
        if let Some(mut item) = self.0.remove(&hash)
            && let Some(Ok(image)) = item.get()
        {
            cx.drop_image(image, Some(window));
        }
    }

    /// Returns the number of images in the cache.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
}

/// Choose which cache keys to evict to bring a cache of `len` entries down to
/// `max_images`, least-recently-used first. Only loaded entries are evictable; entries
/// still loading are never selected (so in-flight work is preserved), which means the
/// cap may be transiently exceeded when many loads are concurrently in flight.
fn select_lru_victims(
    entries: impl Iterator<Item = (u64, bool, u64)>,
    len: usize,
    max_images: usize,
) -> Vec<u64> {
    if len <= max_images {
        return Vec::new();
    }
    let mut loaded: Vec<(u64, u64)> = entries
        .filter_map(|(key, is_loaded, last_used)| is_loaded.then_some((key, last_used)))
        .collect();
    loaded.sort_by_key(|(_, last_used)| *last_used);
    let evictable = (len - max_images).min(loaded.len());
    loaded
        .into_iter()
        .take(evictable)
        .map(|(key, _)| key)
        .collect()
}

/// An [`ImageCache`] that retains at most `max_images` decoded images, evicting the
/// least-recently-used entries (releasing their GPU textures via `drop_image`) once the
/// cap is exceeded. Use this for churning or unbounded image working sets — an infinite
/// feed, gallery, or map — where [`RetainAllImageCache`] would grow without bound.
///
/// Still-loading entries are never evicted; the cap is enforced over decoded images, so a
/// burst of concurrent loads may transiently exceed `max_images` until they resolve and
/// the least-recently-used ones are shed.
pub struct LruImageCache {
    items: HashMap<u64, LruImageEntry>,
    tick: u64,
    max_images: usize,
}

impl fmt::Debug for LruImageCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LruImageCache")
            .field("num_images", &self.items.len())
            .field("max_images", &self.max_images)
            .finish()
    }
}

impl LruImageCache {
    /// Create a new bounded image cache holding at most `max_images` decoded images
    /// (clamped to at least 1).
    pub fn new(max_images: usize, cx: &mut App) -> Entity<Self> {
        let e = cx.new(|_cx| LruImageCache {
            items: HashMap::new(),
            tick: 0,
            max_images: max_images.max(1),
        });
        cx.observe_release(&e, |image_cache, cx| {
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

        if let Some(entry) = self.items.get_mut(&hash) {
            entry.last_used = tick;
            return entry.item.get();
        }

        let fut = AssetLogger::<ImageAssetLoader>::load(source.clone(), cx);
        let task = cx.background_executor().spawn(fut).shared();
        self.items.insert(
            hash,
            LruImageEntry {
                item: ImageCacheItem::Loading(task.clone()),
                last_used: tick,
            },
        );

        let entity = window.current_view();
        window
            .spawn(cx, {
                async move |cx| {
                    _ = task.await;
                    cx.on_next_frame(move |_, cx| {
                        cx.notify(entity);
                    });
                }
            })
            .detach();

        self.evict_over_cap(window, cx);

        None
    }

    fn evict_over_cap(&mut self, window: &mut Window, cx: &mut App) {
        let victims = select_lru_victims(
            self.items.iter().map(|(key, entry)| {
                (
                    *key,
                    matches!(entry.item, ImageCacheItem::Loaded(_)),
                    entry.last_used,
                )
            }),
            self.items.len(),
            self.max_images,
        );
        for key in victims {
            if let Some(mut entry) = self.items.remove(&key)
                && let Some(Ok(image)) = entry.item.get()
            {
                cx.drop_image(image, Some(window));
            }
        }
    }

    /// Clear the image cache, releasing every retained image.
    pub fn clear(&mut self, window: &mut Window, cx: &mut App) {
        for (_, mut entry) in std::mem::replace(&mut self.items, HashMap::new()) {
            if let Some(Ok(image)) = entry.item.get() {
                cx.drop_image(image, Some(window));
            }
        }
    }

    /// Remove a single image from the cache by its source.
    pub fn remove(&mut self, source: &Resource, window: &mut Window, cx: &mut App) {
        let hash = hash(source);
        if let Some(mut entry) = self.items.remove(&hash)
            && let Some(Ok(image)) = entry.item.get()
        {
            cx.drop_image(image, Some(window));
        }
    }

    /// The maximum number of decoded images this cache retains.
    pub fn capacity(&self) -> usize {
        self.max_images
    }

    /// The number of entries (loaded or loading) currently held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
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
        max_images,
    }
}

/// A provider struct for creating a bounded LRU image cache inline.
pub struct LruImageCacheProvider {
    id: ElementId,
    max_images: usize,
}

impl ImageCacheProvider for LruImageCacheProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> AnyImageCache {
        let max_images = self.max_images;
        window
            .with_global_id(self.id.clone(), |global_id, window| {
                window.with_element_state::<Entity<LruImageCache>, _>(
                    global_id,
                    |cache, _window| {
                        let mut cache = cache.unwrap_or_else(|| LruImageCache::new(max_images, cx));
                        (cache.clone(), cache)
                    },
                )
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::select_lru_victims;

    #[test]
    fn lru_victims_within_cap_is_a_noop() {
        let entries = [(1u64, true, 1u64), (2, true, 2)];
        assert!(select_lru_victims(entries.into_iter(), 2, 2).is_empty());
    }

    #[test]
    fn lru_victims_evicts_least_recently_used_first() {
        // Three loaded entries, cap of two → the oldest (lowest last_used) is shed.
        let entries = [(10u64, true, 1u64), (20, true, 3), (30, true, 2)];
        assert_eq!(select_lru_victims(entries.into_iter(), 3, 2), vec![10]);
    }

    #[test]
    fn lru_victims_never_evicts_loading_entries() {
        // Two loading + one loaded, cap of one: only the loaded entry is evictable, so the
        // cap is held by shedding it and the in-flight loads are preserved.
        let entries = [(1u64, false, 5u64), (2, false, 6), (3, true, 7)];
        assert_eq!(select_lru_victims(entries.into_iter(), 3, 1), vec![3]);
    }

    #[test]
    fn lru_victims_sheds_multiple_when_far_over_cap() {
        let entries = [(1u64, true, 1u64), (2, true, 2), (3, true, 3), (4, true, 4)];
        let mut victims = select_lru_victims(entries.into_iter(), 4, 2);
        victims.sort_unstable();
        assert_eq!(victims, vec![1, 2]);
    }
}
