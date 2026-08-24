use crate::{
    AnyElement, AnyImageCache, App, Asset, AssetLogger, Bounds, ContentMask, DefiniteLength,
    Element, ElementId, Entity, GlobalElementId, Hitbox, Image, ImageCache, InspectorElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, Length, ObjectFit, Pixels,
    RenderImage, Resource, SMOOTH_SVG_SCALE_FACTOR, SharedString, SharedUri, StyleRefinement,
    Styled, SvgSize, Task, Window,
    assets::{
        MAX_IMAGE_SOURCE_BYTES, checked_image_frame_len, collect_animation_frames,
        decode_static_image, image_decode_limits, validate_image_source_bytes,
    },
    px, swap_rgba_pa_to_bgra,
    util::is_uri,
};
use anyhow::{Context as _, Result};

use futures::{AsyncReadExt, Future};
use image::{
    AnimationDecoder, Frame, ImageBuffer, ImageDecoder as _, ImageError, ImageFormat, Rgba,
    codecs::{gif::GifDecoder, webp::WebPDecoder},
};
use smallvec::SmallVec;
use std::{
    fmt, fs,
    io::{self, Cursor, Read as _},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use util::ResultExt;
use web_time::Instant;

use super::{Stateful, StatefulInteractiveElement};

/// The delay before showing the loading state.
pub const LOADING_DELAY: Duration = Duration::from_millis(200);

/// A type alias to the resource loader that the `img()` element uses.
///
/// Note: that this is only for Resources, like URLs or file paths.
/// Custom loaders, or external images will not use this asset loader
pub type ImgResourceLoader = AssetLogger<ImageAssetLoader>;

/// A source of image content.
#[derive(Clone)]
pub enum ImageSource {
    /// The image content will be loaded from some resource location
    Resource(Resource),
    /// Cached image data
    Render(Arc<RenderImage>),
    /// Cached image data
    Image(Arc<Image>),
    /// A custom loading function to use
    Custom(Arc<dyn Fn(&mut Window, &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>>>),
}

impl From<SharedUri> for ImageSource {
    fn from(value: SharedUri) -> Self {
        Self::Resource(Resource::Uri(value))
    }
}

impl<'a> From<&'a str> for ImageSource {
    fn from(s: &'a str) -> Self {
        if is_uri(s) {
            Self::Resource(Resource::Uri(s.to_string().into()))
        } else {
            Self::Resource(Resource::Embedded(s.to_string().into()))
        }
    }
}

impl From<String> for ImageSource {
    fn from(s: String) -> Self {
        if is_uri(&s) {
            Self::Resource(Resource::Uri(s.into()))
        } else {
            Self::Resource(Resource::Embedded(s.into()))
        }
    }
}

impl From<SharedString> for ImageSource {
    fn from(s: SharedString) -> Self {
        s.as_ref().into()
    }
}

impl From<&Path> for ImageSource {
    fn from(value: &Path) -> Self {
        Self::Resource(value.to_path_buf().into())
    }
}

impl From<Arc<Path>> for ImageSource {
    fn from(value: Arc<Path>) -> Self {
        Self::Resource(value.into())
    }
}

impl From<PathBuf> for ImageSource {
    fn from(value: PathBuf) -> Self {
        Self::Resource(value.into())
    }
}

impl From<Arc<RenderImage>> for ImageSource {
    fn from(value: Arc<RenderImage>) -> Self {
        Self::Render(value)
    }
}

impl From<Arc<Image>> for ImageSource {
    fn from(value: Arc<Image>) -> Self {
        Self::Image(value)
    }
}

impl<F> From<F> for ImageSource
where
    F: Fn(&mut Window, &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> + 'static,
{
    fn from(value: F) -> Self {
        Self::Custom(Arc::new(value))
    }
}

/// The style of an image element.
pub struct ImageStyle {
    grayscale: bool,
    object_fit: ObjectFit,
    loading: Option<Box<dyn Fn() -> AnyElement>>,
    fallback: Option<Box<dyn Fn() -> AnyElement>>,
}

impl Default for ImageStyle {
    fn default() -> Self {
        Self {
            grayscale: false,
            object_fit: ObjectFit::Contain,
            loading: None,
            fallback: None,
        }
    }
}

/// Style an image element.
pub trait StyledImage: Sized {
    /// Get a mutable [ImageStyle] from the element.
    fn image_style(&mut self) -> &mut ImageStyle;

    /// Set the image to be displayed in grayscale.
    fn grayscale(mut self, grayscale: bool) -> Self {
        self.image_style().grayscale = grayscale;
        self
    }

    /// Set the object fit for the image.
    fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.image_style().object_fit = object_fit;
        self
    }

    /// Set the object fit for the image.
    fn with_fallback(mut self, fallback: impl Fn() -> AnyElement + 'static) -> Self {
        self.image_style().fallback = Some(Box::new(fallback));
        self
    }

    /// Set the object fit for the image.
    fn with_loading(mut self, loading: impl Fn() -> AnyElement + 'static) -> Self {
        self.image_style().loading = Some(Box::new(loading));
        self
    }
}

impl StyledImage for Img {
    fn image_style(&mut self) -> &mut ImageStyle {
        &mut self.style
    }
}

impl StyledImage for Stateful<Img> {
    fn image_style(&mut self) -> &mut ImageStyle {
        &mut self.element.style
    }
}

impl ImageStyle {
    /// Returns true when grayscale rendering is enabled.
    pub fn is_grayscale(&self) -> bool {
        self.grayscale
    }

    /// Stable text key for the configured object fit.
    pub fn object_fit_key(&self) -> &'static str {
        object_fit_key(&self.object_fit)
    }

    /// Returns true when a custom loading element is configured.
    pub fn has_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Returns true when a fallback element is configured.
    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "image_style(grayscale={}, object_fit={}, has_loading={}, has_fallback={})",
            self.is_grayscale(),
            self.object_fit_key(),
            self.has_loading(),
            self.has_fallback()
        )
    }
}

/// An image element.
pub struct Img {
    interactivity: Interactivity,
    source: ImageSource,
    style: ImageStyle,
    image_cache: Option<AnyImageCache>,
}

/// Create a new image element.
#[track_caller]
pub fn img(source: impl Into<ImageSource>) -> Img {
    Img {
        interactivity: Interactivity::new(),
        source: source.into(),
        style: ImageStyle::default(),
        image_cache: None,
    }
}

impl Img {
    /// A list of all format extensions currently supported by this image element.
    ///
    /// AVIF is present on native targets when `image-avif` is enabled. Browser builds deliberately
    /// leave AVIF to DOM-backed surfaces until Kael's raster pipeline can decode it without the
    /// native dav1d library. OpenEXR is present when `image-exr` is enabled.
    pub fn extensions() -> &'static [&'static str] {
        &[
            #[cfg(all(feature = "image-avif", not(target_arch = "wasm32")))]
            "avif",
            "jpg",
            "jpeg",
            "png",
            "gif",
            "webp",
            "tif",
            "tiff",
            "tga",
            "dds",
            "bmp",
            "ico",
            "hdr",
            #[cfg(feature = "image-exr")]
            "exr",
            "pbm",
            "pam",
            "ppm",
            "pgm",
            "ff",
            "farbfeld",
            "qoi",
            "svg",
        ]
    }

    /// Stable text key for the source kind.
    pub fn source_kind(&self) -> &'static str {
        self.source.kind()
    }

    /// Returns true when the image source points at a resource path, URI, or embedded asset.
    pub fn has_resource_source(&self) -> bool {
        self.source.is_resource()
    }

    /// Returns true when an explicit image cache is bound to this element.
    pub fn has_image_cache(&self) -> bool {
        self.image_cache.is_some()
    }

    /// Stable text key for the configured object fit.
    pub fn object_fit_key(&self) -> &'static str {
        self.style.object_fit_key()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "img(source={}, {}, style={}, has_image_cache={})",
            self.source_kind(),
            self.source.to_text(),
            self.style.to_text(),
            self.has_image_cache()
        )
    }

    /// Sets the image cache for the current node.
    ///
    /// If the `image_cache` is not explicitly provided, the function will determine the image cache by:
    ///
    /// 1. Checking if any ancestor node of the current node contains an `ImageCacheElement`, If such a node exists, the image cache specified by that ancestor will be used.
    /// 2. If no ancestor node contains an `ImageCacheElement`, the global image cache will be used as a fallback.
    ///
    /// This mechanism provides a flexible way to manage image caching, allowing precise control when needed,
    /// while ensuring a default behavior when no cache is explicitly specified.
    #[inline]
    pub fn image_cache<I: ImageCache>(self, image_cache: &Entity<I>) -> Self {
        Self {
            image_cache: Some(image_cache.clone().into()),
            ..self
        }
    }
}

impl Deref for Stateful<Img> {
    type Target = Img;

    fn deref(&self) -> &Self::Target {
        &self.element
    }
}

impl DerefMut for Stateful<Img> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.element
    }
}

/// The image state between frames
struct ImgState {
    frame_index: usize,
    last_frame_time: Option<Instant>,
    started_loading: Option<(Instant, Task<()>)>,
}

/// The image layout state between frames
pub struct ImgLayoutState {
    frame_index: usize,
    replacement: Option<AnyElement>,
}

impl Element for Img {
    type RequestLayoutState = ImgLayoutState;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut layout_state = ImgLayoutState {
            frame_index: 0,
            replacement: None,
        };

        window.with_optional_element_state(global_id, |state, window| {
            let mut state = state.map(|state| {
                state.unwrap_or(ImgState {
                    frame_index: 0,
                    last_frame_time: None,
                    started_loading: None,
                })
            });

            let frame_index = state.as_ref().map(|state| state.frame_index).unwrap_or(0);

            let layout_id = self.interactivity.request_layout(
                global_id,
                inspector_id,
                window,
                cx,
                |mut style, window, cx| {
                    let mut replacement_id = None;

                    match self.source.use_data(
                        self.image_cache
                            .clone()
                            .or_else(|| window.image_cache_stack.last().cloned()),
                        window,
                        cx,
                    ) {
                        Some(Ok(data)) => {
                            if let Some(state) = &mut state {
                                let frame_count = data.frame_count();
                                if frame_count > 1 {
                                    let current_time = Instant::now();
                                    if let Some(last_frame_time) = state.last_frame_time {
                                        let elapsed = current_time - last_frame_time;
                                        let frame_duration =
                                            Duration::from(data.delay(state.frame_index));

                                        if elapsed >= frame_duration {
                                            state.frame_index =
                                                (state.frame_index + 1) % frame_count;
                                            state.last_frame_time =
                                                Some(current_time - (elapsed - frame_duration));
                                        }
                                    } else {
                                        state.last_frame_time = Some(current_time);
                                    }
                                }
                                state.started_loading = None;
                            }

                            let image_size = data.render_size(frame_index);
                            style.aspect_ratio = Some(image_size.width / image_size.height);

                            if let Length::Auto = style.size.width {
                                style.size.width = match style.size.height {
                                    Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                                        let height_px =
                                            window.unscaled_ui_length_in_pixels(abs_length);
                                        Length::Definite(
                                            px(image_size.width.0 * height_px.0
                                                / image_size.height.0)
                                            .into(),
                                        )
                                    }
                                    _ => Length::Definite(image_size.width.into()),
                                };
                            }

                            if let Length::Auto = style.size.height {
                                style.size.height = match style.size.width {
                                    Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                                        let width_px =
                                            window.unscaled_ui_length_in_pixels(abs_length);
                                        Length::Definite(
                                            px(image_size.height.0 * width_px.0
                                                / image_size.width.0)
                                            .into(),
                                        )
                                    }
                                    _ => Length::Definite(image_size.height.into()),
                                };
                            }

                            if global_id.is_some() && data.frame_count() > 1 {
                                window.request_animation_frame();
                            }
                        }
                        Some(_err) => {
                            if let Some(fallback) = self.style.fallback.as_ref() {
                                let mut element = fallback();
                                replacement_id = Some(element.request_layout(window, cx));
                                layout_state.replacement = Some(element);
                            }
                            if let Some(state) = &mut state {
                                state.started_loading = None;
                            }
                        }
                        None => {
                            if let Some(state) = &mut state {
                                if let Some((started_loading, _)) = state.started_loading {
                                    if started_loading.elapsed() > LOADING_DELAY
                                        && let Some(loading) = self.style.loading.as_ref()
                                    {
                                        let mut element = loading();
                                        replacement_id = Some(element.request_layout(window, cx));
                                        layout_state.replacement = Some(element);
                                    }
                                } else {
                                    let current_view = window.current_view();
                                    let task = window.spawn(cx, async move |cx| {
                                        cx.background_executor().timer(LOADING_DELAY).await;
                                        cx.update(move |_, cx| {
                                            cx.notify(current_view);
                                        })
                                        .ok();
                                    });
                                    state.started_loading = Some((Instant::now(), task));
                                }
                            }
                        }
                    }

                    window.request_layout(style, replacement_id, cx)
                },
            );

            layout_state.frame_index = frame_index;

            ((layout_id, layout_state), state)
        })
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            bounds.size,
            window,
            cx,
            |_, _, hitbox, window, cx| {
                if let Some(replacement) = &mut request_layout.replacement {
                    replacement.prepaint(window, cx);
                }

                hitbox
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout_state: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let source = self.source.clone();
        self.interactivity.paint(
            global_id,
            inspector_id,
            bounds,
            hitbox.as_ref(),
            window,
            cx,
            |style, window, cx| {
                if let Some(Ok(data)) = source.use_data(
                    self.image_cache
                        .clone()
                        .or_else(|| window.image_cache_stack.last().cloned()),
                    window,
                    cx,
                ) {
                    let new_bounds = self
                        .style
                        .object_fit
                        .get_bounds(bounds, data.size(layout_state.frame_index));
                    let corner_radii = window
                        .ui_corners_in_pixels(style.corner_radii)
                        .clamp_radii_for_quad_size(new_bounds.size);
                    // `ObjectFit::Cover` and `ObjectFit::None` may deliberately
                    // produce draw bounds larger than the replaced element. CSS
                    // object-fit semantics still clip that content to the
                    // element's box; without this mask a landscape image can
                    // paint across adjacent text and controls.
                    window
                        .with_content_mask(Some(ContentMask { bounds }), |window| {
                            window.paint_image(
                                new_bounds,
                                corner_radii,
                                data,
                                layout_state.frame_index,
                                self.style.grayscale,
                            )
                        })
                        .log_err();
                } else if let Some(replacement) = &mut layout_state.replacement {
                    replacement.paint(window, cx);
                }
            },
        )
    }
}

impl Styled for Img {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Img {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl IntoElement for Img {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl StatefulInteractiveElement for Img {}

impl ImageSource {
    /// Stable text key for the source kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Resource(resource) => resource_kind(resource),
            Self::Render(_) => "render",
            Self::Image(_) => "image",
            Self::Custom(_) => "custom",
        }
    }

    /// Returns true when this source points at a resource path, URI, or embedded asset.
    pub fn is_resource(&self) -> bool {
        matches!(self, Self::Resource(_))
    }

    /// Returns true when this source uses a caller-provided loader.
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }

    /// Byte length of a resource identifier without exposing it.
    pub fn resource_len_bytes(&self) -> Option<usize> {
        match self {
            Self::Resource(resource) => Some(resource_len_bytes(resource)),
            _ => None,
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        let resource_len = self
            .resource_len_bytes()
            .map_or_else(|| "none".to_string(), |len| len.to_string());
        format!(
            "image_source(kind={}, resource_len_bytes={})",
            self.kind(),
            resource_len
        )
    }

    pub(crate) fn use_data(
        &self,
        cache: Option<AnyImageCache>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match self {
            ImageSource::Resource(resource) => {
                if let Some(cache) = cache {
                    cache.load(resource, window, cx)
                } else {
                    window.use_asset::<ImgResourceLoader>(resource, cx)
                }
            }
            ImageSource::Custom(loading_fn) => loading_fn(window, cx),
            ImageSource::Render(data) => Some(Ok(data.to_owned())),
            ImageSource::Image(data) => window.use_asset::<AssetLogger<ImageDecoder>>(data, cx),
        }
    }

    pub(crate) fn get_data(
        &self,
        cache: Option<AnyImageCache>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match self {
            ImageSource::Resource(resource) => {
                if let Some(cache) = cache {
                    cache.load(resource, window, cx)
                } else {
                    window.get_asset::<ImgResourceLoader>(resource, cx)
                }
            }
            ImageSource::Custom(loading_fn) => loading_fn(window, cx),
            ImageSource::Render(data) => Some(Ok(data.to_owned())),
            ImageSource::Image(data) => window.get_asset::<AssetLogger<ImageDecoder>>(data, cx),
        }
    }

    /// Remove this image source from the asset system
    pub fn remove_asset(&self, cx: &mut App) {
        match self {
            ImageSource::Resource(resource) => {
                cx.remove_asset::<ImgResourceLoader>(resource);
            }
            ImageSource::Custom(_) | ImageSource::Render(_) => {}
            ImageSource::Image(data) => cx.remove_asset::<AssetLogger<ImageDecoder>>(data),
        }
    }
}

fn object_fit_key(object_fit: &ObjectFit) -> &'static str {
    match object_fit {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::ScaleDown => "scale-down",
        ObjectFit::None => "none",
    }
}

fn resource_kind(resource: &Resource) -> &'static str {
    match resource {
        Resource::Uri(_) => "uri",
        Resource::Embedded(_) => "embedded",
        Resource::Path(_) => "path",
    }
}

fn resource_len_bytes(resource: &Resource) -> usize {
    match resource {
        Resource::Uri(uri) => uri.len(),
        Resource::Embedded(path) => path.len(),
        Resource::Path(path) => path.to_string_lossy().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageCacheError, ImageSource, Img, StyledImage, img};
    use crate::{
        IntoElement, ObjectFit,
        assets::{
            MAX_IMAGE_DIMENSION, MAX_IMAGE_SOURCE_BYTES, checked_image_frame_len,
            collect_animation_frames, validate_image_source_len,
        },
        div,
    };
    use image::Frame;
    use std::path::PathBuf;

    #[test]
    fn image_summary_is_content_safe() {
        let source = ImageSource::from("https://cdn.example.com/private/poster.png");
        assert_eq!(source.kind(), "uri");
        assert!(source.is_resource());
        assert_eq!(
            source.resource_len_bytes(),
            Some("https://cdn.example.com/private/poster.png".len())
        );
        let source_summary = source.to_text();
        assert!(source_summary.contains("kind=uri"));
        assert!(!source_summary.contains("cdn.example.com"));
        assert!(!source_summary.contains("poster.png"));

        let image = img(source)
            .grayscale(true)
            .object_fit(ObjectFit::Cover)
            .with_loading(|| div().into_any_element())
            .with_fallback(|| div().into_any_element());

        assert_eq!(image.source_kind(), "uri");
        assert!(image.has_resource_source());
        assert!(!image.has_image_cache());
        assert_eq!(image.object_fit_key(), "cover");
        assert!(image.style.is_grayscale());
        assert!(image.style.has_loading());
        assert!(image.style.has_fallback());

        let summary = image.to_text();
        assert!(summary.contains("img(source=uri"));
        assert!(summary.contains("object_fit=cover"));
        assert!(summary.contains("has_loading=true"));
        assert!(!summary.contains("cdn.example.com"));
        assert!(!summary.contains("poster.png"));
    }

    #[test]
    fn image_path_summary_is_content_safe() {
        let source = ImageSource::from(PathBuf::from("/private/assets/secret.png"));
        assert_eq!(source.kind(), "path");
        assert_eq!(
            source.resource_len_bytes(),
            Some("/private/assets/secret.png".len())
        );

        let summary = source.to_text();
        assert!(summary.contains("kind=path"));
        assert!(!summary.contains("/private"));
        assert!(!summary.contains("secret.png"));
    }

    #[test]
    fn advertised_extensions_match_enabled_codecs() {
        let extensions = Img::extensions();

        for extension in ["jpg", "png", "gif", "webp", "tiff", "bmp", "qoi", "svg"] {
            assert!(extensions.contains(&extension));
        }
        assert_eq!(
            extensions.contains(&"avif"),
            cfg!(all(feature = "image-avif", not(target_arch = "wasm32")))
        );
        assert_eq!(extensions.contains(&"exr"), cfg!(feature = "image-exr"));
    }

    #[test]
    fn image_resource_limits_reject_invalid_metadata() {
        assert!(validate_image_source_len(1).is_ok());
        assert!(validate_image_source_len(0).is_err());
        assert!(validate_image_source_len(MAX_IMAGE_SOURCE_BYTES + 1).is_err());

        assert!(checked_image_frame_len(1, 1).is_ok());
        assert!(checked_image_frame_len(MAX_IMAGE_DIMENSION + 1, 1).is_err());
        assert!(checked_image_frame_len(MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION).is_err());

        let empty = std::iter::empty::<image::ImageResult<Frame>>();
        assert!(collect_animation_frames(empty).is_err());
    }

    #[test]
    fn image_http_status_error_redacts_location_and_body() {
        let error = ImageCacheError::BadStatus {
            uri: "https://example.com/private/account.png".into(),
            status: http_client::StatusCode::BAD_REQUEST,
            body: "private response body".into(),
        };

        for text in [error.to_string(), format!("{error:?}")] {
            assert!(!text.contains("example.com"));
            assert!(!text.contains("private"));
            assert!(text.contains("400"));
        }
    }

    #[cfg(all(feature = "image-avif", not(target_arch = "wasm32")))]
    #[test]
    fn image_avif_feature_enables_decoder() {
        let result = image::codecs::avif::AvifDecoder::new(std::io::Cursor::new(&[]));

        assert!(result.is_err(), "an empty AVIF payload must be rejected");
    }
}

#[derive(Clone)]
enum ImageDecoder {}

impl Asset for ImageDecoder {
    type Source = Arc<Image>;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move { source.to_image_data(renderer).map_err(Into::into) }
    }
}

/// An image loader for the GPUI asset system
#[derive(Clone)]
pub enum ImageAssetLoader {}

impl Asset for ImageAssetLoader {
    type Source = Resource;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let client = cx.http_client();
        // TODO: Can we make SVGs always rescale?
        // let scale_factor = cx.scale_factor();
        let svg_renderer = cx.svg_renderer();
        let asset_source = cx.asset_source().clone();
        async move {
            let bytes = match source.clone() {
                Resource::Path(path) => read_image_file(path.as_ref())?,
                Resource::Uri(uri) => {
                    let mut response = client
                        .get(uri.as_ref(), ().into(), true)
                        .await
                        .context("loading image asset")?;
                    if !response.status().is_success() {
                        return Err(ImageCacheError::BadStatus {
                            uri,
                            status: response.status(),
                            body: String::new(),
                        });
                    }
                    let mut body = Vec::new();
                    response
                        .body_mut()
                        .take((MAX_IMAGE_SOURCE_BYTES + 1) as u64)
                        .read_to_end(&mut body)
                        .await?;
                    validate_image_source_bytes(&body)?;
                    body
                }
                Resource::Embedded(path) => {
                    let data = asset_source
                        .load(&path)
                        .context("loading embedded image asset")?;
                    if let Some(data) = data {
                        validate_image_source_bytes(&data)?;
                        data.to_vec()
                    } else {
                        return Err(ImageCacheError::Asset("embedded resource not found".into()));
                    }
                }
            };

            let data = if let Ok(format) = image::guess_format(&bytes) {
                let data = match format {
                    ImageFormat::Gif => {
                        let mut decoder = GifDecoder::new(Cursor::new(&bytes))?;
                        decoder.set_limits(image_decode_limits())?;
                        let (width, height) = decoder.dimensions();
                        checked_image_frame_len(width, height)?;
                        collect_animation_frames(decoder.into_frames())?
                    }
                    ImageFormat::WebP => {
                        let mut decoder = WebPDecoder::new(Cursor::new(&bytes))?;
                        decoder.set_limits(image_decode_limits())?;
                        let (width, height) = decoder.dimensions();
                        checked_image_frame_len(width, height)?;

                        if decoder.has_animation() {
                            let _ = decoder.set_background_color(Rgba([0, 0, 0, 0]));
                            collect_animation_frames(decoder.into_frames())?
                        } else {
                            decode_static_image(&bytes, format)?
                        }
                    }
                    _ => decode_static_image(&bytes, format)?,
                };

                RenderImage::new(data)
            } else {
                let pixmap =
                    // TODO: Can we make svgs always rescale?
                    svg_renderer.render_pixmap(&bytes, SvgSize::ScaleFactor(SMOOTH_SVG_SCALE_FACTOR))?;

                let mut buffer =
                    ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take())
                        .ok_or_else(|| anyhow::anyhow!("invalid SVG pixel buffer"))?;

                for pixel in buffer.chunks_exact_mut(4) {
                    swap_rgba_pa_to_bgra(pixel);
                }

                let mut image = RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1));
                image.scale_factor = SMOOTH_SVG_SCALE_FACTOR;
                image
            };

            Ok(Arc::new(data))
        }
    }
}

fn read_image_file(path: &Path) -> Result<Vec<u8>, ImageCacheError> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_SOURCE_BYTES as u64 {
        return Err(anyhow::anyhow!("image source must be a bounded regular file").into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_IMAGE_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_image_source_bytes(&bytes)?;
    Ok(bytes)
}

/// An error that can occur when interacting with the image cache.
#[derive(Error, Clone)]
pub enum ImageCacheError {
    /// Some other kind of error occurred
    #[error("error: {0}")]
    Other(#[from] Arc<anyhow::Error>),
    /// An error that occurred while reading the image from disk.
    #[error("IO error: {0}")]
    Io(Arc<std::io::Error>),
    /// An error that occurred while processing an image.
    #[error("unexpected HTTP status while loading image asset: {status}")]
    BadStatus {
        /// The URI of the image.
        uri: SharedUri,
        /// The HTTP status code.
        status: http_client::StatusCode,
        /// The HTTP response body.
        ///
        /// Kael's built-in loader leaves this empty so response contents are not retained or
        /// exposed through diagnostics.
        body: String,
    },
    /// An error that occurred while processing an asset.
    #[error("asset error: {0}")]
    Asset(SharedString),
    /// An error that occurred while processing an image.
    #[error("image error: {0}")]
    Image(Arc<ImageError>),
    /// An error that occurred while processing an SVG.
    #[error("svg error: {0}")]
    Usvg(Arc<usvg::Error>),
}

impl fmt::Debug for ImageCacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(_) => f.write_str("ImageCacheError::Other"),
            Self::Io(error) => f
                .debug_struct("ImageCacheError::Io")
                .field("kind", &error.kind())
                .finish(),
            Self::BadStatus { uri, status, body } => f
                .debug_struct("ImageCacheError::BadStatus")
                .field("uri_len_bytes", &uri.len())
                .field("status", status)
                .field("body_len_bytes", &body.len())
                .finish(),
            Self::Asset(error) => f
                .debug_tuple("ImageCacheError::Asset")
                .field(error)
                .finish(),
            Self::Image(error) => f
                .debug_tuple("ImageCacheError::Image")
                .field(error)
                .finish(),
            Self::Usvg(error) => f.debug_tuple("ImageCacheError::Usvg").field(error).finish(),
        }
    }
}

impl From<anyhow::Error> for ImageCacheError {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(Arc::new(value))
    }
}

impl From<io::Error> for ImageCacheError {
    fn from(value: io::Error) -> Self {
        Self::Io(Arc::new(value))
    }
}

impl From<usvg::Error> for ImageCacheError {
    fn from(value: usvg::Error) -> Self {
        Self::Usvg(Arc::new(value))
    }
}

impl From<image::ImageError> for ImageCacheError {
    fn from(value: image::ImageError) -> Self {
        Self::Image(Arc::new(value))
    }
}
