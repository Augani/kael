use crate::{
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        icon_button::IconButton,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::{
    io::{Read, Seek},
    panic::Location,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoPlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Buffering,
}

impl VideoPlaybackState {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Buffering => "buffering",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoPlayerSize {
    Sm,
    #[default]
    Md,
    Lg,
    Full,
}

impl VideoPlayerSize {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Sm => "small",
            Self::Md => "medium",
            Self::Lg => "large",
            Self::Full => "full",
        }
    }

    pub fn dimensions(&self) -> (Pixels, Pixels) {
        match self {
            Self::Sm => (px(400.0), px(225.0)),
            Self::Md => (px(640.0), px(360.0)),
            Self::Lg => (px(854.0), px(480.0)),
            Self::Full => (px(1280.0), px(720.0)),
        }
    }

    pub fn controls_height(&self) -> Pixels {
        match self {
            Self::Sm => px(36.0),
            Self::Md => px(44.0),
            Self::Lg => px(52.0),
            Self::Full => px(56.0),
        }
    }

    pub fn icon_size(&self) -> Pixels {
        match self {
            Self::Sm => px(16.0),
            Self::Md => px(20.0),
            Self::Lg => px(24.0),
            Self::Full => px(28.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VideoPlaybackSpeed {
    Quarter,
    Half,
    ThreeQuarter,
    #[default]
    Normal,
    OneAndQuarter,
    OneAndHalf,
    Double,
}

impl VideoPlaybackSpeed {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Quarter => "quarter",
            Self::Half => "half",
            Self::ThreeQuarter => "three-quarter",
            Self::Normal => "normal",
            Self::OneAndQuarter => "one-and-quarter",
            Self::OneAndHalf => "one-and-half",
            Self::Double => "double",
        }
    }

    pub fn multiplier(&self) -> f32 {
        match self {
            Self::Quarter => 0.25,
            Self::Half => 0.5,
            Self::ThreeQuarter => 0.75,
            Self::Normal => 1.0,
            Self::OneAndQuarter => 1.25,
            Self::OneAndHalf => 1.5,
            Self::Double => 2.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::ThreeQuarter => "0.75x",
            Self::Normal => "1x",
            Self::OneAndQuarter => "1.25x",
            Self::OneAndHalf => "1.5x",
            Self::Double => "2x",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Quarter => Self::Half,
            Self::Half => Self::ThreeQuarter,
            Self::ThreeQuarter => Self::Normal,
            Self::Normal => Self::OneAndQuarter,
            Self::OneAndQuarter => Self::OneAndHalf,
            Self::OneAndHalf => Self::Double,
            Self::Double => Self::Quarter,
        }
    }

    pub fn all() -> &'static [VideoPlaybackSpeed] {
        &[
            Self::Quarter,
            Self::Half,
            Self::ThreeQuarter,
            Self::Normal,
            Self::OneAndQuarter,
            Self::OneAndHalf,
            Self::Double,
        ]
    }

    pub fn from_multiplier(multiplier: f32) -> Self {
        let mut closest = Self::Normal;
        let mut closest_delta = f32::MAX;
        for speed in Self::all() {
            let delta = (speed.multiplier() - multiplier).abs();
            if delta < closest_delta {
                closest = *speed;
                closest_delta = delta;
            }
        }
        closest
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoPreload {
    /// Do not eagerly load metadata.
    #[default]
    None,
    /// Load metadata such as duration and dimensions before playback.
    Metadata,
    /// Prepare as much as the current backend can eagerly expose. Today this
    /// behaves like [`Self::Metadata`] until streaming/buffering backends land.
    Auto,
}

impl VideoPreload {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Metadata => "metadata",
            Self::Auto => "auto",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoPlayerRoute {
    /// Use Kael's native media element for normal files/URLs, and automatically
    /// switch to a WebView-hosted browser `<video>` fallback for sources such
    /// as HLS/DASH manifests that the current native backend should not own.
    #[default]
    Auto,
    /// Always use Kael's native media element when possible.
    Native,
    /// Prefer a WebView-hosted browser `<video>` element for URL/file sources.
    WebView,
}

impl VideoPlayerRoute {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Native => "native",
            Self::WebView => "webview",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoCaptionStyle {
    pub text_color: Hsla,
    pub background: Hsla,
    pub font_size: Pixels,
    pub line_height: Pixels,
}

impl Default for VideoCaptionStyle {
    fn default() -> Self {
        Self {
            text_color: kael::white(),
            background: kael::black().opacity(0.76),
            font_size: px(15.0),
            line_height: px(22.0),
        }
    }
}

impl VideoCaptionStyle {
    pub fn to_text(&self) -> String {
        format!(
            "video caption style: text color true, background true, font {}, line height {}",
            media_dimension_class(self.font_size / px(1.0)),
            media_dimension_class(self.line_height / px(1.0))
        )
    }

    pub fn text_color(mut self, color: Hsla) -> Self {
        self.text_color = color;
        self
    }

    pub fn background(mut self, color: Hsla) -> Self {
        self.background = color;
        self
    }

    pub fn font_size(mut self, size: Pixels) -> Self {
        self.font_size = size;
        self
    }

    pub fn line_height(mut self, height: Pixels) -> Self {
        self.line_height = height;
        self
    }
}

pub struct VideoPlayerState {
    playback_state: VideoPlaybackState,
    current_time: f64,
    duration: f64,
    volume: f32,
    is_muted: bool,
    previous_volume: f32,
    playback_speed: VideoPlaybackSpeed,
    is_fullscreen: bool,
    show_controls: bool,
    last_interaction: Instant,
    controls_timeout: Duration,
    is_seeking: bool,
    progress_bounds: Bounds<Pixels>,
    volume_bounds: Bounds<Pixels>,
    is_volume_dragging: bool,
    show_speed_menu: bool,
    show_text_track_menu: bool,
    focus_handle: FocusHandle,
    progress_focus_handle: FocusHandle,
    volume_focus_handle: FocusHandle,
    current_frame: Option<SharedString>,
    video_title: Option<SharedString>,
}

impl VideoPlayerState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            playback_state: VideoPlaybackState::Stopped,
            current_time: 0.0,
            duration: 0.0,
            volume: 1.0,
            is_muted: false,
            previous_volume: 1.0,
            playback_speed: VideoPlaybackSpeed::Normal,
            is_fullscreen: false,
            show_controls: true,
            last_interaction: Instant::now(),
            controls_timeout: Duration::from_secs(3),
            is_seeking: false,
            progress_bounds: Bounds::default(),
            volume_bounds: Bounds::default(),
            is_volume_dragging: false,
            show_speed_menu: false,
            show_text_track_menu: false,
            focus_handle: cx.focus_handle(),
            progress_focus_handle: cx.focus_handle(),
            volume_focus_handle: cx.focus_handle(),
            current_frame: None,
            video_title: None,
        }
    }

    pub fn set_frame(&mut self, frame_path: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.current_frame = Some(frame_path.into());
        cx.notify();
    }

    pub fn clear_frame(&mut self, cx: &mut Context<Self>) {
        self.current_frame = None;
        cx.notify();
    }

    pub fn current_frame(&self) -> Option<&SharedString> {
        self.current_frame.as_ref()
    }

    pub fn has_frame(&self) -> bool {
        self.current_frame.is_some()
    }

    pub fn set_title(&mut self, title: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.video_title = Some(title.into());
        cx.notify();
    }

    pub fn title(&self) -> Option<&SharedString> {
        self.video_title.as_ref()
    }

    pub fn has_title(&self) -> bool {
        self.video_title.is_some()
    }

    pub fn playback_state(&self) -> VideoPlaybackState {
        self.playback_state
    }

    pub fn is_playing(&self) -> bool {
        self.playback_state == VideoPlaybackState::Playing
    }

    pub fn play(&mut self, cx: &mut Context<Self>) {
        self.playback_state = VideoPlaybackState::Playing;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        self.playback_state = VideoPlaybackState::Paused;
        self.show_controls = true;
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.playback_state = VideoPlaybackState::Stopped;
        self.current_time = 0.0;
        self.show_controls = true;
        cx.notify();
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        match self.playback_state {
            VideoPlaybackState::Playing => self.pause(cx),
            VideoPlaybackState::Paused | VideoPlaybackState::Stopped => self.play(cx),
            VideoPlaybackState::Buffering => {}
        }
    }

    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    pub fn set_current_time(&mut self, time: f64, cx: &mut Context<Self>) {
        self.current_time = if time.is_finite() {
            time.clamp(0.0, self.duration)
        } else {
            0.0
        };
        cx.notify();
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn set_duration(&mut self, duration: f64, cx: &mut Context<Self>) {
        if !duration.is_finite() || duration < 0.0 {
            return;
        }
        self.duration = duration;
        self.current_time = self.current_time.min(duration);
        cx.notify();
    }

    pub fn progress(&self) -> f64 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        (self.current_time / self.duration).clamp(0.0, 1.0)
    }

    pub fn progress_class(&self) -> &'static str {
        media_fraction_class(self.progress() as f32)
    }

    pub fn seek(&mut self, position: f64, cx: &mut Context<Self>) {
        let clamped = if position.is_finite() {
            position.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.current_time = clamped * self.duration;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn seek_relative(&mut self, delta: f64, cx: &mut Context<Self>) {
        let new_time = if delta.is_finite() {
            (self.current_time + delta).clamp(0.0, self.duration)
        } else {
            self.current_time
        };
        self.current_time = new_time;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn effective_volume(&self) -> f32 {
        if self.is_muted { 0.0 } else { self.volume }
    }

    pub fn volume_class(&self) -> &'static str {
        media_fraction_class(self.effective_volume())
    }

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.volume > 0.0 {
            self.is_muted = false;
        }
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if self.is_muted {
            self.is_muted = false;
            if self.volume == 0.0 {
                self.volume = self.previous_volume;
            }
        } else {
            self.previous_volume = self.volume;
            self.is_muted = true;
        }
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn playback_speed(&self) -> VideoPlaybackSpeed {
        self.playback_speed
    }

    pub fn set_playback_speed(&mut self, speed: VideoPlaybackSpeed, cx: &mut Context<Self>) {
        self.playback_speed = speed;
        self.show_speed_menu = false;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn cycle_playback_speed(&mut self, cx: &mut Context<Self>) {
        self.playback_speed = self.playback_speed.next();
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn is_fullscreen(&self) -> bool {
        self.is_fullscreen
    }

    pub fn speed_key(&self) -> &'static str {
        self.playback_speed.to_text()
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool, cx: &mut Context<Self>) {
        self.is_fullscreen = fullscreen;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn toggle_fullscreen(&mut self, cx: &mut Context<Self>) {
        self.is_fullscreen = !self.is_fullscreen;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn show_controls(&self) -> bool {
        self.show_controls
    }

    pub fn touch_controls(&mut self, cx: &mut Context<Self>) {
        self.show_controls = true;
        self.last_interaction = Instant::now();
        cx.notify();
    }

    pub fn hide_controls(&mut self, cx: &mut Context<Self>) {
        if self.playback_state == VideoPlaybackState::Playing
            && !self.is_seeking
            && !self.is_volume_dragging
        {
            self.show_controls = false;
            cx.notify();
        }
    }

    pub fn check_auto_hide(&mut self, cx: &mut Context<Self>) {
        if self.last_interaction.elapsed() > self.controls_timeout {
            self.hide_controls(cx);
        }
    }

    pub fn toggle_speed_menu(&mut self, cx: &mut Context<Self>) {
        self.show_speed_menu = !self.show_speed_menu;
        self.show_text_track_menu = false;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn toggle_text_track_menu(&mut self, cx: &mut Context<Self>) {
        self.show_text_track_menu = !self.show_text_track_menu;
        self.show_speed_menu = false;
        self.touch_controls(cx);
        cx.notify();
    }

    pub fn close_text_track_menu(&mut self, cx: &mut Context<Self>) {
        self.show_text_track_menu = false;
        self.touch_controls(cx);
        cx.notify();
    }

    fn update_seek_from_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let track_width = self.progress_bounds.size.width;
        if track_width <= px(0.0) {
            return;
        }

        let relative_x = (position.x - self.progress_bounds.left()).clamp(px(0.0), track_width);
        let percentage = (relative_x / track_width).clamp(0.0, 1.0);
        self.seek(percentage as f64, cx);
    }

    fn update_volume_from_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let track_width = self.volume_bounds.size.width;
        if track_width <= px(0.0) {
            return;
        }

        let relative_x = (position.x - self.volume_bounds.left()).clamp(px(0.0), track_width);
        let percentage = (relative_x / track_width).clamp(0.0, 1.0);
        self.set_volume(percentage as f32, cx);
    }

    fn sync_from_video_snapshot(&mut self, snapshot: VideoSnapshot, cx: &mut Context<Self>) {
        let playback_state = match snapshot.playback_state {
            MediaPlaybackState::Playing => VideoPlaybackState::Playing,
            MediaPlaybackState::Paused => VideoPlaybackState::Paused,
            MediaPlaybackState::Stopped => VideoPlaybackState::Stopped,
        };
        let current_time = snapshot.current_time.as_secs_f64();
        let duration = snapshot.duration.map(|duration| duration.as_secs_f64());
        let playback_speed = VideoPlaybackSpeed::from_multiplier(snapshot.playback_rate);

        let mut changed = false;
        if self.playback_state != playback_state {
            self.playback_state = playback_state;
            changed = true;
        }
        if (self.current_time - current_time).abs() > f64::EPSILON {
            self.current_time = current_time;
            changed = true;
        }
        if let Some(duration) = duration
            && (self.duration - duration).abs() > f64::EPSILON
        {
            self.duration = duration;
            changed = true;
        }
        if (self.volume - snapshot.volume).abs() > f32::EPSILON {
            self.volume = snapshot.volume.clamp(0.0, 1.0);
            changed = true;
        }
        if self.is_muted != snapshot.muted {
            self.is_muted = snapshot.muted;
            changed = true;
        }
        if self.playback_speed != playback_speed {
            self.playback_speed = playback_speed;
            changed = true;
        }

        if changed {
            cx.notify();
        }
    }

    pub fn to_text(&self) -> String {
        format!(
            "video player state: playback {}, progress {}, volume {}, muted {}, speed {}, fullscreen {}, controls {}, seeking {}, volume dragging {}, speed menu {}, text-track menu {}, frame {}, title {}",
            self.playback_state.to_text(),
            self.progress_class(),
            self.volume_class(),
            self.is_muted,
            self.speed_key(),
            self.is_fullscreen,
            self.show_controls,
            self.is_seeking,
            self.is_volume_dragging,
            self.show_speed_menu,
            self.show_text_track_menu,
            self.has_frame(),
            self.has_title()
        )
    }
}

impl Focusable for VideoPlayerState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for VideoPlayerState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

actions!(
    video_player,
    [
        VideoPlayerTogglePlay,
        VideoPlayerMute,
        VideoPlayerFullscreen,
        VideoPlayerSeekForward,
        VideoPlayerSeekBackward,
        VideoPlayerVolumeUp,
        VideoPlayerVolumeDown,
    ]
);

pub fn init_video_player(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", VideoPlayerTogglePlay, Some("VideoPlayer")),
        KeyBinding::new("m", VideoPlayerMute, Some("VideoPlayer")),
        KeyBinding::new("f", VideoPlayerFullscreen, Some("VideoPlayer")),
        KeyBinding::new("right", VideoPlayerSeekForward, Some("VideoPlayer")),
        KeyBinding::new("left", VideoPlayerSeekBackward, Some("VideoPlayer")),
        KeyBinding::new("up", VideoPlayerVolumeUp, Some("VideoPlayer")),
        KeyBinding::new("down", VideoPlayerVolumeDown, Some("VideoPlayer")),
    ]);
}

fn format_time(seconds: f64) -> String {
    let total_seconds = seconds.floor() as i64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

fn object_fit_css_value(object_fit: &ObjectFit) -> &'static str {
    match object_fit {
        ObjectFit::Fill => "fill",
        ObjectFit::Contain => "contain",
        ObjectFit::Cover => "cover",
        ObjectFit::ScaleDown => "scale-down",
        ObjectFit::None => "none",
    }
}

#[derive(IntoElement)]
pub struct VideoPlayer {
    id: ElementId,
    state: Entity<VideoPlayerState>,
    controller: Option<VideoController>,
    playback_route: VideoPlayerRoute,
    content_type: Option<SharedString>,
    webview_options: WebViewVideoOptions,
    size: VideoPlayerSize,
    object_fit: ObjectFit,
    show_controls_chrome: bool,
    show_captions: bool,
    caption_style: VideoCaptionStyle,
    poster: Option<SharedString>,
    show_poster: bool,
    overlay_only: bool,
    on_event: Option<Rc<dyn Fn(VideoEvent, &mut Window, &mut App)>>,
    on_source_changed: Option<Rc<dyn Fn(MediaSource, &mut Window, &mut App)>>,
    on_loaded_metadata: Option<Rc<dyn Fn(Option<Duration>, u32, u32, &mut Window, &mut App)>>,
    on_ready_state_change: Option<Rc<dyn Fn(VideoReadyState, &mut Window, &mut App)>>,
    on_can_play: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_can_play_through: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_waiting: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_progress: Option<Rc<dyn Fn(Vec<TimeRange>, &mut Window, &mut App)>>,
    on_playing: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_paused: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_stopped: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_time_update: Option<Rc<dyn Fn(Duration, &mut Window, &mut App)>>,
    on_seeked: Option<Rc<dyn Fn(Duration, &mut Window, &mut App)>>,
    on_volume_changed: Option<Rc<dyn Fn(f32, bool, &mut Window, &mut App)>>,
    on_rate_change: Option<Rc<dyn Fn(f32, &mut Window, &mut App)>>,
    on_loop_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    on_text_track_added: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_text_track_changed: Option<Rc<dyn Fn(Option<SharedString>, &mut Window, &mut App)>>,
    on_cue_change: Option<Rc<dyn Fn(Vec<TextTrackCue>, &mut Window, &mut App)>>,
    on_ended: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_error: Option<Rc<dyn Fn(String, &mut Window, &mut App)>>,
    on_play: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_pause: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_seek: Option<Rc<dyn Fn(f64, &mut Window, &mut App)>>,
    on_volume_change: Option<Rc<dyn Fn(f32, &mut Window, &mut App)>>,
    on_fullscreen: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    on_playback_speed_change: Option<Rc<dyn Fn(VideoPlaybackSpeed, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

fn video_player_callsite_id(caller: &'static Location<'static>) -> ElementId {
    ElementId::Name(
        format!(
            "video-player:{}:{}:{}",
            caller.file(),
            caller.line(),
            caller.column()
        )
        .into(),
    )
}

fn video_player_child_id(parent: &ElementId, child: impl Into<SharedString>) -> ElementId {
    ElementId::NamedChild(Box::new(parent.clone()), child.into())
}

fn handle_video_seek_accessibility_action(
    request: &AccessibilityActionRequest,
    state: &Entity<VideoPlayerState>,
    controller: &Option<VideoController>,
    handler: &Option<Rc<dyn Fn(f64, &mut Window, &mut App)>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (current, duration) = {
        let state = state.read(cx);
        (state.current_time(), state.duration().max(0.0))
    };
    let Some(next) =
        crate::util::accessibility_adjusted_value(request, current, 0.0, duration, 5.0)
    else {
        return;
    };
    state.update(cx, |state, cx| state.set_current_time(next, cx));
    if let Some(controller) = controller {
        let _ = controller.seek(Duration::from_secs_f64(next));
    }
    if let Some(handler) = handler {
        handler(next, window, cx);
    }
}

fn handle_video_volume_accessibility_action(
    request: &AccessibilityActionRequest,
    state: &Entity<VideoPlayerState>,
    controller: &Option<VideoController>,
    handler: &Option<Rc<dyn Fn(f32, &mut Window, &mut App)>>,
    window: &mut Window,
    cx: &mut App,
) {
    let current = state.read(cx).volume() as f64;
    let Some(next) = crate::util::accessibility_adjusted_value(request, current, 0.0, 1.0, 0.05)
    else {
        return;
    };
    let next = next as f32;
    state.update(cx, |state, cx| state.set_volume(next, cx));
    if let Some(controller) = controller {
        controller.set_volume(next);
    }
    if let Some(handler) = handler {
        handler(next, window, cx);
    }
}

impl VideoPlayer {
    #[track_caller]
    pub fn new(state: Entity<VideoPlayerState>) -> Self {
        Self {
            id: video_player_callsite_id(Location::caller()),
            state,
            controller: None,
            playback_route: VideoPlayerRoute::Auto,
            content_type: None,
            webview_options: WebViewVideoOptions::default(),
            size: VideoPlayerSize::default(),
            object_fit: ObjectFit::Contain,
            show_controls_chrome: true,
            show_captions: true,
            caption_style: VideoCaptionStyle::default(),
            poster: None,
            show_poster: true,
            overlay_only: false,
            on_event: None,
            on_source_changed: None,
            on_loaded_metadata: None,
            on_ready_state_change: None,
            on_can_play: None,
            on_can_play_through: None,
            on_waiting: None,
            on_progress: None,
            on_playing: None,
            on_paused: None,
            on_stopped: None,
            on_time_update: None,
            on_seeked: None,
            on_volume_changed: None,
            on_rate_change: None,
            on_loop_change: None,
            on_text_track_added: None,
            on_text_track_changed: None,
            on_cue_change: None,
            on_ended: None,
            on_error: None,
            on_play: None,
            on_pause: None,
            on_seek: None,
            on_volume_change: None,
            on_fullscreen: None,
            on_playback_speed_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    #[track_caller]
    pub fn source(source: impl Into<MediaSource>, cx: &mut App) -> Self {
        let controller = VideoController::new(source);
        let state = cx.new(VideoPlayerState::new);

        let mut player = Self::new(state).id(video_player_callsite_id(Location::caller()));
        player.controller = Some(controller.clone());

        player
    }

    /// Create a source-backed player from a URL.
    #[track_caller]
    pub fn url(url: impl Into<Arc<str>>, cx: &mut App) -> Self {
        Self::source(MediaSource::url(url), cx)
    }

    /// Create a source-backed player from a local file path.
    #[track_caller]
    pub fn file(path: impl Into<PathBuf>, cx: &mut App) -> Self {
        Self::source(MediaSource::file(path), cx)
    }

    /// Create a source-backed player from in-memory media bytes.
    #[track_caller]
    pub fn bytes(bytes: impl Into<Arc<[u8]>>, cx: &mut App) -> Self {
        Self::source(MediaSource::bytes(bytes), cx)
    }

    /// Create a source-backed player from a keyed reader factory.
    #[track_caller]
    pub fn reader<R>(
        key: impl Into<Arc<str>>,
        open: impl Fn() -> std::io::Result<R> + Send + Sync + 'static,
        cx: &mut App,
    ) -> Self
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        Self::source(MediaSource::reader(key, open), cx)
    }

    pub fn controller(&self) -> Option<VideoController> {
        self.controller.clone()
    }

    pub fn has_controller(&self) -> bool {
        self.controller.is_some()
    }

    pub fn source_kind(&self) -> Option<&'static str> {
        self.controller
            .as_ref()
            .map(|controller| media_source_kind_ui(&controller.source()))
    }

    /// Set how source-backed video should be rendered.
    pub fn playback_route(mut self, route: VideoPlayerRoute) -> Self {
        self.playback_route = route;
        self
    }

    /// Provide a MIME/content type that Auto can use for route selection.
    ///
    /// This is useful for extensionless CDN or signed URLs where `Content-Type`
    /// is the only reliable signal that a source is HLS/DASH/adaptive media and
    /// should use the browser fallback.
    pub fn content_type(mut self, content_type: impl Into<SharedString>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Force Kael's native video element even for sources that Auto would route
    /// through WebView.
    pub fn native_playback(self) -> Self {
        self.playback_route(VideoPlayerRoute::Native)
    }

    /// Prefer the WebView-hosted browser video fallback for URL/file sources.
    pub fn webview_fallback(self) -> Self {
        self.playback_route(VideoPlayerRoute::WebView)
    }

    /// Restore automatic native-vs-WebView route selection.
    pub fn auto_playback_route(self) -> Self {
        self.playback_route(VideoPlayerRoute::Auto)
    }

    /// Configure the WebView-hosted browser video fallback.
    pub fn webview_options(mut self, options: WebViewVideoOptions) -> Self {
        self.webview_options = options;
        self
    }

    pub fn size(mut self, size: VideoPlayerSize) -> Self {
        self.size = size;
        self
    }

    pub fn size_key(&self) -> &'static str {
        self.size.to_text()
    }

    pub fn route_key(&self) -> &'static str {
        self.playback_route.to_text()
    }

    pub fn has_content_type(&self) -> bool {
        self.content_type.is_some()
    }

    pub fn has_poster(&self) -> bool {
        self.poster.is_some()
    }

    pub fn handler_count(&self) -> usize {
        [
            self.on_event.is_some(),
            self.on_source_changed.is_some(),
            self.on_loaded_metadata.is_some(),
            self.on_ready_state_change.is_some(),
            self.on_can_play.is_some(),
            self.on_can_play_through.is_some(),
            self.on_waiting.is_some(),
            self.on_progress.is_some(),
            self.on_playing.is_some(),
            self.on_paused.is_some(),
            self.on_stopped.is_some(),
            self.on_time_update.is_some(),
            self.on_seeked.is_some(),
            self.on_volume_changed.is_some(),
            self.on_rate_change.is_some(),
            self.on_loop_change.is_some(),
            self.on_text_track_added.is_some(),
            self.on_text_track_changed.is_some(),
            self.on_cue_change.is_some(),
            self.on_ended.is_some(),
            self.on_error.is_some(),
            self.on_play.is_some(),
            self.on_pause.is_some(),
            self.on_seek.is_some(),
            self.on_volume_change.is_some(),
            self.on_fullscreen.is_some(),
            self.on_playback_speed_change.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }

    pub fn text_track_count(&self) -> usize {
        self.controller
            .as_ref()
            .map(|controller| controller.text_tracks().len())
            .unwrap_or(0)
    }

    pub fn to_text(&self) -> String {
        format!(
            "video player: source {}, route {}, size {}, controls {}, captions {}, poster {}, show poster {}, overlay {}, content type {}, webview text tracks {}, controller text tracks {}, handlers {}",
            self.source_kind().unwrap_or("none"),
            self.route_key(),
            self.size_key(),
            self.show_controls_chrome,
            self.show_captions,
            self.has_poster(),
            self.show_poster,
            self.overlay_only,
            self.has_content_type(),
            self.webview_options.text_track_count(),
            self.text_track_count(),
            self.handler_count()
        )
    }

    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self.webview_options = self
            .webview_options
            .clone()
            .object_fit(object_fit_css_value(&self.object_fit));
        self
    }

    /// Configure source-backed preload behavior.
    pub fn preload(mut self, preload: VideoPreload) -> Self {
        if matches!(preload, VideoPreload::Metadata | VideoPreload::Auto)
            && let Some(controller) = &self.controller
        {
            let _ = controller.load_metadata();
        }
        self.webview_options = match preload {
            VideoPreload::None => self
                .webview_options
                .clone()
                .preload(WebViewVideoPreload::None),
            VideoPreload::Metadata => self
                .webview_options
                .clone()
                .preload(WebViewVideoPreload::Metadata),
            VideoPreload::Auto => self
                .webview_options
                .clone()
                .preload(WebViewVideoPreload::Auto),
        };
        self
    }

    /// Set whether the built-in playback controls are rendered.
    ///
    /// Keyboard actions and controller-backed playback state remain available
    /// either way, which makes this useful for apps with custom controls.
    pub fn controls(mut self, show: bool) -> Self {
        self.show_controls_chrome = show;
        self.webview_options = self.webview_options.clone().controls(show);
        self
    }

    /// Start source-backed playback as soon as the player is constructed.
    pub fn autoplay(mut self) -> Self {
        if let Some(controller) = &self.controller {
            let _ = controller.play();
        }
        self.webview_options = self.webview_options.clone().autoplay(true);
        self
    }

    /// Set the initial source-backed player volume.
    ///
    /// This configures the internal [`VideoController`] created by
    /// [`VideoPlayer::source`]. For custom `VideoPlayer::new(state)` players,
    /// drive volume through the supplied [`VideoPlayerState`] instead.
    pub fn volume(self, volume: f32) -> Self {
        if let Some(controller) = &self.controller {
            controller.set_volume(volume);
        }
        self
    }

    /// Set whether a source-backed player starts muted.
    pub fn muted(mut self, muted: bool) -> Self {
        if let Some(controller) = &self.controller {
            controller.set_muted(muted);
        }
        self.webview_options = self.webview_options.clone().muted(muted);
        self
    }

    /// Set the initial playback rate for a source-backed player.
    pub fn playback_rate(self, playback_rate: f32) -> Self {
        if let Some(controller) = &self.controller {
            controller.set_playback_rate(playback_rate);
        }
        self
    }

    /// Set whether a source-backed player loops at the end.
    pub fn looping(mut self, looping: bool) -> Self {
        if let Some(controller) = &self.controller {
            controller.set_looping(looping);
        }
        self.webview_options = self.webview_options.clone().looping(looping);
        self
    }

    /// Seek a source-backed player before first playback.
    pub fn start_at(mut self, position: Duration) -> Self {
        if let Some(controller) = &self.controller {
            let _ = controller.seek(position);
        }
        self.webview_options = self.webview_options.clone().start_at(position);
        self
    }

    pub fn show_captions(mut self, show: bool) -> Self {
        self.show_captions = show;
        self
    }

    pub fn caption_style(mut self, style: VideoCaptionStyle) -> Self {
        self.caption_style = style;
        self
    }

    /// Add a parsed text track to a source-backed player.
    pub fn text_track(self, track: TextTrack) -> Self {
        if let Some(controller) = &self.controller {
            controller.add_text_track(track);
        }
        self
    }

    /// Parse and add a SubRip subtitle track to a source-backed player.
    pub fn srt_text_track(
        self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        if let Some(controller) = &self.controller {
            controller.add_srt_text_track(id, label, language, input);
        }
        self
    }

    /// Parse and add a WebVTT subtitle track to a source-backed player.
    pub fn webvtt_text_track(
        mut self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        language: Option<impl Into<SharedString>>,
        input: &str,
    ) -> Self {
        let label = label.into();
        let language = language.map(Into::into);
        if let Some(controller) = &self.controller {
            controller.add_webvtt_text_track(id, label.clone(), language.clone(), input);
        }
        self.webview_options = self
            .webview_options
            .clone()
            .webvtt_text_track(label, language, input);
        self
    }

    /// Select a source-backed text track by id.
    pub fn select_text_track(self, id: impl AsRef<str>) -> Self {
        if let Some(controller) = &self.controller {
            controller.select_text_track(id);
        }
        self
    }

    /// Disable active source-backed text cues.
    pub fn disable_text_track(self) -> Self {
        if let Some(controller) = &self.controller {
            controller.disable_text_track();
        }
        self
    }

    pub fn poster(mut self, poster: impl Into<SharedString>) -> Self {
        let poster = poster.into();
        self.webview_options = self.webview_options.clone().poster(poster.clone());
        self.poster = Some(poster);
        self
    }

    pub fn show_poster(mut self, show: bool) -> Self {
        self.show_poster = show;
        self
    }

    pub fn overlay_only(mut self) -> Self {
        self.overlay_only = true;
        self
    }

    pub fn on_event(
        mut self,
        handler: impl Fn(VideoEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_event = Some(Rc::new(handler));
        self
    }

    pub fn on_source_changed(
        mut self,
        handler: impl Fn(MediaSource, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_source_changed = Some(Rc::new(handler));
        self
    }

    pub fn on_loaded_metadata(
        mut self,
        handler: impl Fn(Option<Duration>, u32, u32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_loaded_metadata = Some(Rc::new(handler));
        self
    }

    pub fn on_ready_state_change(
        mut self,
        handler: impl Fn(VideoReadyState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_ready_state_change = Some(Rc::new(handler));
        self
    }

    pub fn on_can_play(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_can_play = Some(Rc::new(handler));
        self
    }

    pub fn on_can_play_through(
        mut self,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_can_play_through = Some(Rc::new(handler));
        self
    }

    pub fn on_waiting(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_waiting = Some(Rc::new(handler));
        self
    }

    pub fn on_progress(
        mut self,
        handler: impl Fn(Vec<TimeRange>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_progress = Some(Rc::new(handler));
        self
    }

    pub fn on_playing(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_playing = Some(Rc::new(handler));
        self
    }

    pub fn on_paused(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_paused = Some(Rc::new(handler));
        self
    }

    pub fn on_stopped(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_stopped = Some(Rc::new(handler));
        self
    }

    pub fn on_time_update(
        mut self,
        handler: impl Fn(Duration, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_time_update = Some(Rc::new(handler));
        self
    }

    pub fn on_seeked(
        mut self,
        handler: impl Fn(Duration, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_seeked = Some(Rc::new(handler));
        self
    }

    pub fn on_volume_changed(
        mut self,
        handler: impl Fn(f32, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_volume_changed = Some(Rc::new(handler));
        self
    }

    pub fn on_rate_change(
        mut self,
        handler: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_rate_change = Some(Rc::new(handler));
        self
    }

    pub fn on_loop_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_loop_change = Some(Rc::new(handler));
        self
    }

    pub fn on_text_track_added(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_text_track_added = Some(Rc::new(handler));
        self
    }

    pub fn on_text_track_changed(
        mut self,
        handler: impl Fn(Option<SharedString>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_text_track_changed = Some(Rc::new(handler));
        self
    }

    pub fn on_cue_change(
        mut self,
        handler: impl Fn(Vec<TextTrackCue>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_cue_change = Some(Rc::new(handler));
        self
    }

    pub fn on_ended(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_ended = Some(Rc::new(handler));
        self
    }

    pub fn on_error(mut self, handler: impl Fn(String, &mut Window, &mut App) + 'static) -> Self {
        self.on_error = Some(Rc::new(handler));
        self
    }

    pub fn on_play(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_play = Some(Rc::new(handler));
        self
    }

    pub fn on_pause(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_pause = Some(Rc::new(handler));
        self
    }

    pub fn on_seek(mut self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_seek = Some(Rc::new(handler));
        self
    }

    pub fn on_volume_change(
        mut self,
        handler: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_volume_change = Some(Rc::new(handler));
        self
    }

    pub fn on_fullscreen(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_fullscreen = Some(Rc::new(handler));
        self
    }

    pub fn on_playback_speed_change(
        mut self,
        handler: impl Fn(VideoPlaybackSpeed, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_playback_speed_change = Some(Rc::new(handler));
        self
    }
}

fn media_source_kind_ui(source: &MediaSource) -> &'static str {
    match source {
        MediaSource::File(_) => "file",
        MediaSource::Url(_) => "url",
        MediaSource::Bytes(_) => "bytes",
        MediaSource::Reader(_) => "reader",
    }
}

fn media_fraction_class(value: f32) -> &'static str {
    if value <= 0.0 {
        "empty"
    } else if value < 0.25 {
        "low"
    } else if value < 0.75 {
        "medium"
    } else if value < 1.0 {
        "high"
    } else {
        "full"
    }
}

fn media_dimension_class(value: f32) -> &'static str {
    if value <= 0.0 {
        "invalid"
    } else if value < 16.0 {
        "compact"
    } else if value < 32.0 {
        "standard"
    } else {
        "large"
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use kael::TestAppContext;

    #[::core::prelude::v1::test]
    fn video_player_state_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(VideoPlayerState::new));

        let summary = cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_title("Private Launch Cut", cx);
                state.set_frame("/Users/person/secret-frame.png", cx);
                state.set_duration(240.0, cx);
                state.set_current_time(42.0, cx);
                state.set_volume(0.37, cx);
                state.play(cx);
                state.to_text()
            })
        });

        assert!(summary.contains("playback playing"));
        assert!(summary.contains("progress low"));
        assert!(summary.contains("volume medium"));
        assert!(summary.contains("frame true"));
        assert!(summary.contains("title true"));
        assert!(!summary.contains("Private Launch Cut"));
        assert!(!summary.contains("secret-frame"));
        assert!(!summary.contains("42"));
        assert!(!summary.contains("0.37"));
    }

    #[::core::prelude::v1::test]
    fn video_player_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let summary = cx.update(|cx| {
            VideoPlayer::url("https://cdn.example.com/private/launch.m3u8", cx)
                .content_type("application/vnd.apple.mpegurl")
                .webview_fallback()
                .poster("https://cdn.example.com/private/poster.jpg")
                .webvtt_text_track(
                    "captions",
                    "Private captions",
                    Some("en"),
                    "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nSecret line",
                )
                .on_play(|_, _| {})
                .on_error(|_, _, _| {})
                .to_text()
        });

        assert!(summary.contains("source url"));
        assert!(summary.contains("route webview"));
        assert!(summary.contains("poster true"));
        assert!(summary.contains("content type true"));
        assert!(summary.contains("handlers 2"));
        assert!(!summary.contains("cdn.example.com"));
        assert!(!summary.contains("launch.m3u8"));
        assert!(!summary.contains("poster.jpg"));
        assert!(!summary.contains("Private captions"));
        assert!(!summary.contains("Secret line"));
        assert!(!summary.contains("application/vnd.apple.mpegurl"));
    }

    #[::core::prelude::v1::test]
    fn video_player_state_rejects_non_finite_values() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(VideoPlayerState::new));

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_duration(120.0, cx);
                state.set_current_time(f64::NAN, cx);
                assert_eq!(state.current_time(), 0.0);

                state.set_duration(f64::INFINITY, cx);
                assert_eq!(state.duration(), 120.0);

                state.seek(f64::NAN, cx);
                assert_eq!(state.current_time(), 0.0);

                state.set_volume(f32::NAN, cx);
                assert_eq!(state.volume(), 0.0);
            });
        });
    }
}

impl Styled for VideoPlayer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VideoPlayer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut active_text_cues = Vec::new();
        let mut text_tracks = Vec::new();
        let mut selected_text_track_id = None;

        if let Some(controller) = self.controller.as_ref() {
            let snapshot = controller.snapshot();
            active_text_cues = snapshot.active_text_cues.clone();
            text_tracks = controller.text_tracks();
            selected_text_track_id = controller.selected_text_track_id();

            cx.update_entity(&self.state, |state, cx| {
                state.sync_from_video_snapshot(snapshot, cx)
            });

            let should_drain_events = self.on_event.is_some()
                || self.on_source_changed.is_some()
                || self.on_loaded_metadata.is_some()
                || self.on_ready_state_change.is_some()
                || self.on_can_play.is_some()
                || self.on_can_play_through.is_some()
                || self.on_waiting.is_some()
                || self.on_progress.is_some()
                || self.on_playing.is_some()
                || self.on_paused.is_some()
                || self.on_stopped.is_some()
                || self.on_time_update.is_some()
                || self.on_seeked.is_some()
                || self.on_volume_changed.is_some()
                || self.on_rate_change.is_some()
                || self.on_loop_change.is_some()
                || self.on_text_track_added.is_some()
                || self.on_text_track_changed.is_some()
                || self.on_cue_change.is_some()
                || self.on_ended.is_some()
                || self.on_error.is_some();
            if should_drain_events {
                for event in controller.drain_events() {
                    match &event {
                        VideoEvent::SourceChanged { source } => {
                            if let Some(handler) = &self.on_source_changed {
                                handler(source.clone(), window, cx);
                            }
                        }
                        VideoEvent::LoadedMetadata {
                            duration,
                            width,
                            height,
                        } => {
                            if let Some(handler) = &self.on_loaded_metadata {
                                handler(*duration, *width, *height, window, cx);
                            }
                        }
                        VideoEvent::ReadyStateChange { ready_state } => {
                            if let Some(handler) = &self.on_ready_state_change {
                                handler(*ready_state, window, cx);
                            }
                        }
                        VideoEvent::CanPlay => {
                            if let Some(handler) = &self.on_can_play {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::CanPlayThrough => {
                            if let Some(handler) = &self.on_can_play_through {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::Waiting => {
                            if let Some(handler) = &self.on_waiting {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::Progress { buffered_ranges } => {
                            if let Some(handler) = &self.on_progress {
                                handler(buffered_ranges.clone(), window, cx);
                            }
                        }
                        VideoEvent::Playing => {
                            if let Some(handler) = &self.on_playing {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::Paused => {
                            if let Some(handler) = &self.on_paused {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::Stopped => {
                            if let Some(handler) = &self.on_stopped {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::TimeUpdate { current_time } => {
                            if let Some(handler) = &self.on_time_update {
                                handler(*current_time, window, cx);
                            }
                        }
                        VideoEvent::Seeked { current_time } => {
                            if let Some(handler) = &self.on_seeked {
                                handler(*current_time, window, cx);
                            }
                        }
                        VideoEvent::VolumeChange { volume, muted } => {
                            if let Some(handler) = &self.on_volume_changed {
                                handler(*volume, *muted, window, cx);
                            }
                        }
                        VideoEvent::RateChange { playback_rate } => {
                            if let Some(handler) = &self.on_rate_change {
                                handler(*playback_rate, window, cx);
                            }
                        }
                        VideoEvent::LoopChange { looping } => {
                            if let Some(handler) = &self.on_loop_change {
                                handler(*looping, window, cx);
                            }
                        }
                        VideoEvent::FullscreenChange { fullscreen } => {
                            if let Some(handler) = &self.on_fullscreen {
                                handler(*fullscreen, window, cx);
                            }
                        }
                        VideoEvent::PictureInPictureChange { .. } => {}
                        VideoEvent::TextTrackAdded { id } => {
                            if let Some(handler) = &self.on_text_track_added {
                                handler(id.clone(), window, cx);
                            }
                        }
                        VideoEvent::TextTrackChanged { id } => {
                            if let Some(handler) = &self.on_text_track_changed {
                                handler(id.clone(), window, cx);
                            }
                        }
                        VideoEvent::CueChange { cues } => {
                            if let Some(handler) = &self.on_cue_change {
                                handler(cues.clone(), window, cx);
                            }
                        }
                        VideoEvent::Ended => {
                            if let Some(handler) = &self.on_ended {
                                handler(window, cx);
                            }
                        }
                        VideoEvent::Error(error) => {
                            if let Some(handler) = &self.on_error {
                                handler(error.clone(), window, cx);
                            }
                        }
                    }

                    if let Some(handler) = &self.on_event {
                        handler(event, window, cx);
                    }
                }
            }
        }

        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let focus_handle = state.focus_handle(cx);
        let progress_focus_handle = state.progress_focus_handle.clone();
        let volume_focus_handle = state.volume_focus_handle.clone();
        let accessibility_label = state
            .title()
            .cloned()
            .unwrap_or_else(|| "Video player".into())
            .to_string();
        let player_id = self.id.clone();

        let playback_state = state.playback_state();
        let is_playing = state.is_playing();
        let current_time = state.current_time();
        let duration = state.duration();
        let progress = state.progress();
        let volume = state.volume();
        let is_muted = state.is_muted();
        let playback_speed = state.playback_speed();
        let is_fullscreen = window.is_fullscreen();
        let controls_visible = state.show_controls();
        let show_speed_menu = state.show_speed_menu;
        let show_text_track_menu = state.show_text_track_menu;

        let (width, height) = self.size.dimensions();
        let controls_height = self.size.controls_height();
        let icon_size = self.size.icon_size();

        let show_poster = self.show_poster
            && self.poster.is_some()
            && playback_state == VideoPlaybackState::Stopped;

        let current_frame = state.current_frame().cloned();
        let overlay_only = self.overlay_only;
        let controller = self.controller.clone();
        let object_fit = self.object_fit;
        let webview_player = controller.as_ref().and_then(|controller| {
            let source = controller.source();
            let should_use_webview = match self.playback_route {
                VideoPlayerRoute::Auto => {
                    self.content_type.as_ref().is_some_and(|content_type| {
                        recommended_video_playback_route_for_type(content_type).should_use_webview()
                    }) || controller.recommended_route().should_use_webview()
                }
                VideoPlayerRoute::Native => false,
                VideoPlayerRoute::WebView => true,
            };

            should_use_webview
                .then(|| {
                    webview_video_player_url(&source, &self.webview_options)
                        .map(|page| (ElementId::Name(webview_video_player_id(&source)), page))
                })
                .flatten()
        });
        let use_webview_player = webview_player.is_some();
        let show_controls_chrome = self.show_controls_chrome;
        let show_controls = show_controls_chrome && controls_visible && !use_webview_player;
        let show_captions = self.show_captions;
        let caption_style = self.caption_style;

        let user_style = self.style;

        let volume_icon = if is_muted || volume == 0.0 {
            "volume-x"
        } else if volume < 0.5 {
            "volume-1"
        } else {
            "volume-2"
        };

        let play_icon = if is_playing { "pause" } else { "play" };

        let state_entity = self.state.clone();
        let state_for_actions = self.state.clone();
        let state_for_mouse = self.state.clone();

        let on_play = self.on_play.clone();
        let on_pause = self.on_pause.clone();
        let on_seek = self.on_seek.clone();
        let on_volume_change = self.on_volume_change.clone();
        let on_fullscreen = self.on_fullscreen.clone();
        let on_playback_speed_change = self.on_playback_speed_change.clone();
        let on_webview_event = self.on_event.clone();
        let on_webview_loaded_metadata = self.on_loaded_metadata.clone();
        let on_webview_can_play = self.on_can_play.clone();
        let on_webview_can_play_through = self.on_can_play_through.clone();
        let on_webview_waiting = self.on_waiting.clone();
        let on_webview_progress = self.on_progress.clone();
        let on_webview_playing = self.on_playing.clone();
        let on_webview_paused = self.on_paused.clone();
        let on_webview_time_update = self.on_time_update.clone();
        let on_webview_seeked = self.on_seeked.clone();
        let on_webview_volume_changed = self.on_volume_changed.clone();
        let on_webview_rate_change = self.on_rate_change.clone();
        let on_webview_ended = self.on_ended.clone();
        let on_webview_error = self.on_error.clone();

        div()
            .id(player_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(accessibility_label),
            )
            .key_context("VideoPlayer")
            .track_focus(&focus_handle)
            .relative()
            .w(width)
            .h(height)
            .bg(kael::black())
            .rounded(theme.tokens.radius_lg)
            .overflow_hidden()
            .cursor(CursorStyle::Arrow)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_play = on_play.clone();
                let on_pause = on_pause.clone();
                move |_: &VideoPlayerTogglePlay, window, cx| {
                    let is_playing = state.read(cx).is_playing();
                    cx.update_entity(&state, |state, cx| state.toggle_play(cx));
                    if is_playing {
                        if let Some(controller) = &controller {
                            controller.pause();
                        }
                        if let Some(handler) = &on_pause {
                            handler(window, cx);
                        }
                    } else {
                        if let Some(controller) = &controller {
                            let _ = controller.play();
                        }
                        if let Some(handler) = &on_play {
                            handler(window, cx);
                        }
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_volume_change = on_volume_change.clone();
                move |_: &VideoPlayerMute, window, cx| {
                    cx.update_entity(&state, |state, cx| state.toggle_mute(cx));
                    let effective_volume = state.read(cx).effective_volume();
                    if let Some(controller) = &controller {
                        controller.set_muted(state.read(cx).is_muted());
                    }
                    if let Some(handler) = &on_volume_change {
                        handler(effective_volume, window, cx);
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let on_fullscreen = on_fullscreen.clone();
                move |_: &VideoPlayerFullscreen, window, cx| {
                    window.toggle_fullscreen();
                    let is_fullscreen = window.is_fullscreen();
                    cx.update_entity(&state, |state, cx| {
                        state.set_fullscreen(is_fullscreen, cx);
                    });
                    if let Some(handler) = &on_fullscreen {
                        handler(is_fullscreen, window, cx);
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_seek = on_seek.clone();
                move |_: &VideoPlayerSeekForward, window, cx| {
                    cx.update_entity(&state, |state, cx| state.seek_relative(10.0, cx));
                    let current_time = state.read(cx).current_time();
                    if let Some(controller) = &controller {
                        let _ = controller.seek(Duration::from_secs_f64(current_time.max(0.0)));
                    }
                    if let Some(handler) = &on_seek {
                        handler(current_time, window, cx);
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_seek = on_seek.clone();
                move |_: &VideoPlayerSeekBackward, window, cx| {
                    cx.update_entity(&state, |state, cx| state.seek_relative(-10.0, cx));
                    let current_time = state.read(cx).current_time();
                    if let Some(controller) = &controller {
                        let _ = controller.seek(Duration::from_secs_f64(current_time.max(0.0)));
                    }
                    if let Some(handler) = &on_seek {
                        handler(current_time, window, cx);
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_volume_change = on_volume_change.clone();
                move |_: &VideoPlayerVolumeUp, window, cx| {
                    let current = state.read(cx).volume();
                    let new_volume = (current + 0.1).min(1.0);
                    cx.update_entity(&state, |state, cx| state.set_volume(new_volume, cx));
                    if let Some(controller) = &controller {
                        controller.set_volume(new_volume);
                    }
                    if let Some(handler) = &on_volume_change {
                        handler(new_volume, window, cx);
                    }
                }
            })
            .on_action({
                let state = state_for_actions.clone();
                let controller = controller.clone();
                let on_volume_change = on_volume_change.clone();
                move |_: &VideoPlayerVolumeDown, window, cx| {
                    let current = state.read(cx).volume();
                    let new_volume = (current - 0.1).max(0.0);
                    cx.update_entity(&state, |state, cx| state.set_volume(new_volume, cx));
                    if let Some(controller) = &controller {
                        controller.set_volume(new_volume);
                    }
                    if let Some(handler) = &on_volume_change {
                        handler(new_volume, window, cx);
                    }
                }
            })
            .on_mouse_move({
                let state = state_for_mouse.clone();
                window.listener_for(&state, move |state, _: &MouseMoveEvent, _, cx| {
                    state.touch_controls(cx);
                })
            })
            .when(show_poster, {
                let poster = self.poster.clone();
                move |this| {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(poster, |this, poster_src| {
                                this.child(
                                    img(poster_src)
                                        .size_full()
                                        .object_fit(ObjectFit::Cover)
                                )
                            })
                    )
                }
            })
            .when(!show_poster && !overlay_only, |this| {
                if let Some(ref frame) = current_frame {
                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(
                                img(frame.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                            )
                    )
                } else if let Some((element_id, page_url)) = webview_player.clone() {
                    let state = state_entity.clone();
                    let on_event = on_webview_event.clone();
                    let on_loaded_metadata = on_webview_loaded_metadata.clone();
                    let on_can_play = on_webview_can_play.clone();
                    let on_can_play_through = on_webview_can_play_through.clone();
                    let on_waiting = on_webview_waiting.clone();
                    let on_progress = on_webview_progress.clone();
                    let on_playing = on_webview_playing.clone();
                    let on_paused = on_webview_paused.clone();
                    let on_time_update = on_webview_time_update.clone();
                    let on_seeked = on_webview_seeked.clone();
                    let on_volume_changed = on_webview_volume_changed.clone();
                    let on_rate_change = on_webview_rate_change.clone();
                    let on_text_track_changed = self.on_text_track_changed.clone();
                    let on_cue_change = self.on_cue_change.clone();
                    let on_fullscreen = on_fullscreen.clone();
                    let on_ended = on_webview_ended.clone();
                    let on_error = on_webview_error.clone();

                    this.child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(webview(element_id, page_url).size_full().on_message(
                                move |message, window, cx| {
                                    if message.get("kind").and_then(|value| value.as_str())
                                        != Some("kael-video-event")
                                    {
                                        return;
                                    }

                                    let event_name = message
                                        .get("event")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or_default();
                                    let current_time = Duration::from_secs_f64(
                                        message
                                            .get("currentTime")
                                            .and_then(|value| value.as_f64())
                                            .unwrap_or(0.0)
                                            .max(0.0),
                                    );
                                    let duration = message
                                        .get("duration")
                                        .and_then(|value| value.as_f64())
                                        .filter(|duration| duration.is_finite() && *duration >= 0.0)
                                        .map(Duration::from_secs_f64);
                                    let volume = message
                                        .get("volume")
                                        .and_then(|value| value.as_f64())
                                        .unwrap_or(1.0)
                                        .clamp(0.0, 1.0)
                                        as f32;
                                    let muted = message
                                        .get("muted")
                                        .and_then(|value| value.as_bool())
                                        .unwrap_or(false);
                                    let playback_rate = message
                                        .get("playbackRate")
                                        .and_then(|value| value.as_f64())
                                        .unwrap_or(1.0)
                                        .max(0.0) as f32;
                                    let width = message
                                        .get("videoWidth")
                                        .and_then(|value| value.as_u64())
                                        .unwrap_or(0)
                                        .min(u32::MAX as u64) as u32;
                                    let height = message
                                        .get("videoHeight")
                                        .and_then(|value| value.as_u64())
                                        .unwrap_or(0)
                                        .min(u32::MAX as u64) as u32;
                                    let buffered_ranges = message
                                        .get("buffered")
                                        .and_then(|value| value.as_array())
                                        .map(|ranges| {
                                            ranges
                                                .iter()
                                                .filter_map(|range| {
                                                    let range = range.as_array()?;
                                                    let start = range.first()?.as_f64()?;
                                                    let end = range.get(1)?.as_f64()?;
                                                    (start.is_finite()
                                                        && end.is_finite()
                                                        && start >= 0.0
                                                        && end >= start)
                                                        .then(|| {
                                                            TimeRange::new(
                                                                Duration::from_secs_f64(start),
                                                                Duration::from_secs_f64(end),
                                                            )
                                                        })
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();
                                    let fullscreen = message
                                        .get("fullscreen")
                                        .and_then(|value| value.as_bool())
                                        .unwrap_or(false);
                                    let picture_in_picture = message
                                        .get("pictureInPicture")
                                        .and_then(|value| value.as_bool())
                                        .unwrap_or(false);
                                    let selected_text_track_id = message
                                        .get("selectedTextTrack")
                                        .and_then(|value| value.as_object())
                                        .and_then(|track| {
                                            track
                                                .get("id")
                                                .and_then(|value| value.as_str())
                                                .filter(|id| !id.is_empty())
                                                .map(|id| SharedString::from(id.to_string()))
                                        });
                                    let active_cues = message
                                        .get("activeCues")
                                        .and_then(|value| value.as_array())
                                        .map(|cues| {
                                            cues.iter()
                                                .filter_map(|cue| {
                                                    let start = cue
                                                        .get("startTime")
                                                        .and_then(|value| value.as_f64())?;
                                                    let end = cue
                                                        .get("endTime")
                                                        .and_then(|value| value.as_f64())?;
                                                    let text = cue
                                                        .get("text")
                                                        .and_then(|value| value.as_str())
                                                        .unwrap_or_default();
                                                    (start.is_finite()
                                                        && end.is_finite()
                                                        && start >= 0.0
                                                        && end >= start)
                                                        .then(|| {
                                                            TextTrackCue::new(
                                                                Duration::from_secs_f64(start),
                                                                Duration::from_secs_f64(end),
                                                                text.to_string(),
                                                            )
                                                        })
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();

                                    let event = match event_name {
                                        "loadedmetadata" => {
                                            cx.update_entity(&state, |state, cx| {
                                                if let Some(duration) = duration {
                                                    state.set_duration(duration.as_secs_f64(), cx);
                                                }
                                                state.set_current_time(
                                                    current_time.as_secs_f64(),
                                                    cx,
                                                );
                                            });
                                            if let Some(handler) = &on_loaded_metadata {
                                                handler(duration, width, height, window, cx);
                                            }
                                            Some(VideoEvent::LoadedMetadata {
                                                duration,
                                                width,
                                                height,
                                            })
                                        }
                                        "canplay" => {
                                            if let Some(handler) = &on_can_play {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::CanPlay)
                                        }
                                        "canplaythrough" => {
                                            if let Some(handler) = &on_can_play_through {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::CanPlayThrough)
                                        }
                                        "waiting" => {
                                            if let Some(handler) = &on_waiting {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::Waiting)
                                        }
                                        "progress" => {
                                            if let Some(handler) = &on_progress {
                                                handler(buffered_ranges.clone(), window, cx);
                                            }
                                            Some(VideoEvent::Progress { buffered_ranges })
                                        }
                                        "snapshot" => {
                                            cx.update_entity(&state, |state, cx| {
                                                if let Some(duration) = duration {
                                                    state.set_duration(duration.as_secs_f64(), cx);
                                                }
                                                state.set_current_time(
                                                    current_time.as_secs_f64(),
                                                    cx,
                                                );
                                            });
                                            if let Some(handler) = &on_progress {
                                                handler(buffered_ranges.clone(), window, cx);
                                            }
                                            if let Some(handler) = &on_time_update {
                                                handler(current_time, window, cx);
                                            }
                                            Some(VideoEvent::TimeUpdate { current_time })
                                        }
                                        "playing" | "play" => {
                                            cx.update_entity(&state, |state, cx| state.play(cx));
                                            if let Some(handler) = &on_playing {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::Playing)
                                        }
                                        "pause" => {
                                            cx.update_entity(&state, |state, cx| state.pause(cx));
                                            if let Some(handler) = &on_paused {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::Paused)
                                        }
                                        "timeupdate" => {
                                            cx.update_entity(&state, |state, cx| {
                                                state.set_current_time(
                                                    current_time.as_secs_f64(),
                                                    cx,
                                                );
                                            });
                                            if let Some(handler) = &on_time_update {
                                                handler(current_time, window, cx);
                                            }
                                            Some(VideoEvent::TimeUpdate { current_time })
                                        }
                                        "seeked" => {
                                            cx.update_entity(&state, |state, cx| {
                                                state.set_current_time(
                                                    current_time.as_secs_f64(),
                                                    cx,
                                                );
                                            });
                                            if let Some(handler) = &on_seeked {
                                                handler(current_time, window, cx);
                                            }
                                            Some(VideoEvent::Seeked { current_time })
                                        }
                                        "volumechange" => {
                                            cx.update_entity(&state, |state, cx| {
                                                state.volume = volume;
                                                state.is_muted = muted;
                                                state.touch_controls(cx);
                                            });
                                            if let Some(handler) = &on_volume_changed {
                                                handler(volume, muted, window, cx);
                                            }
                                            Some(VideoEvent::VolumeChange { volume, muted })
                                        }
                                        "ratechange" => {
                                            cx.update_entity(&state, |state, cx| {
                                                state.set_playback_speed(
                                                    VideoPlaybackSpeed::from_multiplier(
                                                        playback_rate,
                                                    ),
                                                    cx,
                                                );
                                            });
                                            if let Some(handler) = &on_rate_change {
                                                handler(playback_rate, window, cx);
                                            }
                                            Some(VideoEvent::RateChange { playback_rate })
                                        }
                                        "texttrackchange" => {
                                            if let Some(handler) = &on_text_track_changed {
                                                handler(
                                                    selected_text_track_id.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                            Some(VideoEvent::TextTrackChanged {
                                                id: selected_text_track_id,
                                            })
                                        }
                                        "cuechange" => {
                                            if let Some(handler) = &on_cue_change {
                                                handler(active_cues.clone(), window, cx);
                                            }
                                            Some(VideoEvent::CueChange { cues: active_cues })
                                        }
                                        "fullscreenchange" => {
                                            if let Some(handler) = &on_fullscreen {
                                                handler(fullscreen, window, cx);
                                            }
                                            Some(VideoEvent::FullscreenChange { fullscreen })
                                        }
                                        "enterpictureinpicture" | "leavepictureinpicture" => {
                                            Some(VideoEvent::PictureInPictureChange {
                                                picture_in_picture,
                                            })
                                        }
                                        "ended" => {
                                            cx.update_entity(&state, |state, cx| {
                                                if let Some(duration) = duration {
                                                    state.set_current_time(
                                                        duration.as_secs_f64(),
                                                        cx,
                                                    );
                                                }
                                                state.pause(cx);
                                            });
                                            if let Some(handler) = &on_ended {
                                                handler(window, cx);
                                            }
                                            Some(VideoEvent::Ended)
                                        }
                                        "error" => {
                                            let error = message
                                                .get("message")
                                                .and_then(|value| value.as_str())
                                                .unwrap_or("Video playback failed")
                                                .to_string();
                                            if let Some(handler) = &on_error {
                                                handler(error.clone(), window, cx);
                                            }
                                            Some(VideoEvent::Error(error))
                                        }
                                        _ => None,
                                    };

                                    if let (Some(handler), Some(event)) = (&on_event, event) {
                                        handler(event, window, cx);
                                    }
                                },
                            )),
                    )
                } else {
                    let controller = controller.clone();
                    if let Some(controller) = controller {
                        this.child(
                            div().absolute().inset_0().child(
                                video(controller.source())
                                    .sync_to(&controller.audio_handle())
                                    .object_fit(object_fit)
                                    .size_full(),
                            ),
                        )
                    } else {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(kael::black())
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(kael::white().opacity(0.5))
                                .text_sm()
                                .font_family(theme.tokens.font_family.clone())
                                .child("Video content area"),
                        )
                    }
                }
            })
            .when(
                show_captions && !active_text_cues.is_empty() && !show_poster,
                |this| {
                    let caption_bottom = if show_controls {
                        controls_height + px(52.0)
                    } else {
                        px(32.0)
                    };

                    this.child(
                        div()
                            .id(video_player_child_id(&player_id, "captions-overlay"))
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(caption_bottom)
                            .px(px(24.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.0))
                            .children(active_text_cues.into_iter().map(|cue| {
                                div()
                                    .max_w(relative(0.86))
                                    .px(px(10.0))
                                    .py(px(5.0))
                                    .rounded(theme.tokens.radius_md)
                                    .bg(caption_style.background)
                                    .text_color(caption_style.text_color)
                                    .text_size(caption_style.font_size)
                                    .line_height(caption_style.line_height)
                                    .text_align(TextAlign::Center)
                                    .font_family(theme.tokens.font_family.clone())
                                    .child(cue.text.clone())
                            })),
                    )
                },
            )
            .child({
                let state_play = state_entity.clone();
                let controller_play = controller.clone();
                let on_play_center = on_play.clone();
                let on_pause_center = on_pause.clone();

                div()
                    .id(video_player_child_id(&player_id, "center-play-button"))
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(
                        !use_webview_player
                            && show_controls_chrome
                            && (controls_visible || !is_playing),
                        |this| {
                            this.child(
                            IconButton::new(play_icon)
                                .id(video_player_child_id(&player_id, "play-overlay"))
                                .label(if is_playing { "Pause video" } else { "Play video" })
                                .variant(ButtonVariant::Ghost)
                                .size(px(72.0))
                                .icon_size(px(32.0))
                                .rounded_full()
                                .bg(kael::black().opacity(0.6))
                                .border_2()
                                .border_color(kael::white().opacity(0.3))
                                .text_color(kael::white())
                                .on_click(move |_, window, cx| {
                                    let is_playing_now = state_play.read(cx).is_playing();
                                    cx.update_entity(&state_play, |state, cx| state.toggle_play(cx));
                                    if is_playing_now {
                                        if let Some(controller) = &controller_play {
                                            controller.pause();
                                        }
                                        if let Some(handler) = &on_pause_center {
                                            handler(window, cx);
                                        }
                                    } else {
                                        if let Some(controller) = &controller_play {
                                            let _ = controller.play();
                                        }
                                        if let Some(handler) = &on_play_center {
                                            handler(window, cx);
                                        }
                                    }
                                })
                            )
                        },
                    )
            })
            .when(show_controls, |this| {
                let state_progress = state_entity.clone();
                let state_progress_drag = state_entity.clone();
                let state_progress_move = state_entity.clone();
                let state_progress_up = state_entity.clone();
                let state_progress_key = state_entity.clone();
                let state_progress_increment = state_entity.clone();
                let state_progress_decrement = state_entity.clone();
                let state_progress_set_value = state_entity.clone();
                let controller_seek_progress = controller.clone();
                let controller_seek_drag = controller.clone();
                let controller_seek_key = controller.clone();
                let controller_seek_increment = controller.clone();
                let controller_seek_decrement = controller.clone();
                let controller_seek_set_value = controller.clone();
                let on_seek_progress = on_seek.clone();
                let on_seek_drag = on_seek.clone();
                let on_seek_key = on_seek.clone();
                let on_seek_increment = on_seek.clone();
                let on_seek_decrement = on_seek.clone();
                let on_seek_set_value = on_seek.clone();
                let progress_focus_on_mouse = progress_focus_handle.clone();
                let tracked_progress_focus =
                    progress_focus_handle.clone().tab_index(0).tab_stop(true);

                let state_volume = state_entity.clone();
                let state_volume_icon = state_entity.clone();
                let state_volume_drag = state_entity.clone();
                let state_volume_move = state_entity.clone();
                let state_volume_up = state_entity.clone();
                let state_volume_key = state_entity.clone();
                let state_volume_increment = state_entity.clone();
                let state_volume_decrement = state_entity.clone();
                let state_volume_set_value = state_entity.clone();
                let controller_volume_icon = controller.clone();
                let controller_volume_slider = controller.clone();
                let controller_volume_drag = controller.clone();
                let controller_volume_key = controller.clone();
                let controller_volume_increment = controller.clone();
                let controller_volume_decrement = controller.clone();
                let controller_volume_set_value = controller.clone();
                let on_volume_icon = on_volume_change.clone();
                let on_volume_slider = on_volume_change.clone();
                let on_volume_drag_change = on_volume_change.clone();
                let on_volume_key = on_volume_change.clone();
                let on_volume_increment = on_volume_change.clone();
                let on_volume_decrement = on_volume_change.clone();
                let on_volume_set_value = on_volume_change.clone();
                let volume_focus_on_mouse = volume_focus_handle.clone();
                let tracked_volume_focus =
                    volume_focus_handle.clone().tab_index(0).tab_stop(true);

                let state_play_btn = state_entity.clone();
                let controller_play_btn = controller.clone();
                let on_play_btn = on_play.clone();
                let on_pause_btn = on_pause.clone();

                let state_skip_back = state_entity.clone();
                let controller_seek_back = controller.clone();
                let on_seek_back = on_seek.clone();

                let state_skip_forward = state_entity.clone();
                let controller_seek_forward = controller.clone();
                let on_seek_forward = on_seek.clone();

                let state_speed = state_entity.clone();
                let state_speed_item = state_entity.clone();
                let controller_speed_item = controller.clone();
                let on_speed_change = on_playback_speed_change.clone();

                let state_text_track = state_entity.clone();
                let state_text_track_item = state_entity.clone();
                let controller_text_track_item = controller.clone();
                let has_text_tracks = !text_tracks.is_empty();
                let selected_text_track_id = selected_text_track_id.clone();

                let state_fullscreen = state_entity.clone();
                let on_fullscreen_btn = on_fullscreen.clone();

                this.child(
                    div()
                        .id(video_player_child_id(&player_id, "controls-overlay"))
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(controls_height + px(40.0))
                        .bg(kael::black().opacity(0.7))
                        .flex()
                        .flex_col()
                        .justify_end()
                        .child(
                            div()
                                .id(video_player_child_id(&player_id, "progress-bar-container"))
                                .px(px(12.0))
                                .pb(px(4.0))
                                .child(
                                    div()
                                        .id(video_player_child_id(&player_id, "progress-bar"))
                                        .accessibility(AccessibilityAttributes::slider(
                                            "Playback position",
                                            current_time,
                                            0.0,
                                            duration.max(0.0),
                                            Some(5.0),
                                        ))
                                        .track_focus(&tracked_progress_focus)
                                        .relative()
                                        .h(px(6.0))
                                        .w_full()
                                        .bg(kael::white().opacity(0.3))
                                        .rounded_full()
                                        .cursor(CursorStyle::PointingHand)
                                        .on_accessibility_action(
                                            AccessibilityAction::Increment,
                                            move |request, window, cx| {
                                                handle_video_seek_accessibility_action(
                                                    request,
                                                    &state_progress_increment,
                                                    &controller_seek_increment,
                                                    &on_seek_increment,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                        .on_accessibility_action(
                                            AccessibilityAction::Decrement,
                                            move |request, window, cx| {
                                                handle_video_seek_accessibility_action(
                                                    request,
                                                    &state_progress_decrement,
                                                    &controller_seek_decrement,
                                                    &on_seek_decrement,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                        .on_accessibility_action(
                                            AccessibilityAction::SetValue,
                                            move |request, window, cx| {
                                                handle_video_seek_accessibility_action(
                                                    request,
                                                    &state_progress_set_value,
                                                    &controller_seek_set_value,
                                                    &on_seek_set_value,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                        .child(
                                            canvas_with_prepaint(
                                                {
                                                    let state = state_progress.clone();
                                                    move |bounds, _, cx| {
                                                        state.update(cx, |state, _| {
                                                            state.progress_bounds = bounds;
                                                        });
                                                    }
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .size_full(),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .top_0()
                                                .h_full()
                                                .w(relative(progress as f32))
                                                .bg(theme.tokens.primary)
                                                .rounded_full()
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .left(relative(progress as f32))
                                                .top(px(-3.0))
                                                .ml(px(-6.0))
                                                .size(px(12.0))
                                                .rounded_full()
                                                .bg(theme.tokens.primary)
                                                .border_2()
                                                .border_color(kael::white())
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            window.listener_for(
                                                &state_progress_drag,
                                                {
                                                    let on_seek = on_seek_progress.clone();
                                                    move |state, e: &MouseDownEvent, window, cx| {
                                                        window.focus(&progress_focus_on_mouse);
                                                        state.is_seeking = true;
                                                        state.update_seek_from_position(e.position, cx);
                                                        if let Some(controller) = &controller_seek_progress {
                                                            let _ = controller.fast_seek(Duration::from_secs_f64(state.current_time.max(0.0)));
                                                        }
                                                        if let Some(handler) = &on_seek {
                                                            handler(state.current_time, window, cx);
                                                        }
                                                    }
                                                },
                                            ),
                                        )
                                        .on_mouse_move(
                                            window.listener_for(
                                                &state_progress_move,
                                                {
                                                    let on_seek = on_seek_drag.clone();
                                                    move |state, e: &MouseMoveEvent, window, cx| {
                                                        if state.is_seeking {
                                                            state.update_seek_from_position(e.position, cx);
                                                            if let Some(controller) = &controller_seek_drag {
                                                                let _ = controller.fast_seek(Duration::from_secs_f64(state.current_time.max(0.0)));
                                                            }
                                                            if let Some(handler) = &on_seek {
                                                                handler(state.current_time, window, cx);
                                                            }
                                                        }
                                                    }
                                                },
                                            ),
                                        )
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            window.listener_for(
                                                &state_progress_up,
                                                move |state, _: &MouseUpEvent, _, _| {
                                                    state.is_seeking = false;
                                                },
                                            ),
                                        )
                                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                            let delta = match event.keystroke.key.as_str() {
                                                "left" | "down" => Some(-5.0),
                                                "right" | "up" => Some(5.0),
                                                "home" => Some(f64::NEG_INFINITY),
                                                "end" => Some(f64::INFINITY),
                                                _ => None,
                                            };
                                            if let Some(delta) = delta {
                                                state_progress_key.update(cx, |state, cx| {
                                                    let next = if delta == f64::NEG_INFINITY {
                                                        0.0
                                                    } else if delta == f64::INFINITY {
                                                        state.duration
                                                    } else {
                                                        state.current_time + delta
                                                    };
                                                    state.set_current_time(next, cx);
                                                });
                                                let next = state_progress_key.read(cx).current_time();
                                                if let Some(controller) = &controller_seek_key {
                                                    let _ = controller.seek(Duration::from_secs_f64(next));
                                                }
                                                if let Some(handler) = &on_seek_key {
                                                    handler(next, window, cx);
                                                }
                                                cx.stop_propagation();
                                            }
                                        })
                                )
                        )
                        .child(
                            div()
                                .id(video_player_child_id(&player_id, "controls-bar"))
                                .h(controls_height)
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            IconButton::new("rewind")
                                                .id(video_player_child_id(&player_id, "skip-back-btn"))
                                                .label("Skip back 10 seconds")
                                                .variant(ButtonVariant::Ghost)
                                                .size(px(32.0))
                                                .icon_size(icon_size)
                                                .text_color(kael::white())
                                                .on_click(move |_, window, cx| {
                                                    cx.update_entity(&state_skip_back, |state, cx| {
                                                        state.seek_relative(-10.0, cx);
                                                    });
                                                    let current_time = state_skip_back.read(cx).current_time();
                                                    if let Some(controller) = &controller_seek_back {
                                                        let _ = controller.seek(Duration::from_secs_f64(current_time.max(0.0)));
                                                    }
                                                    if let Some(handler) = &on_seek_back {
                                                        handler(current_time, window, cx);
                                                    }
                                                })
                                        )
                                        .child(
                                            IconButton::new(play_icon)
                                                .id(video_player_child_id(&player_id, "play-pause-btn"))
                                                .label(if is_playing { "Pause video" } else { "Play video" })
                                                .variant(ButtonVariant::Default)
                                                .size(px(40.0))
                                                .icon_size(icon_size)
                                                .rounded_full()
                                                .on_click(move |_, window, cx| {
                                                    let is_playing_now = state_play_btn.read(cx).is_playing();
                                                    cx.update_entity(&state_play_btn, |state, cx| state.toggle_play(cx));
                                                    if is_playing_now {
                                                        if let Some(controller) = &controller_play_btn {
                                                            controller.pause();
                                                        }
                                                        if let Some(handler) = &on_pause_btn {
                                                            handler(window, cx);
                                                        }
                                                    } else {
                                                        if let Some(controller) = &controller_play_btn {
                                                            let _ = controller.play();
                                                        }
                                                        if let Some(handler) = &on_play_btn {
                                                            handler(window, cx);
                                                        }
                                                    }
                                                })
                                        )
                                        .child(
                                            IconButton::new("fast-forward")
                                                .id(video_player_child_id(&player_id, "skip-forward-btn"))
                                                .label("Skip forward 10 seconds")
                                                .variant(ButtonVariant::Ghost)
                                                .size(px(32.0))
                                                .icon_size(icon_size)
                                                .text_color(kael::white())
                                                .on_click(move |_, window, cx| {
                                                    cx.update_entity(&state_skip_forward, |state, cx| {
                                                        state.seek_relative(10.0, cx);
                                                    });
                                                    let current_time = state_skip_forward.read(cx).current_time();
                                                    if let Some(controller) = &controller_seek_forward {
                                                        let _ = controller.seek(Duration::from_secs_f64(current_time.max(0.0)));
                                                    }
                                                    if let Some(handler) = &on_seek_forward {
                                                        handler(current_time, window, cx);
                                                    }
                                                })
                                        )
                                        .child(
                                            div()
                                                .ml(px(8.0))
                                                .text_sm()
                                                .text_color(kael::white())
                                                .font_family(theme.tokens.font_family.clone())
                                                .child(format!("{} / {}", format_time(current_time), format_time(duration)))
                                        )
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .child(
                                                    IconButton::new(volume_icon)
                                                        .id(video_player_child_id(&player_id, "volume-btn"))
                                                        .label(if is_muted { "Unmute video" } else { "Mute video" })
                                                        .variant(ButtonVariant::Ghost)
                                                        .size(px(32.0))
                                                        .icon_size(icon_size)
                                                        .text_color(kael::white())
                                                        .on_click(move |_, window, cx| {
                                                            cx.update_entity(&state_volume_icon, |state, cx| state.toggle_mute(cx));
                                                            let (effective_volume, muted) = {
                                                                let state = state_volume_icon.read(cx);
                                                                (state.effective_volume(), state.is_muted())
                                                            };
                                                            if let Some(controller) = &controller_volume_icon {
                                                                controller.set_muted(muted);
                                                            }
                                                            if let Some(handler) = &on_volume_icon {
                                                                handler(effective_volume, window, cx);
                                                            }
                                                        })
                                                )
                                                .child(
                                                    div()
                                                        .id(video_player_child_id(&player_id, "volume-slider"))
                                                        .accessibility(AccessibilityAttributes::slider(
                                                            "Volume",
                                                            volume as f64,
                                                            0.0,
                                                            1.0,
                                                            Some(0.05),
                                                        ))
                                                        .track_focus(&tracked_volume_focus)
                                                        .relative()
                                                        .w(px(80.0))
                                                        .h(px(4.0))
                                                        .bg(kael::white().opacity(0.3))
                                                        .rounded_full()
                                                        .cursor(CursorStyle::PointingHand)
                                                        .on_accessibility_action(
                                                            AccessibilityAction::Increment,
                                                            move |request, window, cx| {
                                                                handle_video_volume_accessibility_action(
                                                                    request,
                                                                    &state_volume_increment,
                                                                    &controller_volume_increment,
                                                                    &on_volume_increment,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )
                                                        .on_accessibility_action(
                                                            AccessibilityAction::Decrement,
                                                            move |request, window, cx| {
                                                                handle_video_volume_accessibility_action(
                                                                    request,
                                                                    &state_volume_decrement,
                                                                    &controller_volume_decrement,
                                                                    &on_volume_decrement,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )
                                                        .on_accessibility_action(
                                                            AccessibilityAction::SetValue,
                                                            move |request, window, cx| {
                                                                handle_video_volume_accessibility_action(
                                                                    request,
                                                                    &state_volume_set_value,
                                                                    &controller_volume_set_value,
                                                                    &on_volume_set_value,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )
                                                        .child(
                                                            canvas_with_prepaint(
                                                                {
                                                                    let state = state_volume.clone();
                                                                    move |bounds, _, cx| {
                                                                        state.update(cx, |state, _| {
                                                                            state.volume_bounds = bounds;
                                                                        });
                                                                    }
                                                                },
                                                                |_, _, _, _| {},
                                                            )
                                                            .absolute()
                                                            .size_full(),
                                                        )
                                                        .child(
                                                            div()
                                                                .absolute()
                                                                .left_0()
                                                                .top_0()
                                                                .h_full()
                                                                .w(relative(volume))
                                                                .bg(kael::white())
                                                                .rounded_full()
                                                        )
                                                        .child(
                                                            div()
                                                                .absolute()
                                                                .left(relative(volume))
                                                                .top(px(-4.0))
                                                                .ml(px(-6.0))
                                                                .size(px(12.0))
                                                                .rounded_full()
                                                                .bg(kael::white())
                                                        )
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            window.listener_for(
                                                                &state_volume_drag,
                                                                {
                                                                    let on_volume = on_volume_slider.clone();
                                                                    move |state, e: &MouseDownEvent, window, cx| {
                                                                        window.focus(&volume_focus_on_mouse);
                                                                        state.is_volume_dragging = true;
                                                                        state.update_volume_from_position(e.position, cx);
                                                                        if let Some(controller) = &controller_volume_slider {
                                                                            controller.set_volume(state.volume);
                                                                        }
                                                                        if let Some(handler) = &on_volume {
                                                                            handler(state.volume, window, cx);
                                                                        }
                                                                    }
                                                                },
                                                            ),
                                                        )
                                                        .on_mouse_move(
                                                            window.listener_for(
                                                                &state_volume_move,
                                                                {
                                                                    let on_volume = on_volume_drag_change.clone();
                                                                    move |state, e: &MouseMoveEvent, window, cx| {
                                                                        if state.is_volume_dragging {
                                                                            state.update_volume_from_position(e.position, cx);
                                                                            if let Some(controller) = &controller_volume_drag {
                                                                                controller.set_volume(state.volume);
                                                                            }
                                                                            if let Some(handler) = &on_volume {
                                                                                handler(state.volume, window, cx);
                                                                            }
                                                                        }
                                                                    }
                                                                },
                                                            ),
                                                        )
                                                        .on_mouse_up(
                                                            MouseButton::Left,
                                                            window.listener_for(
                                                                &state_volume_up,
                                                                move |state, _: &MouseUpEvent, _, _| {
                                                                    state.is_volume_dragging = false;
                                                                },
                                                            ),
                                                        )
                                                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                                            let next = match event.keystroke.key.as_str() {
                                                                "left" | "down" => Some((volume - 0.05).max(0.0)),
                                                                "right" | "up" => Some((volume + 0.05).min(1.0)),
                                                                "home" => Some(0.0),
                                                                "end" => Some(1.0),
                                                                _ => None,
                                                            };
                                                            if let Some(next) = next {
                                                                state_volume_key.update(cx, |state, cx| {
                                                                    state.set_volume(next, cx);
                                                                });
                                                                if let Some(controller) = &controller_volume_key {
                                                                    controller.set_volume(next);
                                                                }
                                                                if let Some(handler) = &on_volume_key {
                                                                    handler(next, window, cx);
                                                                }
                                                                cx.stop_propagation();
                                                            }
                                                        })
                                                )
                                        )
                                        .when(has_text_tracks, {
                                            let theme = theme.clone();
                                            let captions_player_id = player_id.clone();
                                            move |this| {
                                                this.child(
                                                    div()
                                                        .id(video_player_child_id(&captions_player_id, "captions-menu"))
                                                        .relative()
                                                        .child(
                                                            IconButton::new(if selected_text_track_id.is_some() {
                                                                "captions"
                                                            } else {
                                                                "captions-off"
                                                            })
                                                                .id(video_player_child_id(&captions_player_id, "captions-btn"))
                                                                .label("Choose captions")
                                                                .variant(ButtonVariant::Ghost)
                                                                .size(px(32.0))
                                                                .icon_size(icon_size)
                                                                .text_color(kael::white())
                                                                .on_click(move |_, _, cx| {
                                                                    cx.update_entity(
                                                                        &state_text_track,
                                                                        |state, cx| {
                                                                            state.toggle_text_track_menu(cx);
                                                                        },
                                                                    );
                                                                }),
                                                        )
                                                        .when(show_text_track_menu, {
                                                            let theme = theme.clone();
                                                            let text_tracks = text_tracks.clone();
                                                            let selected_text_track_id =
                                                                selected_text_track_id.clone();
                                                            move |this| {
                                                                let off_selected =
                                                                    selected_text_track_id.is_none();
                                                                let captions_option_parent =
                                                                    captions_player_id.clone();

                                                                this.child(
                                                                    div()
                                                                        .absolute()
                                                                        .bottom(px(36.0))
                                                                        .right_0()
                                                                        .min_w(px(132.0))
                                                                        .max_w(px(220.0))
                                                                        .bg(theme.tokens.popover)
                                                                        .rounded(theme.tokens.radius_md)
                                                                        .border_1()
                                                                        .border_color(theme.tokens.border)
                                                                        .shadow(
                                                                            theme
                                                                                .tokens
                                                                                .shadow_lg
                                                                                .to_vec(),
                                                                        )
                                                                        .py(px(4.0))
                                                                        .child({
                                                                            let state_item =
                                                                                state_text_track_item
                                                                                    .clone();
                                                                            let controller =
                                                                                controller_text_track_item
                                                                                    .clone();

                                                                            Button::new(
                                                                                video_player_child_id(&captions_player_id, "captions-off-option"),
                                                                                "Off",
                                                                            )
                                                                                .variant(ButtonVariant::Ghost)
                                                                                .size(ButtonSize::Sm)
                                                                                .selected(off_selected)
                                                                                .w_full()
                                                                                .justify_start()
                                                                                .text_color(if off_selected {
                                                                                    theme.tokens.primary
                                                                                } else {
                                                                                    theme
                                                                                        .tokens
                                                                                        .popover_foreground
                                                                                })
                                                                                .on_click(
                                                                                    move |_, _, cx| {
                                                                                        if let Some(controller) =
                                                                                            &controller
                                                                                        {
                                                                                            controller
                                                                                                .disable_text_track();
                                                                                        }
                                                                                        cx.update_entity(
                                                                                            &state_item,
                                                                                            |state, cx| {
                                                                                                state
                                                                                                    .close_text_track_menu(cx);
                                                                                            },
                                                                                        );
                                                                                    },
                                                                                )
                                                                        })
                                                                        .children(text_tracks.into_iter().map(
                                                                            move |track| {
                                                                                let state_item =
                                                                                    state_text_track_item
                                                                                        .clone();
                                                                                let controller =
                                                                                    controller_text_track_item
                                                                                        .clone();
                                                                                let track_id =
                                                                                    track.id.clone();
                                                                                let is_selected =
                                                                                    selected_text_track_id
                                                                                        .as_ref()
                                                                                        == Some(&track.id);
                                                                                let label = match &track.language {
                                                                                    Some(language)
                                                                                        if !language
                                                                                            .is_empty() =>
                                                                                    {
                                                                                        format!(
                                                                                            "{} ({})",
                                                                                            track.label,
                                                                                            language
                                                                                        )
                                                                                    }
                                                                                    _ => track.label.to_string(),
                                                                                };

                                                                                Button::new(
                                                                                    video_player_child_id(
                                                                                        &captions_option_parent,
                                                                                        format!("captions-{}", track.id),
                                                                                    ),
                                                                                    label,
                                                                                )
                                                                                    .variant(ButtonVariant::Ghost)
                                                                                    .size(ButtonSize::Sm)
                                                                                    .selected(is_selected)
                                                                                    .w_full()
                                                                                    .justify_start()
                                                                                    .text_color(if is_selected {
                                                                                        theme.tokens.primary
                                                                                    } else {
                                                                                        theme
                                                                                            .tokens
                                                                                            .popover_foreground
                                                                                    })
                                                                                    .on_click(
                                                                                        move |_, _, cx| {
                                                                                            if let Some(controller) =
                                                                                                &controller
                                                                                            {
                                                                                                controller
                                                                                                    .select_text_track(
                                                                                                        track_id
                                                                                                            .as_ref(),
                                                                                                    );
                                                                                            }
                                                                                            cx.update_entity(
                                                                                                &state_item,
                                                                                                |state, cx| {
                                                                                                    state
                                                                                                        .close_text_track_menu(cx);
                                                                                                },
                                                                                            );
                                                                                        },
                                                                                    )
                                                                            },
                                                                        )),
                                                                )
                                                            }
                                                        }),
                                                )
                                            }
                                        })
                                        .child(
                                            div()
                                                .id(video_player_child_id(&player_id, "speed-menu"))
                                                .relative()
                                                .child(
                                                    Button::new(
                                                        video_player_child_id(&player_id, "speed-btn"),
                                                        playback_speed.label(),
                                                    )
                                                        .variant(ButtonVariant::Ghost)
                                                        .size(ButtonSize::Sm)
                                                        .text_color(kael::white())
                                                        .tooltip("Change playback speed")
                                                        .on_click(move |_, _, cx| {
                                                            cx.update_entity(&state_speed, |state, cx| state.toggle_speed_menu(cx));
                                                        })
                                                )
                                                .when(show_speed_menu, {
                                                    let theme = theme.clone();
                                                    let speed_player_id = player_id.clone();
                                                    move |this| {
                                                        this.child(
                                                            div()
                                                                .absolute()
                                                                .bottom(px(36.0))
                                                                .right_0()
                                                                .w(px(80.0))
                                                                .bg(theme.tokens.popover)
                                                                .rounded(theme.tokens.radius_md)
                                                                .border_1()
                                                                .border_color(theme.tokens.border)
                                                                .shadow(theme.tokens.shadow_lg.to_vec())
                                                                .py(px(4.0))
                                                                .children(
                                                                    VideoPlaybackSpeed::all().iter().map(|speed| {
                                                                        let state_item = state_speed_item.clone();
                                                                        let controller = controller_speed_item.clone();
                                                                        let on_speed = on_speed_change.clone();
                                                                        let speed_val = *speed;
                                                                        let is_selected = speed_val == playback_speed;

                                                                        Button::new(
                                                                            video_player_child_id(
                                                                                &speed_player_id,
                                                                                format!("speed-{}", speed_val.label()),
                                                                            ),
                                                                            speed_val.label(),
                                                                        )
                                                                            .variant(ButtonVariant::Ghost)
                                                                            .size(ButtonSize::Sm)
                                                                            .selected(is_selected)
                                                                            .w_full()
                                                                            .justify_start()
                                                                            .text_color(if is_selected {
                                                                                theme.tokens.primary
                                                                            } else {
                                                                                theme.tokens.popover_foreground
                                                                            })
                                                                            .on_click(move |_, window, cx| {
                                                                                cx.update_entity(&state_item, |state, cx| {
                                                                                    state.set_playback_speed(speed_val, cx);
                                                                                });
                                                                                if let Some(controller) = &controller {
                                                                                    controller.set_playback_rate(speed_val.multiplier());
                                                                                }
                                                                                if let Some(handler) = &on_speed {
                                                                                    handler(speed_val, window, cx);
                                                                                }
                                                                            })
                                                                    })
                                                                )
                                                        )
                                                    }
                                                })
                                        )
                                        .child(
                                            IconButton::new(if is_fullscreen { "minimize" } else { "maximize" })
                                                .id(video_player_child_id(&player_id, "fullscreen-btn"))
                                                .label(if is_fullscreen { "Exit full screen" } else { "Enter full screen" })
                                                .variant(ButtonVariant::Ghost)
                                                .size(px(32.0))
                                                .icon_size(icon_size)
                                                .text_color(kael::white())
                                                .on_click(move |_, window, cx| {
                                                    window.toggle_fullscreen();
                                                    let is_fs = window.is_fullscreen();
                                                    cx.update_entity(&state_fullscreen, |state, cx| {
                                                        state.set_fullscreen(is_fs, cx);
                                                    });
                                                    if let Some(handler) = &on_fullscreen_btn {
                                                        handler(is_fs, window, cx);
                                                    }
                                                })
                                        )
                                )
                        )
                )
            })
    }
}
