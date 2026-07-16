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

    /// Return a compact route label for logs, docs, and agent traces.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::WebViewRecommended { .. } => "webview recommended",
        }
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
    /// Return the browser media capability string used by media can-play checks.
    pub fn as_can_play_type(&self) -> &'static str {
        match self {
            Self::No => "",
            Self::Maybe => "maybe",
            Self::Probably => "probably",
        }
    }

    /// Return a stable label for summaries.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::No => "no",
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

impl VideoCapabilityStatus {
    /// Return a stable label for capability reports.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::Roadmap => "roadmap",
        }
    }
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

    /// Count fields with the requested capability status.
    pub fn count_status(&self, status: VideoCapabilityStatus) -> usize {
        self.statuses()
            .into_iter()
            .filter(|candidate| *candidate == status)
            .count()
    }

    /// Count fully implemented capability fields.
    pub fn full_count(&self) -> usize {
        self.count_status(VideoCapabilityStatus::Full)
    }

    /// Count partially implemented capability fields.
    pub fn partial_count(&self) -> usize {
        self.count_status(VideoCapabilityStatus::Partial)
    }

    /// Count roadmap capability fields.
    pub fn roadmap_count(&self) -> usize {
        self.count_status(VideoCapabilityStatus::Roadmap)
    }

    /// Count fields that are not fully implemented in the native media path.
    pub fn native_gap_count(&self) -> usize {
        self.partial_count() + self.roadmap_count()
    }

    /// Return whether any native media capability is partial or roadmap.
    pub fn has_native_gaps(&self) -> bool {
        self.native_gap_count() > 0
    }

    /// Return whether Kael has a browser-video fallback path available.
    pub fn has_webview_fallback(&self) -> bool {
        self.webview_fallback != VideoCapabilityStatus::Roadmap
    }

    /// Return a content-safe summary for dashboards and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video capabilities: full {full}, partial {partial}, roadmap {roadmap}, all full {}",
            self.is_full(),
            full = self.full_count(),
            partial = self.partial_count(),
            roadmap = self.roadmap_count(),
        )
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

/// Playback affordance that an app or generated player may require.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPlaybackRequirement {
    /// Load and play one checked URL/file/bytes/reader source.
    BasicPlayback,
    /// Replace the source at runtime, like assigning `video.src`.
    SourceReplacement,
    /// Probe source or MIME support before rendering.
    CanPlayProbe,
    /// Add, render, and switch SRT/WebVTT text tracks.
    TextTracks,
    /// Seek quickly from custom controls or keyboard shortcuts.
    FastSeek,
    /// Change playback speed.
    PlaybackRate,
    /// Present the player fullscreen.
    Fullscreen,
    /// Use browser picture-in-picture controls.
    PictureInPicture,
    /// Play HLS/DASH/adaptive manifests through the planned route.
    AdaptiveStreaming,
    /// Use native platform hardware decode.
    HardwareDecode,
    /// Select native audio/video streams.
    NativeTrackSelection,
}

impl VideoPlaybackRequirement {
    /// Every known video playback affordance in stable priority order.
    pub fn all() -> &'static [Self] {
        &[
            Self::BasicPlayback,
            Self::SourceReplacement,
            Self::CanPlayProbe,
            Self::TextTracks,
            Self::FastSeek,
            Self::PlaybackRate,
            Self::Fullscreen,
            Self::PictureInPicture,
            Self::AdaptiveStreaming,
            Self::HardwareDecode,
            Self::NativeTrackSelection,
        ]
    }

    /// Requirements expected from the easy "URL in, player out" path.
    ///
    /// This intentionally excludes browser-only or backend-heavy affordances so
    /// generated media players can distinguish the default player promise from
    /// advanced platform work.
    pub fn url_player_baseline() -> &'static [Self] {
        &[
            Self::BasicPlayback,
            Self::SourceReplacement,
            Self::CanPlayProbe,
            Self::TextTracks,
            Self::FastSeek,
            Self::PlaybackRate,
            Self::Fullscreen,
        ]
    }

    /// Stable requirement label for logs, tests, and generated checklists.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::BasicPlayback => "basic playback",
            Self::SourceReplacement => "source replacement",
            Self::CanPlayProbe => "can-play probe",
            Self::TextTracks => "text tracks",
            Self::FastSeek => "fast seek",
            Self::PlaybackRate => "playback rate",
            Self::Fullscreen => "fullscreen",
            Self::PictureInPicture => "picture-in-picture",
            Self::AdaptiveStreaming => "adaptive streaming",
            Self::HardwareDecode => "hardware decode",
            Self::NativeTrackSelection => "native track selection",
        }
    }
}

/// Planned support state for one requested video affordance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPlaybackRequirementStatus {
    /// The selected route covers this requirement.
    Satisfied,
    /// The selected route exposes this requirement with documented limits.
    Limited,
    /// The selected route does not currently cover this requirement.
    Missing,
}

impl VideoPlaybackRequirementStatus {
    /// Stable status label for summaries and generated checklists.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Limited => "limited",
            Self::Missing => "missing",
        }
    }
}

/// Next builder action for a requested video playback affordance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPlaybackRequirementNextAction {
    /// Render with the checked plan as-is.
    RenderPlannedRoute,
    /// Render with documented limits or expose the limitation in UI.
    AcceptLimitedSupport,
    /// Rebuild or render the source through the checked WebView video fallback.
    UseWebViewFallback,
    /// Product/backend work is required before this affordance can be promised.
    BuildNativeBackend,
}

impl VideoPlaybackRequirementNextAction {
    /// Stable action label for logs, setup screens, and generated agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::RenderPlannedRoute => "render planned route",
            Self::AcceptLimitedSupport => "accept limited support",
            Self::UseWebViewFallback => "use webview fallback",
            Self::BuildNativeBackend => "build native backend",
        }
    }
}

/// One requirement evaluated against a checked playback plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoPlaybackRequirementFinding {
    requirement: VideoPlaybackRequirement,
    status: VideoPlaybackRequirementStatus,
}

impl VideoPlaybackRequirementFinding {
    /// Requested playback affordance.
    pub fn requirement(&self) -> VideoPlaybackRequirement {
        self.requirement
    }

    /// Planned support state for the affordance.
    pub fn status(&self) -> VideoPlaybackRequirementStatus {
        self.status
    }

    /// Whether this requirement is fully satisfied by the planned route.
    pub fn is_satisfied(&self) -> bool {
        self.status == VideoPlaybackRequirementStatus::Satisfied
    }

    /// Whether this requirement is usable with documented limits.
    pub fn is_limited(&self) -> bool {
        self.status == VideoPlaybackRequirementStatus::Limited
    }

    /// Whether this requirement is currently missing.
    pub fn is_missing(&self) -> bool {
        self.status == VideoPlaybackRequirementStatus::Missing
    }

    /// Compact non-source summary for generated requirement checklists.
    pub fn to_text(&self) -> String {
        format!(
            "video playback requirement: {} {}",
            self.requirement.to_text(),
            self.status.to_text()
        )
    }
}

/// Requirement coverage report for a checked video playback plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoPlaybackRequirementPlan {
    target: VideoPlaybackPlanTarget,
    findings: Vec<VideoPlaybackRequirementFinding>,
}

impl VideoPlaybackRequirementPlan {
    fn new(
        plan: &VideoPlaybackPlan,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> Self {
        let mut findings = Vec::new();
        for requirement in requirements {
            if findings
                .iter()
                .any(|finding: &VideoPlaybackRequirementFinding| finding.requirement == requirement)
            {
                continue;
            }
            findings.push(VideoPlaybackRequirementFinding {
                requirement,
                status: support_for_video_requirement(plan, requirement),
            });
        }

        Self {
            target: plan.target.clone(),
            findings,
        }
    }

    /// Planned rendering target used to evaluate the requirements.
    pub fn target(&self) -> &VideoPlaybackPlanTarget {
        &self.target
    }

    /// All requested requirement findings in request order after de-duplication.
    pub fn findings(&self) -> &[VideoPlaybackRequirementFinding] {
        &self.findings
    }

    /// Number of requested requirements.
    pub fn requirement_count(&self) -> usize {
        self.findings.len()
    }

    /// Number of fully satisfied requirements.
    pub fn satisfied_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_satisfied())
            .count()
    }

    /// Number of limited requirements.
    pub fn limited_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_limited())
            .count()
    }

    /// Number of missing requirements.
    pub fn missing_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_missing())
            .count()
    }

    /// Requirements fully satisfied by the planned route.
    pub fn satisfied_requirements(&self) -> Vec<VideoPlaybackRequirement> {
        self.findings
            .iter()
            .filter_map(|finding| finding.is_satisfied().then_some(finding.requirement()))
            .collect()
    }

    /// Requirements usable with documented limits.
    pub fn limited_requirements(&self) -> Vec<VideoPlaybackRequirement> {
        self.findings
            .iter()
            .filter_map(|finding| finding.is_limited().then_some(finding.requirement()))
            .collect()
    }

    /// Requirements currently missing from the planned route.
    pub fn missing_requirements(&self) -> Vec<VideoPlaybackRequirement> {
        self.findings
            .iter()
            .filter_map(|finding| finding.is_missing().then_some(finding.requirement()))
            .collect()
    }

    /// Requirements that can be satisfied by routing this source through the
    /// browser-video fallback instead of the selected native route.
    pub fn webview_fallback_requirements(&self) -> Vec<VideoPlaybackRequirement> {
        self.findings
            .iter()
            .filter_map(|finding| {
                (requirement_next_action(&self.target, finding)
                    == VideoPlaybackRequirementNextAction::UseWebViewFallback)
                    .then_some(finding.requirement())
            })
            .collect()
    }

    /// Requirements that need backend/product work before they can be promised.
    pub fn native_backend_work_requirements(&self) -> Vec<VideoPlaybackRequirement> {
        self.findings
            .iter()
            .filter_map(|finding| {
                (requirement_next_action(&self.target, finding)
                    == VideoPlaybackRequirementNextAction::BuildNativeBackend)
                    .then_some(finding.requirement())
            })
            .collect()
    }

    /// Whether the plan should be rerouted through WebView fallback to satisfy
    /// one or more requested affordances.
    pub fn requires_webview_fallback(&self) -> bool {
        !self.webview_fallback_requirements().is_empty()
    }

    /// Whether one or more requested affordances need native/backend work.
    pub fn requires_native_backend_work(&self) -> bool {
        !self.native_backend_work_requirements().is_empty()
    }

    /// Next action for one requested requirement, if it was part of the plan.
    pub fn next_action_for(
        &self,
        requirement: VideoPlaybackRequirement,
    ) -> Option<VideoPlaybackRequirementNextAction> {
        self.findings
            .iter()
            .find(|finding| finding.requirement() == requirement)
            .map(|finding| requirement_next_action(&self.target, finding))
    }

    /// Highest-priority next action for the whole requirement plan.
    pub fn next_action(&self) -> VideoPlaybackRequirementNextAction {
        if self.requires_native_backend_work() {
            VideoPlaybackRequirementNextAction::BuildNativeBackend
        } else if self.requires_webview_fallback() {
            VideoPlaybackRequirementNextAction::UseWebViewFallback
        } else if self.limited_count() > 0 {
            VideoPlaybackRequirementNextAction::AcceptLimitedSupport
        } else {
            VideoPlaybackRequirementNextAction::RenderPlannedRoute
        }
    }

    /// Whether every requested requirement is fully satisfied.
    pub fn is_ready(&self) -> bool {
        self.limited_count() == 0 && self.missing_count() == 0
    }

    /// Whether any requested requirement is limited or missing.
    pub fn has_gaps(&self) -> bool {
        !self.is_ready()
    }

    /// Content-safe summary for generated player audits.
    pub fn to_text(&self) -> String {
        format!(
            "video playback requirements: target {}, requested {}, satisfied {}, limited {}, missing {}, next action {}, ready {}",
            self.target.to_text(),
            self.requirement_count(),
            self.satisfied_count(),
            self.limited_count(),
            self.missing_count(),
            self.next_action().to_text(),
            self.is_ready()
        )
    }
}

fn requirement_next_action(
    target: &VideoPlaybackPlanTarget,
    finding: &VideoPlaybackRequirementFinding,
) -> VideoPlaybackRequirementNextAction {
    if finding.is_satisfied() {
        return VideoPlaybackRequirementNextAction::RenderPlannedRoute;
    }

    if finding.is_limited() {
        return VideoPlaybackRequirementNextAction::AcceptLimitedSupport;
    }

    match finding.requirement() {
        VideoPlaybackRequirement::PictureInPicture
        | VideoPlaybackRequirement::AdaptiveStreaming
            if target.is_native() =>
        {
            VideoPlaybackRequirementNextAction::UseWebViewFallback
        }
        _ => VideoPlaybackRequirementNextAction::BuildNativeBackend,
    }
}

fn support_for_video_requirement(
    plan: &VideoPlaybackPlan,
    requirement: VideoPlaybackRequirement,
) -> VideoPlaybackRequirementStatus {
    match requirement {
        VideoPlaybackRequirement::BasicPlayback
        | VideoPlaybackRequirement::SourceReplacement
        | VideoPlaybackRequirement::CanPlayProbe
        | VideoPlaybackRequirement::TextTracks
        | VideoPlaybackRequirement::Fullscreen => VideoPlaybackRequirementStatus::Satisfied,
        VideoPlaybackRequirement::FastSeek | VideoPlaybackRequirement::PlaybackRate => {
            VideoPlaybackRequirementStatus::Limited
        }
        VideoPlaybackRequirement::PictureInPicture => {
            if plan.target().is_webview_fallback() {
                VideoPlaybackRequirementStatus::Satisfied
            } else {
                VideoPlaybackRequirementStatus::Missing
            }
        }
        VideoPlaybackRequirement::AdaptiveStreaming => {
            if plan.target().is_webview_fallback() {
                VideoPlaybackRequirementStatus::Satisfied
            } else {
                VideoPlaybackRequirementStatus::Missing
            }
        }
        VideoPlaybackRequirement::HardwareDecode
        | VideoPlaybackRequirement::NativeTrackSelection => VideoPlaybackRequirementStatus::Missing,
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

#[derive(Clone, Debug, PartialEq)]
enum WebViewVideoCommandDraft {
    Command(WebViewVideoCommand),
    SeekSeconds { seconds: f64, fast: bool },
}

/// Checked browser-video command for a WebView-hosted video fallback.
#[derive(Clone, Debug, PartialEq)]
pub struct WebViewVideoCommandBuilder {
    draft: WebViewVideoCommandDraft,
}

impl WebViewVideoCommandBuilder {
    /// Build a checked `play()` command.
    pub fn play() -> Self {
        Self::command(WebViewVideoCommand::Play)
    }

    /// Build a checked `pause()` command.
    pub fn pause() -> Self {
        Self::command(WebViewVideoCommand::Pause)
    }

    /// Build a checked play/pause toggle command.
    pub fn toggle_play() -> Self {
        Self::command(WebViewVideoCommand::TogglePlay)
    }

    /// Build a checked stop command.
    pub fn stop() -> Self {
        Self::command(WebViewVideoCommand::Stop)
    }

    /// Build a checked seek command.
    pub fn seek(position: Duration) -> Self {
        Self::command(WebViewVideoCommand::Seek(position))
    }

    /// Build a checked seek command from seconds.
    pub fn seek_secs(seconds: f64) -> Self {
        Self {
            draft: WebViewVideoCommandDraft::SeekSeconds {
                seconds,
                fast: false,
            },
        }
    }

    /// Build a checked fast seek command.
    pub fn fast_seek(position: Duration) -> Self {
        Self::command(WebViewVideoCommand::FastSeek(position))
    }

    /// Build a checked fast seek command from seconds.
    pub fn fast_seek_secs(seconds: f64) -> Self {
        Self {
            draft: WebViewVideoCommandDraft::SeekSeconds {
                seconds,
                fast: true,
            },
        }
    }

    /// Build a checked volume command.
    pub fn volume(volume: f32) -> Self {
        Self::command(WebViewVideoCommand::SetVolume(volume))
    }

    /// Build a checked muted command.
    pub fn muted(muted: bool) -> Self {
        Self::command(WebViewVideoCommand::SetMuted(muted))
    }

    /// Build a checked playback-rate command.
    pub fn playback_rate(playback_rate: f32) -> Self {
        Self::command(WebViewVideoCommand::SetPlaybackRate(playback_rate))
    }

    /// Build a checked looping command.
    pub fn looping(looping: bool) -> Self {
        Self::command(WebViewVideoCommand::SetLooping(looping))
    }

    /// Build a checked browser text-track selection command.
    pub fn select_text_track(selector: impl Into<SharedString>) -> Self {
        Self::command(WebViewVideoCommand::SelectTextTrack(selector.into()))
    }

    /// Build a checked browser text-track disablement command.
    pub fn disable_text_tracks() -> Self {
        Self::command(WebViewVideoCommand::DisableTextTracks)
    }

    /// Build a checked browser fullscreen request command.
    pub fn request_fullscreen() -> Self {
        Self::command(WebViewVideoCommand::RequestFullscreen)
    }

    /// Build a checked browser fullscreen exit command.
    pub fn exit_fullscreen() -> Self {
        Self::command(WebViewVideoCommand::ExitFullscreen)
    }

    /// Build a checked picture-in-picture request command.
    pub fn request_picture_in_picture() -> Self {
        Self::command(WebViewVideoCommand::RequestPictureInPicture)
    }

    /// Build a checked picture-in-picture exit command.
    pub fn exit_picture_in_picture() -> Self {
        Self::command(WebViewVideoCommand::ExitPictureInPicture)
    }

    /// Build a checked snapshot request command.
    pub fn request_snapshot() -> Self {
        Self::command(WebViewVideoCommand::RequestSnapshot)
    }

    /// Wrap an existing raw command and validate it before use.
    pub fn command(command: WebViewVideoCommand) -> Self {
        Self {
            draft: WebViewVideoCommandDraft::Command(command),
        }
    }

    /// Validate this command without consuming it.
    pub fn validate(&self) -> anyhow::Result<()> {
        match &self.draft {
            WebViewVideoCommandDraft::Command(command) => validate_webview_video_command(command),
            WebViewVideoCommandDraft::SeekSeconds { seconds, .. } => {
                validate_video_seek_seconds(*seconds)
            }
        }
    }

    /// Return the command category without exposing generated values.
    pub fn command_kind(&self) -> &'static str {
        match &self.draft {
            WebViewVideoCommandDraft::Command(command) => webview_video_command_kind(command),
            WebViewVideoCommandDraft::SeekSeconds { fast: true, .. } => "fast seek",
            WebViewVideoCommandDraft::SeekSeconds { fast: false, .. } => "seek",
        }
    }

    /// Return whether this command changes playback position.
    pub fn is_seek_command(&self) -> bool {
        matches!(
            &self.draft,
            WebViewVideoCommandDraft::Command(
                WebViewVideoCommand::Seek(_) | WebViewVideoCommand::FastSeek(_)
            ) | WebViewVideoCommandDraft::SeekSeconds { .. }
        )
    }

    /// Return whether this command changes output volume or mute state.
    pub fn is_audio_command(&self) -> bool {
        matches!(
            &self.draft,
            WebViewVideoCommandDraft::Command(
                WebViewVideoCommand::SetVolume(_) | WebViewVideoCommand::SetMuted(_)
            )
        )
    }

    /// Return whether this command changes browser presentation state.
    pub fn is_presentation_command(&self) -> bool {
        matches!(
            &self.draft,
            WebViewVideoCommandDraft::Command(
                WebViewVideoCommand::RequestFullscreen
                    | WebViewVideoCommand::ExitFullscreen
                    | WebViewVideoCommand::RequestPictureInPicture
                    | WebViewVideoCommand::ExitPictureInPicture
            )
        )
    }

    /// Return a content-safe summary for generated WebView video commands.
    pub fn to_text(&self) -> String {
        format!(
            "webview video command: kind {}, seek {}, audio {}, presentation {}",
            self.command_kind(),
            self.is_seek_command(),
            self.is_audio_command(),
            self.is_presentation_command()
        )
    }

    /// Return the checked command.
    pub fn build_checked(self) -> anyhow::Result<WebViewVideoCommand> {
        self.validate()?;
        Ok(match self.draft {
            WebViewVideoCommandDraft::Command(command) => command,
            WebViewVideoCommandDraft::SeekSeconds { seconds, fast } => {
                let position = Duration::from_secs_f64(seconds);
                if fast {
                    WebViewVideoCommand::FastSeek(position)
                } else {
                    WebViewVideoCommand::Seek(position)
                }
            }
        })
    }
}

/// Checked media-control update for an native desktop video player.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoPlaybackControls {
    volume: Option<f32>,
    muted: Option<bool>,
    playback_rate: Option<f32>,
    looping: Option<bool>,
    seek_position: Option<Duration>,
    fast_seek: bool,
}

impl VideoPlaybackControls {
    /// Requested output volume, if configured.
    pub fn volume(&self) -> Option<f32> {
        self.volume
    }

    /// Requested muted state, if configured.
    pub fn muted(&self) -> Option<bool> {
        self.muted
    }

    /// Requested playback rate, if configured.
    pub fn playback_rate(&self) -> Option<f32> {
        self.playback_rate
    }

    /// Requested looping state, if configured.
    pub fn looping(&self) -> Option<bool> {
        self.looping
    }

    /// Requested seek position, if configured.
    pub fn seek_position(&self) -> Option<Duration> {
        self.seek_position
    }

    /// Whether the requested seek should prefer a fast/keyframe-oriented path.
    pub fn uses_fast_seek(&self) -> bool {
        self.fast_seek
    }

    /// Number of configured control updates.
    pub fn update_count(&self) -> usize {
        [
            self.volume.is_some(),
            self.muted.is_some(),
            self.playback_rate.is_some(),
            self.looping.is_some(),
            self.seek_position.is_some(),
        ]
        .into_iter()
        .filter(|configured| *configured)
        .count()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video playback controls: updates {}, volume {}, muted {}, playback rate {}, looping {}, seek {}, fast seek {}",
            self.update_count(),
            self.volume.is_some(),
            self.muted.is_some(),
            self.playback_rate.is_some(),
            self.looping.is_some(),
            self.seek_position.is_some(),
            self.fast_seek
        )
    }
}

/// Builder for validated media-control updates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoPlaybackControlsBuilder {
    volume: Option<f32>,
    muted: Option<bool>,
    playback_rate: Option<f32>,
    looping: Option<bool>,
    seek_position: Option<Duration>,
    seek_seconds: Option<f64>,
    fast_seek: bool,
}

impl VideoPlaybackControlsBuilder {
    /// Create an empty controls builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set output volume in the browser-compatible `0.0..=1.0` range.
    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = Some(volume);
        self
    }

    /// Set muted state.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = Some(muted);
        self
    }

    /// Set playback rate in the checked `0.0625..=16.0` range.
    pub fn playback_rate(mut self, playback_rate: f32) -> Self {
        self.playback_rate = Some(playback_rate);
        self
    }

    /// Set looping state.
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = Some(looping);
        self
    }

    /// Seek to a playback position.
    pub fn seek(mut self, position: Duration) -> Self {
        self.seek_position = Some(position);
        self.seek_seconds = None;
        self.fast_seek = false;
        self
    }

    /// Seek to a playback position using the fast-seek path when supported.
    pub fn fast_seek(mut self, position: Duration) -> Self {
        self.seek_position = Some(position);
        self.seek_seconds = None;
        self.fast_seek = true;
        self
    }

    /// Seek to a playback position expressed in seconds.
    pub fn seek_secs(mut self, seconds: f64) -> Self {
        self.seek_seconds = Some(seconds);
        self.seek_position = None;
        self.fast_seek = false;
        self
    }

    /// Fast-seek to a playback position expressed in seconds.
    pub fn fast_seek_secs(mut self, seconds: f64) -> Self {
        self.seek_seconds = Some(seconds);
        self.seek_position = None;
        self.fast_seek = true;
        self
    }

    /// Return the configured volume.
    pub fn configured_volume(&self) -> Option<f32> {
        self.volume
    }

    /// Return the configured playback rate.
    pub fn configured_playback_rate(&self) -> Option<f32> {
        self.playback_rate
    }

    /// Number of configured control updates.
    pub fn configured_update_count(&self) -> usize {
        [
            self.volume.is_some(),
            self.muted.is_some(),
            self.playback_rate.is_some(),
            self.looping.is_some(),
            self.seek_position.is_some() || self.seek_seconds.is_some(),
        ]
        .into_iter()
        .filter(|configured| *configured)
        .count()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video playback controls: updates {}, volume {}, muted {}, playback rate {}, looping {}, seek {}, fast seek {}",
            self.configured_update_count(),
            self.volume.is_some(),
            self.muted.is_some(),
            self.playback_rate.is_some(),
            self.looping.is_some(),
            self.seek_position.is_some() || self.seek_seconds.is_some(),
            self.fast_seek
        )
    }

    /// Validate the requested control update.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.volume.is_some()
                || self.muted.is_some()
                || self.playback_rate.is_some()
                || self.looping.is_some()
                || self.seek_position.is_some()
                || self.seek_seconds.is_some(),
            "video playback controls must configure at least one update"
        );
        if let Some(volume) = self.volume {
            validate_video_volume(volume)?;
        }
        if let Some(playback_rate) = self.playback_rate {
            validate_video_playback_rate(playback_rate)?;
        }
        if let Some(position) = self.seek_position {
            validate_video_seek_position(position)?;
        }
        if let Some(seconds) = self.seek_seconds {
            validate_video_seek_seconds(seconds)?;
        }
        Ok(())
    }

    /// Build the validated controls.
    pub fn build_checked(self) -> anyhow::Result<VideoPlaybackControls> {
        self.validate()?;
        let seek_position = match (self.seek_position, self.seek_seconds) {
            (Some(position), None) => Some(position),
            (None, Some(seconds)) => Some(Duration::from_secs_f64(seconds)),
            (None, None) => None,
            (Some(_), Some(_)) => unreachable!("builder stores one seek representation"),
        };
        Ok(VideoPlaybackControls {
            volume: self.volume,
            muted: self.muted,
            playback_rate: self.playback_rate,
            looping: self.looping,
            seek_position,
            fast_seek: self.fast_seek,
        })
    }
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

    /// Number of configured browser controls-list tokens.
    pub fn controls_list_count(&self) -> usize {
        self.controls_list.len()
    }

    /// Number of configured browser text tracks.
    pub fn text_track_count(&self) -> usize {
        self.text_tracks.len()
    }

    /// Return whether a poster URL is configured.
    pub fn has_poster(&self) -> bool {
        self.poster.is_some()
    }

    /// Return whether a preload hint is configured.
    pub fn has_preload(&self) -> bool {
        self.preload.is_some()
    }

    /// Return whether a CORS mode is configured.
    pub fn has_cross_origin(&self) -> bool {
        self.cross_origin.is_some()
    }

    /// Return whether a start position is configured.
    pub fn has_start_position(&self) -> bool {
        self.start_position.is_some()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "webview video options: controls {}, autoplay {}, muted {}, looping {}, plays inline {}, poster {}, preload {}, cross origin {}, controls-list {}, disable picture-in-picture {}, start position {}, text tracks {}, object fit {}",
            self.controls,
            self.autoplay,
            self.muted,
            self.looping,
            self.plays_inline,
            self.has_poster(),
            self.has_preload(),
            self.has_cross_origin(),
            self.controls_list_count(),
            self.disable_picture_in_picture,
            self.has_start_position(),
            self.text_track_count(),
            self.object_fit.as_ref()
        )
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

    /// Return the configured source kind without exposing URLs, paths, or keys.
    pub fn source_kind(&self) -> &'static str {
        media_source_kind(&self.source)
    }

    /// Return whether file-backed sources must already exist.
    pub fn requires_existing_file(&self) -> bool {
        self.require_existing_file
    }

    /// Return whether file-backed sources will be canonicalized.
    pub fn canonicalizes_file(&self) -> bool {
        self.canonicalize_file
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "media source: kind {}, require existing file {}, canonicalize file {}",
            self.source_kind(),
            self.requires_existing_file(),
            self.canonicalizes_file()
        )
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

    /// Return a compact target label for logs and agent traces.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::WebViewFallback { .. } => "webview fallback",
        }
    }
}

/// Render instruction produced by a checked video playback plan.
#[derive(Clone, Debug)]
pub enum VideoPlaybackRenderInstruction {
    /// Render with Kael's native video element and control with this controller.
    Native {
        /// Controller initialized with the plan's validated source.
        controller: VideoController,
    },
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

impl VideoPlaybackRenderInstruction {
    /// Return whether this instruction renders through Kael's native video path.
    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native { .. })
    }

    /// Return whether this instruction renders through a browser-video WebView fallback.
    pub fn is_webview_fallback(&self) -> bool {
        matches!(self, Self::WebViewFallback { .. })
    }

    /// Return a content-safe summary for render dispatch logs.
    pub fn to_text(&self) -> String {
        format!(
            "video render instruction: target {}, controller {}",
            if self.is_native() {
                "native"
            } else {
                "webview fallback"
            },
            self.is_native()
        )
    }
}

/// Checked plan for building an native desktop video player from one source.
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

    /// Produce the concrete render instruction for this checked playback plan.
    pub fn render_instruction(&self) -> VideoPlaybackRenderInstruction {
        match &self.target {
            VideoPlaybackPlanTarget::Native => VideoPlaybackRenderInstruction::Native {
                controller: self.controller(),
            },
            VideoPlaybackPlanTarget::WebViewFallback {
                page_url,
                element_id,
                reason,
            } => VideoPlaybackRenderInstruction::WebViewFallback {
                page_url: page_url.clone(),
                element_id: element_id.clone(),
                reason: reason.clone(),
            },
        }
    }

    /// Evaluate requested playback affordances against this checked plan.
    pub fn requirement_plan(
        &self,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> VideoPlaybackRequirementPlan {
        VideoPlaybackRequirementPlan::new(self, requirements)
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

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video playback plan: source {}, target {}, route {}, can play {}, content type {}, webview text tracks {}",
            media_source_kind(&self.source),
            self.target.to_text(),
            self.route.to_text(),
            self.can_play.to_text(),
            self.content_type.is_some(),
            self.webview_options.text_track_count()
        )
    }
}

/// One-object handoff for native desktop URL video playback.
///
/// This is the ergonomic path for generated code that would otherwise reach
/// for `<video src="...">`: validate the URL, choose native playback unless a
/// browser-video fallback is required, and expose the controller/render
/// instruction without logging the media URL.
#[derive(Clone)]
pub struct VideoUrlPlaybackHandoff {
    plan: VideoPlaybackPlan,
}

impl VideoUrlPlaybackHandoff {
    /// Build a checked URL-backed handoff with default fallback options.
    pub fn url(url: impl Into<Arc<str>>) -> anyhow::Result<Self> {
        Self::from_builder(VideoPlaybackPlanBuilder::url(url))
    }

    /// Build a checked URL-backed handoff with browser fallback options.
    pub fn url_with_options(
        url: impl Into<Arc<str>>,
        options: WebViewVideoOptions,
    ) -> anyhow::Result<Self> {
        Self::from_builder(VideoPlaybackPlanBuilder::url(url).webview_options(options))
    }

    /// Build a checked URL-backed handoff when a server-supplied content type
    /// should drive native-vs-browser routing for extensionless URLs.
    pub fn url_with_content_type(
        url: impl Into<Arc<str>>,
        content_type: impl Into<SharedString>,
    ) -> anyhow::Result<Self> {
        Self::from_builder(VideoPlaybackPlanBuilder::url(url).content_type(content_type))
    }

    /// Build from a configured playback-plan builder.
    pub fn from_builder(builder: VideoPlaybackPlanBuilder) -> anyhow::Result<Self> {
        Ok(Self {
            plan: builder.build_checked()?,
        })
    }

    /// Return the checked playback plan.
    pub fn plan(&self) -> &VideoPlaybackPlan {
        &self.plan
    }

    /// Return a fresh controller for the validated URL source.
    pub fn controller(&self) -> VideoController {
        self.plan.controller()
    }

    /// Produce the render instruction selected by the checked plan.
    pub fn render_instruction(&self) -> VideoPlaybackRenderInstruction {
        self.plan.render_instruction()
    }

    /// Browser-style native playback confidence for this URL.
    pub fn can_play(&self) -> VideoCanPlay {
        self.plan.can_play()
    }

    /// Recommended native-vs-WebView route.
    pub fn route(&self) -> &VideoPlaybackRoute {
        self.plan.route()
    }

    /// Planned rendering target.
    pub fn target(&self) -> &VideoPlaybackPlanTarget {
        self.plan.target()
    }

    /// Evaluate requested playback affordances against this checked handoff.
    pub fn requirement_plan(
        &self,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> VideoPlaybackRequirementPlan {
        self.plan.requirement_plan(requirements)
    }

    /// Evaluate the default URL-player affordances expected by generated apps.
    pub fn baseline_requirement_plan(&self) -> VideoPlaybackRequirementPlan {
        self.requirement_plan(
            VideoPlaybackRequirement::url_player_baseline()
                .iter()
                .copied(),
        )
    }

    /// Evaluate every known video playback affordance for audit/roadmap output.
    pub fn full_requirement_plan(&self) -> VideoPlaybackRequirementPlan {
        self.requirement_plan(VideoPlaybackRequirement::all().iter().copied())
    }

    /// Highest-priority next action for the default URL-player promise.
    pub fn baseline_next_action(&self) -> VideoPlaybackRequirementNextAction {
        self.baseline_requirement_plan().next_action()
    }

    /// Return whether the default URL-player promise is ready for this source.
    pub fn baseline_ready(&self) -> bool {
        self.baseline_requirement_plan().is_ready()
    }

    /// Return whether this URL is ready for Kael's native video path.
    pub fn is_native(&self) -> bool {
        self.plan.target().is_native()
    }

    /// Return whether this URL is routed through the browser-video fallback.
    pub fn uses_webview_fallback(&self) -> bool {
        self.plan.target().is_webview_fallback()
    }

    /// Return the WebView fallback page URL when the checked route needs one.
    pub fn webview_page_url(&self) -> Option<&str> {
        self.plan.webview_page_url()
    }

    /// Return the stable browser-video element id when the checked route needs one.
    pub fn webview_element_id(&self) -> Option<&str> {
        self.plan.webview_element_id()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video URL playback handoff: target {}, route {}, can play {}, controller {}, webview page {}, webview element {}",
            self.plan.target().to_text(),
            self.plan.route().to_text(),
            self.plan.can_play().to_text(),
            true,
            self.webview_page_url().is_some(),
            self.webview_element_id().is_some()
        )
    }
}

/// Next app-builder action for a checked desktop video element handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoElementHandoffNextAction {
    /// Render a native Kael video element/controller.
    RenderNativePlayer,
    /// Render the checked browser-video fallback for this source.
    RenderWebViewFallback,
    /// Render with documented native limitations surfaced to the app.
    AcceptLimitedSupport,
    /// Product/backend work is required before the requested promise is true.
    BuildNativeBackend,
}

impl VideoElementHandoffNextAction {
    /// Stable action label for logs, setup screens, and generated agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::RenderNativePlayer => "render native player",
            Self::RenderWebViewFallback => "render webview fallback",
            Self::AcceptLimitedSupport => "accept limited support",
            Self::BuildNativeBackend => "build native backend",
        }
    }
}

/// Customization affordance expected from an Electron-style video element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoElementCustomizationFeature {
    /// Browser-like readable state/properties such as source, duration, ready state, and support.
    Properties,
    /// Playback event callbacks for generated player chrome and agents.
    Events,
    /// App-owned play/pause/seek/volume/rate/loop controls.
    CustomControls,
    /// Timeline scrubbing and fast seek behavior.
    TimelineScrubbing,
    /// Caption/subtitle track UI.
    CaptionsUi,
    /// Native fullscreen presentation.
    Fullscreen,
    /// Browser picture-in-picture presentation.
    PictureInPicture,
    /// Native hardware decode / low-copy media surface expectation.
    HardwareDecode,
    /// Runtime source switching like assigning `video.src`.
    SourceSwitching,
    /// Playlist plus hardware/OS media-key next/previous routing.
    PlaylistMediaKeys,
}

impl VideoElementCustomizationFeature {
    /// Stable feature label for logs and generated checklists.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Properties => "properties",
            Self::Events => "events",
            Self::CustomControls => "custom controls",
            Self::TimelineScrubbing => "timeline scrubbing",
            Self::CaptionsUi => "captions ui",
            Self::Fullscreen => "fullscreen",
            Self::PictureInPicture => "picture-in-picture",
            Self::HardwareDecode => "hardware decode",
            Self::SourceSwitching => "source switching",
            Self::PlaylistMediaKeys => "playlist media keys",
        }
    }
}

/// Support state for one requested video-element customization affordance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoElementCustomizationStatus {
    /// The selected handoff covers this customization.
    Satisfied,
    /// The selected handoff covers this customization with documented limits.
    Limited,
    /// The selected handoff does not cover this customization yet.
    Missing,
}

impl VideoElementCustomizationStatus {
    /// Stable status label for logs and generated checklists.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Satisfied => "satisfied",
            Self::Limited => "limited",
            Self::Missing => "missing",
        }
    }
}

/// Next app-builder action for a video-element customization plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoElementCustomizationNextAction {
    /// Render the configured native or checked fallback player.
    RenderConfiguredPlayer,
    /// Render with documented limits surfaced in the app.
    AcceptLimitedSupport,
    /// Use the checked browser-video fallback for browser-only media behavior.
    UseWebViewFallback,
    /// Add playlist/media-key wiring before claiming this customization.
    ConfigurePlaylistOrHandlers,
    /// Product/backend work is required before this customization can be promised.
    BuildNativeBackend,
}

impl VideoElementCustomizationNextAction {
    /// Stable action label for logs and generated agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::RenderConfiguredPlayer => "render configured player",
            Self::AcceptLimitedSupport => "accept limited support",
            Self::UseWebViewFallback => "use webview fallback",
            Self::ConfigurePlaylistOrHandlers => "configure playlist or handlers",
            Self::BuildNativeBackend => "build native backend",
        }
    }
}

/// One customization finding for an Electron-style video-element replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoElementCustomizationFinding {
    feature: VideoElementCustomizationFeature,
    status: VideoElementCustomizationStatus,
    next_action: VideoElementCustomizationNextAction,
}

impl VideoElementCustomizationFinding {
    /// Requested customization affordance.
    pub fn feature(&self) -> VideoElementCustomizationFeature {
        self.feature
    }

    /// Support state for the affordance.
    pub fn status(&self) -> VideoElementCustomizationStatus {
        self.status
    }

    /// Next app-builder action for the affordance.
    pub fn next_action(&self) -> VideoElementCustomizationNextAction {
        self.next_action
    }

    /// Whether this customization is fully covered.
    pub fn is_satisfied(&self) -> bool {
        self.status == VideoElementCustomizationStatus::Satisfied
    }

    /// Whether this customization is usable with documented limits.
    pub fn is_limited(&self) -> bool {
        self.status == VideoElementCustomizationStatus::Limited
    }

    /// Whether this customization is missing.
    pub fn is_missing(&self) -> bool {
        self.status == VideoElementCustomizationStatus::Missing
    }

    /// Content-safe summary for generated customization checklists.
    pub fn to_text(&self) -> String {
        format!(
            "video element customization: {} {}, next action {}",
            self.feature.to_text(),
            self.status.to_text(),
            self.next_action.to_text()
        )
    }
}

/// Checked customization plan for a generated, highly tweakable video player.
#[derive(Clone)]
pub struct VideoElementCustomizationPlan {
    handoff: VideoElementHandoff,
    features: Vec<VideoElementCustomizationFeature>,
    findings: Vec<VideoElementCustomizationFinding>,
    event_handler_count: usize,
    custom_control_count: usize,
}

impl VideoElementCustomizationPlan {
    /// Underlying source/route/requirement handoff.
    pub fn handoff(&self) -> &VideoElementHandoff {
        &self.handoff
    }

    /// Requested customization features in stable insertion order.
    pub fn features(&self) -> &[VideoElementCustomizationFeature] {
        &self.features
    }

    /// Customization findings in feature order.
    pub fn findings(&self) -> &[VideoElementCustomizationFinding] {
        &self.findings
    }

    /// Number of requested customization features.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Number of expected event handlers.
    pub fn event_handler_count(&self) -> usize {
        self.event_handler_count
    }

    /// Number of expected custom controls.
    pub fn custom_control_count(&self) -> usize {
        self.custom_control_count
    }

    /// Number of fully satisfied customization features.
    pub fn satisfied_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_satisfied())
            .count()
    }

    /// Number of limited customization features.
    pub fn limited_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_limited())
            .count()
    }

    /// Number of missing customization features.
    pub fn missing_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.is_missing())
            .count()
    }

    /// Whether this plan asks for a feature.
    pub fn has_feature(&self, feature: VideoElementCustomizationFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Whether one or more requested features need the browser-video fallback.
    pub fn requires_webview_fallback(&self) -> bool {
        self.findings.iter().any(|finding| {
            finding.next_action() == VideoElementCustomizationNextAction::UseWebViewFallback
        })
    }

    /// Whether one or more requested features need native/backend work.
    pub fn requires_native_backend_work(&self) -> bool {
        self.findings.iter().any(|finding| {
            finding.next_action() == VideoElementCustomizationNextAction::BuildNativeBackend
        })
    }

    /// Whether playlist/media-key wiring is still required.
    pub fn requires_playlist_or_handlers(&self) -> bool {
        self.findings.iter().any(|finding| {
            finding.next_action()
                == VideoElementCustomizationNextAction::ConfigurePlaylistOrHandlers
        })
    }

    /// Highest-priority next action for the whole customization plan.
    pub fn next_action(&self) -> VideoElementCustomizationNextAction {
        if self.requires_native_backend_work() {
            VideoElementCustomizationNextAction::BuildNativeBackend
        } else if self.requires_webview_fallback() {
            VideoElementCustomizationNextAction::UseWebViewFallback
        } else if self.requires_playlist_or_handlers() {
            VideoElementCustomizationNextAction::ConfigurePlaylistOrHandlers
        } else if self.limited_count() > 0 {
            VideoElementCustomizationNextAction::AcceptLimitedSupport
        } else {
            VideoElementCustomizationNextAction::RenderConfiguredPlayer
        }
    }

    /// Whether every requested customization is fully satisfied.
    pub fn is_ready(&self) -> bool {
        self.limited_count() == 0 && self.missing_count() == 0
    }

    /// Content-safe summary for generated player setup.
    pub fn to_text(&self) -> String {
        format!(
            "video element customization plan: features {}, satisfied {}, limited {}, missing {}, event handlers {}, custom controls {}, native {}, webview fallback {}, next action {}",
            self.feature_count(),
            self.satisfied_count(),
            self.limited_count(),
            self.missing_count(),
            self.event_handler_count,
            self.custom_control_count,
            self.handoff.is_native(),
            self.handoff.uses_webview_fallback(),
            self.next_action().to_text()
        )
    }
}

/// Builder for checked video-element customization plans.
#[derive(Clone)]
pub struct VideoElementCustomizationPlanBuilder {
    handoff: VideoElementHandoff,
    features: Vec<VideoElementCustomizationFeature>,
    event_handler_count: usize,
    custom_control_count: usize,
}

impl VideoElementCustomizationPlanBuilder {
    /// Start from a checked video element handoff.
    pub fn new(handoff: VideoElementHandoff) -> Self {
        Self {
            handoff,
            features: Vec::new(),
            event_handler_count: 0,
            custom_control_count: 0,
        }
    }

    /// Request the common `<video>`-like customization surface for generated players.
    pub fn html_video_baseline(mut self) -> Self {
        self = self
            .feature(VideoElementCustomizationFeature::Properties)
            .feature(VideoElementCustomizationFeature::Events)
            .feature(VideoElementCustomizationFeature::CustomControls)
            .feature(VideoElementCustomizationFeature::SourceSwitching);
        if self.event_handler_count == 0 {
            self.event_handler_count = 8;
        }
        if self.custom_control_count == 0 {
            self.custom_control_count = 5;
        }
        self
    }

    /// Request one customization feature.
    pub fn feature(mut self, feature: VideoElementCustomizationFeature) -> Self {
        if !self.features.contains(&feature) {
            self.features.push(feature);
        }
        self
    }

    /// Request custom controls and record the expected control count.
    pub fn custom_controls(mut self, count: usize) -> Self {
        self = self.feature(VideoElementCustomizationFeature::CustomControls);
        self.custom_control_count = count;
        self
    }

    /// Request event wiring and record the expected handler count.
    pub fn event_handlers(mut self, count: usize) -> Self {
        self = self.feature(VideoElementCustomizationFeature::Events);
        self.event_handler_count = count;
        self
    }

    /// Request timeline scrubbing support.
    pub fn timeline_scrubbing(self) -> Self {
        self.feature(VideoElementCustomizationFeature::TimelineScrubbing)
    }

    /// Request caption/subtitle UI.
    pub fn captions_ui(self) -> Self {
        self.feature(VideoElementCustomizationFeature::CaptionsUi)
    }

    /// Request fullscreen presentation.
    pub fn fullscreen(self) -> Self {
        self.feature(VideoElementCustomizationFeature::Fullscreen)
    }

    /// Request browser picture-in-picture presentation.
    pub fn picture_in_picture(self) -> Self {
        self.feature(VideoElementCustomizationFeature::PictureInPicture)
    }

    /// Request native hardware decode / low-copy media surface support.
    pub fn hardware_decode(self) -> Self {
        self.feature(VideoElementCustomizationFeature::HardwareDecode)
    }

    /// Request source switching support.
    pub fn source_switching(self) -> Self {
        self.feature(VideoElementCustomizationFeature::SourceSwitching)
    }

    /// Request playlist/media-key routing.
    pub fn playlist_media_keys(self) -> Self {
        self.feature(VideoElementCustomizationFeature::PlaylistMediaKeys)
    }

    /// Number of requested customization features.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Return whether this builder requests a feature.
    pub fn has_feature(&self, feature: VideoElementCustomizationFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Validate the customization request without consuming the builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.features.is_empty(),
            "video element customization plan must request at least one feature"
        );
        anyhow::ensure!(
            self.event_handler_count <= 64,
            "video element customization plan supports at most 64 event handlers"
        );
        anyhow::ensure!(
            self.custom_control_count <= 32,
            "video element customization plan supports at most 32 custom controls"
        );
        Ok(())
    }

    /// Build the checked customization plan.
    pub fn build_checked(self) -> anyhow::Result<VideoElementCustomizationPlan> {
        self.validate()?;
        let findings = self
            .features
            .iter()
            .copied()
            .map(|feature| video_customization_finding(&self.handoff, feature))
            .collect();
        Ok(VideoElementCustomizationPlan {
            handoff: self.handoff,
            features: self.features,
            findings,
            event_handler_count: self.event_handler_count,
            custom_control_count: self.custom_control_count,
        })
    }
}

fn video_customization_finding(
    handoff: &VideoElementHandoff,
    feature: VideoElementCustomizationFeature,
) -> VideoElementCustomizationFinding {
    let (status, next_action) = match feature {
        VideoElementCustomizationFeature::Properties
        | VideoElementCustomizationFeature::Events
        | VideoElementCustomizationFeature::SourceSwitching => (
            VideoElementCustomizationStatus::Satisfied,
            VideoElementCustomizationNextAction::RenderConfiguredPlayer,
        ),
        VideoElementCustomizationFeature::CustomControls => status_and_action_from_requirements(
            handoff,
            [
                VideoPlaybackRequirement::BasicPlayback,
                VideoPlaybackRequirement::FastSeek,
                VideoPlaybackRequirement::PlaybackRate,
            ],
        ),
        VideoElementCustomizationFeature::TimelineScrubbing => {
            status_and_action_from_requirements(handoff, [VideoPlaybackRequirement::FastSeek])
        }
        VideoElementCustomizationFeature::CaptionsUi => {
            status_and_action_from_requirements(handoff, [VideoPlaybackRequirement::TextTracks])
        }
        VideoElementCustomizationFeature::Fullscreen => {
            status_and_action_from_requirements(handoff, [VideoPlaybackRequirement::Fullscreen])
        }
        VideoElementCustomizationFeature::PictureInPicture => status_and_action_from_requirements(
            handoff,
            [VideoPlaybackRequirement::PictureInPicture],
        ),
        VideoElementCustomizationFeature::HardwareDecode => {
            status_and_action_from_requirements(handoff, [VideoPlaybackRequirement::HardwareDecode])
        }
        VideoElementCustomizationFeature::PlaylistMediaKeys => {
            if handoff.has_playlist() {
                (
                    VideoElementCustomizationStatus::Satisfied,
                    VideoElementCustomizationNextAction::RenderConfiguredPlayer,
                )
            } else {
                (
                    VideoElementCustomizationStatus::Missing,
                    VideoElementCustomizationNextAction::ConfigurePlaylistOrHandlers,
                )
            }
        }
    };

    VideoElementCustomizationFinding {
        feature,
        status,
        next_action,
    }
}

fn status_and_action_from_requirements(
    handoff: &VideoElementHandoff,
    requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
) -> (
    VideoElementCustomizationStatus,
    VideoElementCustomizationNextAction,
) {
    let mut saw_limited = false;
    let mut saw_missing = false;
    let mut saw_webview = false;
    let mut saw_backend = false;

    for requirement in requirements {
        match handoff.requirement_plan().next_action_for(requirement) {
            Some(VideoPlaybackRequirementNextAction::RenderPlannedRoute) => {}
            Some(VideoPlaybackRequirementNextAction::AcceptLimitedSupport) => {
                saw_limited = true;
            }
            Some(VideoPlaybackRequirementNextAction::UseWebViewFallback) => {
                saw_missing = true;
                saw_webview = true;
            }
            Some(VideoPlaybackRequirementNextAction::BuildNativeBackend) | None => {
                saw_missing = true;
                saw_backend = true;
            }
        }
    }

    if saw_backend {
        (
            VideoElementCustomizationStatus::Missing,
            VideoElementCustomizationNextAction::BuildNativeBackend,
        )
    } else if saw_webview {
        (
            VideoElementCustomizationStatus::Missing,
            VideoElementCustomizationNextAction::UseWebViewFallback,
        )
    } else if saw_missing {
        (
            VideoElementCustomizationStatus::Missing,
            VideoElementCustomizationNextAction::BuildNativeBackend,
        )
    } else if saw_limited {
        (
            VideoElementCustomizationStatus::Limited,
            VideoElementCustomizationNextAction::AcceptLimitedSupport,
        )
    } else {
        (
            VideoElementCustomizationStatus::Satisfied,
            VideoElementCustomizationNextAction::RenderConfiguredPlayer,
        )
    }
}

/// One-object replacement contract for an Electron-style `<video>` element.
///
/// The handoff wraps a checked playback plan, optional initial media controls,
/// optional playlist/media-key intent, and the requirement audit that tells a
/// builder whether the app can render native, should use the checked browser
/// fallback, can accept documented limits, or needs backend work.
#[derive(Clone)]
pub struct VideoElementHandoff {
    plan: VideoPlaybackPlan,
    initial_controls: Option<VideoPlaybackControls>,
    playlist: Option<VideoPlaylist>,
    requirements: VideoPlaybackRequirementPlan,
}

impl VideoElementHandoff {
    fn new(
        plan: VideoPlaybackPlan,
        initial_controls: Option<VideoPlaybackControls>,
        playlist: Option<VideoPlaylist>,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> Self {
        let requirements = plan.requirement_plan(requirements);
        Self {
            plan,
            initial_controls,
            playlist,
            requirements,
        }
    }

    /// Checked playback plan for this video element.
    pub fn plan(&self) -> &VideoPlaybackPlan {
        &self.plan
    }

    /// Optional initial controls to apply before first render/playback.
    pub fn initial_controls(&self) -> Option<&VideoPlaybackControls> {
        self.initial_controls.as_ref()
    }

    /// Optional playlist intent for next/previous media keys.
    pub fn playlist(&self) -> Option<&VideoPlaylist> {
        self.playlist.as_ref()
    }

    /// Requirement coverage for the promised player affordances.
    pub fn requirement_plan(&self) -> &VideoPlaybackRequirementPlan {
        &self.requirements
    }

    /// Produce the concrete render instruction selected by the checked plan.
    pub fn render_instruction(&self) -> VideoPlaybackRenderInstruction {
        self.plan.render_instruction()
    }

    /// Return a fresh controller with initial controls applied.
    pub fn controller_checked(&self) -> anyhow::Result<VideoController> {
        let controller = self.plan.controller();
        if let Some(controls) = &self.initial_controls {
            if let Some(volume) = controls.volume() {
                controller.set_volume(volume);
            }
            if let Some(muted) = controls.muted() {
                controller.set_muted(muted);
            }
            if let Some(playback_rate) = controls.playback_rate() {
                controller.set_playback_rate(playback_rate);
            }
            if let Some(looping) = controls.looping() {
                controller.set_looping(looping);
            }
            if let Some(position) = controls.seek_position() {
                if controls.uses_fast_seek() {
                    controller.fast_seek(position)?;
                } else {
                    controller.seek(position)?;
                }
            }
        }
        Ok(controller)
    }

    /// Build a media-key binding for this handoff when playlist intent exists.
    pub fn media_key_binding_builder_checked(
        &self,
    ) -> anyhow::Result<Option<MediaKeyBindingBuilder>> {
        let Some(playlist) = &self.playlist else {
            return Ok(None);
        };
        Ok(Some(
            MediaKeyBindingBuilder::new()
                .video(self.controller_checked()?)
                .playlist(playlist.clone()),
        ))
    }

    /// Highest-priority next app-builder action.
    pub fn next_action(&self) -> VideoElementHandoffNextAction {
        match self.requirements.next_action() {
            VideoPlaybackRequirementNextAction::BuildNativeBackend => {
                VideoElementHandoffNextAction::BuildNativeBackend
            }
            VideoPlaybackRequirementNextAction::UseWebViewFallback => {
                VideoElementHandoffNextAction::RenderWebViewFallback
            }
            VideoPlaybackRequirementNextAction::AcceptLimitedSupport => {
                VideoElementHandoffNextAction::AcceptLimitedSupport
            }
            VideoPlaybackRequirementNextAction::RenderPlannedRoute => {
                if self.plan.target().is_webview_fallback() {
                    VideoElementHandoffNextAction::RenderWebViewFallback
                } else {
                    VideoElementHandoffNextAction::RenderNativePlayer
                }
            }
        }
    }

    /// Whether the handoff can render through Kael's native video path.
    pub fn is_native(&self) -> bool {
        self.plan.target().is_native()
    }

    /// Whether the checked route uses the browser-video fallback.
    pub fn uses_webview_fallback(&self) -> bool {
        self.plan.target().is_webview_fallback()
    }

    /// Whether the promised requirements are fully satisfied.
    pub fn is_ready(&self) -> bool {
        self.requirements.is_ready()
    }

    /// Whether initial controls are configured.
    pub fn has_initial_controls(&self) -> bool {
        self.initial_controls.is_some()
    }

    /// Whether playlist/media-key intent is configured.
    pub fn has_playlist(&self) -> bool {
        self.playlist.is_some()
    }

    /// Number of playlist sources, if configured.
    pub fn playlist_source_count(&self) -> usize {
        self.playlist.as_ref().map(VideoPlaylist::len).unwrap_or(0)
    }

    /// Content-safe summary for generated player setup.
    pub fn to_text(&self) -> String {
        format!(
            "video element handoff: target {}, route {}, can play {}, controls {}, playlist {}, playlist sources {}, requirements {}, satisfied {}, limited {}, missing {}, next action {}",
            self.plan.target().to_text(),
            self.plan.route().to_text(),
            self.plan.can_play().to_text(),
            self.has_initial_controls(),
            self.has_playlist(),
            self.playlist_source_count(),
            self.requirements.requirement_count(),
            self.requirements.satisfied_count(),
            self.requirements.limited_count(),
            self.requirements.missing_count(),
            self.next_action().to_text()
        )
    }
}

/// Builder for checked desktop video element handoffs.
#[derive(Clone)]
pub struct VideoElementHandoffBuilder {
    plan: VideoPlaybackPlanBuilder,
    initial_controls: Option<VideoPlaybackControlsBuilder>,
    playlist: Option<VideoPlaylist>,
    requirements: Vec<VideoPlaybackRequirement>,
}

impl VideoElementHandoffBuilder {
    /// Create a handoff builder from a playback-plan builder.
    pub fn new(plan: VideoPlaybackPlanBuilder) -> Self {
        Self {
            plan,
            initial_controls: None,
            playlist: None,
            requirements: VideoPlaybackRequirement::url_player_baseline().to_vec(),
        }
    }

    /// Create a checked URL-backed handoff builder.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::new(VideoPlaybackPlanBuilder::url(url))
    }

    /// Create a checked file-backed handoff builder.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(VideoPlaybackPlanBuilder::file(path))
    }

    /// Provide a MIME/content type for route selection.
    pub fn content_type(mut self, content_type: impl Into<SharedString>) -> Self {
        self.plan = self.plan.content_type(content_type);
        self
    }

    /// Prefer the browser-video WebView fallback when possible.
    pub fn prefer_webview(mut self) -> Self {
        self.plan = self.plan.prefer_webview();
        self
    }

    /// Configure browser-video fallback options.
    pub fn webview_options(mut self, options: WebViewVideoOptions) -> Self {
        self.plan = self.plan.webview_options(options);
        self
    }

    /// Configure initial media controls for the generated player.
    pub fn initial_controls(mut self, controls: VideoPlaybackControlsBuilder) -> Self {
        self.initial_controls = Some(controls);
        self
    }

    /// Configure playlist intent for next/previous media keys.
    pub fn playlist(mut self, playlist: VideoPlaylist) -> Self {
        self.playlist = Some(playlist);
        self
    }

    /// Replace the default URL-player promise with explicit requirements.
    pub fn requirements(
        mut self,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> Self {
        self.requirements = requirements.into_iter().collect();
        self
    }

    /// Request every known playback affordance for audit/roadmap output.
    pub fn all_requirements(mut self) -> Self {
        self.requirements = VideoPlaybackRequirement::all().to_vec();
        self
    }

    /// Number of requested requirements.
    pub fn requirement_count(&self) -> usize {
        self.requirements.len()
    }

    /// Return whether initial controls are configured.
    pub fn has_initial_controls(&self) -> bool {
        self.initial_controls.is_some()
    }

    /// Return whether playlist intent is configured.
    pub fn has_playlist(&self) -> bool {
        self.playlist.is_some()
    }

    /// Return a content-safe setup summary.
    pub fn to_text(&self) -> String {
        format!(
            "video element handoff builder: source {}, content type {}, prefer webview {}, controls {}, playlist {}, requirements {}",
            self.plan.source_kind(),
            self.plan.has_content_type(),
            self.plan.prefers_webview(),
            self.has_initial_controls(),
            self.has_playlist(),
            self.requirement_count()
        )
    }

    /// Validate the handoff without consuming the builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.requirements.is_empty(),
            "video element handoff must request at least one requirement"
        );
        self.plan.validate()?;
        if let Some(controls) = &self.initial_controls {
            controls.validate()?;
        }
        if let Some(playlist) = &self.playlist {
            playlist.validate()?;
        }
        Ok(())
    }

    /// Build the checked video element handoff.
    pub fn build_checked(self) -> anyhow::Result<VideoElementHandoff> {
        self.validate()?;
        let controls = self
            .initial_controls
            .map(VideoPlaybackControlsBuilder::build_checked)
            .transpose()?;
        let playlist = self
            .playlist
            .map(|playlist| playlist.checked())
            .transpose()?;
        let plan = self.plan.build_checked()?;
        Ok(VideoElementHandoff::new(
            plan,
            controls,
            playlist,
            self.requirements,
        ))
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

    /// Return the configured source kind without exposing URLs, paths, or keys.
    pub fn source_kind(&self) -> &'static str {
        self.source.source_kind()
    }

    /// Return whether MIME/content type was supplied.
    pub fn has_content_type(&self) -> bool {
        self.content_type.is_some()
    }

    /// Return whether browser-video fallback is preferred.
    pub fn prefers_webview(&self) -> bool {
        self.prefer_webview
    }

    /// Return a content-safe summary before building the playback plan.
    pub fn to_text(&self) -> String {
        format!(
            "video playback plan builder: source {}, content type {}, prefer webview {}, webview text tracks {}, webview start position {}",
            self.source_kind(),
            self.has_content_type(),
            self.prefers_webview(),
            self.webview_options.text_track_count(),
            self.webview_options.has_start_position()
        )
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

    /// Build a checked playback plan and evaluate requested affordances.
    pub fn build_requirement_plan_checked(
        self,
        requirements: impl IntoIterator<Item = VideoPlaybackRequirement>,
    ) -> anyhow::Result<VideoPlaybackRequirementPlan> {
        Ok(self.build_checked()?.requirement_plan(requirements))
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

/// Builder for validated native video text tracks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextTrackBuilder {
    track: TextTrack,
}

impl TextTrackBuilder {
    /// Create a builder from already parsed text-track cues.
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        kind: TextTrackKind,
        cues: Vec<TextTrackCue>,
    ) -> Self {
        Self {
            track: TextTrack::new(id, label, language, kind, cues),
        }
    }

    /// Parse a SubRip document into a checked subtitle text track builder.
    pub fn srt(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        Self {
            track: TextTrack::from_srt(id, label, language, input),
        }
    }

    /// Parse a WebVTT document into a checked subtitle text track builder.
    pub fn webvtt(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        Self {
            track: TextTrack::from_webvtt(id, label, language, input),
        }
    }

    /// Validate the text track without consuming the builder.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_text_track(&self.track)
    }

    /// Build the validated text track.
    pub fn build_checked(self) -> anyhow::Result<TextTrack> {
        self.validate()?;
        Ok(self.track)
    }
}

impl From<TextTrack> for TextTrackBuilder {
    fn from(track: TextTrack) -> Self {
        Self { track }
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

    /// Validate and return JavaScript for controlling this controller's browser fallback.
    pub fn webview_command_script_checked(
        &self,
        command: WebViewVideoCommandBuilder,
    ) -> anyhow::Result<SharedString> {
        Ok(self.webview_command_script(command.build_checked()?))
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

    /// Validate and dispatch a command to this controller's WebView-hosted browser fallback.
    pub fn dispatch_webview_command_checked(
        &self,
        window: &mut Window,
        command: WebViewVideoCommandBuilder,
    ) -> anyhow::Result<()> {
        self.dispatch_webview_command(window, command.build_checked()?)
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

    /// Validate and replace the media source.
    ///
    /// This is the preferred path for generated player code and runtime `src`
    /// changes. Use [`MediaSourceBuilder`] when file existence checks,
    /// canonicalization, or an already-composed source should be validated
    /// before the controller resets playback state.
    pub fn set_source_checked(
        &self,
        source: impl Into<MediaSourceBuilder>,
    ) -> anyhow::Result<MediaSource> {
        let source = source.into().build_checked()?;
        self.set_source(source.clone());
        Ok(source)
    }

    /// Replace the source with a URL.
    pub fn set_url(&self, url: impl Into<Arc<str>>) {
        self.set_source(MediaSource::url(url));
    }

    /// Validate and replace the source with a URL.
    pub fn set_url_checked(&self, url: impl Into<Arc<str>>) -> anyhow::Result<MediaSource> {
        self.set_source_checked(MediaSourceBuilder::url(url))
    }

    /// Replace the source with a local file path.
    pub fn set_file(&self, path: impl Into<PathBuf>) {
        self.set_source(MediaSource::file(path));
    }

    /// Validate and replace the source with a local file path.
    pub fn set_file_checked(&self, path: impl Into<PathBuf>) -> anyhow::Result<MediaSource> {
        self.set_source_checked(MediaSourceBuilder::file(path))
    }

    /// Replace the source with in-memory media bytes.
    pub fn set_bytes(&self, bytes: impl Into<Arc<[u8]>>) {
        self.set_source(MediaSource::bytes(bytes));
    }

    /// Validate and replace the source with in-memory media bytes.
    pub fn set_bytes_checked(&self, bytes: impl Into<Arc<[u8]>>) -> anyhow::Result<MediaSource> {
        self.set_source_checked(MediaSourceBuilder::bytes(bytes))
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

    /// Validate and replace the source with a keyed reader factory.
    pub fn set_reader_checked<R>(
        &self,
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
    ) -> anyhow::Result<MediaSource>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        self.set_source_checked(MediaSourceBuilder::reader(key, open))
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

    /// Validate and add a parsed text track.
    ///
    /// Generated caption/subtitle setup should prefer this path so empty
    /// metadata, empty parsed cue sets, invalid cue ranges, and duplicate track
    /// ids fail before the controller changes state.
    pub fn add_text_track_checked(
        &self,
        track: impl Into<TextTrackBuilder>,
    ) -> anyhow::Result<TextTrack> {
        let track = track.into().build_checked()?;
        {
            let state = self.state.borrow();
            anyhow::ensure!(
                !state
                    .text_tracks
                    .iter()
                    .any(|existing| existing.id == track.id),
                "video text track id is already configured"
            );
        }
        self.add_text_track(track.clone());
        Ok(track)
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

    /// Parse, validate, and add a SubRip subtitle track.
    pub fn add_srt_text_track_checked(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> anyhow::Result<TextTrack> {
        self.add_text_track_checked(TextTrackBuilder::srt(id, label, language, input))
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

    /// Parse, validate, and add a WebVTT subtitle track.
    pub fn add_webvtt_text_track_checked(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> anyhow::Result<TextTrack> {
        self.add_text_track_checked(TextTrackBuilder::webvtt(id, label, language, input))
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
        state.selected_text_track = resolve_text_track_index(&state.text_tracks, id);
        let selected_id = state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .map(|track| track.id.clone());
        state.last_active_text_cues.clear();
        state
            .events
            .push_back(VideoEvent::TextTrackChanged { id: selected_id });
    }

    /// Validate and select a text track by id.
    ///
    /// Unlike [`Self::select_text_track`], this fails for empty, malformed, or
    /// unknown ids and leaves the current selection unchanged.
    pub fn select_text_track_checked(&self, id: impl AsRef<str>) -> anyhow::Result<TextTrack> {
        let id = id.as_ref();
        validate_text_track_selector(id)?;
        let mut state = self.state.borrow_mut();
        let index = resolve_text_track_index(&state.text_tracks, id)
            .ok_or_else(|| anyhow!("video text track selector did not match a configured track"))?;
        let track = state.text_tracks[index].clone();
        state.selected_text_track = Some(index);
        state.last_active_text_cues.clear();
        state.events.push_back(VideoEvent::TextTrackChanged {
            id: Some(track.id.clone()),
        });
        Ok(track)
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

    /// Disable the selected text track only when captions/subtitles are active.
    pub fn disable_text_track_checked(&self) -> anyhow::Result<TextTrack> {
        let mut state = self.state.borrow_mut();
        let previous = state
            .selected_text_track
            .and_then(|index| state.text_tracks.get(index))
            .cloned();
        anyhow::ensure!(
            previous.is_some(),
            "video text tracks are already disabled or unavailable"
        );
        state.selected_text_track = None;
        state.last_active_text_cues.clear();
        state
            .events
            .push_back(VideoEvent::TextTrackChanged { id: None });
        Ok(previous.expect("checked above"))
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

    /// Validate and apply a batch of common video controls.
    pub fn apply_controls_checked(
        &self,
        controls: VideoPlaybackControlsBuilder,
    ) -> anyhow::Result<VideoPlaybackControls> {
        let controls = controls.build_checked()?;
        if let Some(volume) = controls.volume {
            self.set_volume(volume);
        }
        if let Some(muted) = controls.muted {
            self.set_muted(muted);
        }
        if let Some(playback_rate) = controls.playback_rate {
            self.set_playback_rate(playback_rate);
        }
        if let Some(looping) = controls.looping {
            self.set_looping(looping);
        }
        if let Some(position) = controls.seek_position {
            if controls.fast_seek {
                self.fast_seek(position)?;
            } else {
                self.seek(position)?;
            }
        }
        Ok(controls)
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
/// `media can-play checks`.
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

fn media_source_kind(source: &MediaSource) -> &'static str {
    match source {
        MediaSource::File(_) => "file",
        MediaSource::Url(_) => "url",
        MediaSource::Bytes(_) => "bytes",
        MediaSource::Reader(_) => "reader",
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

fn validate_video_volume(volume: f32) -> anyhow::Result<()> {
    anyhow::ensure!(volume.is_finite(), "video volume must be finite");
    anyhow::ensure!(
        (0.0..=1.0).contains(&volume),
        "video volume must be between 0.0 and 1.0"
    );
    Ok(())
}

fn validate_video_playback_rate(playback_rate: f32) -> anyhow::Result<()> {
    anyhow::ensure!(
        playback_rate.is_finite(),
        "video playback rate must be finite"
    );
    anyhow::ensure!(
        (0.0625..=16.0).contains(&playback_rate),
        "video playback rate must be between 0.0625 and 16.0"
    );
    Ok(())
}

fn validate_video_seek_position(position: Duration) -> anyhow::Result<()> {
    anyhow::ensure!(
        position.as_secs() <= 7 * 24 * 60 * 60,
        "video seek position cannot exceed 7 days"
    );
    Ok(())
}

fn validate_video_seek_seconds(seconds: f64) -> anyhow::Result<()> {
    anyhow::ensure!(seconds.is_finite(), "video seek seconds must be finite");
    anyhow::ensure!(seconds >= 0.0, "video seek seconds cannot be negative");
    anyhow::ensure!(
        seconds <= 7.0 * 24.0 * 60.0 * 60.0,
        "video seek seconds cannot exceed 7 days"
    );
    Ok(())
}

fn webview_video_command_kind(command: &WebViewVideoCommand) -> &'static str {
    match command {
        WebViewVideoCommand::Play => "play",
        WebViewVideoCommand::Pause => "pause",
        WebViewVideoCommand::TogglePlay => "toggle play",
        WebViewVideoCommand::Stop => "stop",
        WebViewVideoCommand::Seek(_) => "seek",
        WebViewVideoCommand::FastSeek(_) => "fast seek",
        WebViewVideoCommand::SetVolume(_) => "volume",
        WebViewVideoCommand::SetMuted(_) => "muted",
        WebViewVideoCommand::SetPlaybackRate(_) => "playback rate",
        WebViewVideoCommand::SetLooping(_) => "looping",
        WebViewVideoCommand::SelectTextTrack(_) => "select text track",
        WebViewVideoCommand::DisableTextTracks => "disable text tracks",
        WebViewVideoCommand::RequestFullscreen => "request fullscreen",
        WebViewVideoCommand::ExitFullscreen => "exit fullscreen",
        WebViewVideoCommand::RequestPictureInPicture => "request picture-in-picture",
        WebViewVideoCommand::ExitPictureInPicture => "exit picture-in-picture",
        WebViewVideoCommand::RequestSnapshot => "request snapshot",
    }
}

fn validate_webview_video_command(command: &WebViewVideoCommand) -> anyhow::Result<()> {
    match command {
        WebViewVideoCommand::Seek(position) | WebViewVideoCommand::FastSeek(position) => {
            validate_video_seek_position(*position)
        }
        WebViewVideoCommand::SetVolume(volume) => validate_video_volume(*volume),
        WebViewVideoCommand::SetPlaybackRate(playback_rate) => {
            validate_video_playback_rate(*playback_rate)
        }
        WebViewVideoCommand::SelectTextTrack(selector) => {
            validate_webview_text_track_selector(selector.as_ref())
        }
        WebViewVideoCommand::Play
        | WebViewVideoCommand::Pause
        | WebViewVideoCommand::TogglePlay
        | WebViewVideoCommand::Stop
        | WebViewVideoCommand::SetMuted(_)
        | WebViewVideoCommand::SetLooping(_)
        | WebViewVideoCommand::DisableTextTracks
        | WebViewVideoCommand::RequestFullscreen
        | WebViewVideoCommand::ExitFullscreen
        | WebViewVideoCommand::RequestPictureInPicture
        | WebViewVideoCommand::ExitPictureInPicture
        | WebViewVideoCommand::RequestSnapshot => Ok(()),
    }
}

fn validate_webview_text_track_selector(selector: &str) -> anyhow::Result<()> {
    validate_text_track_selector(selector)
}

fn validate_text_track_selector(selector: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !selector.trim().is_empty(),
        "video text track selector cannot be empty"
    );
    anyhow::ensure!(
        selector.len() <= 256,
        "video text track selector cannot exceed 256 bytes"
    );
    anyhow::ensure!(
        !selector.chars().any(char::is_control),
        "video text track selector cannot contain control characters"
    );
    Ok(())
}

fn validate_text_track(track: &TextTrack) -> anyhow::Result<()> {
    validate_text_track_selector(track.id.as_ref())?;
    validate_non_empty_trimmed(&track.label, "video text track label")?;
    if let Some(language) = &track.language {
        validate_non_empty_trimmed(language, "video text track language")?;
    }
    anyhow::ensure!(
        !track.cues.is_empty(),
        "video text track must include at least one cue"
    );
    for cue in &track.cues {
        anyhow::ensure!(
            cue.start < cue.end,
            "video text track cue start must be before cue end"
        );
        validate_non_empty_trimmed(&cue.text, "video text track cue text")?;
    }
    Ok(())
}

fn resolve_text_track_index(text_tracks: &[TextTrack], selector: &str) -> Option<usize> {
    text_tracks
        .iter()
        .position(|track| track.id.as_ref() == selector)
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

    /// Return whether the playlist has a current source.
    pub fn has_current_source(&self) -> bool {
        self.current_index().is_some()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "video playlist: sources {}, current {}, repeat {}",
            self.len(),
            self.has_current_source(),
            self.repeat_enabled()
        )
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

    /// Return whether play/pause/stop keys are routed to audio.
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }

    /// Return whether play/pause/stop keys are routed to video.
    pub fn has_video(&self) -> bool {
        self.video.is_some()
    }

    /// Return whether next/previous keys are routed through a playlist.
    pub fn has_playlist(&self) -> bool {
        self.playlist.is_some()
    }

    /// Return whether a next-track callback is configured.
    pub fn has_next_track_callback(&self) -> bool {
        self.on_next_track.is_some()
    }

    /// Return whether a previous-track callback is configured.
    pub fn has_previous_track_callback(&self) -> bool {
        self.on_previous_track.is_some()
    }

    /// Return whether an unhandled-key callback is configured.
    pub fn has_unhandled_callback(&self) -> bool {
        self.on_unhandled.is_some()
    }

    /// Return the configured playlist source count, if present.
    pub fn playlist_source_count(&self) -> usize {
        self.playlist
            .as_ref()
            .map(VideoPlaylist::len)
            .unwrap_or_default()
    }

    /// Return a content-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "media-key binding: audio {}, video {}, playlist {}, playlist sources {}, next callback {}, previous callback {}, unhandled callback {}",
            self.has_audio(),
            self.has_video(),
            self.has_playlist(),
            self.playlist_source_count(),
            self.has_next_track_callback(),
            self.has_previous_track_callback(),
            self.has_unhandled_callback()
        )
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
        TextTrackBuilder, TextTrackCue, TextTrackKind, TimeRange, VideoCanPlay,
        VideoCapabilityStatus, VideoController, VideoElementCustomizationFeature,
        VideoElementCustomizationNextAction, VideoElementCustomizationPlanBuilder,
        VideoElementCustomizationStatus, VideoElementHandoffBuilder, VideoElementHandoffNextAction,
        VideoEvent, VideoPlaybackControlsBuilder, VideoPlaybackPlanBuilder,
        VideoPlaybackPlanTarget, VideoPlaybackRenderInstruction, VideoPlaybackRequirement,
        VideoPlaybackRequirementNextAction, VideoPlaybackRequirementStatus, VideoPlaylist,
        VideoReadyState, VideoUrlPlaybackHandoff, WebViewVideoCommand, WebViewVideoCommandBuilder,
        WebViewVideoCrossOrigin, WebViewVideoOptions, WebViewVideoPreload, WebViewVideoTextTrack,
        buffer_strategy_for_motion, buffered_ranges_for_source, can_play_video_source,
        can_play_video_type, parse_text_track_timestamp, push_ready_state_events,
        recommended_video_playback_route, recommended_video_playback_route_for_type,
        video_capability_report, video_frame_delay, webview_video_player_command_script,
        webview_video_player_id, webview_video_player_url,
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
        assert_eq!(VideoCanPlay::No.to_text(), "no");
        assert_eq!(VideoCanPlay::Maybe.to_text(), "maybe");
        assert_eq!(VideoCanPlay::Probably.to_text(), "probably");
        assert_eq!(
            VideoController::file("movie.mp4").can_play_source(),
            VideoCanPlay::Probably
        );
    }

    #[test]
    fn video_playback_plan_builds_native_url_player() {
        let builder = VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
            .webview_options(WebViewVideoOptions::default().controls(false));

        assert_eq!(builder.source_kind(), "url");
        assert!(!builder.has_content_type());
        assert!(!builder.prefers_webview());
        assert_eq!(
            builder.to_text(),
            "video playback plan builder: source url, content type false, prefer webview false, webview text tracks 0, webview start position false"
        );
        assert!(!builder.to_text().contains("cdn.example.com"));

        let plan = builder.build_checked().unwrap();

        assert!(plan.target().is_native());
        assert!(plan.route().is_native());
        assert_eq!(plan.can_play(), VideoCanPlay::Probably);
        assert_eq!(plan.content_type(), None);
        assert!(!plan.webview_options().controls);
        assert!(plan.webview_page_url().is_none());
        assert!(matches!(plan.source(), MediaSource::Url(_)));
        assert!(matches!(plan.controller().source(), MediaSource::Url(_)));
        assert_eq!(
            plan.to_text(),
            "video playback plan: source url, target native, route native, can play probably, content type false, webview text tracks 0"
        );
        assert!(!plan.to_text().contains("cdn.example.com"));
        assert_eq!(plan.target().to_text(), "native");

        match plan.render_instruction() {
            ref instruction @ VideoPlaybackRenderInstruction::Native { ref controller } => {
                assert!(matches!(controller.source(), MediaSource::Url(_)));
                assert_eq!(
                    instruction.to_text(),
                    "video render instruction: target native, controller true"
                );
            }
            VideoPlaybackRenderInstruction::WebViewFallback { .. } => {
                panic!("expected native render instruction")
            }
        }
    }

    #[test]
    fn video_playback_plan_routes_adaptive_streams_to_webview() {
        let builder = VideoPlaybackPlanBuilder::url("https://cdn.example.com/live?id=123")
            .content_type("application/vnd.apple.mpegurl; charset=utf-8")
            .webview_options(
                WebViewVideoOptions::default()
                    .autoplay(true)
                    .muted(true)
                    .object_fit("cover"),
            );

        assert!(builder.has_content_type());
        assert!(!builder.prefers_webview());
        assert_eq!(
            builder.to_text(),
            "video playback plan builder: source url, content type true, prefer webview false, webview text tracks 0, webview start position false"
        );
        assert!(!builder.to_text().contains("live?id"));
        assert!(!builder.to_text().contains("mpegurl"));

        let plan = builder.build_checked().unwrap();

        assert!(plan.target().is_webview_fallback());
        assert!(plan.route().should_use_webview());
        assert_eq!(plan.can_play(), VideoCanPlay::No);
        assert_eq!(
            plan.content_type(),
            Some("application/vnd.apple.mpegurl; charset=utf-8")
        );
        assert_eq!(
            plan.to_text(),
            "video playback plan: source url, target webview fallback, route webview recommended, can play no, content type true, webview text tracks 0"
        );
        assert!(!plan.to_text().contains("cdn.example.com"));
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

        match plan.render_instruction() {
            ref instruction @ VideoPlaybackRenderInstruction::WebViewFallback {
                ref page_url,
                ref element_id,
                ref reason,
            } => {
                assert!(page_url.starts_with("data:text/html"));
                assert!(element_id.starts_with("kael-video-player-"));
                assert!(reason.contains("HLS"));
                assert_eq!(
                    instruction.to_text(),
                    "video render instruction: target webview fallback, controller false"
                );
                assert!(!instruction.to_text().contains("data:text/html"));
                assert!(!instruction.to_text().contains("HLS"));
            }
            VideoPlaybackRenderInstruction::Native { .. } => {
                panic!("expected WebView render instruction")
            }
        }
    }

    #[test]
    fn video_url_playback_handoff_builds_native_url_player() {
        let handoff =
            VideoUrlPlaybackHandoff::url("https://cdn.example.com/private/movie.mp4").unwrap();

        assert!(handoff.is_native());
        assert!(!handoff.uses_webview_fallback());
        assert!(handoff.route().is_native());
        assert_eq!(handoff.can_play(), VideoCanPlay::Probably);
        assert!(handoff.webview_page_url().is_none());
        assert!(handoff.webview_element_id().is_none());
        assert!(matches!(handoff.controller().source(), MediaSource::Url(_)));
        assert!(matches!(handoff.plan().source(), MediaSource::Url(_)));

        match handoff.render_instruction() {
            VideoPlaybackRenderInstruction::Native { controller } => {
                assert!(matches!(controller.source(), MediaSource::Url(_)));
            }
            VideoPlaybackRenderInstruction::WebViewFallback { .. } => {
                panic!("expected native render instruction")
            }
        }

        let summary = handoff.to_text();
        assert_eq!(
            summary,
            "video URL playback handoff: target native, route native, can play probably, controller true, webview page false, webview element false"
        );
        assert!(!summary.contains("cdn.example.com"));
        assert!(!summary.contains("private"));
        assert!(!summary.contains("movie.mp4"));
    }

    #[test]
    fn video_url_playback_handoff_reports_baseline_requirements() {
        let handoff =
            VideoUrlPlaybackHandoff::url("https://cdn.example.com/private/movie.mp4").unwrap();

        assert_eq!(VideoPlaybackRequirement::all().len(), 11);
        assert_eq!(
            VideoPlaybackRequirement::url_player_baseline(),
            &[
                VideoPlaybackRequirement::BasicPlayback,
                VideoPlaybackRequirement::SourceReplacement,
                VideoPlaybackRequirement::CanPlayProbe,
                VideoPlaybackRequirement::TextTracks,
                VideoPlaybackRequirement::FastSeek,
                VideoPlaybackRequirement::PlaybackRate,
                VideoPlaybackRequirement::Fullscreen,
            ]
        );

        let baseline = handoff.baseline_requirement_plan();
        assert!(baseline.target().is_native());
        assert_eq!(baseline.requirement_count(), 7);
        assert_eq!(baseline.satisfied_count(), 5);
        assert_eq!(baseline.limited_count(), 2);
        assert_eq!(baseline.missing_count(), 0);
        assert_eq!(
            baseline.limited_requirements(),
            vec![
                VideoPlaybackRequirement::FastSeek,
                VideoPlaybackRequirement::PlaybackRate
            ]
        );
        assert_eq!(
            handoff.baseline_next_action(),
            VideoPlaybackRequirementNextAction::AcceptLimitedSupport
        );
        assert!(!handoff.baseline_ready());
        assert!(!baseline.requires_webview_fallback());
        assert!(!baseline.requires_native_backend_work());
        assert!(!baseline.to_text().contains("cdn.example.com"));
        assert!(!baseline.to_text().contains("private"));

        let full = handoff.full_requirement_plan();
        assert_eq!(
            full.requirement_count(),
            VideoPlaybackRequirement::all().len()
        );
        assert!(full.requires_webview_fallback());
        assert!(full.requires_native_backend_work());
        assert_eq!(
            full.next_action_for(VideoPlaybackRequirement::PictureInPicture),
            Some(VideoPlaybackRequirementNextAction::UseWebViewFallback)
        );
        assert_eq!(
            full.next_action_for(VideoPlaybackRequirement::HardwareDecode),
            Some(VideoPlaybackRequirementNextAction::BuildNativeBackend)
        );
    }

    #[test]
    fn video_element_handoff_builds_url_player_with_controls_and_playlist() {
        let playlist = VideoPlaylist::new([
            MediaSource::url("https://cdn.example.com/private/movie.mp4"),
            MediaSource::url("https://cdn.example.com/private/trailer.mp4"),
        ])
        .repeat(true);

        let builder = VideoElementHandoffBuilder::url("https://cdn.example.com/private/movie.mp4")
            .initial_controls(
                VideoPlaybackControlsBuilder::new()
                    .volume(0.4)
                    .muted(true)
                    .playback_rate(1.25)
                    .looping(true),
            )
            .playlist(playlist);

        assert_eq!(builder.requirement_count(), 7);
        assert!(builder.has_initial_controls());
        assert!(builder.has_playlist());
        assert_eq!(
            builder.to_text(),
            "video element handoff builder: source url, content type false, prefer webview false, controls true, playlist true, requirements 7"
        );
        assert!(!builder.to_text().contains("cdn.example.com"));

        let handoff = builder.build_checked().unwrap();
        assert!(handoff.is_native());
        assert!(!handoff.uses_webview_fallback());
        assert!(handoff.has_initial_controls());
        assert!(handoff.has_playlist());
        assert_eq!(handoff.playlist_source_count(), 2);
        assert_eq!(
            handoff.next_action(),
            VideoElementHandoffNextAction::AcceptLimitedSupport
        );
        assert!(!handoff.is_ready());
        assert_eq!(
            handoff.requirement_plan().limited_requirements(),
            vec![
                VideoPlaybackRequirement::FastSeek,
                VideoPlaybackRequirement::PlaybackRate
            ]
        );

        let controller = handoff.controller_checked().unwrap();
        assert_eq!(controller.volume_level(), 0.4);
        assert!(controller.is_muted());
        assert_eq!(controller.playback_rate_value(), 1.25);
        assert!(controller.is_looping());

        let media_keys = handoff
            .media_key_binding_builder_checked()
            .unwrap()
            .unwrap();
        assert!(media_keys.has_video());
        assert!(media_keys.has_playlist());
        assert_eq!(media_keys.playlist_source_count(), 2);

        assert_eq!(
            handoff.to_text(),
            "video element handoff: target native, route native, can play probably, controls true, playlist true, playlist sources 2, requirements 7, satisfied 5, limited 2, missing 0, next action accept limited support"
        );
        assert!(!handoff.to_text().contains("cdn.example.com"));
        assert!(!handoff.to_text().contains("private"));
    }

    #[test]
    fn video_element_handoff_routes_adaptive_player_to_webview_fallback() {
        let handoff = VideoElementHandoffBuilder::url("https://cdn.example.com/live/master.m3u8")
            .content_type("application/vnd.apple.mpegurl")
            .requirements([
                VideoPlaybackRequirement::BasicPlayback,
                VideoPlaybackRequirement::AdaptiveStreaming,
                VideoPlaybackRequirement::PictureInPicture,
            ])
            .build_checked()
            .unwrap();

        assert!(!handoff.is_native());
        assert!(handoff.uses_webview_fallback());
        assert!(handoff.is_ready());
        assert_eq!(
            handoff.next_action(),
            VideoElementHandoffNextAction::RenderWebViewFallback
        );
        match handoff.render_instruction() {
            VideoPlaybackRenderInstruction::WebViewFallback { element_id, .. } => {
                assert!(element_id.starts_with("kael-video-player-"));
            }
            VideoPlaybackRenderInstruction::Native { .. } => panic!("expected WebView fallback"),
        }
        assert_eq!(
            handoff.to_text(),
            "video element handoff: target webview fallback, route webview recommended, can play no, controls false, playlist false, playlist sources 0, requirements 3, satisfied 3, limited 0, missing 0, next action render webview fallback"
        );
        assert!(!handoff.to_text().contains("master.m3u8"));
        assert!(!handoff.to_text().contains("mpegurl"));
    }

    #[test]
    fn video_element_customization_plan_audits_html_video_like_controls() {
        let playlist = VideoPlaylist::new([
            MediaSource::url("https://cdn.example.com/private/movie.mp4"),
            MediaSource::url("https://cdn.example.com/private/trailer.mp4"),
        ]);
        let handoff = VideoElementHandoffBuilder::url("https://cdn.example.com/private/movie.mp4")
            .initial_controls(
                VideoPlaybackControlsBuilder::new()
                    .volume(0.5)
                    .playback_rate(1.25),
            )
            .playlist(playlist)
            .build_checked()
            .unwrap();

        let plan = VideoElementCustomizationPlanBuilder::new(handoff)
            .html_video_baseline()
            .timeline_scrubbing()
            .captions_ui()
            .fullscreen()
            .playlist_media_keys()
            .build_checked()
            .unwrap();

        assert_eq!(plan.feature_count(), 8);
        assert_eq!(plan.event_handler_count(), 8);
        assert_eq!(plan.custom_control_count(), 5);
        assert!(plan.has_feature(VideoElementCustomizationFeature::CustomControls));
        assert!(plan.has_feature(VideoElementCustomizationFeature::PlaylistMediaKeys));
        assert_eq!(plan.missing_count(), 0);
        assert_eq!(plan.limited_count(), 2);
        assert_eq!(
            plan.next_action(),
            VideoElementCustomizationNextAction::AcceptLimitedSupport
        );
        assert!(!plan.is_ready());
        assert!(!plan.requires_webview_fallback());
        assert!(!plan.requires_native_backend_work());
        assert!(!plan.requires_playlist_or_handlers());

        let timeline = plan
            .findings()
            .iter()
            .find(|finding| {
                finding.feature() == VideoElementCustomizationFeature::TimelineScrubbing
            })
            .unwrap();
        assert_eq!(timeline.status(), VideoElementCustomizationStatus::Limited);
        assert_eq!(
            timeline.next_action(),
            VideoElementCustomizationNextAction::AcceptLimitedSupport
        );
        assert_eq!(
            plan.to_text(),
            "video element customization plan: features 8, satisfied 6, limited 2, missing 0, event handlers 8, custom controls 5, native true, webview fallback false, next action accept limited support"
        );
        assert!(!plan.to_text().contains("cdn.example.com"));
        assert!(!timeline.to_text().contains("movie.mp4"));
    }

    #[test]
    fn video_element_customization_plan_routes_browser_and_backend_gaps() {
        let native_handoff =
            VideoElementHandoffBuilder::url("https://cdn.example.com/private/movie.mp4")
                .all_requirements()
                .build_checked()
                .unwrap();
        let native_plan = VideoElementCustomizationPlanBuilder::new(native_handoff)
            .picture_in_picture()
            .feature(VideoElementCustomizationFeature::PlaylistMediaKeys)
            .feature(VideoElementCustomizationFeature::Properties)
            .build_checked()
            .unwrap();

        assert_eq!(native_plan.missing_count(), 2);
        assert!(native_plan.requires_webview_fallback());
        assert!(native_plan.requires_playlist_or_handlers());
        assert_eq!(
            native_plan.next_action(),
            VideoElementCustomizationNextAction::UseWebViewFallback
        );

        let backend_handoff =
            VideoElementHandoffBuilder::url("https://cdn.example.com/private/movie.mp4")
                .all_requirements()
                .build_checked()
                .unwrap();
        let backend_plan = VideoElementCustomizationPlanBuilder::new(backend_handoff)
            .hardware_decode()
            .build_checked()
            .unwrap();

        assert!(backend_plan.requires_native_backend_work());
        assert_eq!(
            backend_plan.next_action(),
            VideoElementCustomizationNextAction::BuildNativeBackend
        );
        assert!(!backend_plan.to_text().contains("private"));
    }

    #[test]
    fn video_element_customization_plan_rejects_invalid_generated_requests() {
        let handoff = VideoElementHandoffBuilder::url("https://cdn.example.com/private/movie.mp4")
            .build_checked()
            .unwrap();

        assert!(
            VideoElementCustomizationPlanBuilder::new(handoff.clone())
                .build_checked()
                .is_err()
        );
        assert!(
            VideoElementCustomizationPlanBuilder::new(handoff.clone())
                .event_handlers(65)
                .build_checked()
                .is_err()
        );
        assert!(
            VideoElementCustomizationPlanBuilder::new(handoff)
                .custom_controls(33)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn video_url_playback_handoff_routes_adaptive_streams_to_webview() {
        let handoff = VideoUrlPlaybackHandoff::url_with_content_type(
            "https://cdn.example.com/live/master.m3u8",
            "application/vnd.apple.mpegurl",
        )
        .unwrap();

        assert!(!handoff.is_native());
        assert!(handoff.uses_webview_fallback());
        assert!(handoff.route().should_use_webview());
        assert_eq!(handoff.can_play(), VideoCanPlay::No);
        assert!(
            handoff
                .webview_page_url()
                .unwrap()
                .starts_with("data:text/html")
        );
        assert!(
            handoff
                .webview_element_id()
                .unwrap()
                .starts_with("kael-video-player-")
        );

        match handoff.render_instruction() {
            VideoPlaybackRenderInstruction::WebViewFallback {
                page_url,
                element_id,
                reason,
            } => {
                assert!(page_url.starts_with("data:text/html"));
                assert!(element_id.starts_with("kael-video-player-"));
                assert!(reason.contains("HLS"));
            }
            VideoPlaybackRenderInstruction::Native { .. } => {
                panic!("expected WebView render instruction")
            }
        }

        let summary = handoff.to_text();
        assert_eq!(
            summary,
            "video URL playback handoff: target webview fallback, route webview recommended, can play no, controller true, webview page true, webview element true"
        );
        assert!(!summary.contains("cdn.example.com"));
        assert!(!summary.contains("master.m3u8"));
        assert!(!summary.contains("mpegurl"));
        assert!(!summary.contains("data:text/html"));
    }

    #[test]
    fn video_playback_requirement_plan_reports_native_route_gaps() {
        let plan = VideoPlaybackPlanBuilder::url("https://cdn.example.com/movie.mp4")
            .build_checked()
            .unwrap();

        let requirements = plan.requirement_plan([
            VideoPlaybackRequirement::BasicPlayback,
            VideoPlaybackRequirement::TextTracks,
            VideoPlaybackRequirement::PlaybackRate,
            VideoPlaybackRequirement::AdaptiveStreaming,
            VideoPlaybackRequirement::HardwareDecode,
            VideoPlaybackRequirement::BasicPlayback,
        ]);

        assert!(requirements.target().is_native());
        assert_eq!(requirements.requirement_count(), 5);
        assert_eq!(requirements.satisfied_count(), 2);
        assert_eq!(requirements.limited_count(), 1);
        assert_eq!(requirements.missing_count(), 2);
        assert_eq!(
            requirements.satisfied_requirements(),
            vec![
                VideoPlaybackRequirement::BasicPlayback,
                VideoPlaybackRequirement::TextTracks
            ]
        );
        assert_eq!(
            requirements.limited_requirements(),
            vec![VideoPlaybackRequirement::PlaybackRate]
        );
        assert_eq!(
            requirements.missing_requirements(),
            vec![
                VideoPlaybackRequirement::AdaptiveStreaming,
                VideoPlaybackRequirement::HardwareDecode
            ]
        );
        assert_eq!(
            requirements.webview_fallback_requirements(),
            vec![VideoPlaybackRequirement::AdaptiveStreaming]
        );
        assert_eq!(
            requirements.native_backend_work_requirements(),
            vec![VideoPlaybackRequirement::HardwareDecode]
        );
        assert!(requirements.requires_webview_fallback());
        assert!(requirements.requires_native_backend_work());
        assert_eq!(
            requirements.next_action(),
            VideoPlaybackRequirementNextAction::BuildNativeBackend
        );
        assert_eq!(
            requirements.next_action_for(VideoPlaybackRequirement::AdaptiveStreaming),
            Some(VideoPlaybackRequirementNextAction::UseWebViewFallback)
        );
        assert_eq!(
            requirements.next_action_for(VideoPlaybackRequirement::PlaybackRate),
            Some(VideoPlaybackRequirementNextAction::AcceptLimitedSupport)
        );
        assert!(requirements.has_gaps());
        assert!(!requirements.is_ready());
        assert_eq!(
            requirements.to_text(),
            "video playback requirements: target native, requested 5, satisfied 2, limited 1, missing 2, next action build native backend, ready false"
        );
        assert!(!requirements.to_text().contains("cdn.example.com"));

        let finding = requirements
            .findings()
            .iter()
            .find(|finding| finding.requirement() == VideoPlaybackRequirement::PlaybackRate)
            .unwrap();
        assert_eq!(finding.status(), VideoPlaybackRequirementStatus::Limited);
        assert_eq!(
            finding.to_text(),
            "video playback requirement: playback rate limited"
        );
        assert!(finding.is_limited());
    }

    #[test]
    fn video_playback_requirement_plan_covers_browser_fallback_requirements() {
        let requirements = VideoPlaybackPlanBuilder::url("https://cdn.example.com/live.m3u8")
            .content_type("application/vnd.apple.mpegurl")
            .build_requirement_plan_checked([
                VideoPlaybackRequirement::BasicPlayback,
                VideoPlaybackRequirement::AdaptiveStreaming,
                VideoPlaybackRequirement::PictureInPicture,
                VideoPlaybackRequirement::NativeTrackSelection,
            ])
            .unwrap();

        assert!(requirements.target().is_webview_fallback());
        assert_eq!(requirements.requirement_count(), 4);
        assert_eq!(requirements.satisfied_count(), 3);
        assert_eq!(requirements.limited_count(), 0);
        assert_eq!(requirements.missing_count(), 1);
        assert_eq!(
            requirements.missing_requirements(),
            vec![VideoPlaybackRequirement::NativeTrackSelection]
        );
        assert!(!requirements.requires_webview_fallback());
        assert!(requirements.requires_native_backend_work());
        assert_eq!(
            requirements.native_backend_work_requirements(),
            vec![VideoPlaybackRequirement::NativeTrackSelection]
        );
        assert_eq!(
            requirements.next_action_for(VideoPlaybackRequirement::AdaptiveStreaming),
            Some(VideoPlaybackRequirementNextAction::RenderPlannedRoute)
        );
        assert_eq!(
            requirements.to_text(),
            "video playback requirements: target webview fallback, requested 4, satisfied 3, limited 0, missing 1, next action build native backend, ready false"
        );
        assert!(!requirements.to_text().contains("live.m3u8"));
        assert_eq!(
            VideoPlaybackRequirement::HardwareDecode.to_text(),
            "hardware decode"
        );
        assert_eq!(VideoPlaybackRequirementStatus::Missing.to_text(), "missing");
        assert_eq!(
            VideoPlaybackRequirementNextAction::UseWebViewFallback.to_text(),
            "use webview fallback"
        );
    }

    #[test]
    fn video_playback_requirement_plan_summary_is_content_safe() {
        let plan = VideoPlaybackPlanBuilder::url("https://secret.example.com/private/movie.mp4")
            .content_type("video/mp4; codecs=\"secret-codec\"")
            .build_checked()
            .unwrap();

        let requirements = plan.requirement_plan([
            VideoPlaybackRequirement::CanPlayProbe,
            VideoPlaybackRequirement::FastSeek,
            VideoPlaybackRequirement::HardwareDecode,
        ]);

        assert_eq!(
            requirements.to_text(),
            "video playback requirements: target native, requested 3, satisfied 1, limited 1, missing 1, next action build native backend, ready false"
        );
        assert!(!requirements.to_text().contains("secret.example.com"));
        assert!(!requirements.to_text().contains("private"));
        assert!(!requirements.to_text().contains("secret-codec"));
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
        let source_builder = MediaSourceBuilder::url("https://cdn.example.com/private.mp4");
        assert_eq!(source_builder.source_kind(), "url");
        assert_eq!(
            source_builder.to_text(),
            "media source: kind url, require existing file false, canonicalize file false"
        );
        assert!(!source_builder.to_text().contains("private.mp4"));
        let file_builder = MediaSourceBuilder::file("/tmp/movie.mp4")
            .require_existing_file()
            .canonicalize_file();
        assert!(file_builder.requires_existing_file());
        assert!(file_builder.canonicalizes_file());
        assert_eq!(
            file_builder.to_text(),
            "media source: kind file, require existing file true, canonicalize file true"
        );
        assert!(!file_builder.to_text().contains("/tmp/movie.mp4"));

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
        assert_eq!(options.controls_list_count(), 2);
        assert_eq!(options.text_track_count(), 2);
        assert!(options.has_poster());
        assert!(!options.has_preload());
        assert!(!options.has_cross_origin());
        assert!(!options.has_start_position());
        assert_eq!(
            options.to_text(),
            "webview video options: controls true, autoplay false, muted false, looping false, plays inline true, poster true, preload false, cross origin false, controls-list 2, disable picture-in-picture false, start position false, text tracks 2, object fit cover"
        );
        assert!(!options.to_text().contains("poster.jpg"));
        assert!(!options.to_text().contains("captions.vtt"));
        assert!(!options.to_text().contains("WEBVTT"));
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
    fn webview_video_command_builder_validates_generated_commands() {
        assert!(WebViewVideoCommandBuilder::volume(0.5).validate().is_ok());
        let volume = WebViewVideoCommandBuilder::volume(0.5);
        assert_eq!(volume.command_kind(), "volume");
        assert!(!volume.is_seek_command());
        assert!(volume.is_audio_command());
        assert!(!volume.is_presentation_command());
        assert_eq!(
            volume.to_text(),
            "webview video command: kind volume, seek false, audio true, presentation false"
        );
        assert!(!volume.to_text().contains("0.5"));

        assert!(
            WebViewVideoCommandBuilder::volume(f32::NAN)
                .validate()
                .is_err()
        );
        assert!(WebViewVideoCommandBuilder::volume(1.5).validate().is_err());
        assert!(
            WebViewVideoCommandBuilder::playback_rate(1.25)
                .validate()
                .is_ok()
        );
        assert!(
            WebViewVideoCommandBuilder::playback_rate(0.0)
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoCommandBuilder::playback_rate(f32::INFINITY)
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoCommandBuilder::seek_secs(12.5)
                .validate()
                .is_ok()
        );
        let seek = WebViewVideoCommandBuilder::seek_secs(12.5);
        assert_eq!(seek.command_kind(), "seek");
        assert!(seek.is_seek_command());
        assert!(!seek.is_audio_command());
        assert_eq!(
            seek.to_text(),
            "webview video command: kind seek, seek true, audio false, presentation false"
        );
        assert!(!seek.to_text().contains("12.5"));

        assert!(
            WebViewVideoCommandBuilder::seek_secs(f64::NAN)
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoCommandBuilder::fast_seek_secs(-1.0)
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoCommandBuilder::select_text_track("en")
                .validate()
                .is_ok()
        );
        let track = WebViewVideoCommandBuilder::select_text_track("English captions");
        assert_eq!(track.command_kind(), "select text track");
        assert!(!track.is_seek_command());
        assert_eq!(
            track.to_text(),
            "webview video command: kind select text track, seek false, audio false, presentation false"
        );
        assert!(!track.to_text().contains("English"));

        assert!(
            WebViewVideoCommandBuilder::select_text_track("   ")
                .validate()
                .is_err()
        );
        assert!(
            WebViewVideoCommandBuilder::select_text_track("en\n")
                .validate()
                .is_err()
        );

        let command = WebViewVideoCommandBuilder::fast_seek_secs(2.5)
            .build_checked()
            .unwrap();
        assert_eq!(
            command,
            WebViewVideoCommand::FastSeek(Duration::from_secs_f64(2.5))
        );
        assert!(
            WebViewVideoCommandBuilder::command(WebViewVideoCommand::SetVolume(2.0))
                .build_checked()
                .is_err()
        );
        let fullscreen = WebViewVideoCommandBuilder::request_fullscreen();
        assert_eq!(fullscreen.command_kind(), "request fullscreen");
        assert!(fullscreen.is_presentation_command());
        assert_eq!(
            fullscreen.to_text(),
            "webview video command: kind request fullscreen, seek false, audio false, presentation true"
        );
    }

    #[test]
    fn video_controller_returns_checked_webview_command_scripts() {
        let controller = VideoController::url("https://cdn.example.com/movie.mp4");
        let script = controller
            .webview_command_script_checked(WebViewVideoCommandBuilder::playback_rate(1.5))
            .unwrap();
        assert!(script.contains("video.playbackRate=1.5"));

        assert!(
            controller
                .webview_command_script_checked(WebViewVideoCommandBuilder::playback_rate(f32::NAN))
                .is_err()
        );
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
        assert_eq!(VideoCapabilityStatus::Full.to_text(), "full");
        assert_eq!(VideoCapabilityStatus::Partial.to_text(), "partial");
        assert_eq!(VideoCapabilityStatus::Roadmap.to_text(), "roadmap");
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
        assert_eq!(report.full_count(), 8);
        assert_eq!(report.partial_count(), 2);
        assert_eq!(report.roadmap_count(), 3);
        assert_eq!(report.native_gap_count(), 5);
        assert_eq!(report.count_status(VideoCapabilityStatus::Full), 8);
        assert_eq!(report.count_status(VideoCapabilityStatus::Partial), 2);
        assert_eq!(report.count_status(VideoCapabilityStatus::Roadmap), 3);
        assert!(report.has_native_gaps());
        assert!(report.has_webview_fallback());
        assert_eq!(
            report.to_text(),
            "video capabilities: full 8, partial 2, roadmap 3, all full false"
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
        assert!(playlist.has_current_source());
        assert_eq!(
            playlist.to_text(),
            "video playlist: sources 3, current true, repeat false"
        );
        assert!(!playlist.to_text().contains("one.mp4"));
        assert!(!playlist.to_text().contains("example.com"));
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
        assert_eq!(
            playlist.to_text(),
            "video playlist: sources 1, current true, repeat true"
        );
        assert!(!playlist.to_text().contains("one.mp4"));
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
        let binding = MediaKeyBindingBuilder::new()
            .video(controller.clone())
            .playlist(VideoPlaylist::new([MediaSource::url(
                "https://example.com/two.mp4",
            )]))
            .on_next_track(|_| {})
            .on_unhandled(|_, _| {});
        assert!(binding.has_video());
        assert!(binding.has_playlist());
        assert_eq!(binding.playlist_source_count(), 1);
        assert!(binding.has_next_track_callback());
        assert!(!binding.has_previous_track_callback());
        assert!(binding.has_unhandled_callback());
        assert_eq!(
            binding.to_text(),
            "media-key binding: audio false, video true, playlist true, playlist sources 1, next callback true, previous callback false, unhandled callback true"
        );
        assert!(!binding.to_text().contains("two.mp4"));
        assert!(!binding.to_text().contains("example.com"));
        assert!(binding.validate().is_ok());

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
    fn video_playback_controls_builder_validates_generated_controls() {
        assert!(VideoPlaybackControlsBuilder::new().validate().is_err());
        assert!(
            VideoPlaybackControlsBuilder::new()
                .volume(0.5)
                .muted(true)
                .playback_rate(1.25)
                .looping(true)
                .fast_seek_secs(42.0)
                .validate()
                .is_ok()
        );
        let builder = VideoPlaybackControlsBuilder::new()
            .volume(0.5)
            .muted(true)
            .playback_rate(1.25)
            .fast_seek_secs(42.0);
        assert_eq!(builder.configured_update_count(), 4);
        assert_eq!(
            builder.to_text(),
            "video playback controls: updates 4, volume true, muted true, playback rate true, looping false, seek true, fast seek true"
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .volume(f32::NAN)
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .volume(1.1)
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .playback_rate(0.0)
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .playback_rate(f32::INFINITY)
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .seek_secs(f64::NAN)
                .validate()
                .is_err()
        );
        assert!(
            VideoPlaybackControlsBuilder::new()
                .seek_secs(-1.0)
                .validate()
                .is_err()
        );

        let controls = VideoPlaybackControlsBuilder::new()
            .volume(0.25)
            .fast_seek_secs(2.5)
            .build_checked()
            .unwrap();
        assert_eq!(controls.volume(), Some(0.25));
        assert_eq!(controls.seek_position(), Some(Duration::from_secs_f64(2.5)));
        assert!(controls.uses_fast_seek());
        assert_eq!(controls.update_count(), 2);
        assert_eq!(
            controls.to_text(),
            "video playback controls: updates 2, volume true, muted false, playback rate false, looping false, seek true, fast seek true"
        );
    }

    #[test]
    fn video_controller_applies_checked_control_batch() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));
        let controls = controller
            .apply_controls_checked(
                VideoPlaybackControlsBuilder::new()
                    .volume(0.6)
                    .muted(true)
                    .playback_rate(1.25)
                    .looping(true),
            )
            .unwrap();

        assert_eq!(controls.volume(), Some(0.6));
        assert_eq!(controls.muted(), Some(true));
        assert_eq!(controls.playback_rate(), Some(1.25));
        assert_eq!(controls.looping(), Some(true));

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.volume, 0.6);
        assert!(snapshot.muted);
        assert_eq!(snapshot.playback_rate, 1.25);
        assert!(snapshot.looping);
        assert_eq!(controller.audio_handle().speed(), 1.25);

        assert!(
            controller
                .apply_controls_checked(VideoPlaybackControlsBuilder::new().volume(2.0))
                .is_err()
        );
        assert_eq!(controller.volume_level(), 0.6);
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
    fn video_controller_checked_source_replacement_validates_before_mutation() {
        let controller = VideoController::url("https://cdn.example.com/initial.mp4")
            .volume(0.4)
            .muted(true)
            .playback_rate(1.5)
            .looping(true);
        controller.drain_events();

        assert!(
            controller
                .set_url_checked(" https://cdn.example.com/next.mp4")
                .is_err()
        );
        assert_eq!(
            controller.source(),
            MediaSource::url("https://cdn.example.com/initial.mp4")
        );
        assert!(controller.drain_events().is_empty());

        let source = controller
            .set_bytes_checked(Arc::<[u8]>::from([4, 5, 6]))
            .unwrap();
        assert_eq!(source, MediaSource::bytes(Arc::<[u8]>::from([4, 5, 6])));
        assert_eq!(controller.source(), source);
        assert_eq!(controller.volume_level(), 0.4);
        assert!(controller.is_muted());
        assert_eq!(controller.playback_rate_value(), 1.5);
        assert!(controller.is_looping());
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

        assert!(controller.set_bytes_checked(Arc::<[u8]>::from([])).is_err());
        assert_eq!(controller.source(), source);
        assert!(controller.drain_events().is_empty());
    }

    #[test]
    fn video_controller_checked_source_replacement_accepts_configured_builder() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([1]));

        assert!(
            controller
                .set_source_checked(MediaSourceBuilder::file("/definitely/not/a/movie.mp4"))
                .is_ok()
        );
        assert!(
            controller
                .set_source_checked(
                    MediaSourceBuilder::file("/definitely/not/a/movie.mp4").require_existing_file()
                )
                .is_err()
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
    fn video_controller_checked_text_track_selection_validates_before_mutation() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]))
            .srt_text_track(
                "en",
                "English",
                Some("en"),
                "1\n00:00:01,000 --> 00:00:04,000\nHello world\n",
            )
            .srt_text_track(
                "es",
                "Spanish",
                Some("es"),
                "1\n00:00:01,000 --> 00:00:04,000\nHola mundo\n",
            );
        controller.drain_events();

        let selected = controller.select_text_track_checked("es").unwrap();
        assert_eq!(selected.id.as_ref(), "es");
        assert_eq!(
            controller
                .selected_text_track_id()
                .map(|id| id.to_string())
                .as_deref(),
            Some("es")
        );
        assert_eq!(
            controller.drain_events(),
            vec![VideoEvent::TextTrackChanged {
                id: Some("es".into())
            }]
        );

        assert!(controller.select_text_track_checked("   ").is_err());
        assert!(controller.select_text_track_checked("es\n").is_err());
        assert!(controller.select_text_track_checked("fr").is_err());
        assert_eq!(
            controller
                .selected_text_track_id()
                .map(|id| id.to_string())
                .as_deref(),
            Some("es")
        );
        assert!(controller.drain_events().is_empty());

        let disabled = controller.disable_text_track_checked().unwrap();
        assert_eq!(disabled.id.as_ref(), "es");
        assert_eq!(controller.selected_text_track_id(), None);
        assert_eq!(
            controller.drain_events(),
            vec![VideoEvent::TextTrackChanged { id: None }]
        );
        assert!(controller.disable_text_track_checked().is_err());
        assert!(controller.drain_events().is_empty());
    }

    #[test]
    fn text_track_builder_validates_generated_tracks() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n";
        assert!(
            TextTrackBuilder::srt("en", "English", Some("en"), srt)
                .validate()
                .is_ok()
        );
        assert!(
            TextTrackBuilder::srt("", "English", Some("en"), srt)
                .validate()
                .is_err()
        );
        assert!(
            TextTrackBuilder::srt("en", "   ", Some("en"), srt)
                .validate()
                .is_err()
        );
        assert!(
            TextTrackBuilder::srt("en", "English", Some(""), srt)
                .validate()
                .is_err()
        );
        assert!(
            TextTrackBuilder::srt("en", "English", Some("en"), "not a caption")
                .validate()
                .is_err()
        );
        assert!(
            TextTrackBuilder::new(
                "en",
                "English",
                Some("en"),
                TextTrackKind::Subtitles,
                vec![TextTrackCue::new(
                    Duration::from_secs(3),
                    Duration::from_secs(2),
                    "backwards"
                )],
            )
            .validate()
            .is_err()
        );
        assert!(
            TextTrackBuilder::new(
                "en",
                "English",
                Some("en"),
                TextTrackKind::Subtitles,
                vec![TextTrackCue::new(
                    Duration::from_secs(1),
                    Duration::from_secs(2),
                    "   "
                )],
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn video_controller_adds_checked_text_tracks_before_mutation() {
        let controller = VideoController::bytes(Arc::<[u8]>::from([]));
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello world\n";

        let track = controller
            .add_srt_text_track_checked("en", "English", Some("en"), srt)
            .unwrap();
        assert_eq!(track.id.as_ref(), "en");
        assert_eq!(controller.text_tracks().len(), 1);
        assert_eq!(
            controller
                .selected_text_track_id()
                .map(|id| id.to_string())
                .as_deref(),
            Some("en")
        );
        assert_eq!(
            controller.drain_events(),
            vec![
                VideoEvent::TextTrackChanged {
                    id: Some("en".into())
                },
                VideoEvent::TextTrackAdded { id: "en".into() },
            ]
        );

        assert!(
            controller
                .add_srt_text_track_checked("en", "Duplicate", Some("en"), srt)
                .is_err()
        );
        assert!(
            controller
                .add_webvtt_text_track_checked("es", "Spanish", Some("es"), "not captions")
                .is_err()
        );
        assert_eq!(controller.text_tracks().len(), 1);
        assert!(controller.drain_events().is_empty());
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
