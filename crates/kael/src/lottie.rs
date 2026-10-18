use crate::{
    App, Asset, AssetLogger, DevicePixels, Pixels, RenderImage, Resource, SharedString, SharedUri,
    Size, px, size, util::is_uri,
};
use anyhow::Context as _;
use futures::AsyncReadExt;
use image::{Frame, ImageBuffer, Rgba};
use rasterlottie::{PreparedAnimation, RenderConfig, Renderer, Rgba8};
use smallvec::SmallVec;
use std::{
    cell::RefCell,
    collections::VecDeque,
    fs, io,
    io::Read as _,
    path::{Path, PathBuf},
    str,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use web_time::Instant;

const MAX_LOTTIE_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOTTIE_DIMENSION: u32 = 8_192;
const MAX_LOTTIE_FRAME_BYTES: usize = 256 * 1024 * 1024;
const MAX_LOTTIE_RENDER_BATCH_BYTES: usize = 256 * 1024 * 1024;
const MAX_LOTTIE_FRAMES: usize = 100_000;
const MAX_LOTTIE_FPS: f32 = 1_000.0;
const MAX_LOTTIE_RENDER_BATCH: usize = 256;
const MAX_PREPARED_ANIMATIONS_PER_THREAD: usize = 4;
const MAX_PREPARED_IMAGE_BYTES_PER_THREAD: u64 = 64 * 1024 * 1024;
const MAX_PREPARED_ANIMATIONS_GLOBAL: usize = 16;
const MAX_PREPARED_IMAGE_BYTES_GLOBAL: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PreparedCacheUsage {
    entries: usize,
    decoded_image_bytes: u64,
}

/// Metadata only: prepared animations remain in their creating thread's TLS.
struct PreparedCacheBudget {
    max_entries: usize,
    max_decoded_image_bytes: u64,
    usage: parking_lot::Mutex<PreparedCacheUsage>,
}

impl PreparedCacheBudget {
    fn new(max_entries: usize, max_decoded_image_bytes: u64) -> Self {
        Self {
            max_entries,
            max_decoded_image_bytes,
            usage: parking_lot::Mutex::new(PreparedCacheUsage::default()),
        }
    }

    fn reserve(self: &Arc<Self>, bytes: u64) -> Option<PreparedCacheReservation> {
        let mut usage = self.usage.lock();
        let entries = usage.entries.checked_add(1)?;
        let decoded_image_bytes = usage.decoded_image_bytes.checked_add(bytes)?;
        if entries > self.max_entries || decoded_image_bytes > self.max_decoded_image_bytes {
            return None;
        }
        *usage = PreparedCacheUsage {
            entries,
            decoded_image_bytes,
        };
        Some(PreparedCacheReservation {
            budget: self.clone(),
            decoded_image_bytes: bytes,
        })
    }
}

struct PreparedCacheReservation {
    budget: Arc<PreparedCacheBudget>,
    decoded_image_bytes: u64,
}

impl Drop for PreparedCacheReservation {
    fn drop(&mut self) {
        let mut usage = self.budget.usage.lock();
        usage.entries -= 1;
        usage.decoded_image_bytes -= self.decoded_image_bytes;
    }
}

struct PreparedAnimationCacheEntry {
    source: Weak<[u8]>,
    // Field order matters: dispose prepared resources before releasing quota.
    animation: PreparedAnimation,
    reservation: PreparedCacheReservation,
}

impl PreparedAnimationCacheEntry {
    fn into_active_animation(self) -> PreparedAnimation {
        let Self {
            animation,
            reservation,
            ..
        } = self;
        // Active rendering is outside the idle-cache budget. Recompute the
        // charge after rendering because another lazy image may have decoded.
        drop(reservation);
        animation
    }
}

struct PreparedAnimationCache {
    entries: VecDeque<PreparedAnimationCacheEntry>,
    decoded_image_bytes: u64,
    max_entries: usize,
    max_decoded_image_bytes: u64,
    budget: Arc<PreparedCacheBudget>,
}

impl PreparedAnimationCache {
    fn new(
        max_entries: usize,
        max_decoded_image_bytes: u64,
        budget: Arc<PreparedCacheBudget>,
    ) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            decoded_image_bytes: 0,
            max_entries,
            max_decoded_image_bytes,
            budget,
        }
    }

    fn remove(&mut self, index: usize) -> Option<PreparedAnimationCacheEntry> {
        let entry = self.entries.remove(index)?;
        self.decoded_image_bytes -= entry.reservation.decoded_image_bytes;
        Some(entry)
    }

    fn retire(&mut self, source: Option<&Weak<[u8]>>) -> Vec<PreparedAnimationCacheEntry> {
        let mut retired = Vec::new();
        let mut index = 0;
        while index < self.entries.len() {
            let entry = &self.entries[index];
            if entry.source.strong_count() == 0
                || source.is_some_and(|source| Weak::ptr_eq(&entry.source, source))
            {
                retired.push(self.remove(index).unwrap());
            } else {
                index += 1;
            }
        }
        retired
    }

    #[cfg(test)]
    fn clear(&mut self) -> VecDeque<PreparedAnimationCacheEntry> {
        self.decoded_image_bytes = 0;
        std::mem::take(&mut self.entries)
    }
}

#[cfg(not(test))]
static PREPARED_CACHE_BUDGET: std::sync::LazyLock<Arc<PreparedCacheBudget>> =
    std::sync::LazyLock::new(|| {
        Arc::new(PreparedCacheBudget::new(
            MAX_PREPARED_ANIMATIONS_GLOBAL,
            MAX_PREPARED_IMAGE_BYTES_GLOBAL,
        ))
    });

fn default_prepared_animation_cache() -> PreparedAnimationCache {
    #[cfg(not(test))]
    let budget = PREPARED_CACHE_BUDGET.clone();
    // Unit tests get an isolated budget; multi-thread admission tests explicitly
    // share an injected budget instead of altering process-global limits.
    #[cfg(test)]
    let budget = Arc::new(PreparedCacheBudget::new(
        MAX_PREPARED_ANIMATIONS_GLOBAL,
        MAX_PREPARED_IMAGE_BYTES_GLOBAL,
    ));
    PreparedAnimationCache::new(
        MAX_PREPARED_ANIMATIONS_PER_THREAD,
        MAX_PREPARED_IMAGE_BYTES_PER_THREAD,
        budget,
    )
}

thread_local! {
    /// Idle decoded image buffers are bounded to 64 MiB / 4 entries per thread and
    /// 256 MiB / 16 entries across threads. These limits exclude active renders,
    /// parsed models, other preparation caches, scratch and allocator overhead.
    /// Prepared animations contain Rc/RefCell and never move between threads.
    static PREPARED_ANIMATION_CACHE: RefCell<PreparedAnimationCache> =
        RefCell::new(default_prepared_animation_cache());

    #[cfg(test)]
    static PREPARED_ANIMATION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    #[cfg(test)]
    static RENDERED_FRAME_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A type alias to the resource loader that the `lottie()` element uses.
pub type LottieResourceLoader = AssetLogger<LottieAssetLoader>;

/// A source of Lottie animation content.
#[derive(Clone)]
pub enum LottieSource {
    /// The animation should be loaded from a resource location.
    Resource(Resource),
    /// The animation bytes are already available in memory.
    Bytes(Arc<[u8]>),
    /// The animation has already been decoded.
    Animation(Arc<LottieAnimation>),
}

/// The playback state for a Lottie animation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackState {
    /// The animation is advancing over time.
    Playing,
    /// The animation is paused on its current frame.
    Paused,
    /// The animation is stopped and reset to the first frame.
    Stopped,
}

/// The loop policy used by a Lottie player.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LoopMode {
    /// Play to the last frame and stop.
    #[default]
    Once,
    /// Restart from the first frame when the animation ends.
    Loop,
    /// Reverse direction at the ends of the timeline.
    PingPong,
}

/// Parsed Lottie animation metadata and source bytes.
///
/// Idle worker preparations retain at most 64 MiB of decoded image pixels and
/// four entries per thread, with process-wide limits of 256 MiB and 16 entries.
/// Larger valid preparations render without being cached. These bounds exclude
/// active rendering, parsed models, other preparation caches, scratch buffers,
/// poster/output frames and allocator overhead; they do not bound total RSS.
#[derive(Clone, Debug)]
pub struct LottieAnimation {
    data: Arc<LottieData>,
}

/// Playback controller for a decoded Lottie animation.
#[derive(Clone, Debug)]
pub struct LottiePlayer {
    animation: Arc<LottieAnimation>,
    current_frame: usize,
    state: PlaybackState,
    loop_mode: LoopMode,
    started_at: Option<Instant>,
    elapsed_before_pause: Duration,
}

#[derive(Debug)]
struct LottieData {
    bytes: Arc<[u8]>,
    native_size: Size<Pixels>,
    native_pixel_size: Size<DevicePixels>,
    total_frames: usize,
    fps: f32,
    in_point: f32,
    poster_frame: Arc<RenderImage>,
}

/// Internal asset-cache key used for Lottie resource loading.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum LottieAssetSource {
    /// Load animation bytes from a resource location.
    Resource(Resource),
    /// Load animation bytes directly from memory.
    Bytes(Arc<[u8]>),
}

#[derive(Clone, Debug)]
pub(crate) struct LottieRenderBatch {
    pub(crate) render_size: Size<DevicePixels>,
    pub(crate) frames: Vec<LottieRenderedFrame>,
}

#[derive(Clone, Debug)]
pub(crate) struct LottieRenderedFrame {
    pub(crate) frame_index: usize,
    pub(crate) image: Arc<RenderImage>,
}

struct LottieRenderPlan {
    width: u32,
    height: u32,
    frame_bytes: usize,
    config: RenderConfig,
}

/// An error that can occur when loading or rendering a Lottie animation.
#[derive(Clone, Debug, Error)]
pub enum LottieError {
    /// An unexpected error occurred while loading the animation.
    #[error("error: {0}")]
    Other(Arc<anyhow::Error>),
    /// An IO error occurred while loading or decoding animation data.
    #[error("io error: {0}")]
    Io(Arc<io::Error>),
    /// A UTF-8 decoding error occurred while parsing JSON animation data.
    #[error("utf-8 error: {0}")]
    Utf8(Arc<str::Utf8Error>),
    /// The requested embedded asset does not exist.
    #[error("asset error: {0}")]
    Asset(SharedString),
    /// The requested URI returned an error status.
    #[error("unexpected HTTP status while loading Lottie asset: {status}")]
    BadStatus {
        /// The requested URI.
        uri: SharedUri,
        /// The received status code.
        status: http_client::StatusCode,
        /// The first line of the response body.
        body: String,
    },
    /// The Lottie data could not be parsed or rendered.
    #[error("lottie error: {0}")]
    Rasterlottie(Arc<rasterlottie::RasterlottieError>),
    /// The rendered frame buffer shape was invalid.
    #[error("invalid lottie frame buffer")]
    InvalidFrameBuffer,
}

fn is_dotlottie(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

impl LottieSource {
    /// Construct an in-memory source from compile-time bytes.
    pub fn from_static_bytes(bytes: &'static [u8]) -> Self {
        Self::Bytes(Arc::<[u8]>::from(bytes))
    }

    /// Returns the high-level source class without exposing paths, URLs, or bytes.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Resource(Resource::Uri(_)) => "uri",
            Self::Resource(Resource::Path(_)) => "path",
            Self::Resource(Resource::Embedded(_)) => "embedded",
            Self::Bytes(_) => "bytes",
            Self::Animation(_) => "animation",
        }
    }

    /// Returns true when the source carries in-memory bytes.
    pub fn has_bytes(&self) -> bool {
        matches!(self, Self::Bytes(_))
    }

    /// Returns the in-memory byte length when this source is byte-backed.
    pub fn byte_len(&self) -> Option<usize> {
        match self {
            Self::Bytes(bytes) => Some(bytes.len()),
            _ => None,
        }
    }

    /// Returns a content-safe summary of this Lottie source.
    pub fn to_text(&self) -> String {
        format!(
            "lottie source: kind {}, bytes {}, decoded {}",
            self.kind(),
            self.byte_len()
                .map(|len| len.to_string())
                .unwrap_or_else(|| "none".to_string()),
            matches!(self, Self::Animation(_))
        )
    }

    pub(crate) fn use_animation(
        &self,
        window: &mut crate::Window,
        cx: &mut App,
    ) -> Option<Result<Arc<LottieAnimation>, LottieError>> {
        match self {
            Self::Resource(resource) => window.use_asset::<LottieResourceLoader>(
                &LottieAssetSource::Resource(resource.clone()),
                cx,
            ),
            Self::Bytes(bytes) => window
                .use_asset::<LottieResourceLoader>(&LottieAssetSource::Bytes(bytes.clone()), cx),
            Self::Animation(animation) => Some(Ok(animation.clone())),
        }
    }

    pub(crate) fn get_animation(
        &self,
        window: &mut crate::Window,
        cx: &mut App,
    ) -> Option<Result<Arc<LottieAnimation>, LottieError>> {
        match self {
            Self::Resource(resource) => window.get_asset::<LottieResourceLoader>(
                &LottieAssetSource::Resource(resource.clone()),
                cx,
            ),
            Self::Bytes(bytes) => window
                .get_asset::<LottieResourceLoader>(&LottieAssetSource::Bytes(bytes.clone()), cx),
            Self::Animation(animation) => Some(Ok(animation.clone())),
        }
    }

    /// Remove this animation source from the asset system.
    pub fn remove_asset(&self, cx: &mut App) {
        match self {
            Self::Resource(resource) => {
                cx.remove_asset::<LottieResourceLoader>(&LottieAssetSource::Resource(
                    resource.clone(),
                ));
            }
            Self::Bytes(bytes) => {
                cx.remove_asset::<LottieResourceLoader>(&LottieAssetSource::Bytes(bytes.clone()));
            }
            Self::Animation(_) => {}
        }
    }
}

impl LottieAnimation {
    /// Parse a Lottie animation from bytes.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, LottieError> {
        Self::build(bytes.into())
    }

    /// Parse a Lottie animation from a JSON string.
    pub fn from_json_str(json: &str) -> Result<Self, LottieError> {
        Self::build(Arc::<[u8]>::from(json.as_bytes().to_vec()))
    }

    /// Returns the natural animation size in logical pixels.
    pub fn size(&self) -> Size<Pixels> {
        self.data.native_size
    }

    /// Returns the number of frames in the animation timeline.
    pub fn total_frames(&self) -> usize {
        self.data.total_frames
    }

    /// Returns the animation frame rate.
    pub fn fps(&self) -> f32 {
        self.data.fps
    }

    /// Returns the animation duration.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.data.total_frames as f32 / self.data.fps.max(1.0))
    }

    /// Returns a content-safe summary of parsed animation metadata.
    pub fn to_text(&self) -> String {
        format!(
            "lottie animation: frames {}, fps {:.1}, size {}x{}, duration_ms {}",
            self.total_frames(),
            self.fps(),
            self.size().width.0.round() as i32,
            self.size().height.0.round() as i32,
            self.duration().as_millis()
        )
    }

    /// Returns a pre-rendered first frame at the animation's native size.
    pub fn poster_frame(&self) -> Arc<RenderImage> {
        self.data.poster_frame.clone()
    }

    pub(crate) fn native_pixel_size(&self) -> Size<DevicePixels> {
        self.data.native_pixel_size
    }

    pub(crate) fn timeline_frame(&self, frame_index: usize) -> f32 {
        self.data.in_point + frame_index.min(self.data.total_frames.saturating_sub(1)) as f32
    }

    #[cfg(test)]
    pub(crate) fn render_batch(
        &self,
        render_size: Size<DevicePixels>,
        frames: &[usize],
    ) -> Result<LottieRenderBatch, LottieError> {
        self.render_batch_with_budget(render_size, frames, MAX_LOTTIE_RENDER_BATCH_BYTES)
    }

    pub(crate) fn max_admitted_frame_count(
        &self,
        render_size: Size<DevicePixels>,
    ) -> Result<usize, LottieError> {
        self.max_admitted_frame_count_with_budget(render_size, MAX_LOTTIE_RENDER_BATCH_BYTES)
    }

    pub(crate) fn max_admitted_frame_count_with_budget(
        &self,
        render_size: Size<DevicePixels>,
        max_bytes: usize,
    ) -> Result<usize, LottieError> {
        let plan = self.render_plan(render_size)?;
        let frames = (max_bytes / plan.frame_bytes).min(MAX_LOTTIE_RENDER_BATCH);
        if frames == 0 {
            return Err(anyhow::anyhow!(
                "Lottie frame exceeds the {max_bytes} byte render batch limit"
            )
            .into());
        }
        Ok(frames)
    }

    fn render_plan(
        &self,
        render_size: Size<DevicePixels>,
    ) -> Result<LottieRenderPlan, LottieError> {
        validate_render_size(render_size)?;
        let scale = render_scale(self.data.native_pixel_size, render_size);
        let (width, height) = render_canvas_dimensions(self.data.native_pixel_size, scale)?;
        Ok(LottieRenderPlan {
            width,
            height,
            frame_bytes: checked_frame_len(width, height)?,
            config: RenderConfig::new(Rgba8::TRANSPARENT, scale),
        })
    }

    pub(crate) fn render_batch_cancellable(
        &self,
        render_size: Size<DevicePixels>,
        frames: &[usize],
        cancelled: &AtomicBool,
    ) -> Result<Option<LottieRenderBatch>, LottieError> {
        self.render_batch_with_control(render_size, frames, MAX_LOTTIE_RENDER_BATCH_BYTES, |_| {
            !cancelled.load(Ordering::Acquire)
        })
    }

    #[cfg(test)]
    pub(crate) fn render_batch_with_budget(
        &self,
        render_size: Size<DevicePixels>,
        frames: &[usize],
        max_bytes: usize,
    ) -> Result<LottieRenderBatch, LottieError> {
        self.render_batch_with_control(render_size, frames, max_bytes, |_| true)
            .map(|batch| batch.expect("unconditionally enabled render"))
    }

    pub(crate) fn render_batch_with_control(
        &self,
        render_size: Size<DevicePixels>,
        frames: &[usize],
        max_bytes: usize,
        mut should_continue: impl FnMut(usize) -> bool,
    ) -> Result<Option<LottieRenderBatch>, LottieError> {
        if !should_continue(0) {
            return Ok(None);
        }
        validate_render_size(render_size)?;
        if frames.len() > MAX_LOTTIE_RENDER_BATCH {
            return Err(anyhow::anyhow!(
                "Lottie render batch cannot exceed {MAX_LOTTIE_RENDER_BATCH} frames"
            )
            .into());
        }
        if frames.is_empty() {
            return Ok(Some(LottieRenderBatch {
                render_size,
                frames: Vec::new(),
            }));
        }
        let plan = self.render_plan(render_size)?;
        // Bound output buffers retained by this call before preparation, renderer
        // execution or the output Vec allocation. Prepared assets, renderer/layer
        // scratch, concurrent calls and the element's frame cache are additional.
        let batch_bytes = plan
            .frame_bytes
            .checked_mul(frames.len())
            .ok_or_else(|| anyhow::anyhow!("Lottie render batch byte count overflowed"))?;
        if batch_bytes > max_bytes {
            return Err(anyhow::anyhow!(
                "Lottie render batch cannot exceed {max_bytes} retained frame bytes"
            )
            .into());
        }
        if !should_continue(0) {
            return Ok(None);
        }
        with_prepared_animation(&self.data.bytes, |prepared| {
            let mut rendered_frames = Vec::with_capacity(frames.len());

            for &frame_index in frames {
                if !should_continue(rendered_frames.len()) {
                    return Ok(None);
                }
                #[cfg(test)]
                RENDERED_FRAME_COUNT.with(|count| count.set(count.get() + 1));
                let frame = prepared.render_frame(self.timeline_frame(frame_index), plan.config);
                // A synchronous frame cannot be interrupted. Retired work releases
                // its frame and partial batch as soon as that frame returns.
                if !should_continue(rendered_frames.len() + 1) {
                    return Ok(None);
                }
                let frame = frame?;
                if (frame.width, frame.height) != (plan.width, plan.height) {
                    return Err(LottieError::InvalidFrameBuffer);
                }
                rendered_frames.push(LottieRenderedFrame {
                    frame_index,
                    image: raster_frame_to_image(frame)?,
                });
            }

            Ok(Some(LottieRenderBatch {
                render_size,
                frames: rendered_frames,
            }))
        })
    }

    fn build(bytes: Arc<[u8]>) -> Result<Self, LottieError> {
        validate_lottie_bytes(&bytes)?;
        let animation = parse_animation(&bytes)?;
        validate_animation_metadata(&animation)?;
        let native_pixel_size = size(
            DevicePixels(animation.width as i32),
            DevicePixels(animation.height as i32),
        );
        let native_size = size(px(animation.width as f32), px(animation.height as f32));
        let fps = animation.frame_rate;
        let total_frames = animation.duration_frames().ceil() as usize;
        let in_point = animation.in_point;
        let prepared = prepare_animation(&animation)?;
        let poster_frame =
            raster_frame_to_image(prepared.render_frame(in_point, RenderConfig::default())?)?;
        cache_prepared_animation(&bytes, prepared);

        Ok(Self {
            data: Arc::new(LottieData {
                bytes,
                native_size,
                native_pixel_size,
                total_frames,
                fps,
                in_point,
                poster_frame,
            }),
        })
    }
}

impl LottiePlayer {
    /// Create a player for the given animation.
    pub fn new(animation: Arc<LottieAnimation>) -> Self {
        Self {
            animation,
            current_frame: 0,
            state: PlaybackState::Stopped,
            loop_mode: LoopMode::Once,
            started_at: None,
            elapsed_before_pause: Duration::ZERO,
        }
    }

    /// Returns the animation controlled by this player.
    pub fn animation(&self) -> &Arc<LottieAnimation> {
        &self.animation
    }

    /// Returns the current playback state.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Returns the current loop mode.
    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// Set the loop mode.
    pub fn set_loop_mode(&mut self, loop_mode: LoopMode) {
        self.loop_mode = loop_mode;
    }

    /// Start playback from the current position.
    pub fn play(&mut self) {
        self.play_at(Instant::now());
    }

    /// Pause playback and keep the current frame.
    pub fn pause(&mut self) {
        self.pause_at(Instant::now());
    }

    /// Stop playback and reset to the first frame.
    pub fn stop(&mut self) {
        self.started_at = None;
        self.elapsed_before_pause = Duration::ZERO;
        self.current_frame = 0;
        self.state = PlaybackState::Stopped;
    }

    /// Seek to a specific frame.
    pub fn seek_to_frame(&mut self, frame_index: usize) {
        self.seek_to_frame_at(frame_index, Instant::now());
    }

    /// Returns the frame index that should currently be displayed.
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    /// Returns a content-safe summary of playback state.
    pub fn to_text(&self) -> String {
        format!(
            "lottie player: state {}, loop {}, current_frame {}, frames {}, animating {}",
            self.state.to_text(),
            self.loop_mode.to_text(),
            self.current_frame(),
            self.animation.total_frames(),
            self.is_animating()
        )
    }

    /// Update the player using the provided timestamp and return the active frame index.
    pub fn update(&mut self, now: Instant) -> usize {
        let step = self.elapsed_at(now).as_secs_f32() * self.animation.fps();
        let step = step.floor().max(0.0) as usize;
        self.current_frame = self.frame_for_step(step);

        if self.state == PlaybackState::Playing
            && self.loop_mode == LoopMode::Once
            && self.current_frame + 1 >= self.animation.total_frames()
        {
            self.state = PlaybackState::Stopped;
            self.started_at = None;
            self.elapsed_before_pause = Duration::from_secs_f32(
                self.animation.total_frames() as f32 / self.animation.fps(),
            );
        }

        self.current_frame
    }

    pub(crate) fn is_animating(&self) -> bool {
        self.state == PlaybackState::Playing && self.animation.total_frames() > 1
    }

    pub(crate) fn upcoming_frames(&self, now: Instant, count: usize) -> Vec<usize> {
        let count = count.clamp(1, MAX_LOTTIE_RENDER_BATCH);
        let base_step = (self.elapsed_at(now).as_secs_f32() * self.animation.fps())
            .floor()
            .max(0.0) as usize;
        let mut frames = Vec::with_capacity(count);

        for offset in 0..count {
            let frame = self.frame_for_step(base_step.saturating_add(offset));
            if !frames.contains(&frame) {
                frames.push(frame);
            }

            if self.loop_mode == LoopMode::Once && frame + 1 >= self.animation.total_frames() {
                break;
            }
        }

        frames
    }

    pub(crate) fn play_at(&mut self, now: Instant) {
        if self.state == PlaybackState::Playing {
            return;
        }

        self.started_at = Some(now.checked_sub(self.elapsed_before_pause).unwrap_or(now));
        self.state = PlaybackState::Playing;
    }

    pub(crate) fn pause_at(&mut self, now: Instant) {
        if self.state != PlaybackState::Playing {
            return;
        }

        self.elapsed_before_pause = self.elapsed_at(now);
        self.started_at = None;
        self.state = PlaybackState::Paused;
    }

    pub(crate) fn seek_to_frame_at(&mut self, frame_index: usize, now: Instant) {
        let clamped = frame_index.min(self.animation.total_frames().saturating_sub(1));
        self.current_frame = clamped;
        self.elapsed_before_pause = Duration::from_secs_f32(clamped as f32 / self.animation.fps());
        if self.state == PlaybackState::Playing {
            self.started_at = Some(now.checked_sub(self.elapsed_before_pause).unwrap_or(now));
        }
    }

    fn elapsed_at(&self, now: Instant) -> Duration {
        match self.state {
            PlaybackState::Playing => self
                .started_at
                .map(|started_at| now.saturating_duration_since(started_at))
                .unwrap_or(self.elapsed_before_pause),
            PlaybackState::Paused | PlaybackState::Stopped => self.elapsed_before_pause,
        }
    }

    fn frame_for_step(&self, step: usize) -> usize {
        let frame_count = self.animation.total_frames();
        if frame_count <= 1 {
            return 0;
        }

        match self.loop_mode {
            LoopMode::Once => step.min(frame_count - 1),
            LoopMode::Loop => step % frame_count,
            LoopMode::PingPong => {
                let cycle = frame_count.saturating_mul(2).saturating_sub(2).max(1);
                let step = step % cycle;
                if step < frame_count {
                    step
                } else {
                    cycle - step
                }
            }
        }
    }
}

impl PlaybackState {
    /// Returns a stable lowercase label for this playback state.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

impl LoopMode {
    /// Returns a stable lowercase label for this loop mode.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Loop => "loop",
            Self::PingPong => "ping-pong",
        }
    }
}

impl Asset for LottieAssetLoader {
    type Source = LottieAssetSource;
    type Output = Result<Arc<LottieAnimation>, LottieError>;

    fn cache_bytes(output: &Self::Output) -> u64 {
        output.as_ref().map_or(0, |animation| {
            (animation.data.bytes.len() as u64)
                .saturating_add(animation.data.poster_frame.decoded_bytes())
        })
    }

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl futures::Future<Output = Self::Output> + Send + 'static {
        let client = cx.http_client();
        let asset_source = cx.asset_source().clone();

        async move {
            let bytes = match source {
                LottieAssetSource::Resource(resource) => match resource {
                    Resource::Path(path) => read_lottie_file(path.as_ref())?,
                    Resource::Uri(uri) => {
                        let mut response = client
                            .get(uri.as_ref(), ().into(), true)
                            .await
                            .context("loading Lottie asset")?;
                        if !response.status().is_success() {
                            return Err(LottieError::BadStatus {
                                uri,
                                status: response.status(),
                                body: String::new(),
                            });
                        }
                        let mut body = Vec::new();
                        response
                            .body_mut()
                            .take((MAX_LOTTIE_SOURCE_BYTES + 1) as u64)
                            .read_to_end(&mut body)
                            .await?;
                        if body.len() > MAX_LOTTIE_SOURCE_BYTES {
                            return Err(anyhow::anyhow!(
                                "Lottie source cannot exceed {MAX_LOTTIE_SOURCE_BYTES} bytes"
                            )
                            .into());
                        }
                        Arc::<[u8]>::from(body)
                    }
                    Resource::Embedded(path) => {
                        let data = asset_source.load(&path).ok().flatten();
                        if let Some(data) = data {
                            validate_lottie_bytes(&data)?;
                            Arc::<[u8]>::from(data)
                        } else {
                            return Err(LottieError::Asset("embedded resource not found".into()));
                        }
                    }
                },
                LottieAssetSource::Bytes(bytes) => bytes,
            };

            Ok(Arc::new(LottieAnimation::from_bytes(bytes)?))
        }
    }
}

/// Asset loader for decoded Lottie animations.
#[derive(Clone)]
pub enum LottieAssetLoader {}

impl From<SharedUri> for LottieSource {
    fn from(value: SharedUri) -> Self {
        Self::Resource(Resource::Uri(value))
    }
}

impl From<Resource> for LottieSource {
    fn from(value: Resource) -> Self {
        Self::Resource(value)
    }
}

impl From<&str> for LottieSource {
    fn from(value: &str) -> Self {
        if is_uri(value) {
            Self::Resource(Resource::Uri(value.to_string().into()))
        } else {
            Self::Resource(Resource::Embedded(value.to_string().into()))
        }
    }
}

impl From<String> for LottieSource {
    fn from(value: String) -> Self {
        if is_uri(&value) {
            Self::Resource(Resource::Uri(value.into()))
        } else {
            Self::Resource(Resource::Embedded(value.into()))
        }
    }
}

impl From<SharedString> for LottieSource {
    fn from(value: SharedString) -> Self {
        value.as_ref().into()
    }
}

impl From<&Path> for LottieSource {
    fn from(value: &Path) -> Self {
        Self::Resource(value.to_path_buf().into())
    }
}

impl From<PathBuf> for LottieSource {
    fn from(value: PathBuf) -> Self {
        Self::Resource(value.into())
    }
}

impl From<Arc<Path>> for LottieSource {
    fn from(value: Arc<Path>) -> Self {
        Self::Resource(value.into())
    }
}

impl From<Vec<u8>> for LottieSource {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(Arc::<[u8]>::from(value))
    }
}

impl From<Arc<[u8]>> for LottieSource {
    fn from(value: Arc<[u8]>) -> Self {
        Self::Bytes(value)
    }
}

impl From<&'static [u8]> for LottieSource {
    fn from(value: &'static [u8]) -> Self {
        Self::from_static_bytes(value)
    }
}

impl From<Arc<LottieAnimation>> for LottieSource {
    fn from(value: Arc<LottieAnimation>) -> Self {
        Self::Animation(value)
    }
}

impl From<io::Error> for LottieError {
    fn from(value: io::Error) -> Self {
        Self::Io(Arc::new(value))
    }
}

impl From<anyhow::Error> for LottieError {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(Arc::new(value))
    }
}

impl From<str::Utf8Error> for LottieError {
    fn from(value: str::Utf8Error) -> Self {
        Self::Utf8(Arc::new(value))
    }
}

impl From<rasterlottie::RasterlottieError> for LottieError {
    fn from(value: rasterlottie::RasterlottieError) -> Self {
        Self::Rasterlottie(Arc::new(value))
    }
}

fn parse_animation(bytes: &[u8]) -> Result<rasterlottie::Animation, LottieError> {
    validate_lottie_bytes(bytes)?;
    if is_dotlottie(bytes) {
        Ok(rasterlottie::Animation::from_dotlottie_bytes(bytes)?)
    } else {
        let json = str::from_utf8(bytes)?;
        Ok(rasterlottie::Animation::from_json_str(json)?)
    }
}

fn prepare_animation(
    animation: &rasterlottie::Animation,
) -> Result<PreparedAnimation, LottieError> {
    let prepared = Renderer::target_corpus().prepare(animation)?;

    #[cfg(test)]
    PREPARED_ANIMATION_COUNT.with(|count| count.set(count.get() + 1));

    Ok(prepared)
}

fn take_prepared_animation_from(
    cache: &RefCell<PreparedAnimationCache>,
    bytes: &Arc<[u8]>,
) -> Option<PreparedAnimation> {
    let source = Arc::downgrade(bytes);
    let (entry, retired) = {
        let mut cache = cache.borrow_mut();
        let retired = cache.retire(None);
        let entry = cache
            .entries
            .iter()
            .position(|entry| Weak::ptr_eq(&entry.source, &source))
            .and_then(|index| cache.remove(index));
        (entry, retired)
    };
    // No TLS borrow or budget lock is held while resources are destroyed.
    drop(retired);
    entry.map(PreparedAnimationCacheEntry::into_active_animation)
}

fn cache_prepared_animation_in(
    cache: &RefCell<PreparedAnimationCache>,
    bytes: &Arc<[u8]>,
    animation: PreparedAnimation,
) {
    let source = Arc::downgrade(bytes);
    let bytes = animation.cached_decoded_image_bytes();
    let (budget, mut retired, can_retain) = {
        let mut cache = cache.borrow_mut();
        let mut retired = cache.retire(Some(&source));
        let can_retain = cache.max_entries > 0
            && bytes <= cache.max_decoded_image_bytes
            && cache.budget.max_entries > 0
            && bytes <= cache.budget.max_decoded_image_bytes;
        if can_retain {
            while cache.entries.len() >= cache.max_entries
                || cache.decoded_image_bytes > cache.max_decoded_image_bytes - bytes
            {
                let index = cache.entries.len() - 1;
                retired.push(cache.remove(index).unwrap());
            }
        }
        (cache.budget.clone(), retired, can_retain)
    };
    retired.clear();
    if !can_retain {
        return;
    }
    loop {
        if let Some(reservation) = budget.reserve(bytes) {
            let mut cache = cache.borrow_mut();
            cache.decoded_image_bytes += bytes;
            cache.entries.push_front(PreparedAnimationCacheEntry {
                source,
                animation,
                reservation,
            });
            return;
        }
        // Other threads may own the global quota. Evict only this local LRU,
        // outside its borrow, and bypass caching if no local entry can free it.
        let entry = {
            let mut cache = cache.borrow_mut();
            let index = cache.entries.len().checked_sub(1);
            index.and_then(|index| cache.remove(index))
        };
        let Some(entry) = entry else {
            return;
        };
        drop(entry);
    }
}

fn cache_prepared_animation(bytes: &Arc<[u8]>, animation: PreparedAnimation) {
    PREPARED_ANIMATION_CACHE.with(|cache| cache_prepared_animation_in(cache, bytes, animation));
}

fn with_prepared_animation_in<T>(
    cache: &RefCell<PreparedAnimationCache>,
    bytes: &Arc<[u8]>,
    render: impl FnOnce(&PreparedAnimation) -> Result<Option<T>, LottieError>,
) -> Result<Option<T>, LottieError> {
    let animation = if let Some(animation) = take_prepared_animation_from(cache, bytes) {
        animation
    } else {
        prepare_animation(&parse_animation(bytes)?)?
    };
    // No TLS borrow or budget lock crosses rendering, including nested calls.
    let result = render(&animation);
    // Cancellation retires the preparation instead of retaining obsolete assets.
    if !matches!(result, Ok(None)) {
        cache_prepared_animation_in(cache, bytes, animation);
    }
    result
}

fn with_prepared_animation<T>(
    bytes: &Arc<[u8]>,
    render: impl FnOnce(&PreparedAnimation) -> Result<Option<T>, LottieError>,
) -> Result<Option<T>, LottieError> {
    PREPARED_ANIMATION_CACHE.with(|cache| with_prepared_animation_in(cache, bytes, render))
}

#[cfg(test)]
fn reset_prepared_animation_cache() {
    let retired = PREPARED_ANIMATION_CACHE.with(|cache| cache.borrow_mut().clear());
    drop(retired);
    PREPARED_ANIMATION_COUNT.with(|count| count.set(0));
    RENDERED_FRAME_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn prepared_animation_count() -> usize {
    PREPARED_ANIMATION_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
fn prepared_animation_cache_len() -> usize {
    PREPARED_ANIMATION_CACHE.with(|cache| cache.borrow().entries.len())
}

#[cfg(test)]
fn is_prepared_animation_cached(bytes: &Arc<[u8]>) -> bool {
    let source = Arc::downgrade(bytes);
    PREPARED_ANIMATION_CACHE.with(|cache| {
        cache
            .borrow()
            .entries
            .iter()
            .any(|entry| Weak::ptr_eq(&entry.source, &source))
    })
}

fn raster_frame_to_image(
    frame: rasterlottie::RasterFrame,
) -> Result<Arc<RenderImage>, LottieError> {
    let expected_len = checked_frame_len(frame.width, frame.height)?;
    if frame.pixels.len() != expected_len {
        return Err(LottieError::InvalidFrameBuffer);
    }
    let mut pixels = frame.pixels;
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(frame.width, frame.height, pixels)
        .ok_or(LottieError::InvalidFrameBuffer)?;

    Ok(Arc::new(RenderImage::new(SmallVec::from_elem(
        Frame::new(buffer),
        1,
    ))))
}

fn validate_lottie_bytes(bytes: &[u8]) -> Result<(), LottieError> {
    if bytes.is_empty() {
        return Err(anyhow::anyhow!("Lottie source cannot be empty").into());
    }
    if bytes.len() > MAX_LOTTIE_SOURCE_BYTES {
        return Err(
            anyhow::anyhow!("Lottie source cannot exceed {MAX_LOTTIE_SOURCE_BYTES} bytes").into(),
        );
    }
    Ok(())
}

fn validate_animation_metadata(animation: &rasterlottie::Animation) -> Result<(), LottieError> {
    if animation.width == 0
        || animation.height == 0
        || animation.width > MAX_LOTTIE_DIMENSION
        || animation.height > MAX_LOTTIE_DIMENSION
    {
        return Err(anyhow::anyhow!(
            "Lottie dimensions must be between 1 and {MAX_LOTTIE_DIMENSION} pixels"
        )
        .into());
    }
    checked_frame_len(animation.width, animation.height)?;
    if !animation.frame_rate.is_finite()
        || animation.frame_rate <= 0.0
        || animation.frame_rate > MAX_LOTTIE_FPS
    {
        return Err(anyhow::anyhow!(
            "Lottie frame rate must be finite, positive, and at most {MAX_LOTTIE_FPS}"
        )
        .into());
    }
    let duration_frames = animation.duration_frames();
    if !duration_frames.is_finite()
        || duration_frames <= 0.0
        || duration_frames.ceil() > MAX_LOTTIE_FRAMES as f32
        || !animation.in_point.is_finite()
    {
        return Err(anyhow::anyhow!("Lottie timeline metadata is invalid or too large").into());
    }
    Ok(())
}

fn validate_render_size(render_size: Size<DevicePixels>) -> Result<(), LottieError> {
    if render_size.width.0 <= 0 || render_size.height.0 <= 0 {
        return Err(anyhow::anyhow!("Lottie render dimensions must be positive").into());
    }
    checked_frame_len(render_size.width.0 as u32, render_size.height.0 as u32)?;
    Ok(())
}

fn checked_frame_len(width: u32, height: u32) -> Result<usize, LottieError> {
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| anyhow::anyhow!("Lottie frame dimensions overflowed"))?;
    if byte_len == 0 || byte_len > MAX_LOTTIE_FRAME_BYTES {
        return Err(
            anyhow::anyhow!("Lottie frame cannot exceed {MAX_LOTTIE_FRAME_BYTES} bytes").into(),
        );
    }
    Ok(byte_len)
}

fn read_lottie_file(path: &Path) -> Result<Arc<[u8]>, LottieError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_LOTTIE_SOURCE_BYTES as u64 {
        return Err(anyhow::anyhow!("Lottie source must be a bounded regular file").into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_LOTTIE_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_lottie_bytes(&bytes)?;
    Ok(Arc::<[u8]>::from(bytes))
}

fn render_scale(native_size: Size<DevicePixels>, render_size: Size<DevicePixels>) -> f32 {
    let native_width = native_size.width.0.max(1) as f32;
    let native_height = native_size.height.0.max(1) as f32;
    let render_width = render_size.width.0.max(1) as f32;
    let render_height = render_size.height.0.max(1) as f32;

    (render_width / native_width)
        .min(render_height / native_height)
        .max(1.0 / native_width.max(native_height))
}

fn render_canvas_dimensions(
    native_size: Size<DevicePixels>,
    scale: f32,
) -> Result<(u32, u32), LottieError> {
    // Match the pinned rasterlottie resolve_output_canvas_size f32 multiplication
    // and ceil exactly. Fitted output can differ from the requested rectangle,
    // including a rounding pixel; actual-pixel regressions guard this coupling.
    if !scale.is_finite() || scale <= 0.0 {
        return Err(anyhow::anyhow!("Lottie render scale must be finite and positive").into());
    }
    let width = ((native_size.width.0 as f32) * scale).ceil().max(1.0);
    let height = ((native_size.height.0 as f32) * scale).ceil().max(1.0);
    if !width.is_finite()
        || !height.is_finite()
        || width > u32::MAX as f32
        || height > u32::MAX as f32
    {
        return Err(anyhow::anyhow!("Lottie scaled canvas dimensions overflowed").into());
    }
    Ok((width as u32, height as u32))
}

/// Embed a Lottie asset directly into the binary at compile time.
#[macro_export]
macro_rules! include_lottie {
    ($path:literal) => {{ $crate::LottieSource::from_static_bytes(include_bytes!($path)) }};
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_LOTTIE: &str = r#"{
        "v":"5.7.6",
        "fr":30,
        "ip":0,
        "op":30,
        "w":64,
        "h":32,
        "layers":[]
    }"#;

    #[test]
    fn cancelled_render_skips_preparation_and_retires_partial_batch_assets() {
        let animation =
            LottieAnimation::from_json_str(r#"{"fr":30,"ip":0,"op":30,"w":1,"h":1,"layers":[]}"#)
                .unwrap();
        let size = size(DevicePixels(1), DevicePixels(1));
        reset_prepared_animation_cache();
        assert!(
            animation
                .render_batch_cancellable(size, &[0, 1, 2], &AtomicBool::new(true))
                .unwrap()
                .is_none()
        );
        assert_eq!(prepared_animation_count(), 0);
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 0);

        // Finish two genuine frames, then retire both the partial output and
        // the prepared animation instead of retaining either in the worker LRU.
        assert!(
            animation
                .render_batch_with_control(size, &[0, 1, 2], 12, |completed| completed < 2)
                .unwrap()
                .is_none()
        );
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 2);
        assert_eq!(prepared_animation_count(), 1);
        assert_eq!(prepared_animation_cache_len(), 0);
        let resumed = animation.render_batch(size, &[2, 3]).unwrap();
        assert_eq!(resumed.frames.len(), 2);
        assert_eq!(resumed.frames[0].frame_index, 2);
        assert_eq!(resumed.frames[0].image.size(0), size);
        assert_eq!(prepared_animation_count(), 2);
        assert_eq!(prepared_animation_cache_len(), 1);
        reset_prepared_animation_cache();
    }

    #[test]
    fn parses_basic_animation_metadata() {
        let animation = LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap();

        assert_eq!(animation.total_frames(), 30);
        assert_eq!(animation.fps(), 30.0);
        assert_eq!(animation.size(), size(px(64.0), px(32.0)));
        assert_eq!(animation.poster_frame().frame_count(), 1);
        assert_eq!(
            animation.poster_frame().size(0),
            size(DevicePixels(64), DevicePixels(32))
        );
    }

    #[test]
    fn loops_player_frames() {
        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let mut player = LottiePlayer::new(animation);
        let start = Instant::now();

        player.set_loop_mode(LoopMode::Loop);
        player.play_at(start);

        assert_eq!(player.update(start + Duration::from_millis(1100)), 3);
    }

    #[test]
    fn ping_pongs_player_frames() {
        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let mut player = LottiePlayer::new(animation);
        let start = Instant::now();

        player.set_loop_mode(LoopMode::PingPong);
        player.play_at(start);

        let frame = player.update(start + Duration::from_secs_f32(31.0 / 30.0));
        assert_eq!(frame, 27);
    }

    #[test]
    fn renders_requested_frame_batch_size() {
        let animation = LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap();
        let batch = animation
            .render_batch(size(DevicePixels(128), DevicePixels(64)), &[0, 1, 2])
            .unwrap();

        assert_eq!(batch.frames.len(), 3);
        assert_eq!(batch.render_size, size(DevicePixels(128), DevicePixels(64)));
        for frame in batch.frames {
            assert_eq!(
                frame.image.size(0),
                size(DevicePixels(128), DevicePixels(64))
            );
        }
    }

    #[test]
    fn render_batch_rejects_aggregate_bytes_before_preparation_or_pixels() {
        let animation =
            LottieAnimation::from_json_str(r#"{"fr":30,"ip":0,"op":30,"w":1,"h":1,"layers":[]}"#)
                .unwrap();
        reset_prepared_animation_cache();
        let render_size = size(DevicePixels(1), DevicePixels(1));
        assert_eq!(
            animation.max_admitted_frame_count(render_size).unwrap(),
            MAX_LOTTIE_RENDER_BATCH
        );
        assert!(
            animation
                .max_admitted_frame_count_with_budget(render_size, 3)
                .is_err()
        );
        assert_eq!(
            animation
                .max_admitted_frame_count_with_budget(render_size, 4)
                .unwrap(),
            1
        );
        assert_eq!(
            animation
                .max_admitted_frame_count_with_budget(render_size, 8)
                .unwrap(),
            2
        );
        assert!(
            animation
                .render_batch_with_budget(render_size, &[0, 1, 2], 8)
                .is_err()
        );
        assert_eq!(prepared_animation_count(), 0);
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 0);
        assert_eq!(prepared_animation_cache_len(), 0);
        let batch = animation
            .render_batch_with_budget(render_size, &[0, 1], 8)
            .unwrap();
        assert_eq!(batch.frames.len(), 2);
        assert!(
            batch
                .frames
                .iter()
                .all(|frame| frame.image.size(0) == render_size)
        );
        assert_eq!(prepared_animation_count(), 1);
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 2);
        assert!(
            animation
                .render_batch_with_budget(render_size, &[0, 1], 7)
                .is_err()
        );
        assert_eq!(prepared_animation_count(), 1);
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 2);
        reset_prepared_animation_cache();
        let empty = animation
            .render_batch_with_budget(render_size, &[], 0)
            .unwrap();
        assert!(empty.frames.is_empty());
        assert_eq!(empty.render_size, render_size);
        assert_eq!(prepared_animation_count(), 0);
        assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 0);
    }

    #[test]
    fn render_batch_accounts_for_actual_fitted_and_rounded_canvas() {
        for (native, requested, actual) in
            [((13, 1), (7, 7), (8, 1)), ((1, 8192), (10000, 1), (1, 1))]
        {
            let animation = LottieAnimation::from_json_str(&format!(
                r#"{{"fr":30,"ip":0,"op":1,"w":{},"h":{},"layers":[]}}"#,
                native.0, native.1,
            ))
            .unwrap();
            let requested = size(DevicePixels(requested.0), DevicePixels(requested.1));
            let exact_bytes = actual.0 * actual.1 * 4;
            reset_prepared_animation_cache();
            assert!(
                animation
                    .max_admitted_frame_count_with_budget(requested, exact_bytes - 1)
                    .is_err()
            );
            assert_eq!(
                animation
                    .max_admitted_frame_count_with_budget(requested, exact_bytes)
                    .unwrap(),
                1
            );
            assert!(
                animation
                    .render_batch_with_budget(requested, &[0], exact_bytes - 1)
                    .is_err()
            );
            assert_eq!(prepared_animation_count(), 0);
            assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 0);
            let batch = animation
                .render_batch_with_budget(requested, &[0], exact_bytes)
                .unwrap();
            assert_eq!(batch.render_size, requested);
            assert_eq!(
                batch.frames[0].image.size(0),
                size(DevicePixels(actual.0 as i32), DevicePixels(actual.1 as i32))
            );
            assert_eq!(prepared_animation_count(), 1);
            assert_eq!(RENDERED_FRAME_COUNT.with(std::cell::Cell::get), 1);
            reset_prepared_animation_cache();
        }
    }

    #[test]
    fn repeated_batches_reuse_the_thread_local_prepared_animation() {
        reset_prepared_animation_cache();
        let animation = LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap();

        assert_eq!(prepared_animation_count(), 1);
        assert!(is_prepared_animation_cached(&animation.data.bytes));

        for frames in [&[0, 1][..], &[2, 3][..]] {
            animation
                .render_batch(size(DevicePixels(64), DevicePixels(32)), frames)
                .unwrap();
        }

        assert_eq!(prepared_animation_count(), 1);
        assert_eq!(prepared_animation_cache_len(), 1);
        reset_prepared_animation_cache();
    }

    #[test]
    fn prepared_animation_cache_is_bounded_and_evicts_lru_entries() {
        reset_prepared_animation_cache();
        let animations = (0..=MAX_PREPARED_ANIMATIONS_PER_THREAD)
            .map(|_| LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(prepared_animation_count(), animations.len());
        assert_eq!(
            prepared_animation_cache_len(),
            MAX_PREPARED_ANIMATIONS_PER_THREAD
        );
        assert!(!is_prepared_animation_cached(&animations[0].data.bytes));
        assert!(is_prepared_animation_cached(
            &animations.last().unwrap().data.bytes
        ));

        animations[0]
            .render_batch(size(DevicePixels(64), DevicePixels(32)), &[0])
            .unwrap();
        assert_eq!(prepared_animation_count(), animations.len() + 1);
        assert_eq!(
            prepared_animation_cache_len(),
            MAX_PREPARED_ANIMATIONS_PER_THREAD
        );
        assert!(is_prepared_animation_cached(&animations[0].data.bytes));
        assert!(!is_prepared_animation_cached(&animations[1].data.bytes));
        reset_prepared_animation_cache();
    }

    fn pixel_source(width: u32, two_frames: bool) -> Arc<[u8]> {
        use base64::Engine as _;
        use image::ImageEncoder as _;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ColorType::Rgba8.into())
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let second_asset = if two_frames {
            format!(
                r#",{{"id":"second","w":{width},"h":1,"e":1,"p":"data:image/png;base64,{data}"}}"#
            )
        } else {
            String::new()
        };
        let second_layer = if two_frames {
            r#",{"ty":2,"ind":2,"refId":"second","ip":1,"op":2}"#
        } else {
            ""
        };
        Arc::from(format!(r#"{{"fr":30,"ip":0,"op":2,"w":{width},"h":1,
            "assets":[{{"id":"first","w":{width},"h":1,"e":1,"p":"data:image/png;base64,{data}"}}{second_asset}],
            "layers":[{{"ty":2,"ind":1,"refId":"first","ip":0,"op":1}}{second_layer}]}}"#).into_bytes())
    }

    fn render_pixel_source(
        cache: &RefCell<PreparedAnimationCache>,
        source: &Arc<[u8]>,
        frame: f32,
    ) -> Vec<u8> {
        with_prepared_animation_in(cache, source, |prepared| {
            let frame = prepared.render_frame(frame, RenderConfig::default())?;
            assert!(
                frame
                    .pixels
                    .chunks_exact(4)
                    .all(|pixel| pixel == [255, 0, 0, 255])
            );
            Ok(Some(frame.pixels))
        })
        .unwrap()
        .unwrap()
    }

    fn cache_usage(budget: &PreparedCacheBudget) -> PreparedCacheUsage {
        *budget.usage.lock()
    }

    #[test]
    fn prepared_cache_local_bytes_evict_lru_and_oversize_still_renders() {
        let budget = Arc::new(PreparedCacheBudget::new(16, 64));
        let cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
        let sources: Vec<_> = (0..3).map(|_| pixel_source(1, false)).collect();
        for source in &sources {
            render_pixel_source(&cache, source, 0.0);
        }
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 2,
                decoded_image_bytes: 8
            }
        );
        assert!(take_prepared_animation_from(&cache, &sources[0]).is_none());
        // Touch source1, so source2 is the next byte-pressure victim.
        assert_eq!(
            render_pixel_source(&cache, &sources[1], 0.0),
            [255, 0, 0, 255]
        );
        render_pixel_source(&cache, &sources[0], 0.0);
        assert!(take_prepared_animation_from(&cache, &sources[2]).is_none());
        assert_eq!(
            Arc::strong_count(&sources[0]),
            1,
            "cache identity must stay weak"
        );
        let oversized = pixel_source(3, false);
        for _ in 0..2 {
            assert_eq!(render_pixel_source(&cache, &oversized, 0.0).len(), 12);
        }
        assert!(take_prepared_animation_from(&cache, &oversized).is_none());
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 2,
                decoded_image_bytes: 8
            }
        );
        drop(cache);
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn prepared_cache_recharges_lazy_growth_and_replaces_same_source() {
        let budget = Arc::new(PreparedCacheBudget::new(16, 64));
        let cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
        let growing = pixel_source(1, true);
        let other = pixel_source(1, false);
        render_pixel_source(&cache, &growing, 0.0);
        render_pixel_source(&cache, &other, 0.0);
        assert_eq!(cache_usage(&budget).decoded_image_bytes, 8);
        render_pixel_source(&cache, &growing, 1.0);
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 8
            }
        );
        assert!(take_prepared_animation_from(&cache, &other).is_none());
        let replacement = prepare_animation(&parse_animation(&growing).unwrap()).unwrap();
        replacement
            .render_frame(0.0, RenderConfig::default())
            .unwrap();
        cache_prepared_animation_in(&cache, &growing, replacement);
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 4
            }
        );
        drop(growing);
        assert_eq!(
            cache_usage(&budget).entries,
            1,
            "idle cleanup is lazy but remains bounded"
        );
        render_pixel_source(&cache, &other, 0.0);
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 4
            }
        );
        drop(cache);
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn prepared_cache_global_pressure_bypasses_retention_and_bounds_zero_image_entries() {
        let budget = Arc::new(PreparedCacheBudget::new(1, 4));
        let first_cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
        let second_cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
        let first = pixel_source(1, false);
        let second = pixel_source(1, false);
        render_pixel_source(&first_cache, &first, 0.0);
        // Valid frames are returned even when another thread/cache owns all quota.
        for _ in 0..2 {
            render_pixel_source(&second_cache, &second, 0.0);
        }
        assert_eq!(second_cache.borrow().entries.len(), 0);
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 4
            }
        );
        drop(first_cache);
        render_pixel_source(&second_cache, &second, 0.0);
        assert_eq!(cache_usage(&budget).entries, 1);
        drop(second_cache);
        let zero_cache = RefCell::new(PreparedAnimationCache::new(4, 0, budget.clone()));
        let other_zero_cache = RefCell::new(PreparedAnimationCache::new(4, 0, budget.clone()));
        let zero: Arc<[u8]> = Arc::from(SIMPLE_LOTTIE.as_bytes());
        let other_zero: Arc<[u8]> = Arc::from(SIMPLE_LOTTIE.as_bytes());
        with_prepared_animation_in(&zero_cache, &zero, |_| Ok(Some(()))).unwrap();
        with_prepared_animation_in(&other_zero_cache, &other_zero, |_| Ok(Some(()))).unwrap();
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 0
            }
        );
        assert_eq!(other_zero_cache.borrow().entries.len(), 0);
        drop(zero_cache);
        drop(other_zero_cache);
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn prepared_cache_checkout_error_cancel_panic_and_reentrancy_balance_quota() {
        let budget = Arc::new(PreparedCacheBudget::new(4, 8));
        let cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
        let source = pixel_source(1, false);
        render_pixel_source(&cache, &source, 0.0);
        let other = pixel_source(1, false);
        with_prepared_animation_in(&cache, &source, |prepared| {
            assert_eq!(
                cache_usage(&budget),
                PreparedCacheUsage::default(),
                "checkout releases idle quota"
            );
            assert!(cache.try_borrow_mut().is_ok());
            assert!(budget.usage.try_lock().is_some());
            assert_eq!(
                prepared.render_frame(0.0, RenderConfig::default())?.pixels,
                [255, 0, 0, 255]
            );
            render_pixel_source(&cache, &other, 0.0);
            Ok(Some(()))
        })
        .unwrap();
        assert_eq!(cache_usage(&budget).decoded_image_bytes, 8);
        let error = with_prepared_animation_in::<()>(&cache, &source, |_| {
            Err(anyhow::anyhow!("render error").into())
        });
        assert!(error.is_err());
        assert_eq!(cache_usage(&budget).decoded_image_bytes, 8);
        assert!(
            with_prepared_animation_in::<()>(&cache, &source, |_| Ok(None))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 1,
                decoded_image_bytes: 4
            }
        );
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_prepared_animation_in::<()>(&cache, &other, |_| {
                panic!("simulated raster panic")
            });
        }));
        assert!(panic.is_err());
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
        render_pixel_source(&cache, &source, 0.0);
        drop(cache);
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn prepared_cache_metadata_admission_checks_overflow_without_allocating_pixels() {
        let budget = Arc::new(PreparedCacheBudget::new(2, u64::MAX));
        let reservation = budget.reserve(u64::MAX).unwrap();
        assert!(budget.reserve(1).is_none());
        let empty = budget.reserve(0).unwrap();
        assert!(budget.reserve(0).is_none());
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 2,
                decoded_image_bytes: u64::MAX
            }
        );
        drop(reservation);
        drop(empty);
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn prepared_cache_shared_admission_is_race_safe_and_thread_exit_releases() {
        let budget = Arc::new(PreparedCacheBudget::new(2, 8));
        let start = Arc::new(std::sync::Barrier::new(9));
        let finish = Arc::new(std::sync::Barrier::new(9));
        let (tx, rx) = std::sync::mpsc::channel();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let budget = budget.clone();
                let start = start.clone();
                let finish = finish.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let cache = RefCell::new(PreparedAnimationCache::new(4, 8, budget.clone()));
                    let source = pixel_source(1, false);
                    start.wait();
                    render_pixel_source(&cache, &source, 0.0);
                    let usage = cache_usage(&budget);
                    assert!(usage.entries <= 2 && usage.decoded_image_bytes <= 8);
                    tx.send(cache.borrow().entries.len()).unwrap();
                    finish.wait();
                    // Drop the source and leave its cache idle until this thread exits.
                    drop(source);
                })
            })
            .collect();
        start.wait();
        assert_eq!((0..8).map(|_| rx.recv().unwrap()).sum::<usize>(), 2);
        assert_eq!(
            cache_usage(&budget),
            PreparedCacheUsage {
                entries: 2,
                decoded_image_bytes: 8
            }
        );
        finish.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(cache_usage(&budget), PreparedCacheUsage::default());
    }

    #[test]
    fn source_and_animation_summaries_do_not_leak_locations_or_bytes() {
        let uri = LottieSource::from("https://cdn.example.com/private/spinner.json");
        assert_eq!(uri.kind(), "uri");
        assert_eq!(
            uri.to_text(),
            "lottie source: kind uri, bytes none, decoded false"
        );
        assert!(!uri.to_text().contains("cdn.example.com"));

        let embedded = LottieSource::from(Resource::Embedded("private-spinner.json".into()));
        assert_eq!(embedded.kind(), "embedded");
        assert!(!embedded.to_text().contains("private-spinner"));

        let path = LottieSource::from(PathBuf::from("/tmp/secret-animation.json"));
        assert_eq!(path.kind(), "path");
        assert!(!path.to_text().contains("secret-animation"));

        let bytes = LottieSource::from(vec![1, 2, 3, 4]);
        assert_eq!(bytes.kind(), "bytes");
        assert!(bytes.has_bytes());
        assert_eq!(bytes.byte_len(), Some(4));
        assert_eq!(
            bytes.to_text(),
            "lottie source: kind bytes, bytes 4, decoded false"
        );

        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let decoded = LottieSource::from(animation.clone());
        assert_eq!(decoded.kind(), "animation");
        assert_eq!(
            decoded.to_text(),
            "lottie source: kind animation, bytes none, decoded true"
        );
        assert_eq!(
            animation.to_text(),
            "lottie animation: frames 30, fps 30.0, size 64x32, duration_ms 1000"
        );
    }

    #[test]
    fn player_summary_reports_state_without_animation_content() {
        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let mut player = LottiePlayer::new(animation);
        let start = Instant::now();

        assert_eq!(PlaybackState::Stopped.to_text(), "stopped");
        assert_eq!(LoopMode::PingPong.to_text(), "ping-pong");
        assert_eq!(
            player.to_text(),
            "lottie player: state stopped, loop once, current_frame 0, frames 30, animating false"
        );

        player.set_loop_mode(LoopMode::Loop);
        player.play_at(start);

        assert_eq!(
            player.to_text(),
            "lottie player: state playing, loop loop, current_frame 0, frames 30, animating true"
        );
        assert!(!player.to_text().contains("5.7.6"));
    }

    #[test]
    fn rejects_unbounded_sources_metadata_and_render_batches() {
        assert!(LottieAnimation::from_bytes(Vec::<u8>::new()).is_err());
        assert!(LottieAnimation::from_bytes(vec![b' '; MAX_LOTTIE_SOURCE_BYTES + 1]).is_err());

        for invalid in [
            SIMPLE_LOTTIE.replace("\"w\":64", "\"w\":0"),
            SIMPLE_LOTTIE.replace("\"fr\":30", "\"fr\":0"),
            SIMPLE_LOTTIE.replace("\"op\":30", "\"op\":100001"),
        ] {
            assert!(LottieAnimation::from_json_str(&invalid).is_err());
        }

        let animation = LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap();
        assert!(
            animation
                .render_batch(
                    size(DevicePixels(64), DevicePixels(32)),
                    &vec![0; MAX_LOTTIE_RENDER_BATCH + 1],
                )
                .is_err()
        );
        assert!(
            animation
                .render_batch(size(DevicePixels(-1), DevicePixels(32)), &[0])
                .is_err()
        );
    }

    #[test]
    fn playback_arithmetic_contains_extreme_elapsed_time() {
        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let mut player = LottiePlayer::new(animation);
        let now = Instant::now();
        player.elapsed_before_pause = Duration::MAX;
        player.state = PlaybackState::Paused;
        player.play_at(now);
        assert_eq!(player.started_at, Some(now));

        player.set_loop_mode(LoopMode::Loop);
        let upcoming = player.upcoming_frames(now, usize::MAX);
        assert!(!upcoming.is_empty());
        assert!(upcoming.len() <= player.animation.total_frames());
    }

    #[test]
    fn http_status_display_redacts_location_and_body() {
        let error = LottieError::BadStatus {
            uri: "https://example.com/private/account.json".into(),
            status: http_client::StatusCode::BAD_REQUEST,
            body: "private response body".into(),
        };
        let text = error.to_string();
        assert!(!text.contains("example.com"));
        assert!(!text.contains("private"));
        assert!(text.contains("400"));
    }
}

#[cfg(test)]
mod asset_payload_tests {
    use super::*;
    use crate::TestAppContext;
    use futures::FutureExt as _;

    const TINY: &[u8] = br#"{"fr":30,"ip":0,"op":2,"w":1,"h":1,"nm":"A","layers":[]}"#;

    #[test]
    fn lottie_loader_and_logger_charge_source_plus_native_poster() {
        let output = Ok(Arc::new(
            LottieAnimation::from_bytes(Arc::<[u8]>::from(TINY)).unwrap(),
        ));
        let expected = TINY.len() as u64 + 4;
        assert_eq!(LottieAssetLoader::cache_bytes(&output), expected);
        assert_eq!(LottieResourceLoader::cache_bytes(&output), expected);
    }

    #[test]
    fn completed_lottie_payload_obeys_exact_shared_cache_budget() {
        let mut cx = TestAppContext::single();
        let first = LottieAssetSource::Bytes(Arc::from(TINY));
        let mut next_bytes = TINY.to_vec();
        let offset = next_bytes.iter().position(|byte| *byte == b'A').unwrap();
        next_bytes[offset] = b'B';
        let next = LottieAssetSource::Bytes(next_bytes.into());
        let charge = TINY.len() as u64 + 4;
        cx.update(|cx| cx.set_completed_asset_cache_limits(8, charge));
        let first_task = cx.update(|cx| {
            cx.fetch_asset_checked::<LottieResourceLoader>(&first)
                .unwrap()
                .0
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 1);
            assert_eq!(cx.completed_asset_bytes(), charge);
        });
        let next_task = cx.update(|cx| {
            cx.fetch_asset_checked::<LottieResourceLoader>(&next)
                .unwrap()
                .0
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(cx.completed_asset_count(), 1);
            assert_eq!(cx.completed_asset_bytes(), charge);
            assert!(!cx.fetch_asset::<LottieResourceLoader>(&next).1);
            assert!(cx.fetch_asset::<LottieResourceLoader>(&first).1);
        });
        assert_eq!(
            first_task
                .now_or_never()
                .unwrap()
                .unwrap()
                .poster_frame()
                .decoded_bytes(),
            4
        );
        assert_eq!(
            next_task
                .now_or_never()
                .unwrap()
                .unwrap()
                .poster_frame()
                .decoded_bytes(),
            4
        );
    }
}
