//! Audio playback APIs for GPUI applications.

use crate::{
    App, Asset, AssetLogger, Bounds, DefiniteLength, Element, GlobalElementId, ImageCacheError,
    InspectorElementId, IntoElement, LayoutId, Length, MediaKeyEvent, ObjectFit, Pixels,
    RenderImage, Style, StyleRefinement, Styled, Window, px,
};
use anyhow::anyhow;
use futures::Future;
use image::{Delay, Frame, ImageBuffer, Rgba};
use refineable::Refineable;
use smallvec::SmallVec;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
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
    buffer_strategy: VideoBufferStrategy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VideoBufferStrategy {
    backward_window: usize,
    forward_window: usize,
    cache_limit: usize,
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

/// Bind the app's media-key events to an audio playback handle.
pub fn bind_audio_media_keys(app: &App, handle: &AudioHandle) {
    let handle = handle.clone();
    app.on_media_key_event(move |event, _| match event {
        MediaKeyEvent::Play => {
            let _ = handle.play();
        }
        MediaKeyEvent::Pause => handle.pause(),
        MediaKeyEvent::PlayPause => {
            if handle.state() == MediaPlaybackState::Playing {
                handle.pause();
            } else {
                let _ = handle.play();
            }
        }
        MediaKeyEvent::Stop => handle.stop(),
        MediaKeyEvent::NextTrack | MediaKeyEvent::PreviousTrack => {}
    });
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
                .and_then(|previous| {
                    Some(
                        frames[index]
                            .timestamp
                            .saturating_sub(frames[previous].timestamp),
                    )
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
            self.decoder.restart()?;
            self.frames.clear();
            self.exhausted = false;
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
    let movement = previous_position.map(|previous_position| {
        if position >= previous_position {
            position - previous_position
        } else {
            previous_position - position
        }
    });
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
        MIN_VIDEO_FRAME_PREFETCH, buffer_strategy_for_motion, video_frame_delay,
    };
    use kael_media::VideoFrame;
    use std::{sync::Arc, time::Duration};

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
}
