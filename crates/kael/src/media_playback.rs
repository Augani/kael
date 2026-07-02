//! Audio playback APIs for GPUI applications.

use crate::{
    App, Asset, AssetLogger, Bounds, DefiniteLength, Element, GlobalElementId, ImageCacheError,
    InspectorElementId, IntoElement, LayoutId, Length, MediaKeyEvent, ObjectFit, Pixels,
    RenderImage, SharedString, Style, StyleRefinement, Styled, Window, px,
};
use anyhow::{Context as _, anyhow};
use futures::Future;
use image::{Delay, Frame, ImageBuffer, Rgba};
use refineable::Refineable;
use smallvec::SmallVec;
use std::{
    cell::RefCell,
    collections::VecDeque,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    io::{Read, Seek},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use util::ResultExt;

use kael_media::VideoFrameStream;
pub use kael_media::{
    AudioHandle, AudioPlaybackError, MediaDecodeError, MediaDecoder, MediaSource,
    PlaybackState as MediaPlaybackState, VideoFrame, VideoMetadata,
};

const DEFAULT_VIDEO_FRAME_DELAY: Duration = Duration::from_millis(33);
const MIN_VIDEO_FRAME_CACHE_LIMIT: usize = 16;
const MAX_VIDEO_FRAME_CACHE_LIMIT: usize = 64;
const MIN_VIDEO_FRAME_PREFETCH: usize = 6;
const MAX_VIDEO_FRAME_PREFETCH: usize = 32;
const MIN_VIDEO_FRAME_RETAIN: usize = 4;
const MAX_VIDEO_FRAME_RETAIN: usize = 16;

type VideoResourceLoader = AssetLogger<VideoAssetLoader>;

#[derive(Clone, Debug)]
struct BufferedVideoAsset {
    metadata: VideoMetadata,
    duration: Duration,
}

impl BufferedVideoAsset {
    fn width(&self) -> Pixels {
        px(self.metadata.width as f32)
    }

    fn height(&self) -> Pixels {
        px(self.metadata.height as f32)
    }
}

#[derive(Clone)]
enum VideoAssetLoader {}

impl Asset for VideoAssetLoader {
    type Source = MediaSource;
    type Output = Result<Arc<BufferedVideoAsset>, ImageCacheError>;

    fn load(
        source: Self::Source,
        _cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        async move {
            smol::unblock(move || load_video_asset(source))
                .await
                .map_err(|error| ImageCacheError::from(anyhow!(error.to_string())))
        }
    }
}

#[derive(Default)]
struct VideoState {
    autoplay_started: bool,
    buffered_video: Option<BufferedVideoPlayback>,
    internal_audio: Option<AudioHandle>,
    use_local_clock: bool,
    local_position: Duration,
    local_started_at: Option<Instant>,
    local_state: MediaPlaybackState,
}

struct CachedVideoFrame {
    timestamp: Duration,
    image: Arc<RenderImage>,
}

struct BufferedVideoPlayback {
    decoder: VideoFrameStream,
    frames: VecDeque<CachedVideoFrame>,
    exhausted: bool,
    last_requested_position: Option<Duration>,
    last_seek_position: Option<Duration>,
    buffer_strategy: VideoBufferStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VideoBufferStrategy {
    backward_window: usize,
    forward_window: usize,
    cache_limit: usize,
}

/// Readiness level for a video controller, modeled after the browser media
/// element states without requiring Kael to expose a DOM.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum VideoReadyState {
    /// No metadata has been loaded yet.
    #[default]
    Nothing,
    /// Metadata such as dimensions and duration is available.
    Metadata,
    /// Enough data is available to display the current frame.
    CurrentData,
    /// Enough data is available to continue playback for a short time.
    FutureData,
    /// The controller expects playback can continue without buffering.
    EnoughData,
}

/// A buffered or seekable time range for media playback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimeRange {
    /// Start time, inclusive.
    pub start: Duration,
    /// End time, exclusive.
    pub end: Duration,
}

impl TimeRange {
    /// Create a normalized time range.
    pub fn new(start: Duration, end: Duration) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    /// Return the range duration.
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Return whether `position` is inside this range.
    pub fn contains(&self, position: Duration) -> bool {
        position >= self.start && position < self.end
    }
}

/// Recommended rendering route for a media source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoPlaybackRoute {
    /// Use Kael's native media controller and `video(source)` element.
    Native,
    /// Prefer a WebView island because the source needs browser media features
    /// that the current native backend does not expose yet.
    WebViewRecommended {
        /// Human-readable reason for the recommendation.
        reason: SharedString,
    },
}

impl VideoPlaybackRoute {
    /// Return whether the native Kael media path is recommended.
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native)
    }

    /// Return whether a WebView island is recommended.
    pub fn should_use_webview(&self) -> bool {
        matches!(self, Self::WebViewRecommended { .. })
    }
}

/// Browser-style media support confidence for Kael's native video path.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum VideoCanPlay {
    /// The current native path should not be used for this type/source.
    #[default]
    No,
    /// The current native path may be able to decode it, but support is not
    /// strong enough to treat as a first-choice production path.
    Maybe,
    /// The source/type maps to the native media path Kael currently targets.
    Probably,
}

impl VideoCanPlay {
    /// Return the browser-style string used by HTMLMediaElement.canPlayType.
    pub fn as_can_play_type(&self) -> &'static str {
        match self {
            Self::No => "",
            Self::Maybe => "maybe",
            Self::Probably => "probably",
        }
    }
}

/// Capability maturity for a video feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoCapabilityStatus {
    /// The public API and current backend cover the capability.
    Full,
    /// The capability is usable with documented limits.
    Partial,
    /// The capability is intentionally not implemented in the native path yet.
    Roadmap,
}

/// Current native/WebView media capability report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCapabilityReport {
    /// URL, file, bytes, and reader source constructors.
    pub source_types: VideoCapabilityStatus,
    /// Browser-shaped controller commands, events, and snapshots.
    pub controller: VideoCapabilityStatus,
    /// Runtime source replacement such as `video.src = next_url`.
    pub source_replacement: VideoCapabilityStatus,
    /// `canPlayType`-style confidence helpers.
    pub can_play_type: VideoCapabilityStatus,
    /// Native-vs-WebView route recommendation helpers.
    pub route_recommendation: VideoCapabilityStatus,
    /// WebView-hosted browser video fallback helper.
    pub webview_fallback: VideoCapabilityStatus,
    /// SRT/WebVTT text-track parsing, selection, and caption rendering.
    pub text_tracks: VideoCapabilityStatus,
    /// Timeline scrubbing and low-latency seek API shape.
    pub fast_seek: VideoCapabilityStatus,
    /// Playback-rate control in the current software audio path.
    pub playback_rate: VideoCapabilityStatus,
    /// Native fullscreen integration for the high-level player.
    pub fullscreen: VideoCapabilityStatus,
    /// Native HLS/DASH streaming playback.
    pub native_adaptive_streaming: VideoCapabilityStatus,
    /// Platform hardware decode and low-copy/zero-copy surfaces.
    pub hardware_decode: VideoCapabilityStatus,
    /// Native audio/video stream selection.
    pub native_track_selection: VideoCapabilityStatus,
}

impl VideoCapabilityReport {
    /// Return whether every field is fully implemented.
    pub fn is_full(&self) -> bool {
        self.statuses()
            .into_iter()
            .all(|status| status == VideoCapabilityStatus::Full)
    }

    /// Return the report statuses as a compact list for dashboards/tests.
    pub fn statuses(&self) -> [VideoCapabilityStatus; 13] {
        [
            self.source_types,
            self.controller,
            self.source_replacement,
            self.can_play_type,
            self.route_recommendation,
            self.webview_fallback,
            self.text_tracks,
            self.fast_seek,
            self.playback_rate,
            self.fullscreen,
            self.native_adaptive_streaming,
            self.hardware_decode,
            self.native_track_selection,
        ]
    }
}

/// Return Kael's current video capability report.
pub fn video_capability_report() -> VideoCapabilityReport {
    VideoCapabilityReport {
        source_types: VideoCapabilityStatus::Full,
        controller: VideoCapabilityStatus::Full,
        source_replacement: VideoCapabilityStatus::Full,
        can_play_type: VideoCapabilityStatus::Full,
        route_recommendation: VideoCapabilityStatus::Full,
        webview_fallback: VideoCapabilityStatus::Full,
        text_tracks: VideoCapabilityStatus::Full,
        fast_seek: VideoCapabilityStatus::Partial,
        playback_rate: VideoCapabilityStatus::Partial,
        fullscreen: VideoCapabilityStatus::Full,
        native_adaptive_streaming: VideoCapabilityStatus::Roadmap,
        hardware_decode: VideoCapabilityStatus::Roadmap,
        native_track_selection: VideoCapabilityStatus::Roadmap,
    }
}

/// Browser preload mode for a WebView-hosted video fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewVideoPreload {
    /// Ask the browser not to eagerly preload media.
    None,
    /// Ask the browser to load metadata only.
    Metadata,
    /// Let the browser preload according to its default media policy.
    Auto,
}

impl WebViewVideoPreload {
    fn as_html_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Metadata => "metadata",
            Self::Auto => "auto",
        }
    }
}

/// CORS mode for a WebView-hosted video fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebViewVideoCrossOrigin {
    /// Use anonymous CORS.
    Anonymous,
    /// Include credentials for CORS requests.
    UseCredentials,
}

impl WebViewVideoCrossOrigin {
    fn as_html_value(self) -> &'static str {
        match self {
            Self::Anonymous => "anonymous",
            Self::UseCredentials => "use-credentials",
        }
    }
}

/// A browser text track for a WebView-hosted video fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebViewVideoTextTrack {
    /// HTML track kind such as `subtitles`, `captions`, or `chapters`.
    pub kind: SharedString,
    /// User-facing label.
    pub label: SharedString,
    /// BCP-47-ish language tag when known.
    pub language: Option<SharedString>,
    /// Browser-readable track source URL.
    pub src: SharedString,
    /// Whether this track should be the browser default.
    pub default: bool,
}

impl WebViewVideoTextTrack {
    /// Create a WebVTT track from a URL or data URL.
    pub fn webvtt(
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        src: impl Into<SharedString>,
    ) -> Self {
        Self {
            kind: "subtitles".into(),
            label: label.into(),
            language: language.map(Into::into),
            src: src.into(),
            default: false,
        }
    }

    /// Create an inline WebVTT track from source text.
    pub fn inline_webvtt(
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        Self::webvtt(
            label,
            language,
            format!(
                "data:text/vtt;charset=utf-8,{}",
                percent_encode_data_url(input)
            ),
        )
    }

    /// Set the HTML track kind.
    pub fn kind(mut self, kind: impl Into<SharedString>) -> Self {
        self.kind = kind.into();
        self
    }

    /// Mark this track as the browser default.
    pub fn default(mut self, default: bool) -> Self {
        self.default = default;
        self
    }
}

/// Browser-video commands for a WebView-hosted video fallback.
#[derive(Clone, Debug, PartialEq)]
pub enum WebViewVideoCommand {
    /// Call `video.play()`.
    Play,
    /// Call `video.pause()`.
    Pause,
    /// Toggle between `play()` and `pause()`.
    TogglePlay,
    /// Pause and reset `currentTime` to zero.
    Stop,
    /// Set `currentTime`.
    Seek(Duration),
    /// Set `currentTime` with the browser's fast seek path when available.
    FastSeek(Duration),
    /// Set `volume`, clamped to `0.0..=1.0`.
    SetVolume(f32),
    /// Set `muted`.
    SetMuted(bool),
    /// Set `playbackRate`.
    SetPlaybackRate(f32),
    /// Set `loop`.
    SetLooping(bool),
    /// Show the text track matching an id, label, language, or zero-based index.
    SelectTextTrack(SharedString),
    /// Disable all browser text tracks.
    DisableTextTracks,
    /// Request browser fullscreen for the fallback video element.
    RequestFullscreen,
    /// Exit browser fullscreen when supported by the page.
    ExitFullscreen,
    /// Request picture-in-picture for the fallback video element.
    RequestPictureInPicture,
    /// Exit picture-in-picture when supported by the page.
    ExitPictureInPicture,
    /// Post a snapshot event through the WebView bridge.
    RequestSnapshot,
}

/// Options for building a WebView-hosted browser video fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebViewVideoOptions {
    /// Render native browser controls.
    pub controls: bool,
    /// Request autoplay from the browser.
    pub autoplay: bool,
    /// Start muted.
    pub muted: bool,
    /// Loop playback.
    pub looping: bool,
    /// Add `playsinline` for platforms that distinguish inline/fullscreen video.
    pub plays_inline: bool,
    /// Poster image URL shown before playback.
    pub poster: Option<SharedString>,
    /// Browser preload mode.
    pub preload: Option<WebViewVideoPreload>,
    /// CORS mode for media requests.
    pub cross_origin: Option<WebViewVideoCrossOrigin>,
    /// Space-separated browser controls-list tokens.
    pub controls_list: Vec<SharedString>,
    /// Disable browser picture-in-picture affordances when supported.
    pub disable_picture_in_picture: bool,
    /// Initial playback position requested after metadata loads.
    pub start_position: Option<Duration>,
    /// Browser text tracks rendered inside the fallback page.
    pub text_tracks: Vec<WebViewVideoTextTrack>,
    /// CSS `object-fit` value for the embedded browser video element.
    pub object_fit: SharedString,
}

impl Default for WebViewVideoOptions {
    fn default() -> Self {
        Self {
            controls: true,
            autoplay: false,
            muted: false,
            looping: false,
            plays_inline: true,
            poster: None,
            preload: None,
            cross_origin: None,
            controls_list: Vec::new(),
            disable_picture_in_picture: false,
            start_position: None,
            text_tracks: Vec::new(),
            object_fit: "contain".into(),
        }
    }
}

impl WebViewVideoOptions {
    /// Set whether browser controls are visible.
    pub fn controls(mut self, controls: bool) -> Self {
        self.controls = controls;
        self
    }

    /// Set whether autoplay is requested.
    pub fn autoplay(mut self, autoplay: bool) -> Self {
        self.autoplay = autoplay;
        self
    }

    /// Set whether playback starts muted.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Set whether playback loops.
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Set whether `playsinline` is present.
    pub fn plays_inline(mut self, plays_inline: bool) -> Self {
        self.plays_inline = plays_inline;
        self
    }

    /// Set the poster image URL.
    pub fn poster(mut self, poster: impl Into<SharedString>) -> Self {
        self.poster = Some(poster.into());
        self
    }

    /// Set the browser preload mode.
    pub fn preload(mut self, preload: WebViewVideoPreload) -> Self {
        self.preload = Some(preload);
        self
    }

    /// Set the CORS mode for media requests.
    pub fn cross_origin(mut self, cross_origin: WebViewVideoCrossOrigin) -> Self {
        self.cross_origin = Some(cross_origin);
        self
    }

    /// Replace the browser controls-list tokens.
    pub fn controls_list<I, S>(mut self, controls_list: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SharedString>,
    {
        self.controls_list = controls_list.into_iter().map(Into::into).collect();
        self
    }

    /// Append one browser controls-list token.
    pub fn controls_list_item(mut self, item: impl Into<SharedString>) -> Self {
        self.controls_list.push(item.into());
        self
    }

    /// Set whether browser picture-in-picture affordances should be disabled.
    pub fn disable_picture_in_picture(mut self, disabled: bool) -> Self {
        self.disable_picture_in_picture = disabled;
        self
    }

    /// Request an initial playback position after metadata loads.
    pub fn start_at(mut self, position: Duration) -> Self {
        self.start_position = Some(position);
        self
    }

    /// Add a browser text track to the fallback page.
    pub fn text_track(mut self, track: WebViewVideoTextTrack) -> Self {
        self.text_tracks.push(track);
        self
    }

    /// Add an inline WebVTT text track to the fallback page.
    pub fn webvtt_text_track(
        self,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        self.text_track(WebViewVideoTextTrack::inline_webvtt(label, language, input))
    }

    /// Set the CSS `object-fit` value.
    pub fn object_fit(mut self, object_fit: impl Into<SharedString>) -> Self {
        self.object_fit = object_fit.into();
        self
    }

    /// Validate browser-video fallback options before embedding them.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_optional_media_url(
            self.poster.as_ref().map(|poster| poster.as_ref()),
            "video poster URL",
        )?;
        validate_webview_video_start_position(self.start_position)?;
        validate_webview_video_object_fit(&self.object_fit)?;

        for token in &self.controls_list {
            validate_html_token(token, "video controls-list token")?;
        }
        for track in &self.text_tracks {
            track.validate()?;
        }

        Ok(())
    }

    /// Return a validated clone of these options.
    pub fn checked(&self) -> anyhow::Result<Self> {
        self.validate()?;
        Ok(self.clone())
    }
}

impl WebViewVideoTextTrack {
    /// Validate a browser text track before embedding it in a WebView fallback.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_html_token(&self.kind, "video text track kind")?;
        validate_non_empty_trimmed(&self.label, "video text track label")?;
        if let Some(language) = &self.language {
            validate_non_empty_trimmed(language, "video text track language")?;
        }
        validate_media_url(&self.src, "video text track source URL")?;
        Ok(())
    }
}

/// Builder for checked media sources used by generated video/audio code.
#[derive(Clone)]
pub struct MediaSourceBuilder {
    source: MediaSource,
    require_existing_file: bool,
    canonicalize_file: bool,
}

impl MediaSourceBuilder {
    /// Create a checked URL-backed media source builder.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::new(MediaSource::url(url))
    }

    /// Create a checked local file media source builder.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(MediaSource::file(path))
    }

    /// Create a checked in-memory media source builder.
    pub fn bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::new(MediaSource::bytes(bytes))
    }

    /// Create a checked keyed reader media source builder.
    pub fn reader<R>(
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
    ) -> Self
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        Self::new(MediaSource::reader(key, open))
    }

    /// Wrap an existing media source in the checked builder.
    pub fn new(source: impl Into<MediaSource>) -> Self {
        Self {
            source: source.into(),
            require_existing_file: false,
            canonicalize_file: false,
        }
    }

    /// Require file-backed sources to exist and be files before building.
    pub fn require_existing_file(mut self) -> Self {
        self.require_existing_file = true;
        self
    }

    /// Canonicalize file-backed sources before building.
    pub fn canonicalize_file(mut self) -> Self {
        self.canonicalize_file = true;
        self
    }

    /// Validate the configured source without consuming the builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_media_source(
            &self.source,
            self.require_existing_file,
            self.canonicalize_file,
        )
    }

    /// Build a validated media source.
    pub fn build_checked(self) -> anyhow::Result<MediaSource> {
        let Self {
            mut source,
            require_existing_file,
            canonicalize_file,
        } = self;
        validate_media_source(&source, require_existing_file, canonicalize_file)?;
        if canonicalize_file && let MediaSource::File(path) = source {
            source = MediaSource::file(path.canonicalize().with_context(|| {
                format!("could not canonicalize media file {}", path.display())
            })?);
        }
        Ok(source)
    }

    /// Build a validated controller for this media source.
    pub fn controller_checked(self) -> anyhow::Result<VideoController> {
        Ok(VideoController::new(self.build_checked()?))
    }
}

impl From<MediaSource> for MediaSourceBuilder {
    fn from(source: MediaSource) -> Self {
        Self::new(source)
    }
}

/// Planned rendering target for a checked video source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoPlaybackPlanTarget {
    /// Render with Kael's native `video(source)` element and [`VideoController`].
    Native,
    /// Render through a WebView-hosted browser `<video>` fallback.
    WebViewFallback {
        /// Data URL containing the fallback browser video page.
        page_url: SharedString,
        /// Stable WebView element id for this source.
        element_id: SharedString,
        /// Human-readable route recommendation reason.
        reason: SharedString,
    },
}

impl VideoPlaybackPlanTarget {
    /// Return whether the plan uses Kael's native video path.
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native)
    }

    /// Return whether the plan uses the browser-video WebView fallback.
    pub fn is_webview_fallback(&self) -> bool {
        matches!(self, Self::WebViewFallback { .. })
    }
}

/// Checked plan for building an Electron-style video player from one source.
#[derive(Clone)]
pub struct VideoPlaybackPlan {
    source: MediaSource,
    content_type: Option<SharedString>,
    route: VideoPlaybackRoute,
    can_play: VideoCanPlay,
    webview_options: WebViewVideoOptions,
    target: VideoPlaybackPlanTarget,
}

impl VideoPlaybackPlan {
    /// The validated media source.
    pub fn source(&self) -> MediaSource {
        self.source.clone()
    }

    /// Optional MIME/content type used to refine route selection.
    pub fn content_type(&self) -> Option<&str> {
        self.content_type
            .as_ref()
            .map(|content_type| content_type.as_ref())
    }

    /// Browser-style native playback confidence for the source/content type.
    pub fn can_play(&self) -> VideoCanPlay {
        self.can_play
    }

    /// Recommended native-vs-WebView route.
    pub fn route(&self) -> &VideoPlaybackRoute {
        &self.route
    }

    /// Checked browser fallback options.
    pub fn webview_options(&self) -> &WebViewVideoOptions {
        &self.webview_options
    }

    /// Planned rendering target.
    pub fn target(&self) -> &VideoPlaybackPlanTarget {
        &self.target
    }

    /// Create a controller for the validated source.
    pub fn controller(&self) -> VideoController {
        VideoController::new(self.source.clone())
    }

    /// Return the WebView fallback page URL when this plan targets WebView.
    pub fn webview_page_url(&self) -> Option<&str> {
        match &self.target {
            VideoPlaybackPlanTarget::WebViewFallback { page_url, .. } => Some(page_url.as_ref()),
            VideoPlaybackPlanTarget::Native => None,
        }
    }

    /// Return the WebView fallback element id when this plan targets WebView.
    pub fn webview_element_id(&self) -> Option<&str> {
        match &self.target {
            VideoPlaybackPlanTarget::WebViewFallback { element_id, .. } => {
                Some(element_id.as_ref())
            }
            VideoPlaybackPlanTarget::Native => None,
        }
    }
}

/// Builder for checked video playback plans.
#[derive(Clone)]
pub struct VideoPlaybackPlanBuilder {
    source: MediaSourceBuilder,
    content_type: Option<SharedString>,
    webview_options: WebViewVideoOptions,
    prefer_webview: bool,
}

impl VideoPlaybackPlanBuilder {
    /// Create a video playback plan builder from a media source builder.
    pub fn new(source: impl Into<MediaSourceBuilder>) -> Self {
        Self {
            source: source.into(),
            content_type: None,
            webview_options: WebViewVideoOptions::default(),
            prefer_webview: false,
        }
    }

    /// Create a checked URL-backed video playback plan builder.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::new(MediaSourceBuilder::url(url))
    }

    /// Create a checked local-file video playback plan builder.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(MediaSourceBuilder::file(path))
    }

    /// Create a checked in-memory video playback plan builder.
    pub fn bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::new(MediaSourceBuilder::bytes(bytes))
    }

    /// Create a checked keyed-reader video playback plan builder.
    pub fn reader<R>(
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
    ) -> Self
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        Self::new(MediaSourceBuilder::reader(key, open))
    }

    /// Require file-backed sources to exist and be files before building.
    pub fn require_existing_file(mut self) -> Self {
        self.source = self.source.require_existing_file();
        self
    }

    /// Canonicalize file-backed sources before building.
    pub fn canonicalize_file(mut self) -> Self {
        self.source = self.source.canonicalize_file();
        self
    }

    /// Provide a MIME/content type for extensionless URLs or server-driven media.
    pub fn content_type(mut self, content_type: impl Into<SharedString>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Prefer the browser-video WebView fallback when the source can be wrapped.
    pub fn prefer_webview(mut self) -> Self {
        self.prefer_webview = true;
        self
    }

    /// Configure browser-video fallback options.
    pub fn webview_options(mut self, options: WebViewVideoOptions) -> Self {
        self.webview_options = options;
        self
    }

    /// Validate the plan without consuming the builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.source.validate()?;
        if let Some(content_type) = &self.content_type {
            validate_media_content_type(content_type)?;
        }
        self.webview_options.validate()
    }

    /// Build a checked playback plan.
    pub fn build_checked(self) -> anyhow::Result<VideoPlaybackPlan> {
        self.validate()?;
        let source = self.source.build_checked()?;
        let content_type = self.content_type;
        let type_route = content_type
            .as_ref()
            .map(|content_type| recommended_video_playback_route_for_type(content_type));
        let route = type_route
            .clone()
            .filter(VideoPlaybackRoute::should_use_webview)
            .unwrap_or_else(|| recommended_video_playback_route(&source));
        let can_play = content_type
            .as_ref()
            .map(|content_type| can_play_video_type(content_type))
            .unwrap_or_else(|| can_play_video_source(&source));
        let webview_options = self.webview_options.checked()?;

        let target = if self.prefer_webview || route.should_use_webview() {
            let page_url = webview_video_player_url(&source, &webview_options).ok_or_else(|| {
                anyhow!(
                    "video source cannot be wrapped for WebView fallback; use a URL or file source"
                )
            })?;
            let reason = match &route {
                VideoPlaybackRoute::WebViewRecommended { reason } => reason.clone(),
                VideoPlaybackRoute::Native => {
                    "WebView fallback was explicitly requested for this video".into()
                }
            };
            VideoPlaybackPlanTarget::WebViewFallback {
                page_url,
                element_id: webview_video_player_id(&source),
                reason,
            }
        } else {
            VideoPlaybackPlanTarget::Native
        };

        Ok(VideoPlaybackPlan {
            source,
            content_type,
            route,
            can_play,
            webview_options,
            target,
        })
    }
}

/// A snapshot of a video's current state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoSnapshot {
    /// Current playback state.
    pub playback_state: MediaPlaybackState,
    /// Current playback position.
    pub current_time: Duration,
    /// Media duration when known.
    pub duration: Option<Duration>,
    /// Video metadata when known.
    pub metadata: Option<VideoMetadata>,
    /// Controller readiness.
    pub ready_state: VideoReadyState,
    /// Known buffered ranges. Local file, bytes, and reader sources report the
    /// full duration after metadata loads; URL-backed streaming ranges remain
    /// empty until a backend can report them accurately.
    pub buffered_ranges: Vec<TimeRange>,
    /// Current volume where `1.0` is original amplitude.
    pub volume: f32,
    /// Whether audio output is muted.
    pub muted: bool,
    /// Requested playback rate. Non-`1.0` values are applied to the current
    /// software audio backend; pitch-preserving time stretching is not yet
    /// implemented.
    pub playback_rate: f32,
    /// Whether playback should restart after reaching the end.
    pub looping: bool,
    /// Active text cues for the selected text track at [`Self::current_time`].
    pub active_text_cues: Vec<TextTrackCue>,
    /// Last controller error, if any.
    pub error: Option<String>,
}

/// A video text track kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextTrackKind {
    /// Subtitles translate or transcribe dialog for viewers who can hear audio.
    #[default]
    Subtitles,
    /// Captions include dialog plus sound cues for viewers who may not hear audio.
    Captions,
    /// Chapter markers.
    Chapters,
    /// Timed metadata.
    Metadata,
}

/// A timed text cue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextTrackCue {
    /// Start time, inclusive.
    pub start: Duration,
    /// End time, exclusive.
    pub end: Duration,
    /// Cue text. Multi-line cues preserve embedded newlines.
    pub text: SharedString,
}

impl TextTrackCue {
    /// Create a cue from a time range and text.
    pub fn new(start: Duration, end: Duration, text: impl Into<SharedString>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
        }
    }

    /// Return whether this cue is active at `position`.
    pub fn is_active_at(&self, position: Duration) -> bool {
        position >= self.start && position < self.end
    }
}

/// A named set of timed text cues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextTrack {
    /// Stable track identifier.
    pub id: SharedString,
    /// User-facing label.
    pub label: SharedString,
    /// BCP-47-ish language tag when known.
    pub language: Option<SharedString>,
    /// Track kind.
    pub kind: TextTrackKind,
    cues: Vec<TextTrackCue>,
}

impl TextTrack {
    /// Create a text track from already parsed cues.
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        kind: TextTrackKind,
        mut cues: Vec<TextTrackCue>,
    ) -> Self {
        cues.sort_by_key(|cue| (cue.start, cue.end));
        Self {
            id: id.into(),
            label: label.into(),
            language: language.map(Into::into),
            kind,
            cues,
        }
    }

    /// Parse a SubRip document into a subtitle text track.
    pub fn from_srt(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            language,
            TextTrackKind::Subtitles,
            parse_srt_cues(input),
        )
    }

    /// Parse a WebVTT document into a subtitle text track.
    pub fn from_webvtt(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        Self::new(
            id,
            label,
            language,
            TextTrackKind::Subtitles,
            parse_webvtt_cues(input),
        )
    }

    /// Return cues sorted by start time.
    pub fn cues(&self) -> &[TextTrackCue] {
        &self.cues
    }

    /// Return all cues active at `position`.
    pub fn active_cues(&self, position: Duration) -> Vec<TextTrackCue> {
        self.cues
            .iter()
            .filter(|cue| cue.is_active_at(position))
            .cloned()
            .collect()
    }
}

/// Events emitted by [`VideoController`].
#[derive(Clone, Debug, PartialEq)]
pub enum VideoEvent {
    /// The controller source changed and media state was reset.
    SourceChanged {
        /// New media source.
        source: MediaSource,
    },
    /// Video metadata was loaded.
    LoadedMetadata {
        /// Duration reported by the media when available.
        duration: Option<Duration>,
        /// Native video width in pixels.
        width: u32,
        /// Native video height in pixels.
        height: u32,
    },
    /// The controller readiness changed.
    ReadyStateChange {
        /// New readiness value.
        ready_state: VideoReadyState,
    },
    /// Buffered ranges changed.
    Progress {
        /// Current buffered ranges.
        buffered_ranges: Vec<TimeRange>,
    },
    /// Enough current media data is available to start playback.
    CanPlay,
    /// The controller expects playback can continue without buffering.
    CanPlayThrough,
    /// Playback is waiting for additional media data.
    Waiting,
    /// Playback started or resumed.
    Playing,
    /// Playback paused.
    Paused,
    /// Playback stopped and returned to the beginning.
    Stopped,
    /// Playback position changed.
    Seeked {
        /// New playback position.
        current_time: Duration,
    },
    /// Current playback time changed.
    TimeUpdate {
        /// Current playback position.
        current_time: Duration,
    },
    /// Volume or muted state changed.
    VolumeChange {
        /// Effective volume where `1.0` is original amplitude.
        volume: f32,
        /// Whether output is muted.
        muted: bool,
    },
    /// Playback rate changed.
    RateChange {
        /// Requested playback rate.
        playback_rate: f32,
    },
    /// Looping changed.
    LoopChange {
        /// Whether playback should restart at the end.
        looping: bool,
    },
    /// Browser fullscreen state changed for a WebView-routed video fallback.
    FullscreenChange {
        /// Whether the fallback video currently owns browser fullscreen.
        fullscreen: bool,
    },
    /// Picture-in-picture state changed for a WebView-routed video fallback.
    PictureInPictureChange {
        /// Whether the fallback video is currently in picture-in-picture.
        picture_in_picture: bool,
    },
    /// Playback reached the end.
    Ended,
    /// A text track was added.
    TextTrackAdded {
        /// Added track identifier.
        id: SharedString,
    },
    /// Selected text track changed. `None` disables text cues.
    TextTrackChanged {
        /// Selected track identifier.
        id: Option<SharedString>,
    },
    /// Active text cues changed for the selected track.
    CueChange {
        /// Active cues.
        cues: Vec<TextTrackCue>,
    },
    /// An operation failed.
    Error(String),
}

/// Errors from high-level video control operations.
#[derive(Debug, Error)]
pub enum VideoPlaybackError {
    /// Video metadata or frames could not be decoded.
    #[error(transparent)]
    Decode(#[from] MediaDecodeError),
    /// Audio playback failed.
    #[error(transparent)]
    Audio(#[from] AudioPlaybackError),
}

#[derive(Debug)]
struct VideoControllerState {
    source: MediaSource,
    audio: AudioHandle,
    metadata: Option<VideoMetadata>,
    duration: Option<Duration>,
    ready_state: VideoReadyState,
    volume: f32,
    muted: bool,
    playback_rate: f32,
    looping: bool,
    buffered_ranges: Vec<TimeRange>,
    emitted_can_play: bool,
    emitted_can_play_through: bool,
    text_tracks: Vec<TextTrack>,
    selected_text_track: Option<usize>,
    last_active_text_cues: Vec<TextTrackCue>,
    error: Option<String>,
    last_event_position: Duration,
    events: VecDeque<VideoEvent>,
}

/// A browser-style high-level controller for one video source.
///
/// This is the public control surface that `kael_ui::VideoPlayer::source(...)`
/// can build on: URL/file/bytes in, playback commands out, and snapshots/events
/// for custom controls. Rendering still uses the existing `video(source)` element
/// and software decode path until the hardware-surface backends land.
#[derive(Clone, Debug)]
pub struct VideoController {
    state: Rc<RefCell<VideoControllerState>>,
}

impl VideoController {
    /// Create a controller for a video source.
    pub fn new(source: impl Into<MediaSource>) -> Self {
        let source = source.into();
        Self {
            state: Rc::new(RefCell::new(VideoControllerState {
                audio: AudioHandle::new(source.clone()),
                source,
                metadata: None,
                duration: None,
                ready_state: VideoReadyState::Nothing,
                volume: 1.0,
                muted: false,
                playback_rate: 1.0,
                looping: false,
                buffered_ranges: Vec::new(),
                emitted_can_play: false,
                emitted_can_play_through: false,
                text_tracks: Vec::new(),
                selected_text_track: None,
                last_active_text_cues: Vec::new(),
                error: None,
                last_event_position: Duration::ZERO,
                events: VecDeque::new(),
            })),
        }
    }

    /// Create a controller for a URL-backed video source.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::new(MediaSource::url(url))
    }

    /// Create a controller for a local file path.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(MediaSource::file(path))
    }

    /// Create a controller for in-memory media bytes.
    pub fn bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::new(MediaSource::bytes(bytes))
    }

    /// Create a controller from a keyed reader factory.
    pub fn reader<R>(
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
    ) -> Self
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        Self::new(MediaSource::reader(key, open))
    }

    /// Eagerly load metadata for fluent controller setup.
    pub fn preload_metadata(self) -> Self {
        let _ = self.load_metadata();
        self
    }

    /// Set initial output volume for fluent controller setup.
    pub fn volume(self, volume: f32) -> Self {
        self.set_volume(volume);
        self
    }

    /// Set initial muted state for fluent controller setup.
    pub fn muted(self, muted: bool) -> Self {
        self.set_muted(muted);
        self
    }

    /// Set initial playback rate for fluent controller setup.
    pub fn playback_rate(self, playback_rate: f32) -> Self {
        self.set_playback_rate(playback_rate);
        self
    }

    /// Set looping for fluent controller setup.
    pub fn looping(self, looping: bool) -> Self {
        self.set_looping(looping);
        self
    }

    /// Seek before first playback for fluent controller setup.
    pub fn start_at(self, position: Duration) -> Self {
        let _ = self.seek(position);
        self
    }

    /// Add a parsed text track for fluent controller setup.
    pub fn text_track(self, track: TextTrack) -> Self {
        self.add_text_track(track);
        self
    }

    /// Parse and add a SubRip text track for fluent controller setup.
    pub fn srt_text_track(
        self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        self.add_srt_text_track(id, label, language, input);
        self
    }

    /// Parse and add a WebVTT text track for fluent controller setup.
    pub fn webvtt_text_track(
        self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        self.add_webvtt_text_track(id, label, language, input);
        self
    }

    /// Select a text track for fluent controller setup.
    pub fn selected_text_track(self, id: impl AsRef<str>) -> Self {
        self.select_text_track(id);
        self
    }

    /// Disable text tracks for fluent controller setup.
    pub fn text_track_disabled(self) -> Self {
        self.disable_text_track();
        self
    }

    /// Return the media source this controller owns.
    pub fn source(&self) -> MediaSource {
        self.state.borrow().source.clone()
    }

    /// Return the recommended rendering route for the current source.
    pub fn recommended_route(&self) -> VideoPlaybackRoute {
        recommended_video_playback_route(&self.source())
    }

    /// Return native playback confidence for the current source.
    pub fn can_play_source(&self) -> VideoCanPlay {
        can_play_video_source(&self.source())
    }

    /// Return the WebView element id used when this source is browser-routed.
    pub fn webview_player_id(&self) -> SharedString {
        webview_video_player_id(&self.source())
    }

    /// Return JavaScript for controlling this controller's browser fallback.
    pub fn webview_command_script(&self, command: WebViewVideoCommand) -> SharedString {
        webview_video_player_command_script(command)
    }

    /// Dispatch a command to this controller's WebView-hosted browser fallback.
    ///
    /// This is only meaningful when the same source is rendered through
    /// [`webview_video_player_url`] or `kael_ui::VideoPlayer`'s WebView route.
    pub fn dispatch_webview_command(
        &self,
        window: &mut Window,
        command: WebViewVideoCommand,
    ) -> anyhow::Result<()> {
        window.evaluate_webview_javascript(
            self.webview_player_id(),
            self.webview_command_script(command),
        )
    }

    /// Replace the media source and reset metadata, readiness, and playback
    /// position while preserving volume, muted state, playback rate, looping,
    /// and configured text tracks.
    pub fn set_source(&self, source: impl Into<MediaSource>) {
        let source = source.into();
        let mut state = self.state.borrow_mut();
        state.audio.stop();
        let audio = AudioHandle::new(source.clone());
        audio.set_volume(if state.muted { 0.0 } else { state.volume });
        audio.set_speed(state.playback_rate);

        state.source = source.clone();
        state.audio = audio;
        state.metadata = None;
        state.duration = None;
        state.ready_state = VideoReadyState::Nothing;
        state.buffered_ranges.clear();
        state.emitted_can_play = false;
        state.emitted_can_play_through = false;
        state.last_active_text_cues.clear();
        state.error = None;
        state.last_event_position = Duration::ZERO;
        state.events.push_back(VideoEvent::SourceChanged { source });
        state.events.push_back(VideoEvent::ReadyStateChange {
            ready_state: VideoReadyState::Nothing,
        });
        state.events.push_back(VideoEvent::Progress {
            buffered_ranges: Vec::new(),
        });
        state.events.push_back(VideoEvent::TimeUpdate {
            current_time: Duration::ZERO,
        });
    }

    /// Replace the source with a URL.
    pub fn set_url(&self, url: impl Into<Arc<str>>) {
        self.set_source(MediaSource::url(url));
    }

    /// Replace the source with a local file path.
    pub fn set_file(&self, path: impl Into<PathBuf>) {
        self.set_source(MediaSource::file(path));
    }

    /// Replace the source with in-memory media bytes.
    pub fn set_bytes(&self, bytes: impl Into<Arc<[u8]>>) {
        self.set_source(MediaSource::bytes(bytes));
    }

    /// Replace the source with a keyed reader factory.
    pub fn set_reader<R>(
        &self,
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
    ) where
        R: Read + Seek + Send + Sync + 'static,
    {
        self.set_source(MediaSource::reader(key, open));
    }

    /// Return a clonable audio handle for lower-level integrations.
    pub fn audio_handle(&self) -> AudioHandle {
        self.state.borrow().audio.clone()
    }

    /// Load video metadata if it has not already been loaded.
    pub fn load_metadata(&self) -> Result<VideoMetadata, VideoPlaybackError> {
        if let Some(metadata) = self.state.borrow().metadata {
            return Ok(metadata);
        }

        let source = self.source();
        match MediaDecoder::new(source.clone()).video_metadata() {
            Ok(metadata) => {
                let duration = metadata.duration;
                let buffered_ranges = buffered_ranges_for_source(&source, duration);
                let mut state = self.state.borrow_mut();
                state.metadata = Some(metadata);
                state.duration = duration;
                state.error = None;
                state.events.push_back(VideoEvent::LoadedMetadata {
                    duration,
                    width: metadata.width,
                    height: metadata.height,
                });
                push_ready_state_events(&mut state, VideoReadyState::Metadata);
                if state.buffered_ranges != buffered_ranges {
                    state.buffered_ranges = buffered_ranges;
                    let buffered_ranges = state.buffered_ranges.clone();
                    state
                        .events
                        .push_back(VideoEvent::Progress { buffered_ranges });
                }
                if state.buffered_ranges.is_empty() {
                    state.events.push_back(VideoEvent::Waiting);
                } else {
                    push_ready_state_events(&mut state, VideoReadyState::EnoughData);
                }
                Ok(metadata)
            }
            Err(error) => {
                self.record_error(error.to_string());
                Err(error.into())
            }
        }
    }

    /// Start or resume playback.
    pub fn play(&self) -> Result<(), VideoPlaybackError> {
        {
            let state = self.state.borrow();
            if state.metadata.is_none() {
                drop(state);
                self.load_metadata()?;
            }
        }

        match self.state.borrow().audio.play() {
            Ok(()) => {
                let mut state = self.state.borrow_mut();
                state.error = None;
                push_ready_state_events(&mut state, VideoReadyState::CurrentData);
                state.events.push_back(VideoEvent::Playing);
                Ok(())
            }
            Err(error) => {
                self.record_error(error.to_string());
                Err(error.into())
            }
        }
    }

    /// Pause playback at the current position.
    pub fn pause(&self) {
        let audio = self.state.borrow().audio.clone();
        audio.pause();
        self.state.borrow_mut().events.push_back(VideoEvent::Paused);
    }

    /// Stop playback and return to the beginning.
    pub fn stop(&self) {
        let audio = self.state.borrow().audio.clone();
        audio.stop();
        let mut state = self.state.borrow_mut();
        state.last_event_position = Duration::ZERO;
        state.events.push_back(VideoEvent::Stopped);
    }

    fn seek_with_mode(&self, position: Duration, fast: bool) -> Result<(), VideoPlaybackError> {
        {
            let state = self.state.borrow();
            if state.metadata.is_none() {
                drop(state);
                self.load_metadata()?;
            }
        }

        let audio = self.state.borrow().audio.clone();
        // The current software backend uses the same stream-level seek for both
        // modes. Platform backends can use `fast` to prefer keyframe seeks.
        let _prefer_keyframe = fast;
        match audio.seek(position) {
            Ok(()) => {
                let current_time = audio.position();
                let mut state = self.state.borrow_mut();
                state.last_event_position = current_time;
                state.error = None;
                state.events.push_back(VideoEvent::Seeked { current_time });
                state
                    .events
                    .push_back(VideoEvent::TimeUpdate { current_time });
                Ok(())
            }
            Err(error) => {
                self.record_error(error.to_string());
                Err(error.into())
            }
        }
    }

    /// Seek to the requested position.
    pub fn seek(&self, position: Duration) -> Result<(), VideoPlaybackError> {
        self.seek_with_mode(position, false)
    }

    /// Prefer a low-latency seek near the requested position.
    ///
    /// Today this uses the same software seek path as [`Self::seek`]. The
    /// explicit API lets platform backends choose keyframe-oriented seeks for
    /// scrubbers and thumbnail previews without changing app code later.
    pub fn fast_seek(&self, position: Duration) -> Result<(), VideoPlaybackError> {
        self.seek_with_mode(position, true)
    }

    /// Set the output volume where `1.0` is the original amplitude.
    pub fn set_volume(&self, volume: f32) {
        let mut state = self.state.borrow_mut();
        state.volume = volume.max(0.0);
        let effective_volume = if state.muted { 0.0 } else { state.volume };
        state.audio.set_volume(effective_volume);
        let volume = state.volume;
        let muted = state.muted;
        state
            .events
            .push_back(VideoEvent::VolumeChange { volume, muted });
    }

    /// Set whether output is muted.
    pub fn set_muted(&self, muted: bool) {
        let mut state = self.state.borrow_mut();
        state.muted = muted;
        let effective_volume = if muted { 0.0 } else { state.volume };
        state.audio.set_volume(effective_volume);
        let volume = state.volume;
        state
            .events
            .push_back(VideoEvent::VolumeChange { volume, muted });
    }

    /// Set the requested playback rate.
    ///
    /// Non-`1.0` values are applied to the current audio sink. Pitch-preserving
    /// time stretching and independent decoded-video clock control are backend
    /// work that will land with the stronger media pipelines.
    pub fn set_playback_rate(&self, playback_rate: f32) {
        let mut state = self.state.borrow_mut();
        state.playback_rate = sanitize_video_playback_rate(playback_rate);
        state.audio.set_speed(state.playback_rate);
        let playback_rate = state.playback_rate;
        state
            .events
            .push_back(VideoEvent::RateChange { playback_rate });
    }

    /// Set whether playback should loop.
    pub fn set_looping(&self, looping: bool) {
        let mut state = self.state.borrow_mut();
        state.looping = looping;
        state.events.push_back(VideoEvent::LoopChange { looping });
    }

    /// Add a parsed text track. The first added track is selected automatically.
    pub fn add_text_track(&self, track: TextTrack) {
        let mut state = self.state.borrow_mut();
        let id = track.id.clone();
        state.text_tracks.push(track);
        if state.selected_text_track.is_none() {
            state.selected_text_track = Some(state.text_tracks.len() - 1);
            state.events.push_back(VideoEvent::TextTrackChanged {
                id: Some(id.clone()),
            });
        }
        state.events.push_back(VideoEvent::TextTrackAdded { id });
    }

    /// Parse and add a SubRip subtitle track.
    pub fn add_srt_text_track(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) {
        self.add_text_track(TextTrack::from_srt(id, label, language, input));
    }

    /// Parse and add a WebVTT subtitle track.
    pub fn add_webvtt_text_track(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) {
        self.add_text_track(TextTrack::from_webvtt(id, label, language, input));
    }

    /// Return the configured text tracks.
    pub fn text_tracks(&self) -> Vec<TextTrack> {
        self.state.borrow().text_tracks.clone()
    }

    /// Return the active text track, if captions/subtitles are enabled.
    pub fn active_text_track(&self) -> Option<TextTrack> {
        let state = self.state.borrow();
        state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .cloned()
    }

    /// Return the selected text track id, if captions/subtitles are enabled.
    pub fn selected_text_track_id(&self) -> Option<SharedString> {
        self.active_text_track().map(|track| track.id)
    }

    /// Return active cues for the selected text track at an arbitrary position.
    pub fn active_text_cues_at(&self, position: Duration) -> Vec<TextTrackCue> {
        let state = self.state.borrow();
        state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .map(|track| track.active_cues(position))
            .unwrap_or_default()
    }

    /// Select a text track by id. Unknown ids disable active cues.
    pub fn select_text_track(&self, id: impl AsRef<str>) {
        let id = id.as_ref();
        let mut state = self.state.borrow_mut();
        state.selected_text_track = state
            .text_tracks
            .iter()
            .position(|track| track.id.as_ref() == id);
        let selected_id = state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .map(|track| track.id.clone());
        state.last_active_text_cues.clear();
        state
            .events
            .push_back(VideoEvent::TextTrackChanged { id: selected_id });
    }

    /// Disable the selected text track.
    pub fn disable_text_track(&self) {
        let mut state = self.state.borrow_mut();
        state.selected_text_track = None;
        state.last_active_text_cues.clear();
        state
            .events
            .push_back(VideoEvent::TextTrackChanged { id: None });
    }

    /// Return the current playback state.
    pub fn playback_state(&self) -> MediaPlaybackState {
        self.state.borrow().audio.state()
    }

    /// Return whether playback is actively advancing.
    pub fn is_playing(&self) -> bool {
        self.playback_state() == MediaPlaybackState::Playing
    }

    /// Return whether playback is paused.
    pub fn is_paused(&self) -> bool {
        self.playback_state() == MediaPlaybackState::Paused
    }

    /// Return whether playback is paused.
    pub fn paused(&self) -> bool {
        self.is_paused()
    }

    /// Return the current playback position.
    pub fn current_time(&self) -> Duration {
        self.state.borrow().audio.position()
    }

    /// Return the current playback position in seconds.
    pub fn current_time_secs(&self) -> f64 {
        self.current_time().as_secs_f64()
    }

    /// Set the current playback position.
    pub fn set_current_time(&self, position: Duration) -> Result<(), VideoPlaybackError> {
        self.seek(position)
    }

    /// Set the current playback position from seconds.
    pub fn set_current_time_secs(&self, seconds: f64) -> Result<(), VideoPlaybackError> {
        self.set_current_time(Duration::from_secs_f64(seconds.max(0.0)))
    }

    /// Prefer a low-latency seek from seconds.
    pub fn fast_seek_secs(&self, seconds: f64) -> Result<(), VideoPlaybackError> {
        self.fast_seek(Duration::from_secs_f64(seconds.max(0.0)))
    }

    /// Set the current playback position.
    pub fn set_position(&self, position: Duration) -> Result<(), VideoPlaybackError> {
        self.set_current_time(position)
    }

    /// Return the known duration, if available.
    pub fn duration(&self) -> Option<Duration> {
        let state = self.state.borrow();
        state
            .duration
            .or_else(|| state.audio.duration().ok().flatten())
    }

    /// Return the known duration in seconds, if available.
    pub fn duration_secs(&self) -> Option<f64> {
        self.duration().map(|duration| duration.as_secs_f64())
    }

    /// Return loaded metadata, if available.
    pub fn metadata(&self) -> Option<VideoMetadata> {
        self.state.borrow().metadata
    }

    /// Return the current ready state.
    pub fn ready_state(&self) -> VideoReadyState {
        self.state.borrow().ready_state
    }

    /// Return current buffered ranges.
    pub fn buffered_ranges(&self) -> Vec<TimeRange> {
        self.state.borrow().buffered_ranges.clone()
    }

    /// Return the configured output volume.
    pub fn volume_level(&self) -> f32 {
        self.state.borrow().volume
    }

    /// Return whether output is muted.
    pub fn is_muted(&self) -> bool {
        self.state.borrow().muted
    }

    /// Return whether output is muted.
    pub fn muted_state(&self) -> bool {
        self.is_muted()
    }

    /// Return the configured playback rate.
    pub fn playback_rate_value(&self) -> f32 {
        self.state.borrow().playback_rate
    }

    /// Return the configured playback rate.
    pub fn rate(&self) -> f32 {
        self.playback_rate_value()
    }

    /// Return whether playback loops at the end.
    pub fn is_looping(&self) -> bool {
        self.state.borrow().looping
    }

    /// Return whether playback loops at the end.
    pub fn looping_enabled(&self) -> bool {
        self.is_looping()
    }

    /// Return the last controller error, if any.
    pub fn error(&self) -> Option<String> {
        self.state.borrow().error.clone()
    }

    /// Return a state snapshot and emit time/end events when the clock advanced.
    pub fn snapshot(&self) -> VideoSnapshot {
        let mut state = self.state.borrow_mut();
        let playback_state = state.audio.state();
        let current_time = state.audio.position();
        let duration = state
            .duration
            .or_else(|| state.audio.duration().ok().flatten());
        let active_text_cues = state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .map(|track| track.active_cues(current_time))
            .unwrap_or_default();

        if playback_state == MediaPlaybackState::Stopped
            && duration
                .is_some_and(|duration| current_time >= duration && duration > Duration::ZERO)
            && state.last_event_position < current_time
        {
            state.events.push_back(VideoEvent::Ended);
            if state.looping {
                let audio = state.audio.clone();
                drop(state);
                let _ = audio.seek(Duration::ZERO);
                let _ = audio.play();
                return self.snapshot();
            }
            state = self.state.borrow_mut();
        }

        if current_time != state.last_event_position {
            state.last_event_position = current_time;
            state
                .events
                .push_back(VideoEvent::TimeUpdate { current_time });
        }

        if active_text_cues != state.last_active_text_cues {
            state.last_active_text_cues = active_text_cues.clone();
            state.events.push_back(VideoEvent::CueChange {
                cues: active_text_cues.clone(),
            });
        }

        VideoSnapshot {
            playback_state,
            current_time,
            duration,
            metadata: state.metadata,
            ready_state: state.ready_state,
            buffered_ranges: state.buffered_ranges.clone(),
            volume: state.volume,
            muted: state.muted,
            playback_rate: state.playback_rate,
            looping: state.looping,
            active_text_cues,
            error: state.error.clone(),
        }
    }

    /// Drain pending events in FIFO order.
    pub fn drain_events(&self) -> Vec<VideoEvent> {
        self.state.borrow_mut().events.drain(..).collect()
    }

    fn record_error(&self, error: String) {
        let mut state = self.state.borrow_mut();
        state.error = Some(error.clone());
        state.events.push_back(VideoEvent::Error(error));
    }
}

fn buffered_ranges_for_source(source: &MediaSource, duration: Option<Duration>) -> Vec<TimeRange> {
    match (source, duration) {
        (MediaSource::File(_), Some(duration))
        | (MediaSource::Bytes(_), Some(duration))
        | (MediaSource::Reader(_), Some(duration))
            if duration > Duration::ZERO =>
        {
            vec![TimeRange::new(Duration::ZERO, duration)]
        }
        _ => Vec::new(),
    }
}

fn push_ready_state_events(state: &mut VideoControllerState, ready_state: VideoReadyState) {
    if ready_state > state.ready_state {
        state.ready_state = ready_state;
        state
            .events
            .push_back(VideoEvent::ReadyStateChange { ready_state });
    }

    if state.ready_state >= VideoReadyState::CurrentData && !state.emitted_can_play {
        state.emitted_can_play = true;
        state.events.push_back(VideoEvent::CanPlay);
    }

    if state.ready_state >= VideoReadyState::EnoughData && !state.emitted_can_play_through {
        state.emitted_can_play_through = true;
        state.events.push_back(VideoEvent::CanPlayThrough);
    }
}

/// Recommend whether a media source should use Kael's native media path or an
/// explicit WebView island.
///
/// The current native backend can open many direct file/URL/container sources
/// through FFmpeg, but browser-managed streaming manifests such as HLS and DASH
/// are better routed through WebView until native streaming backends land.
pub fn recommended_video_playback_route(source: &MediaSource) -> VideoPlaybackRoute {
    match media_manifest_extension(source).as_deref() {
        Some("m3u8") => VideoPlaybackRoute::WebViewRecommended {
            reason: "HLS playlists need browser/WebView media support until native streaming backends land".into(),
        },
        Some("mpd") => VideoPlaybackRoute::WebViewRecommended {
            reason: "DASH manifests need browser/WebView media support until native streaming backends land".into(),
        },
        _ => VideoPlaybackRoute::Native,
    }
}

/// Recommend a playback route from a MIME/content type.
///
/// This is useful when a server exposes a streaming manifest behind an
/// extensionless URL but the app already has a `Content-Type` header.
pub fn recommended_video_playback_route_for_type(mime_type: &str) -> VideoPlaybackRoute {
    let essence = media_mime_essence(mime_type);
    match essence.as_str() {
        "application/vnd.apple.mpegurl"
        | "application/x-mpegurl"
        | "audio/mpegurl"
        | "audio/x-mpegurl" => VideoPlaybackRoute::WebViewRecommended {
            reason:
                "HLS MIME types need browser/WebView media support until native streaming backends land"
                    .into(),
        },
        "application/dash+xml" | "application/vnd.ms-sstr+xml" => {
            VideoPlaybackRoute::WebViewRecommended {
                reason: "Adaptive streaming manifests need browser/WebView media support until native streaming backends land".into(),
            }
        }
        _ => VideoPlaybackRoute::Native,
    }
}

/// Return native playback confidence for a MIME type, similar to
/// `HTMLMediaElement.canPlayType`.
pub fn can_play_video_type(mime_type: &str) -> VideoCanPlay {
    let essence = media_mime_essence(mime_type);

    match essence.as_str() {
        "application/vnd.apple.mpegurl"
        | "application/x-mpegurl"
        | "audio/mpegurl"
        | "audio/x-mpegurl"
        | "application/dash+xml"
        | "application/vnd.ms-sstr+xml" => VideoCanPlay::No,
        "video/mp4" | "video/quicktime" | "video/webm" | "video/x-matroska" => {
            VideoCanPlay::Probably
        }
        "video/x-msvideo" | "video/mpeg" | "video/ogg" | "application/ogg" => VideoCanPlay::Maybe,
        _ => VideoCanPlay::No,
    }
}

/// Return native playback confidence for a concrete media source.
pub fn can_play_video_source(source: &MediaSource) -> VideoCanPlay {
    match media_manifest_extension(source).as_deref() {
        Some("m3u8" | "mpd") => VideoCanPlay::No,
        Some("mp4" | "m4v" | "mov" | "webm" | "mkv") => VideoCanPlay::Probably,
        Some("avi" | "mpeg" | "mpg" | "ogv" | "ogg") => VideoCanPlay::Maybe,
        Some(_) => VideoCanPlay::Maybe,
        None => match source {
            MediaSource::Bytes(_) | MediaSource::Reader(_) => VideoCanPlay::Maybe,
            MediaSource::File(_) | MediaSource::Url(_) => VideoCanPlay::Maybe,
        },
    }
}

/// Build a `data:` URL containing a minimal browser `<video>` page for WebView
/// fallback playback.
///
/// This returns `None` for byte and reader sources because WebView needs a URL
/// it can load directly. Use this with [`crate::webview`] when
/// [`recommended_video_playback_route`] returns
/// [`VideoPlaybackRoute::WebViewRecommended`].
pub fn webview_video_player_url(
    source: &MediaSource,
    options: &WebViewVideoOptions,
) -> Option<SharedString> {
    let src = match source {
        MediaSource::Url(url) => url.as_ref().to_owned(),
        MediaSource::File(path) => file_url_for_path(path)?,
        MediaSource::Bytes(_) | MediaSource::Reader(_) => return None,
    };
    Some(
        format!(
            "data:text/html;charset=utf-8,{}",
            percent_encode_data_url(&webview_video_player_html(&src, options))
        )
        .into(),
    )
}

/// Return the stable WebView element id used for a source-backed browser video fallback.
pub fn webview_video_player_id(source: &MediaSource) -> SharedString {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("kael-video-player-{:x}", hasher.finish()).into()
}

/// Return JavaScript that applies a browser-video command to the fallback page.
///
/// Use this with [`Window::evaluate_webview_javascript`] and
/// [`webview_video_player_id`] when an app needs to control a WebView-routed
/// fallback as if it were an HTML video element.
pub fn webview_video_player_command_script(command: WebViewVideoCommand) -> SharedString {
    let command = match command {
        WebViewVideoCommand::Play => {
            "const result=video.play();if(result&&result.catch){result.catch(()=>{});}".to_string()
        }
        WebViewVideoCommand::Pause => "video.pause();".to_string(),
        WebViewVideoCommand::TogglePlay => {
            "if(video.paused){const result=video.play();if(result&&result.catch){result.catch(()=>{});}}else{video.pause();}".to_string()
        }
        WebViewVideoCommand::Stop => "video.pause();video.currentTime=0;".to_string(),
        WebViewVideoCommand::Seek(position) => {
            format!("video.currentTime={};", position.as_secs_f64().max(0.0))
        }
        WebViewVideoCommand::FastSeek(position) => {
            format!(
                "if(video.fastSeek){{video.fastSeek({0});}}else{{video.currentTime={0};}}",
                position.as_secs_f64().max(0.0)
            )
        }
        WebViewVideoCommand::SetVolume(volume) => {
            format!("video.volume={};", volume.clamp(0.0, 1.0))
        }
        WebViewVideoCommand::SetMuted(muted) => format!("video.muted={muted};"),
        WebViewVideoCommand::SetPlaybackRate(playback_rate) => {
            format!("video.playbackRate={};", playback_rate.max(0.0))
        }
        WebViewVideoCommand::SetLooping(looping) => format!("video.loop={looping};"),
        WebViewVideoCommand::SelectTextTrack(selector) => {
            let selector = serde_json::to_string(selector.as_ref())
                .unwrap_or_else(|_| "\"\"".to_string());
            format!(
                r#"const selector={selector};for(let i=0;i<video.textTracks.length;i++){{const track=video.textTracks[i];const matches=track.id===selector||track.label===selector||track.language===selector||String(i)===selector;track.mode=matches?"showing":"disabled";}}post("texttrackchange");post("cuechange",{{activeCues:activeCues()}});"#
            )
        }
        WebViewVideoCommand::DisableTextTracks => {
            r#"for(let i=0;i<video.textTracks.length;i++){video.textTracks[i].mode="disabled";}post("texttrackchange");post("cuechange",{activeCues:activeCues()});"#
                .to_string()
        }
        WebViewVideoCommand::RequestFullscreen => {
            r#"const request=video.requestFullscreen||video.webkitRequestFullscreen;if(request){const result=request.call(video);if(result&&result.catch){result.catch(()=>{});}}"#.to_string()
        }
        WebViewVideoCommand::ExitFullscreen => {
            r#"const exit=document.exitFullscreen||document.webkitExitFullscreen;if(exit){const result=exit.call(document);if(result&&result.catch){result.catch(()=>{});}}"#.to_string()
        }
        WebViewVideoCommand::RequestPictureInPicture => {
            r#"if(video.requestPictureInPicture){const result=video.requestPictureInPicture();if(result&&result.catch){result.catch(()=>{});}}"#.to_string()
        }
        WebViewVideoCommand::ExitPictureInPicture => {
            r#"if(document.pictureInPictureElement&&document.exitPictureInPicture){const result=document.exitPictureInPicture();if(result&&result.catch){result.catch(()=>{});}}"#.to_string()
        }
        WebViewVideoCommand::RequestSnapshot => {
            r#"post("snapshot",{buffered:ranges()});"#.to_string()
        }
    };

    format!(
        r#"(()=>{{const video=document.querySelector("video");if(!video){{return;}}const ranges=()=>{{const out=[];for(let i=0;i<video.buffered.length;i++){{out.push([video.buffered.start(i),video.buffered.end(i)]);}}return out;}};const fullscreen=()=>document.fullscreenElement===video||document.webkitFullscreenElement===video;const pictureInPicture=()=>document.pictureInPictureElement===video;const trackId=(track,index)=>track.id||track.label||track.language||String(index);const selectedTrack=()=>{{for(let i=0;i<video.textTracks.length;i++){{const track=video.textTracks[i];if(track.mode==="showing"){{return{{id:trackId(track,i),label:track.label||"",language:track.language||"",kind:track.kind||"",index:i}};}}}}return null;}};const activeCues=()=>{{const track=selectedTrack();if(!track)return[];const browserTrack=video.textTracks[track.index];const cues=browserTrack&&browserTrack.activeCues?browserTrack.activeCues:[];const out=[];for(let i=0;i<cues.length;i++){{const cue=cues[i];out.push({{startTime:cue.startTime||0,endTime:cue.endTime||0,text:cue.text||""}});}}return out;}};const post=(event,extra={{}})=>{{try{{window.gpui&&window.gpui.postMessage({{kind:"kael-video-event",event,currentTime:video.currentTime||0,duration:Number.isFinite(video.duration)?video.duration:null,volume:video.volume,muted:video.muted,playbackRate:video.playbackRate,paused:video.paused,ended:video.ended,videoWidth:video.videoWidth||0,videoHeight:video.videoHeight||0,fullscreen:fullscreen(),pictureInPicture:pictureInPicture(),selectedTextTrack:selectedTrack(),...extra}});}}catch(_){{}}}};{command}}})();"#
    )
    .into()
}

fn webview_video_player_html(src: &str, options: &WebViewVideoOptions) -> String {
    let mut attributes = Vec::<String>::new();
    if options.controls {
        attributes.push("controls".to_string());
    }
    if options.autoplay {
        attributes.push("autoplay".to_string());
    }
    if options.muted {
        attributes.push("muted".to_string());
    }
    if options.looping {
        attributes.push("loop".to_string());
    }
    if options.plays_inline {
        attributes.push("playsinline".to_string());
    }
    if let Some(poster) = &options.poster {
        attributes.push(format!(r#"poster="{}""#, escape_html_attribute(poster)));
    }
    if let Some(preload) = options.preload {
        attributes.push(format!(r#"preload="{}""#, preload.as_html_value()));
    }
    if let Some(cross_origin) = options.cross_origin {
        attributes.push(format!(r#"crossorigin="{}""#, cross_origin.as_html_value()));
    }
    if !options.controls_list.is_empty() {
        let controls_list = options
            .controls_list
            .iter()
            .map(|token| escape_html_token(token))
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !controls_list.is_empty() {
            attributes.push(format!(r#"controlslist="{controls_list}""#));
        }
    }
    if options.disable_picture_in_picture {
        attributes.push("disablePictureInPicture".to_string());
    }
    let attributes = if attributes.is_empty() {
        String::new()
    } else {
        format!(" {}", attributes.join(" "))
    };
    let src = escape_html_attribute(src);
    let object_fit = escape_css_value(options.object_fit.as_ref());
    let tracks = options
        .text_tracks
        .iter()
        .map(|track| {
            let kind = escape_html_token(&track.kind);
            let label = escape_html_attribute(&track.label);
            let src = escape_html_attribute(&track.src);
            let language = track
                .language
                .as_ref()
                .map(|language| format!(r#" srclang="{}""#, escape_html_attribute(language)));
            let default = if track.default { " default" } else { "" };
            format!(
                r#"<track kind="{kind}" label="{label}"{} src="{src}"{default}>"#,
                language.unwrap_or_default()
            )
        })
        .collect::<String>();
    let start_script = options
        .start_position
        .map(|position| {
            format!(
                r#"<script>const video=document.querySelector("video");video.addEventListener("loadedmetadata",()=>{{try{{video.currentTime={};}}catch(_){{}}}},{{once:true}});</script>"#,
                position.as_secs_f64().max(0.0)
            )
        })
        .unwrap_or_default();
    let event_script = r#"<script>(()=>{const video=document.querySelector("video");const ranges=()=>{const out=[];for(let i=0;i<video.buffered.length;i++){out.push([video.buffered.start(i),video.buffered.end(i)]);}return out;};const fullscreen=()=>document.fullscreenElement===video||document.webkitFullscreenElement===video;const pictureInPicture=()=>document.pictureInPictureElement===video;const trackId=(track,index)=>track.id||track.label||track.language||String(index);const selectedTrack=()=>{for(let i=0;i<video.textTracks.length;i++){const track=video.textTracks[i];if(track.mode==="showing"){return{id:trackId(track,i),label:track.label||"",language:track.language||"",kind:track.kind||"",index:i};}}return null;};const activeCues=()=>{const track=selectedTrack();if(!track)return[];const browserTrack=video.textTracks[track.index];const cues=browserTrack&&browserTrack.activeCues?browserTrack.activeCues:[];const out=[];for(let i=0;i<cues.length;i++){const cue=cues[i];out.push({startTime:cue.startTime||0,endTime:cue.endTime||0,text:cue.text||""});}return out;};const post=(event,extra={})=>{try{window.gpui&&window.gpui.postMessage({kind:"kael-video-event",event,currentTime:video.currentTime||0,duration:Number.isFinite(video.duration)?video.duration:null,volume:video.volume,muted:video.muted,playbackRate:video.playbackRate,paused:video.paused,ended:video.ended,videoWidth:video.videoWidth||0,videoHeight:video.videoHeight||0,fullscreen:fullscreen(),pictureInPicture:pictureInPicture(),selectedTextTrack:selectedTrack(),...extra});}catch(_){}};const bindTrackEvents=()=>{for(let i=0;i<video.textTracks.length;i++){const track=video.textTracks[i];if(track.__kaelBound)continue;track.__kaelBound=true;track.addEventListener("cuechange",()=>post("cuechange",{activeCues:activeCues()}));}};["loadedmetadata","canplay","canplaythrough","waiting","playing","pause","seeked","timeupdate","volumechange","ratechange","ended"].forEach(event=>video.addEventListener(event,()=>post(event)));["fullscreenchange","webkitfullscreenchange"].forEach(event=>document.addEventListener(event,()=>post("fullscreenchange")));["enterpictureinpicture","leavepictureinpicture"].forEach(event=>video.addEventListener(event,()=>post(event)));video.addEventListener("loadedmetadata",()=>{bindTrackEvents();post("texttrackchange");post("cuechange",{activeCues:activeCues()});});if(video.textTracks){video.textTracks.addEventListener("change",()=>{post("texttrackchange");post("cuechange",{activeCues:activeCues()});});video.textTracks.addEventListener("addtrack",()=>{bindTrackEvents();post("texttrackchange");});video.textTracks.addEventListener("removetrack",()=>{post("texttrackchange");post("cuechange",{activeCues:activeCues()});});}bindTrackEvents();video.addEventListener("progress",()=>post("progress",{buffered:ranges()}));video.addEventListener("error",()=>post("error",{message:video.error?video.error.message:"Video playback failed"}));})();</script>"#;

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>html,body{{margin:0;width:100%;height:100%;background:#000;overflow:hidden}}video{{display:block;width:100%;height:100%;object-fit:{object_fit};background:#000}}</style></head><body><video{attributes} src="{src}">{tracks}</video>{start_script}{event_script}</body></html>"#
    )
}

fn file_url_for_path(path: &std::path::Path) -> Option<String> {
    let path = path.to_str()?;
    Some(format!("file://{}", percent_encode_path(path)))
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect()
}

fn escape_css_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
}

fn percent_encode_path(value: &str) -> String {
    percent_encode_with(value, |byte| {
        matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~')
    })
}

fn percent_encode_data_url(value: &str) -> String {
    percent_encode_with(value, |byte| matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

fn percent_encode_with(value: &str, allow: impl Fn(u8) -> bool) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || allow(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn media_manifest_extension(source: &MediaSource) -> Option<String> {
    match source {
        MediaSource::File(path) => path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase()),
        MediaSource::Url(url) => url_manifest_extension(url.as_ref()),
        MediaSource::Bytes(_) | MediaSource::Reader(_) => None,
    }
}

fn media_mime_essence(mime_type: &str) -> String {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase()
}

fn url_manifest_extension(url: &str) -> Option<String> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/');
    path.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
}

fn sanitize_video_playback_rate(playback_rate: f32) -> f32 {
    if playback_rate.is_finite() && playback_rate > 0.0 {
        playback_rate
    } else {
        1.0
    }
}

fn validate_media_source(
    source: &MediaSource,
    require_existing_file: bool,
    canonicalize_file: bool,
) -> anyhow::Result<()> {
    match source {
        MediaSource::Url(url) => validate_media_url(url.as_ref(), "video source URL"),
        MediaSource::File(path) => {
            anyhow::ensure!(
                !path.as_os_str().is_empty(),
                "video source file path cannot be empty"
            );
            if require_existing_file || canonicalize_file {
                let metadata = std::fs::metadata(path).with_context(|| {
                    format!("video source file does not exist: {}", path.display())
                })?;
                anyhow::ensure!(
                    metadata.is_file(),
                    "video source path must be a file: {}",
                    path.display()
                );
            }
            Ok(())
        }
        MediaSource::Bytes(bytes) => {
            anyhow::ensure!(!bytes.is_empty(), "video source bytes cannot be empty");
            Ok(())
        }
        MediaSource::Reader(_) => {
            validate_non_empty_trimmed(source.reader_key().unwrap_or_default(), "video reader key")
        }
    }
}

fn validate_media_content_type(content_type: &str) -> anyhow::Result<()> {
    validate_non_empty_trimmed(content_type, "video content type")?;
    let essence = media_mime_essence(content_type);
    let (media_type, media_subtype) = essence
        .split_once('/')
        .ok_or_else(|| anyhow!("video content type must include a type and subtype"))?;
    anyhow::ensure!(
        !media_type.is_empty() && !media_subtype.is_empty(),
        "video content type must include a type and subtype"
    );
    validate_media_type_token(media_type, "video content type")?;
    validate_media_type_token(media_subtype, "video content subtype")?;
    Ok(())
}

fn validate_media_type_token(value: &str, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.chars().all(|ch| ch.is_ascii_alphanumeric()
            || matches!(ch, '!' | '#' | '$' | '&' | '-' | '^' | '_' | '+' | '.')),
        "{label} contains invalid characters"
    );
    Ok(())
}

fn validate_optional_media_url(url: Option<&str>, label: &str) -> anyhow::Result<()> {
    if let Some(url) = url {
        validate_media_url(url, label)?;
    }
    Ok(())
}

fn validate_media_url(url: &str, label: &str) -> anyhow::Result<()> {
    validate_non_empty_trimmed(url, label)?;
    let parsed = http_client::Url::parse(url).with_context(|| format!("{label} is invalid"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https" | "file" | "data"),
        "{label} must use http, https, file, or data"
    );
    if matches!(parsed.scheme(), "http" | "https") {
        anyhow::ensure!(parsed.host_str().is_some(), "{label} must include a host");
    }
    Ok(())
}

fn validate_non_empty_trimmed(value: &str, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

fn validate_html_token(value: &str, label: &str) -> anyhow::Result<()> {
    validate_non_empty_trimmed(value, label)?;
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')),
        "{label} must contain only ASCII letters, numbers, '-' or '_'"
    );
    Ok(())
}

fn validate_webview_video_object_fit(object_fit: &str) -> anyhow::Result<()> {
    validate_non_empty_trimmed(object_fit, "video object-fit")?;
    anyhow::ensure!(
        matches!(
            object_fit,
            "contain" | "cover" | "fill" | "none" | "scale-down"
        ),
        "video object-fit must be contain, cover, fill, none, or scale-down"
    );
    Ok(())
}

fn validate_webview_video_start_position(position: Option<Duration>) -> anyhow::Result<()> {
    if let Some(position) = position {
        anyhow::ensure!(
            position < Duration::from_secs(365 * 24 * 60 * 60),
            "video start position is unreasonably large"
        );
    }
    Ok(())
}

fn parse_srt_cues(input: &str) -> Vec<TextTrackCue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let Some(timing_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some((start_text, end_text)) = lines[timing_index].split_once("-->") else {
            continue;
        };
        let (Some(start), Some(end)) = (
            parse_text_track_timestamp(start_text),
            parse_text_track_timestamp(end_text),
        ) else {
            continue;
        };
        let text = lines[timing_index + 1..].join("\n");
        if text.is_empty() {
            continue;
        }
        cues.push(TextTrackCue::new(start, end, text));
    }
    cues
}

fn parse_webvtt_cues(input: &str) -> Vec<TextTrackCue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let first = lines[0].trim();
        if first == "WEBVTT"
            || first.starts_with("NOTE")
            || first.starts_with("STYLE")
            || first.starts_with("REGION")
        {
            continue;
        }
        let Some(timing_index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some((start_text, end_with_settings)) = lines[timing_index].split_once("-->") else {
            continue;
        };
        let end_text = end_with_settings
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let (Some(start), Some(end)) = (
            parse_text_track_timestamp(start_text),
            parse_text_track_timestamp(end_text),
        ) else {
            continue;
        };
        let text = lines[timing_index + 1..].join("\n");
        if text.is_empty() {
            continue;
        }
        cues.push(TextTrackCue::new(start, end, text));
    }
    cues
}

fn parse_text_track_timestamp(text: &str) -> Option<Duration> {
    let (clock, millis) = text.trim().split_once(['.', ','])?;
    let milliseconds: u64 = millis.parse().ok()?;
    if milliseconds >= 1000 {
        return None;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds): (u64, u64, u64) = match parts.len() {
        3 => (
            parts[0].trim().parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ),
        2 => (0, parts[0].trim().parse().ok()?, parts[1].parse().ok()?),
        _ => return None,
    };
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    Some(Duration::from_millis(
        hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + milliseconds,
    ))
}

/// A video element that synchronizes decoded frames to an audio playback clock.
pub struct Video {
    source: MediaSource,
    object_fit: ObjectFit,
    autoplay: bool,
    synced_audio: Option<AudioHandle>,
    style: StyleRefinement,
}

/// Create a new video element for the given media source.
#[track_caller]
pub fn video(source: impl Into<MediaSource>) -> Video {
    Video {
        source: source.into(),
        object_fit: ObjectFit::Contain,
        autoplay: false,
        synced_audio: None,
        style: StyleRefinement::default(),
    }
}

impl Video {
    /// Start synchronized playback automatically once the media has loaded.
    pub fn autoplay(mut self) -> Self {
        self.autoplay = true;
        self
    }

    /// Synchronize this video's frame clock to the given audio playback handle.
    pub fn sync_to(mut self, handle: &AudioHandle) -> Self {
        self.synced_audio = Some(handle.clone());
        self
    }

    /// Set the object-fit policy used to place the decoded video frame within its bounds.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

/// Create a new audio playback handle for the given media source.
pub fn audio(source: impl Into<MediaSource>) -> AudioHandle {
    AudioHandle::new(source)
}

/// Decode a video source into an animated render image.
pub fn decode_video_image(
    source: impl Into<MediaSource>,
) -> Result<Arc<RenderImage>, MediaDecodeError> {
    let (_, video_frames) = decode_video_frames(source)?;
    let mut frames = SmallVec::<[Frame; 1]>::new();
    for (index, frame) in video_frames.iter().enumerate() {
        let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
            frame.width,
            frame.height,
            frame.data.as_ref().to_vec(),
        )
        .ok_or_else(|| MediaDecodeError::Decode("invalid video frame buffer".into()))?;
        frames.push(Frame::from_parts(
            buffer,
            0,
            0,
            video_frame_delay(&video_frames, index),
        ));
    }

    Ok(Arc::new(RenderImage::new(frames)))
}

type MediaKeyTrackCallback = Box<dyn FnMut(&mut App)>;

/// Ordered video sources for playlist-style next/previous media controls.
#[derive(Clone, Default)]
pub struct VideoPlaylist {
    sources: Vec<MediaSource>,
    current_index: usize,
    repeat: bool,
}

impl VideoPlaylist {
    /// Create a playlist from ordered media sources.
    pub fn new(sources: impl IntoIterator<Item = impl Into<MediaSource>>) -> Self {
        Self {
            sources: sources.into_iter().map(Into::into).collect(),
            current_index: 0,
            repeat: false,
        }
    }

    /// Create an empty playlist.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add one media source.
    pub fn push(mut self, source: impl Into<MediaSource>) -> Self {
        self.sources.push(source.into());
        self
    }

    /// Set the current playlist index.
    pub fn start_at(mut self, index: usize) -> Self {
        self.current_index = index.min(self.sources.len().saturating_sub(1));
        self
    }

    /// Set whether next/previous wrap at playlist boundaries.
    pub fn repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }

    /// Return the ordered sources.
    pub fn sources(&self) -> &[MediaSource] {
        &self.sources
    }

    /// Return the number of sources in the playlist.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Return whether the playlist has no sources.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Return whether next/previous wrap at playlist boundaries.
    pub fn repeat_enabled(&self) -> bool {
        self.repeat
    }

    /// Return the current index.
    pub fn current_index(&self) -> Option<usize> {
        if self.sources.is_empty() {
            None
        } else {
            Some(self.current_index)
        }
    }

    /// Return the current source.
    pub fn current_source(&self) -> Option<MediaSource> {
        self.current_index()
            .and_then(|index| self.sources.get(index))
            .cloned()
    }

    /// Advance to the next source and return it.
    pub fn next_source(&mut self) -> Option<MediaSource> {
        if self.sources.is_empty() {
            return None;
        }

        if self.current_index + 1 < self.sources.len() {
            self.current_index += 1;
        } else if self.repeat {
            self.current_index = 0;
        } else {
            return None;
        }

        self.current_source()
    }

    /// Move to the previous source and return it.
    pub fn previous_source(&mut self) -> Option<MediaSource> {
        if self.sources.is_empty() {
            return None;
        }

        if self.current_index > 0 {
            self.current_index -= 1;
        } else if self.repeat {
            self.current_index = self.sources.len() - 1;
        } else {
            return None;
        }

        self.current_source()
    }

    /// Validate playlist sources before using the playlist for generated media UI.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.sources.is_empty(),
            "video playlist must include at least one source"
        );
        for (index, source) in self.sources.iter().enumerate() {
            validate_media_source(source, false, false)
                .with_context(|| format!("video playlist source {index} is invalid"))?;
        }
        Ok(())
    }

    /// Return a validated clone of this playlist.
    pub fn checked(&self) -> anyhow::Result<Self> {
        self.validate()?;
        Ok(self.clone())
    }
}

/// Builder for routing hardware media keys to a media controller.
#[derive(Default)]
pub struct MediaKeyBindingBuilder {
    audio: Option<AudioHandle>,
    video: Option<VideoController>,
    playlist: Option<VideoPlaylist>,
    on_next_track: Option<MediaKeyTrackCallback>,
    on_previous_track: Option<MediaKeyTrackCallback>,
    on_unhandled: Option<Box<dyn FnMut(MediaKeyEvent, &mut App)>>,
}

impl MediaKeyBindingBuilder {
    /// Create an empty media-key binding builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Route play/pause/stop keys to an audio handle.
    pub fn audio(mut self, handle: AudioHandle) -> Self {
        self.audio = Some(handle);
        self
    }

    /// Route play/pause/stop keys to a video controller.
    pub fn video(mut self, controller: VideoController) -> Self {
        self.video = Some(controller);
        self
    }

    /// Route next/previous media keys to source changes on the video controller.
    ///
    /// This is intended for file/URL/media-library players where next/previous
    /// should simply replace the current `VideoController` source. Use
    /// [`Self::on_next_track`] and [`Self::on_previous_track`] for app-owned
    /// queues that need database updates, analytics, or custom preload logic.
    pub fn playlist(mut self, playlist: VideoPlaylist) -> Self {
        self.playlist = Some(playlist);
        self
    }

    /// Handle next-track keys.
    pub fn on_next_track(mut self, callback: impl FnMut(&mut App) + 'static) -> Self {
        self.on_next_track = Some(Box::new(callback));
        self
    }

    /// Handle previous-track keys.
    pub fn on_previous_track(mut self, callback: impl FnMut(&mut App) + 'static) -> Self {
        self.on_previous_track = Some(Box::new(callback));
        self
    }

    /// Handle keys that were not consumed by the configured target or callbacks.
    pub fn on_unhandled(mut self, callback: impl FnMut(MediaKeyEvent, &mut App) + 'static) -> Self {
        self.on_unhandled = Some(Box::new(callback));
        self
    }

    /// Validate the binding before installing it.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(playlist) = &self.playlist {
            anyhow::ensure!(
                self.video.is_some(),
                "media-key playlist routing requires a video controller"
            );
            playlist.validate()?;
        }

        anyhow::ensure!(
            self.audio.is_some()
                || self.video.is_some()
                || self.on_next_track.is_some()
                || self.on_previous_track.is_some()
                || self.on_unhandled.is_some(),
            "media-key binding must configure an audio handle, video controller, track callback, or unhandled callback"
        );

        Ok(())
    }

    /// Install the binding after validating common generated-app mistakes.
    pub fn install_checked(self, app: &App) -> anyhow::Result<()> {
        self.validate()?;
        self.install(app);
        Ok(())
    }

    /// Install the binding on the app.
    pub fn install(self, app: &App) {
        let Self {
            audio,
            video,
            mut playlist,
            mut on_next_track,
            mut on_previous_track,
            mut on_unhandled,
        } = self;

        app.on_media_key_event(move |event, app| {
            let handled = match event {
                MediaKeyEvent::Play => play_media_target(audio.as_ref(), video.as_ref()),
                MediaKeyEvent::Pause => pause_media_target(audio.as_ref(), video.as_ref()),
                MediaKeyEvent::PlayPause => toggle_media_target(audio.as_ref(), video.as_ref()),
                MediaKeyEvent::Stop => stop_media_target(audio.as_ref(), video.as_ref()),
                MediaKeyEvent::NextTrack => {
                    if let Some(source) = playlist.as_mut().and_then(VideoPlaylist::next_source)
                        && let Some(video) = video.as_ref()
                    {
                        video.set_source(source);
                        true
                    } else if let Some(callback) = on_next_track.as_mut() {
                        callback(app);
                        true
                    } else {
                        false
                    }
                }
                MediaKeyEvent::PreviousTrack => {
                    if let Some(source) = playlist.as_mut().and_then(VideoPlaylist::previous_source)
                        && let Some(video) = video.as_ref()
                    {
                        video.set_source(source);
                        true
                    } else if let Some(callback) = on_previous_track.as_mut() {
                        callback(app);
                        true
                    } else {
                        false
                    }
                }
            };

            if !handled && let Some(callback) = on_unhandled.as_mut() {
                callback(event, app);
            }
        });
    }
}

fn play_media_target(audio: Option<&AudioHandle>, video: Option<&VideoController>) -> bool {
    if let Some(video) = video {
        let _ = video.play();
        true
    } else if let Some(audio) = audio {
        let _ = audio.play();
        true
    } else {
        false
    }
}

fn pause_media_target(audio: Option<&AudioHandle>, video: Option<&VideoController>) -> bool {
    if let Some(video) = video {
        video.pause();
        true
    } else if let Some(audio) = audio {
        audio.pause();
        true
    } else {
        false
    }
}

fn stop_media_target(audio: Option<&AudioHandle>, video: Option<&VideoController>) -> bool {
    if let Some(video) = video {
        video.stop();
        true
    } else if let Some(audio) = audio {
        audio.stop();
        true
    } else {
        false
    }
}

fn toggle_media_target(audio: Option<&AudioHandle>, video: Option<&VideoController>) -> bool {
    if let Some(video) = video {
        if video.is_playing() {
            video.pause();
        } else {
            let _ = video.play();
        }
        true
    } else if let Some(audio) = audio {
        if audio.state() == MediaPlaybackState::Playing {
            audio.pause();
        } else {
            let _ = audio.play();
        }
        true
    } else {
        false
    }
}

/// Bind the app's media-key events to an audio playback handle.
pub fn bind_audio_media_keys(app: &App, handle: &AudioHandle) {
    MediaKeyBindingBuilder::new()
        .audio(handle.clone())
        .install(app);
}

/// Bind the app's media-key events to a video controller.
pub fn bind_video_media_keys(app: &App, controller: &VideoController) {
    MediaKeyBindingBuilder::new()
        .video(controller.clone())
        .install(app);
}

impl Element for Video {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<crate::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);

        if let Some(Ok(video)) = window.use_asset::<VideoResourceLoader>(&self.source, cx) {
            style.aspect_ratio = Some(video.width() / video.height());

            if let Length::Auto = style.size.width {
                style.size.width = match style.size.height {
                    Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                        let height_px = abs_length.to_pixels(window.rem_size());
                        Length::Definite(
                            px(video.width().0 * height_px.0 / video.height().0).into(),
                        )
                    }
                    _ => Length::Definite(video.width().into()),
                };
            }

            if let Length::Auto = style.size.height {
                style.size.height = match style.size.width {
                    Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                        let width_px = abs_length.to_pixels(window.rem_size());
                        Length::Definite(px(video.height().0 * width_px.0 / video.width().0).into())
                    }
                    _ => Length::Definite(video.height().into()),
                };
            }
        }

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(Ok(video)) = window.use_asset::<VideoResourceLoader>(&self.source, cx) else {
            return;
        };

        let mut style = Style::default();
        style.refine(&self.style);
        let frame = window.with_optional_element_state(
            global_id,
            |state: Option<Option<VideoState>>, window| {
                let mut state = state.map(|state| state.unwrap_or_default());
                let mut should_animate = false;
                let frame = if let Some(state) = &mut state {
                    let (playback_state, position) = state.playback_position(
                        &self.source,
                        self.synced_audio.as_ref(),
                        self.autoplay,
                        video.duration,
                        Instant::now(),
                    );
                    should_animate = playback_state == MediaPlaybackState::Playing;
                    state
                        .frame_for_position(&self.source, position)
                        .unwrap_or_else(|error| {
                            log::error!("failed to decode buffered video frame: {}", error);
                            None
                        })
                } else {
                    None
                };

                if should_animate {
                    window.request_animation_frame();
                }

                (frame, state)
            },
        );

        let Some(frame) = frame else {
            return;
        };

        let draw_bounds = self.object_fit.get_bounds(bounds, frame.size(0));
        let corner_radii = style
            .corner_radii
            .to_pixels(window.rem_size())
            .clamp_radii_for_quad_size(draw_bounds.size);
        window
            .paint_image(draw_bounds, corner_radii, frame, 0, false)
            .log_err();
    }
}

impl IntoElement for Video {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Video {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn video_frame_delay(frames: &[VideoFrame], index: usize) -> Delay {
    let delay = frames
        .get(index + 1)
        .map(|next| next.timestamp.saturating_sub(frames[index].timestamp))
        .filter(|delay| !delay.is_zero())
        .or_else(|| {
            index
                .checked_sub(1)
                .map(|previous| {
                    frames[index]
                        .timestamp
                        .saturating_sub(frames[previous].timestamp)
                })
                .filter(|delay| !delay.is_zero())
        })
        .unwrap_or(DEFAULT_VIDEO_FRAME_DELAY);

    Delay::from_saturating_duration(delay)
}

fn decode_video_frames(
    source: impl Into<MediaSource>,
) -> Result<(VideoMetadata, Vec<VideoFrame>), MediaDecodeError> {
    let source = source.into();
    let decoder = MediaDecoder::new(source);
    let metadata = decoder.video_metadata()?;
    let video_frames = decoder.decode_video_frames()?;
    if video_frames.is_empty() {
        return Err(MediaDecodeError::Decode("no video frames decoded".into()));
    }

    Ok((metadata, video_frames))
}

fn load_video_asset(
    source: impl Into<MediaSource>,
) -> Result<Arc<BufferedVideoAsset>, MediaDecodeError> {
    let metadata = MediaDecoder::new(source).video_metadata()?;
    let duration = metadata.duration.unwrap_or(Duration::MAX);

    Ok(Arc::new(BufferedVideoAsset { metadata, duration }))
}

fn video_frame_to_render_image(frame: &VideoFrame) -> Result<Arc<RenderImage>, MediaDecodeError> {
    let buffer = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
        frame.width,
        frame.height,
        frame.data.as_ref().to_vec(),
    )
    .ok_or_else(|| MediaDecodeError::Decode("invalid video frame buffer".into()))?;
    Ok(Arc::new(RenderImage::new(SmallVec::from_elem(
        Frame::new(buffer),
        1,
    ))))
}

impl VideoState {
    fn frame_for_position(
        &mut self,
        source: &MediaSource,
        position: Duration,
    ) -> Result<Option<Arc<RenderImage>>, MediaDecodeError> {
        if self
            .buffered_video
            .as_ref()
            .is_none_or(|buffered_video| buffered_video.source() != source)
        {
            self.buffered_video = Some(BufferedVideoPlayback::new(source.clone())?);
        }

        let buffered_video = self
            .buffered_video
            .as_mut()
            .expect("buffered video initialized above");
        let frame = buffered_video.frame_for_position(position)?;
        if self.use_local_clock && buffered_video.is_finished_at(position) {
            self.local_position = buffered_video.last_timestamp().unwrap_or(position);
            self.local_started_at = None;
            self.local_state = MediaPlaybackState::Stopped;
        }

        Ok(frame)
    }

    fn playback_position(
        &mut self,
        source: &MediaSource,
        synced_audio: Option<&AudioHandle>,
        autoplay: bool,
        duration: Duration,
        now: Instant,
    ) -> (MediaPlaybackState, Duration) {
        if synced_audio.is_some() {
            self.use_local_clock = false;
        }

        if autoplay && !self.autoplay_started {
            self.start_playback(source, synced_audio, now);
        }

        if !self.use_local_clock {
            if let Some(handle) = synced_audio
                .cloned()
                .or_else(|| self.internal_audio.clone())
            {
                return (handle.state(), handle.position().min(duration));
            }
        }

        self.local_snapshot(duration, now)
    }

    fn start_playback(
        &mut self,
        source: &MediaSource,
        synced_audio: Option<&AudioHandle>,
        now: Instant,
    ) {
        let playback_handle = synced_audio.cloned().or_else(|| {
            Some(
                self.internal_audio
                    .get_or_insert_with(|| AudioHandle::new(source.clone()))
                    .clone(),
            )
        });
        let started_with_audio = playback_handle
            .as_ref()
            .is_some_and(|handle| handle.play().is_ok());

        if !started_with_audio {
            self.play_local(now);
            self.use_local_clock = true;
        } else {
            self.use_local_clock = false;
        }

        self.autoplay_started = true;
    }

    fn play_local(&mut self, now: Instant) {
        if self.local_state == MediaPlaybackState::Playing {
            return;
        }

        self.local_started_at = Some(now);
        self.local_state = MediaPlaybackState::Playing;
    }

    fn local_snapshot(
        &mut self,
        duration: Duration,
        now: Instant,
    ) -> (MediaPlaybackState, Duration) {
        let mut position = if self.local_state == MediaPlaybackState::Playing {
            self.local_position
                + self
                    .local_started_at
                    .map(|started_at| now.saturating_duration_since(started_at))
                    .unwrap_or_default()
        } else {
            self.local_position
        };

        if position >= duration && duration > Duration::ZERO {
            position = duration;
            self.local_position = duration;
            self.local_started_at = None;
            self.local_state = MediaPlaybackState::Stopped;
        }

        (self.local_state, position)
    }
}

impl BufferedVideoPlayback {
    fn new(source: MediaSource) -> Result<Self, MediaDecodeError> {
        Ok(Self {
            decoder: VideoFrameStream::new(source)?,
            frames: VecDeque::new(),
            exhausted: false,
            last_requested_position: None,
            last_seek_position: None,
            buffer_strategy: VideoBufferStrategy::default(),
        })
    }

    fn source(&self) -> &MediaSource {
        self.decoder.source()
    }

    fn frame_for_position(
        &mut self,
        position: Duration,
    ) -> Result<Option<Arc<RenderImage>>, MediaDecodeError> {
        self.update_buffer_strategy(position);

        if self
            .frames
            .front()
            .is_some_and(|frame| position < frame.timestamp)
        {
            self.decoder.seek(position)?;
            self.frames.clear();
            self.exhausted = false;
            self.last_seek_position = Some(position);
        }

        self.ensure_frame_for_position(position)?;
        self.prefetch_frames(position)?;
        self.trim_cache(position);
        self.last_requested_position = Some(position);

        Ok(self.cached_frame_for_position(position))
    }

    fn ensure_frame_for_position(&mut self, position: Duration) -> Result<(), MediaDecodeError> {
        loop {
            if self.has_frame_for_position(position) {
                return Ok(());
            }

            match self.decoder.next_frame()? {
                Some(frame) => self.push_frame(frame)?,
                None => {
                    self.exhausted = true;
                    return Ok(());
                }
            }
        }
    }

    fn prefetch_frames(&mut self, position: Duration) -> Result<(), MediaDecodeError> {
        while !self.exhausted
            && self.frames.len() < self.buffer_strategy.cache_limit
            && self.frames_after(position) < self.buffer_strategy.forward_window
        {
            match self.decoder.next_frame()? {
                Some(frame) => self.push_frame(frame)?,
                None => {
                    self.exhausted = true;
                    break;
                }
            }
        }

        Ok(())
    }

    fn has_frame_for_position(&self, position: Duration) -> bool {
        if let Some(first_frame) = self.frames.front() {
            if position < first_frame.timestamp {
                return true;
            }
        } else {
            return false;
        }

        if self.exhausted {
            return true;
        }

        self.frames
            .back()
            .is_some_and(|last_frame| last_frame.timestamp > position)
    }

    fn cached_frame_for_position(&self, position: Duration) -> Option<Arc<RenderImage>> {
        let mut selected = self.frames.front()?;
        for frame in &self.frames {
            if frame.timestamp > position {
                break;
            }
            selected = frame;
        }

        Some(selected.image.clone())
    }

    fn push_frame(&mut self, frame: VideoFrame) -> Result<(), MediaDecodeError> {
        self.frames.push_back(CachedVideoFrame {
            timestamp: frame.timestamp,
            image: video_frame_to_render_image(&frame)?,
        });

        Ok(())
    }

    fn trim_cache(&mut self, position: Duration) {
        while self.frames.len() > self.buffer_strategy.cache_limit {
            let Some(first_frame) = self.frames.front() else {
                break;
            };

            if first_frame.timestamp > position {
                break;
            }

            if self.frames_before_or_at(position) <= self.buffer_strategy.backward_window {
                break;
            }

            self.frames.pop_front();
        }
    }

    fn is_finished_at(&self, position: Duration) -> bool {
        self.exhausted
            && self
                .frames
                .back()
                .is_some_and(|last_frame| last_frame.timestamp <= position)
    }

    fn last_timestamp(&self) -> Option<Duration> {
        self.frames.back().map(|frame| frame.timestamp)
    }

    fn update_buffer_strategy(&mut self, position: Duration) {
        self.buffer_strategy = buffer_strategy_for_motion(
            self.last_requested_position,
            position,
            self.estimated_frame_interval(),
        );
    }

    fn estimated_frame_interval(&self) -> Duration {
        let mut total_nanos = 0u128;
        let mut interval_count = 0u128;

        for (current, next) in self.frames.iter().zip(self.frames.iter().skip(1)) {
            let interval = next.timestamp.saturating_sub(current.timestamp);
            if interval.is_zero() {
                continue;
            }

            total_nanos += interval.as_nanos();
            interval_count += 1;
        }

        if interval_count == 0 {
            return DEFAULT_VIDEO_FRAME_DELAY;
        }

        let average_nanos = (total_nanos / interval_count).min(u64::MAX as u128) as u64;
        Duration::from_nanos(average_nanos).max(DEFAULT_VIDEO_FRAME_DELAY / 2)
    }

    fn frames_before_or_at(&self, position: Duration) -> usize {
        self.frames
            .iter()
            .take_while(|frame| frame.timestamp <= position)
            .count()
    }

    fn frames_after(&self, position: Duration) -> usize {
        self.frames
            .iter()
            .filter(|frame| frame.timestamp > position)
            .count()
    }
}

impl Default for VideoBufferStrategy {
    fn default() -> Self {
        Self {
            backward_window: MIN_VIDEO_FRAME_RETAIN,
            forward_window: MIN_VIDEO_FRAME_PREFETCH,
            cache_limit: MIN_VIDEO_FRAME_CACHE_LIMIT,
        }
    }
}

fn buffer_strategy_for_motion(
    previous_position: Option<Duration>,
    position: Duration,
    estimated_frame_interval: Duration,
) -> VideoBufferStrategy {
    let estimated_frame_interval = if estimated_frame_interval.is_zero() {
        DEFAULT_VIDEO_FRAME_DELAY
    } else {
        estimated_frame_interval
    };
    let movement = previous_position.map(|previous_position| position.abs_diff(previous_position));
    let moved_backward =
        previous_position.is_some_and(|previous_position| position < previous_position);
    let movement_frames = movement
        .map(|movement| duration_to_frame_steps(movement, estimated_frame_interval))
        .unwrap_or(0);
    let forward_window = clamp_usize(
        MIN_VIDEO_FRAME_PREFETCH + movement_frames.saturating_mul(2),
        MIN_VIDEO_FRAME_PREFETCH,
        MAX_VIDEO_FRAME_PREFETCH,
    );
    let backward_window = clamp_usize(
        MIN_VIDEO_FRAME_RETAIN
            + if moved_backward {
                movement_frames
            } else {
                movement_frames / 2
            },
        MIN_VIDEO_FRAME_RETAIN,
        MAX_VIDEO_FRAME_RETAIN,
    );
    let cache_limit = clamp_usize(
        backward_window + forward_window + 2,
        MIN_VIDEO_FRAME_CACHE_LIMIT,
        MAX_VIDEO_FRAME_CACHE_LIMIT,
    );

    VideoBufferStrategy {
        backward_window,
        forward_window,
        cache_limit,
    }
}

fn duration_to_frame_steps(duration: Duration, frame_interval: Duration) -> usize {
    let frame_interval_nanos = frame_interval.as_nanos();
    if frame_interval_nanos == 0 {
        return 0;
    }

    ((duration.as_nanos() + frame_interval_nanos.saturating_sub(1)) / frame_interval_nanos)
        .min(usize::MAX as u128) as usize
}

fn clamp_usize(value: usize, min: usize, max: usize) -> usize {
    value.max(min).min(max)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_VIDEO_FRAME_DELAY, MAX_VIDEO_FRAME_CACHE_LIMIT, MIN_VIDEO_FRAME_CACHE_LIMIT,
        MIN_VIDEO_FRAME_PREFETCH, MediaKeyBindingBuilder, MediaSourceBuilder, TextTrack,
        TextTrackCue, TimeRange, VideoCanPlay, VideoCapabilityStatus, VideoController, VideoEvent,
        VideoPlaybackPlanBuilder, VideoPlaybackPlanTarget, VideoPlaylist, VideoReadyState,
        WebViewVideoCommand, WebViewVideoCrossOrigin, WebViewVideoOptions, WebViewVideoPreload,
        WebViewVideoTextTrack, buffer_strategy_for_motion, buffered_ranges_for_source,
        can_play_video_source, can_play_video_type, parse_text_track_timestamp,
        push_ready_state_events, recommended_video_playback_route,
        recommended_video_playback_route_for_type, video_capability_report, video_frame_delay,
        webview_video_player_command_script, webview_video_player_id, webview_video_player_url,
    };
    use kael_media::{MediaSource, PlaybackState, VideoFrame};
    use std::{io::Cursor, sync::Arc, time::Duration};

    #[test]
    fn final_frame_delay_falls_back_to_previous_gap() {
        let frames = vec![
            VideoFrame {
                data: Arc::<[u8]>::from(vec![0; 4]),
                width: 1,
                height: 1,
                timestamp: Duration::ZERO,
            },
            VideoFrame {
                data: Arc::<[u8]>::from(vec![0; 4]),
                width: 1,
                height: 1,
                timestamp: Duration::from_millis(120),
            },
        ];

        assert_eq!(
            video_frame_delay(&frames, 1),
            image::Delay::from_saturating_duration(Duration::from_millis(120))
        );
        assert_eq!(
            video_frame_delay(&frames[..1], 0),
            image::Delay::from_saturating_duration(DEFAULT_VIDEO_FRAME_DELAY)
        );
    }

    #[test]
    fn buffer_strategy_grows_with_seek_distance() {
        let steady = buffer_strategy_for_motion(
            Some(Duration::from_millis(100)),
            Duration::from_millis(133),
            DEFAULT_VIDEO_FRAME_DELAY,
        );
        let seek = buffer_strategy_for_motion(
            Some(Duration::from_millis(100)),
            Duration::from_secs(4),
            DEFAULT_VIDEO_FRAME_DELAY,
        );

        assert!(seek.forward_window > steady.forward_window);
        assert!(seek.cache_limit > steady.cache_limit);
    }

    #[test]
    fn buffer_strategy_stays_within_configured_bounds() {
        let strategy = buffer_strategy_for_motion(
            Some(Duration::ZERO),
            Duration::from_secs(30),
            DEFAULT_VIDEO_FRAME_DELAY,
        );

        assert!(strategy.cache_limit >= MIN_VIDEO_FRAME_CACHE_LIMIT);
        assert!(strategy.cache_limit <= MAX_VIDEO_FRAME_CACHE_LIMIT);
        assert!(strategy.forward_window >= MIN_VIDEO_FRAME_PREFETCH);
    }

    #[test]
    fn local_sources_report_known_buffered_range() {
        let duration = Duration::from_secs(12);
        let full_range = vec![TimeRange::new(Duration::ZERO, duration)];

        assert_eq!(
            buffered_ranges_for_source(
                &MediaSource::bytes(Arc::<[u8]>::from([1, 2, 3])),
                Some(duration)
            ),
            full_range
        );
        assert_eq!(
            buffered_ranges_for_source(&MediaSource::file("movie.mp4"), Some(duration)),
            full_range
        );
        assert_eq!(
            buffered_ranges_for_source(
                &MediaSource::url("https://example.com/movie.mp4"),
                Some(duration)
            ),
            Vec::<TimeRange>::new()
        );

        let range = TimeRange::new(Duration::from_secs(5), Duration::from_secs(3));
        assert_eq!(range.duration(), Duration::ZERO);
        assert!(!range.contains(Duration::from_secs(5)));
    }

    #[test]
    fn playback_route_recommends_webview_for_streaming_manifests() {
        assert!(
            recommended_video_playback_route(&MediaSource::url(
                "https://example.com/live/master.m3u8?token=abc"
            ))
            .should_use_webview()
        );
        assert!(
            recommended_video_playback_route(&MediaSource::file("movie.mpd")).should_use_webview()
        );
        assert!(
            recommended_video_playback_route(&MediaSource::url("https://example.com/movie.mp4"))
                .is_native()
        );
        assert!(
            VideoController::url("https://example.com/movie.mp4")
                .recommended_route()
                .is_native()
        );
    }

    #[test]
    fn playback_route_can_be_recommended_from_mime_type() {
        assert!(
            recommended_video_playback_route_for_type(
                "application/vnd.apple.mpegurl; charset=utf-8"
            )
            .should_use_webview()
        );
        assert!(
            recommended_video_playback_route_for_type("APPLICATION/DASH+XML").should_use_webview()
        );
        assert!(
            recommended_video_playback_route_for_type("application/vnd.ms-sstr+xml")
                .should_use_webview()
        );
        assert!(recommended_video_playback_route_for_type("video/mp4").is_native());
        assert!(
            recommended_video_playback_route(&MediaSource::url(
                "https://example.com/playback?id=stream"
            ))
            .is_native()
        );
    }

    #[test]
    fn can_play_video_type_and_source_report_native_confidence() {
        assert_eq!(
            can_play_video_type("video/mp4; codecs=\"avc1.42E01E\""),
            VideoCanPlay::Probably
        );
        assert_eq!(
            can_play_video_type("application/vnd.apple.mpegurl"),
            VideoCanPlay::No
        );
        assert_eq!(
            can_play_video_type("APPLICATION/DASH+XML"),
            VideoCanPlay::No
        );
        assert_eq!(
            can_play_video_type("application/vnd.ms-sstr+xml"),
            VideoCanPlay::No
        );
        assert_eq!(
            can_play_video_source(&MediaSource::url("https://example.com/movie.webm#fragment")),
            VideoCanPlay::Probably
        );
        assert_eq!(
            can_play_video_source(&MediaSource::file("playlist.m3u8")),
            VideoCanPlay::No
        );
        assert_eq!(
            can_play_video_source(&MediaSource::bytes(Arc::<[u8]>::from([1, 2, 3]))),
            VideoCanPlay::Maybe
        );
        assert_eq!(VideoCanPlay::Maybe.as_can_play_type(), "maybe");
        assert_eq!(
            VideoController::file("movie.mp4").can_play_source(),
            VideoCanPlay::Probably
        );
    }

    #[test]
    fn video_playback_plan_builds_native_url_player() {
        let plan = VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
            .webview_options(WebViewVideoOptions::default().controls(false))
            .build_checked()
            .unwrap();

        assert!(plan.target().is_native());
        assert!(plan.route().is_native());
        assert_eq!(plan.can_play(), VideoCanPlay::Probably);
        assert_eq!(plan.content_type(), None);
        assert!(!plan.webview_options().controls);
        assert!(plan.webview_page_url().is_none());
        assert!(matches!(plan.source(), MediaSource::Url(_)));
        assert!(matches!(plan.controller().source(), MediaSource::Url(_)));
    }

    #[test]
    fn video_playback_plan_routes_adaptive_streams_to_webview() {
        let plan = VideoPlaybackPlanBuilder::url("https://cdn.example.com/live?id=123")
            .content_type("application/vnd.apple.mpegurl; charset=utf-8")
            .webview_options(
                WebViewVideoOptions::default()
                    .autoplay(true)
                    .muted(true)
                    .object_fit("cover"),
            )
            .build_checked()
            .unwrap();

        assert!(plan.target().is_webview_fallback());
        assert!(plan.route().should_use_webview());
        assert_eq!(plan.can_play(), VideoCanPlay::No);
        assert_eq!(
            plan.content_type(),
            Some("application/vnd.apple.mpegurl; charset=utf-8")
        );
        assert!(
            plan.webview_page_url()
                .unwrap()
                .starts_with("data:text/html")
        );
        assert!(
            plan.webview_element_id()
                .unwrap()
                .starts_with("kael-video-player-")
        );
        match plan.target() {
            VideoPlaybackPlanTarget::WebViewFallback { reason, .. } => {
                assert!(reason.contains("HLS"));
            }
            VideoPlaybackPlanTarget::Native => panic!("expected WebView fallback target"),
        }
    }

    #[test]
    fn video_playback_plan_rejects_generated_footguns() {
        assert!(
            VideoPlaybackPlanBuilder::url(" https://cdn.example.com/movie.mp4")
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
                .content_type(" video/mp4")
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
                .content_type("video mp4")
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackPlanBuilder::bytes(Arc::<[u8]>::from([1, 2, 3]))
                .prefer_webview()
                .build_checked()
                .is_err()
        );
        assert!(
            VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
                .webview_options(WebViewVideoOptions::default().object_fit("cover;position:fixed"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn webview_video_player_url_wraps_url_sources() {
        let source = MediaSource::url("https://cdn.example.com/live/master.m3u8?token=a b");
        let url = webview_video_player_url(
            &source,
            &WebViewVideoOptions::default()
                .autoplay(true)
                .muted(true)
                .looping(true)
                .object_fit("cover"),
        )
        .unwrap();

        assert!(url.starts_with("data:text/html;charset=utf-8,"));
        assert!(url.contains("master.m3u8"));
        assert!(url.contains("token%3Da%20b"));
        assert!(url.contains("autoplay"));
        assert!(url.contains("muted"));
        assert!(url.contains("loop"));
        assert!(url.contains("object-fit%3Acover"));
    }

    #[test]
    fn webview_video_player_url_supports_browser_video_options() {
        let source = MediaSource::url("https://cdn.example.com/movie.mp4");
        let url = webview_video_player_url(
            &source,
            &WebViewVideoOptions::default()
                .poster("https://cdn.example.com/poster.jpg")
                .preload(WebViewVideoPreload::Metadata)
                .cross_origin(WebViewVideoCrossOrigin::Anonymous)
                .controls_list(["nodownload", "nofullscreen"])
                .disable_picture_in_picture(true)
                .start_at(Duration::from_secs(42))
                .text_track(
                    WebViewVideoTextTrack::webvtt(
                        "English",
                        Some("en"),
                        "https://cdn.example.com/captions.vtt",
                    )
                    .default(true),
                )
                .webvtt_text_track(
                    "Inline",
                    Some("en-inline"),
                    "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello",
                ),
        )
        .unwrap();

        assert!(url.contains("poster%3D%22https%3A%2F%2Fcdn.example.com%2Fposter.jpg%22"));
        assert!(url.contains("preload%3D%22metadata%22"));
        assert!(url.contains("crossorigin%3D%22anonymous%22"));
        assert!(url.contains("controlslist%3D%22nodownload%20nofullscreen%22"));
        assert!(url.contains("disablePictureInPicture"));
        assert!(url.contains("currentTime%3D42"));
        assert!(url.contains("kind%3D%22subtitles%22"));
        assert!(url.contains("label%3D%22English%22"));
        assert!(url.contains("srclang%3D%22en%22"));
        assert!(url.contains("captions.vtt"));
        assert!(url.contains("default%3E"));
        assert!(url.contains("data%3Atext%2Fvtt%3Bcharset%3Dutf-8"));
        assert!(url.contains("WEBVTT%250A%250A00%253A00%253A00.000"));
        assert!(url.contains("gpui.postMessage"));
        assert!(url.contains("kael-video-event"));
        assert!(url.contains("loadedmetadata"));
        assert!(url.contains("canplaythrough"));
        assert!(url.contains("volumechange"));
        assert!(url.contains("selectedTextTrack"));
        assert!(url.contains("activeCues"));
        assert!(url.contains("texttrackchange"));
        assert!(url.contains("cuechange"));
        assert!(url.contains("fullscreenchange"));
        assert!(url.contains("enterpictureinpicture"));
        assert!(url.contains("leavepictureinpicture"));
    }

    #[test]
    fn checked_media_source_builder_validates_agent_generated_inputs() {
        let source = MediaSourceBuilder::url("https://cdn.example.com/movie.mp4")
            .build_checked()
            .unwrap();
        assert!(matches!(source, MediaSource::Url(_)));

        assert!(
            MediaSourceBuilder::url(" https://cdn.example.com/movie.mp4")
                .validate()
                .is_err()
        );
        assert!(
            MediaSourceBuilder::url("ftp://cdn.example.com/movie.mp4")
                .validate()
                .is_err()
        );
        assert!(
            MediaSourceBuilder::file("")
                .require_existing_file()
                .validate()
                .is_err()
        );
        assert!(
            MediaSourceBuilder::file("/definitely/not/a/movie.mp4")
                .require_existing_file()
                .validate()
                .is_err()
        );
        assert!(
            MediaSourceBuilder::bytes(Arc::<[u8]>::from([]))
                .validate()
                .is_err()
        );
        assert!(
            MediaSourceBuilder::reader("", || Ok(Cursor::new(Vec::<u8>::new())))
                .validate()
                .is_err()
        );

        let controller = MediaSourceBuilder::bytes(Arc::<[u8]>::from([1, 2, 3]))
            .controller_checked()
            .unwrap();
        assert!(matches!(controller.source(), MediaSource::Bytes(_)));
    }

    #[test]
    fn webview_video_options_validate_before_embedding() {
        let options = WebViewVideoOptions::default()
            .poster("https://cdn.example.com/poster.jpg")
            .controls_list(["nodownload", "nofullscreen"])
            .object_fit("cover")
            .text_track(
                WebViewVideoTextTrack::webvtt(
                    "English",
                    Some("en"),
                    "https://cdn.example.com/captions.vtt",
                )
                .default(true),
            )
            .webvtt_text_track(
                "Inline",
                Some("en-inline"),
                "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello",
            );

        assert!(options.validate().is_ok());
        assert!(options.checked().is_ok());
        assert!(
            WebViewVideoOptions::default()
                .poster("javascript:alert(1)")
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoOptions::default()
                .controls_list(["no download"])
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoOptions::default()
                .object_fit("cover;position:absolute")
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoOptions::default()
                .text_track(WebViewVideoTextTrack::webvtt(
                    "",
                    Some("en"),
                    "https://cdn.example.com/captions.vtt",
                ))
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoOptions::default()
                .text_track(WebViewVideoTextTrack::webvtt(
                    "English",
                    Some("en"),
                    "javascript:alert(1)",
                ))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn webview_video_player_helpers_expose_control_target_and_commands() {
        let source = MediaSource::url("https://cdn.example.com/movie.mp4");
        assert_eq!(
            webview_video_player_id(&source),
            webview_video_player_id(&source)
        );
        assert_ne!(
            webview_video_player_id(&source),
            webview_video_player_id(&MediaSource::url("https://cdn.example.com/other.mp4"))
        );
        assert_eq!(
            VideoController::new(source.clone()).webview_player_id(),
            webview_video_player_id(&source)
        );

        let play = webview_video_player_command_script(WebViewVideoCommand::Play);
        assert!(play.contains("document.querySelector"));
        assert!(play.contains("video.play"));
        assert!(
            VideoController::new(source)
                .webview_command_script(WebViewVideoCommand::Pause)
                .contains("video.pause")
        );

        let seek = webview_video_player_command_script(WebViewVideoCommand::FastSeek(
            Duration::from_secs(12),
        ));
        assert!(seek.contains("fastSeek"));
        assert!(seek.contains("12"));

        let volume = webview_video_player_command_script(WebViewVideoCommand::SetVolume(2.0));
        assert!(volume.contains("video.volume=1"));

        let captions = webview_video_player_command_script(WebViewVideoCommand::SelectTextTrack(
            "English \"CC\"".into(),
        ));
        assert!(captions.contains("const selector=\"English \\\"CC\\\"\""));
        assert!(captions.contains("video.textTracks.length"));
        assert!(captions.contains("track.id===selector"));
        assert!(captions.contains("track.label===selector"));
        assert!(captions.contains("track.language===selector"));
        assert!(captions.contains("String(i)===selector"));
        assert!(captions.contains("track.mode=matches?\"showing\":\"disabled\""));
        assert!(captions.contains("post(\"texttrackchange\")"));
        assert!(captions.contains("post(\"cuechange\""));
        assert!(captions.contains("activeCues()"));

        let disable_captions =
            webview_video_player_command_script(WebViewVideoCommand::DisableTextTracks);
        assert!(disable_captions.contains("video.textTracks.length"));
        assert!(disable_captions.contains("mode=\"disabled\""));
        assert!(disable_captions.contains("post(\"texttrackchange\")"));
        assert!(disable_captions.contains("activeCues()"));

        let request_fullscreen =
            webview_video_player_command_script(WebViewVideoCommand::RequestFullscreen);
        assert!(request_fullscreen.contains("video.requestFullscreen"));
        assert!(request_fullscreen.contains("video.webkitRequestFullscreen"));
        assert!(request_fullscreen.contains("result.catch(()=>{})"));

        let exit_fullscreen =
            webview_video_player_command_script(WebViewVideoCommand::ExitFullscreen);
        assert!(exit_fullscreen.contains("document.exitFullscreen"));
        assert!(exit_fullscreen.contains("document.webkitExitFullscreen"));

        let request_pip =
            webview_video_player_command_script(WebViewVideoCommand::RequestPictureInPicture);
        assert!(request_pip.contains("video.requestPictureInPicture"));
        assert!(request_pip.contains("result.catch(()=>{})"));

        let exit_pip =
            webview_video_player_command_script(WebViewVideoCommand::ExitPictureInPicture);
        assert!(exit_pip.contains("document.pictureInPictureElement"));
        assert!(exit_pip.contains("document.exitPictureInPicture"));

        let snapshot = webview_video_player_command_script(WebViewVideoCommand::RequestSnapshot);
        assert!(snapshot.contains("kael-video-event"));
        assert!(snapshot.contains("snapshot"));
        assert!(snapshot.contains("buffered"));
        assert!(snapshot.contains("selectedTextTrack"));
        assert!(snapshot.contains("pictureInPicture"));
        assert!(snapshot.contains("fullscreen"));
    }

    #[test]
    fn webview_video_player_url_handles_file_sources_and_rejects_memory_sources() {
        let url = webview_video_player_url(
            &MediaSource::file("/tmp/movie file.mp4"),
            &WebViewVideoOptions::default(),
        )
        .unwrap();

        assert!(url.contains("file%3A%2F%2F%2Ftmp%2Fmovie%2520file.mp4"));
        assert!(
            webview_video_player_url(
                &MediaSource::bytes(Arc::<[u8]>::from([1, 2, 3])),
                &WebViewVideoOptions::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn video_capability_report_is_honest_about_backend_gaps() {
        let report = video_capability_report();

        assert!(!report.is_full());
        assert_eq!(report.source_types, VideoCapabilityStatus::Full);
        assert_eq!(report.controller, VideoCapabilityStatus::Full);
        assert_eq!(report.source_replacement, VideoCapabilityStatus::Full);
        assert_eq!(report.can_play_type, VideoCapabilityStatus::Full);
        assert_eq!(report.route_recommendation, VideoCapabilityStatus::Full);
        assert_eq!(report.webview_fallback, VideoCapabilityStatus::Full);
        assert_eq!(report.text_tracks, VideoCapabilityStatus::Full);
        assert_eq!(report.fast_seek, VideoCapabilityStatus::Partial);
        assert_eq!(report.playback_rate, VideoCapabilityStatus::Partial);
        assert_eq!(report.fullscreen, VideoCapabilityStatus::Full);
        assert_eq!(
            report.native_adaptive_streaming,
            VideoCapabilityStatus::Roadmap
        );
        assert_eq!(report.hardware_decode, VideoCapabilityStatus::Roadmap);
        assert_eq!(
            report.native_track_selection,
            VideoCapabilityStatus::Roadmap
        );
    }

    #[test]
    fn ready_state_events_are_emitted_once() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));
        {
            let mut state = controller.state.borrow_mut();
            push_ready_state_events(&mut state, VideoReadyState::CurrentData);
            push_ready_state_events(&mut state, VideoReadyState::EnoughData);
            push_ready_state_events(&mut state, VideoReadyState::EnoughData);
        }

        assert_eq!(
            controller.drain_events(),
            vec![
                VideoEvent::ReadyStateChange {
                    ready_state: VideoReadyState::CurrentData,
                },
                VideoEvent::CanPlay,
                VideoEvent::ReadyStateChange {
                    ready_state: VideoReadyState::EnoughData,
                },
                VideoEvent::CanPlayThrough,
            ]
        );
    }

    #[test]
    fn video_controller_source_constructors_match_media_sources() {
        assert!(matches!(
            VideoController::url("https://example.com/movie.mp4").source(),
            MediaSource::Url(_)
        ));
        assert!(matches!(
            VideoController::file("movie.mp4").source(),
            MediaSource::File(_)
        ));
        assert!(matches!(
            VideoController::bytes(Arc::<[u8]>::from([1, 2, 3])).source(),
            MediaSource::Bytes(_)
        ));
    }

    #[test]
    fn video_playlist_advances_sources_and_respects_repeat() {
        let mut playlist = VideoPlaylist::new([
            MediaSource::url("https://example.com/one.mp4"),
            MediaSource::url("https://example.com/two.mp4"),
            MediaSource::url("https://example.com/three.mp4"),
        ]);

        assert_eq!(playlist.current_index(), Some(0));
        assert!(matches!(
            playlist.current_source(),
            Some(MediaSource::Url(url)) if url.as_ref() == "https://example.com/one.mp4"
        ));
        assert!(matches!(
            playlist.next_source(),
            Some(MediaSource::Url(url)) if url.as_ref() == "https://example.com/two.mp4"
        ));
        assert_eq!(playlist.current_index(), Some(1));
        assert!(matches!(
            playlist.previous_source(),
            Some(MediaSource::Url(url)) if url.as_ref() == "https://example.com/one.mp4"
        ));
        assert!(playlist.previous_source().is_none());
        assert_eq!(playlist.current_index(), Some(0));

        let mut repeating = VideoPlaylist::new([
            MediaSource::url("https://example.com/one.mp4"),
            MediaSource::url("https://example.com/two.mp4"),
        ])
        .start_at(1)
        .repeat(true);

        assert!(matches!(
            repeating.next_source(),
            Some(MediaSource::Url(url)) if url.as_ref() == "https://example.com/one.mp4"
        ));
        assert_eq!(repeating.current_index(), Some(0));
        assert!(matches!(
            repeating.previous_source(),
            Some(MediaSource::Url(url)) if url.as_ref() == "https://example.com/two.mp4"
        ));
        assert_eq!(repeating.current_index(), Some(1));
    }

    #[test]
    fn video_playlist_validates_generated_sources() {
        assert!(VideoPlaylist::empty().validate().is_err());

        assert!(
            VideoPlaylist::new([MediaSource::url("https://example.com/one.mp4")])
                .checked()
                .is_ok()
        );
        assert!(
            VideoPlaylist::new([MediaSource::url("   ")])
                .validate()
                .is_err()
        );

        let playlist =
            VideoPlaylist::new([MediaSource::url("https://example.com/one.mp4")]).repeat(true);
        assert_eq!(playlist.len(), 1);
        assert!(!playlist.is_empty());
        assert!(playlist.repeat_enabled());
    }

    #[test]
    fn media_key_binding_validates_playlist_routing() {
        assert!(MediaKeyBindingBuilder::new().validate().is_err());
        assert!(
            MediaKeyBindingBuilder::new()
                .playlist(VideoPlaylist::new([MediaSource::url(
                    "https://example.com/one.mp4"
                )]))
                .validate()
                .is_err()
        );

        let controller = VideoController::url("https://example.com/one.mp4");
        assert!(
            MediaKeyBindingBuilder::new()
                .video(controller)
                .playlist(VideoPlaylist::new([MediaSource::url(
                    "https://example.com/two.mp4"
                )]))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn video_controller_tracks_volume_muting_and_events() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));

        controller.set_volume(0.4);
        controller.set_muted(true);
        controller.set_playback_rate(1.5);
        controller.set_looping(true);

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.volume, 0.4);
        assert!(snapshot.muted);
        assert_eq!(snapshot.playback_rate, 1.5);
        assert_eq!(controller.audio_handle().speed(), 1.5);
        assert!(snapshot.looping);
        assert_eq!(snapshot.playback_state, PlaybackState::Stopped);
        assert_eq!(snapshot.ready_state, VideoReadyState::Nothing);

        assert_eq!(
            controller.drain_events(),
            vec![
                VideoEvent::VolumeChange {
                    volume: 0.4,
                    muted: false,
                },
                VideoEvent::VolumeChange {
                    volume: 0.4,
                    muted: true,
                },
                VideoEvent::RateChange { playback_rate: 1.5 },
                VideoEvent::LoopChange { looping: true },
            ]
        );
    }

    #[test]
    fn video_controller_can_replace_source_at_runtime() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([1, 2, 3]))
            .volume(0.4)
            .muted(true)
            .playback_rate(1.5)
            .looping(true)
            .srt_text_track(
                "en",
                "English",
                Some("en"),
                "1\n00:00:01,000 --> 00:00:04,000\nHello world\n",
            )
            .selected_text_track("en");

        controller.drain_events();
        controller.set_bytes(Arc::<[u8]>::from([4, 5, 6]));

        let MediaSource::Bytes(bytes) = controller.source() else {
            panic!("expected bytes source");
        };
        assert_eq!(bytes.as_ref(), &[4, 5, 6]);
        assert_eq!(controller.audio_handle().source(), controller.source());
        assert_eq!(controller.metadata(), None);
        assert_eq!(controller.duration(), None);
        assert_eq!(controller.current_time(), Duration::ZERO);
        assert_eq!(controller.ready_state(), VideoReadyState::Nothing);
        assert_eq!(controller.buffered_ranges(), Vec::<TimeRange>::new());
        assert_eq!(controller.volume_level(), 0.4);
        assert!(controller.is_muted());
        assert_eq!(controller.playback_rate_value(), 1.5);
        assert_eq!(controller.audio_handle().speed(), 1.5);
        assert!(controller.is_looping());
        assert_eq!(controller.text_tracks().len(), 1);
        assert_eq!(
            controller.active_text_track().map(|track| track.label),
            Some("English".into())
        );
        assert_eq!(
            controller.drain_events(),
            vec![
                VideoEvent::SourceChanged {
                    source: MediaSource::bytes(Arc::<[u8]>::from([4, 5, 6])),
                },
                VideoEvent::ReadyStateChange {
                    ready_state: VideoReadyState::Nothing,
                },
                VideoEvent::Progress {
                    buffered_ranges: Vec::new(),
                },
                VideoEvent::TimeUpdate {
                    current_time: Duration::ZERO,
                },
            ]
        );
    }

    #[test]
    fn video_controller_fluent_setup_configures_state_and_tracks() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]))
            .volume(0.6)
            .muted(true)
            .playback_rate(1.25)
            .looping(true)
            .srt_text_track(
                "en",
                "English",
                Some("en"),
                "1\n00:00:01,000 --> 00:00:04,000\nHello world\n",
            )
            .selected_text_track("en");

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.volume, 0.6);
        assert!(snapshot.muted);
        assert_eq!(snapshot.playback_rate, 1.25);
        assert!(snapshot.looping);
        assert_eq!(controller.playback_state(), PlaybackState::Stopped);
        assert!(!controller.is_playing());
        assert!(!controller.is_paused());
        assert!(!controller.paused());
        assert_eq!(controller.current_time(), Duration::ZERO);
        assert_eq!(controller.current_time_secs(), 0.0);
        assert_eq!(controller.duration(), None);
        assert_eq!(controller.duration_secs(), None);
        assert_eq!(controller.metadata(), None);
        assert_eq!(controller.ready_state(), VideoReadyState::Nothing);
        assert_eq!(controller.buffered_ranges(), Vec::<TimeRange>::new());
        assert_eq!(controller.volume_level(), 0.6);
        assert!(controller.is_muted());
        assert!(controller.muted_state());
        assert_eq!(controller.playback_rate_value(), 1.25);
        assert_eq!(controller.rate(), 1.25);
        assert!(controller.is_looping());
        assert!(controller.looping_enabled());
        assert_eq!(controller.error(), None);
        assert!(controller.set_current_time_secs(0.25).is_err());
        assert!(controller.fast_seek(Duration::from_millis(250)).is_err());
        assert!(controller.fast_seek_secs(0.25).is_err());
        assert_eq!(controller.current_time(), Duration::ZERO);
        assert_eq!(controller.current_time_secs(), 0.0);
        assert_eq!(controller.text_tracks().len(), 1);
        assert_eq!(
            controller
                .selected_text_track_id()
                .map(|id| id.to_string())
                .as_deref(),
            Some("en")
        );
        assert_eq!(
            controller.active_text_track().map(|track| track.label),
            Some("English".into())
        );
        assert_eq!(
            controller.active_text_cues_at(Duration::from_millis(1_500)),
            vec![TextTrackCue::new(
                Duration::from_millis(1_000),
                Duration::from_millis(4_000),
                "Hello world"
            )]
        );
        controller.disable_text_track();
        assert_eq!(controller.selected_text_track_id(), None);
        assert_eq!(controller.active_text_track(), None);
    }

    #[test]
    fn video_controller_emits_error_when_metadata_load_fails() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));

        assert!(controller.load_metadata().is_err());
        assert!(controller.snapshot().error.is_some());
        assert!(
            controller
                .drain_events()
                .iter()
                .any(|event| matches!(event, VideoEvent::Error(_)))
        );
    }

    #[test]
    fn text_track_parses_srt_and_webvtt_cues() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,500 --> 00:00:08,000\nSecond\nline\n";
        let srt_track = TextTrack::from_srt("en", "English", Some("en"), srt);
        assert_eq!(srt_track.cues().len(), 2);
        assert_eq!(
            srt_track.active_cues(Duration::from_millis(2_000)),
            vec![TextTrackCue::new(
                Duration::from_millis(1_000),
                Duration::from_millis(4_000),
                "Hello world"
            )]
        );

        let vtt = "WEBVTT\n\nNOTE ignored\n\n1\n00:00:01.000 --> 00:00:04.000 position:50%\nHello world\n\n00:05.500 --> 00:08.000\nSecond\nline\n";
        let vtt_track = TextTrack::from_webvtt("en", "English", Some("en"), vtt);
        assert_eq!(vtt_track.cues(), srt_track.cues());
        assert_eq!(
            parse_text_track_timestamp("01:02:03.004"),
            Some(Duration::from_millis(3_723_004))
        );
        assert!(parse_text_track_timestamp("00:60:00.000").is_none());
    }

    #[test]
    fn video_controller_reports_active_text_cues() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));
        controller.add_srt_text_track(
            "en",
            "English",
            Some("en"),
            "1\n00:00:01,000 --> 00:00:04,000\nHello world\n",
        );

        assert_eq!(
            controller.active_text_cues_at(Duration::from_millis(1_500)),
            vec![TextTrackCue::new(
                Duration::from_millis(1_000),
                Duration::from_millis(4_000),
                "Hello world"
            )]
        );
        assert_eq!(
            controller.active_text_cues_at(Duration::from_millis(4_000)),
            Vec::<TextTrackCue>::new()
        );

        let events = controller.drain_events();
        assert!(events.iter().any(
            |event| matches!(event, VideoEvent::TextTrackAdded { id } if id.as_ref() == "en")
        ));
        assert!(events.iter().any(
            |event| matches!(event, VideoEvent::TextTrackChanged { id: Some(id) } if id.as_ref() == "en")
        ));
    }
}
