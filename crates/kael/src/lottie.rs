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
    path::{Path, PathBuf},
    str,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

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
    #[error("unexpected http status for {uri}: {status}, body: {body}")]
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
        let animation = parse_animation(&bytes)?;
        let native_pixel_size = size(
            DevicePixels(animation.width.max(1) as i32),
            DevicePixels(animation.height.max(1) as i32),
        );
        let native_size = size(
            px(animation.width.max(1) as f32),
            px(animation.height.max(1) as f32),
        );
        let fps = animation.frame_rate.max(1.0);
        let total_frames = animation.duration_frames().ceil().max(1.0) as usize;
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
        let base_step = (self.elapsed_at(now).as_secs_f32() * self.animation.fps())
            .floor()
            .max(0.0) as usize;
        let mut frames = Vec::with_capacity(count.max(1));

        for offset in 0..count.max(1) {
            let frame = self.frame_for_step(base_step + offset);
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

        self.started_at = Some(now - self.elapsed_before_pause);
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
            self.started_at = Some(now - self.elapsed_before_pause);
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
                    Resource::Path(path) => fs::read(path.as_ref())?,
                    Resource::Uri(uri) => {
                        let mut response = client
                            .get(uri.as_ref(), ().into(), true)
                            .await
                            .with_context(|| format!("loading lottie asset from {uri:?}"))?;
                        let mut body = Vec::new();
                        response.body_mut().read_to_end(&mut body).await?;
                        if !response.status().is_success() {
                            let mut body_text = String::from_utf8_lossy(&body).into_owned();
                            let first_line = body_text.lines().next().unwrap_or("").trim_end();
                            body_text.truncate(first_line.len());
                            return Err(LottieError::BadStatus {
                                uri,
                                status: response.status(),
                                body: body_text,
                            });
                        }
                        body
                    }
                    Resource::Embedded(path) => {
                        let data = asset_source.load(&path).ok().flatten();
                        if let Some(data) = data {
                            data.to_vec()
                        } else {
                            return Err(LottieError::Asset(
                                format!("Embedded resource not found: {path}").into(),
                            ));
                        }
                    }
                },
                LottieAssetSource::Bytes(bytes) => bytes.as_ref().to_vec(),
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
}
