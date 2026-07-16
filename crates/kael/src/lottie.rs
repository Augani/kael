use crate::{
    App, Asset, AssetLogger, DevicePixels, Pixels, RenderImage, Resource, SharedString, SharedUri,
    Size, px, size, util::is_uri,
};
use anyhow::Context as _;
use futures::AsyncReadExt;
use image::{Frame, ImageBuffer, Rgba};
use rasterlottie::{RenderConfig, Renderer, Rgba8};
use smallvec::SmallVec;
use std::{
    fs, io,
    io::Read as _,
    path::{Path, PathBuf},
    str,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

const MAX_LOTTIE_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOTTIE_DIMENSION: u32 = 8_192;
const MAX_LOTTIE_FRAME_BYTES: usize = 256 * 1024 * 1024;
const MAX_LOTTIE_FRAMES: usize = 100_000;
const MAX_LOTTIE_FPS: f32 = 1_000.0;
const MAX_LOTTIE_RENDER_BATCH: usize = 256;

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

    pub(crate) fn render_batch(
        &self,
        render_size: Size<DevicePixels>,
        frames: &[usize],
    ) -> Result<LottieRenderBatch, LottieError> {
        validate_render_size(render_size)?;
        if frames.len() > MAX_LOTTIE_RENDER_BATCH {
            return Err(anyhow::anyhow!(
                "Lottie render batch cannot exceed {MAX_LOTTIE_RENDER_BATCH} frames"
            )
            .into());
        }
        let animation = parse_animation(&self.data.bytes)?;
        let prepared = Renderer::target_corpus().prepare(&animation)?;
        let scale = render_scale(self.data.native_pixel_size, render_size);
        let config = RenderConfig::new(Rgba8::TRANSPARENT, scale);
        let mut rendered_frames = Vec::with_capacity(frames.len());

        for &frame_index in frames {
            let frame = prepared.render_frame(self.timeline_frame(frame_index), config)?;
            rendered_frames.push(LottieRenderedFrame {
                frame_index,
                image: raster_frame_to_image(frame)?,
            });
        }

        Ok(LottieRenderBatch {
            render_size,
            frames: rendered_frames,
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
        let prepared = Renderer::target_corpus().prepare(&animation)?;
        let poster_frame =
            raster_frame_to_image(prepared.render_frame(in_point, RenderConfig::default())?)?;

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
