use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::icon_button::IconButton;
use crate::theme::Theme;
use kael::{prelude::*, *};
#[cfg(feature = "audio")]
use std::cell::RefCell;
#[cfg(feature = "audio")]
use std::io::BufReader;
use std::{panic::Location, rc::Rc};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum AudioPlayerSize {
    Compact,
    #[default]
    Full,
}

impl AudioPlayerSize {
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum PlaybackSpeed {
    Half,
    #[default]
    Normal,
    OneAndHalf,
    Double,
}

impl PlaybackSpeed {
    pub fn to_text(self) -> &'static str {
        match self {
            PlaybackSpeed::Half => "half",
            PlaybackSpeed::Normal => "normal",
            PlaybackSpeed::OneAndHalf => "one-and-half",
            PlaybackSpeed::Double => "double",
        }
    }

    pub fn value(&self) -> f32 {
        match self {
            PlaybackSpeed::Half => 0.5,
            PlaybackSpeed::Normal => 1.0,
            PlaybackSpeed::OneAndHalf => 1.5,
            PlaybackSpeed::Double => 2.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            PlaybackSpeed::Half => "0.5x",
            PlaybackSpeed::Normal => "1x",
            PlaybackSpeed::OneAndHalf => "1.5x",
            PlaybackSpeed::Double => "2x",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            PlaybackSpeed::Half => PlaybackSpeed::Normal,
            PlaybackSpeed::Normal => PlaybackSpeed::OneAndHalf,
            PlaybackSpeed::OneAndHalf => PlaybackSpeed::Double,
            PlaybackSpeed::Double => PlaybackSpeed::Half,
        }
    }
}

#[cfg(feature = "audio")]
pub struct AudioBackend {
    sink: rodio::Sink,
    _stream: rodio::OutputStream,
    stream_handle: rodio::OutputStreamHandle,
    file_path: Option<String>,
}

#[cfg(feature = "audio")]
impl AudioBackend {
    pub fn new() -> Option<Self> {
        let (stream, stream_handle) = rodio::OutputStream::try_default().ok()?;
        let sink = rodio::Sink::try_new(&stream_handle).ok()?;
        sink.pause();
        Some(Self {
            sink,
            _stream: stream,
            stream_handle,
            file_path: None,
        })
    }

    pub fn load(&mut self, path: &str) -> Result<std::time::Duration, String> {
        use rodio::Source;

        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        let source = rodio::Decoder::new(reader).map_err(|e| e.to_string())?;
        let duration = source.total_duration().unwrap_or(std::time::Duration::ZERO);

        self.sink.stop();
        self.sink = rodio::Sink::try_new(&self.stream_handle).map_err(|e| e.to_string())?;
        self.sink.append(source);
        self.sink.pause();
        self.file_path = Some(path.to_string());

        Ok(duration)
    }

    pub fn play(&self) {
        self.sink.play();
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn stop(&self) {
        self.sink.stop();
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    pub fn set_speed(&self, speed: f32) {
        self.sink.set_speed(speed);
    }

    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }
}

pub struct AudioPlayerState {
    is_playing: bool,
    is_muted: bool,
    current_time: f32,
    duration: f32,
    volume: f32,
    playback_speed: PlaybackSpeed,
    focus_handle: FocusHandle,
    progress_focus_handle: FocusHandle,
    volume_focus_handle: FocusHandle,
    progress_dragging: bool,
    volume_dragging: bool,
    progress_bounds: Bounds<Pixels>,
    volume_bounds: Bounds<Pixels>,
    source_path: Option<String>,
    #[cfg(feature = "audio")]
    backend: Option<Rc<RefCell<AudioBackend>>>,
}

impl AudioPlayerState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            is_playing: false,
            is_muted: false,
            current_time: 0.0,
            duration: 0.0,
            volume: 0.8,
            playback_speed: PlaybackSpeed::Normal,
            focus_handle: cx.focus_handle(),
            progress_focus_handle: cx.focus_handle(),
            volume_focus_handle: cx.focus_handle(),
            progress_dragging: false,
            volume_dragging: false,
            progress_bounds: Bounds::default(),
            volume_bounds: Bounds::default(),
            source_path: None,
            #[cfg(feature = "audio")]
            backend: AudioBackend::new().map(|backend| Rc::new(RefCell::new(backend))),
        }
    }

    #[cfg(feature = "audio")]
    pub fn load_file(&mut self, path: impl Into<String>, cx: &mut Context<Self>) -> bool {
        let path_str = path.into();
        self.source_path = Some(path_str.clone());

        if let Some(ref backend) = self.backend
            && let Ok(mut backend) = backend.try_borrow_mut()
        {
            match backend.load(&path_str) {
                Ok(duration) => {
                    self.duration = duration.as_secs_f32();
                    self.current_time = 0.0;
                    self.is_playing = false;
                    backend.set_volume(self.volume);
                    backend.set_speed(self.playback_speed.value());
                    cx.notify();
                    return true;
                }
                Err(e) => {
                    eprintln!("Failed to load audio: {}", e);
                    return false;
                }
            }
        }
        false
    }

    #[cfg(not(feature = "audio"))]
    pub fn load_file(&mut self, path: impl Into<String>, cx: &mut Context<Self>) -> bool {
        self.source_path = Some(path.into());
        cx.notify();
        false
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    pub fn set_playing(&mut self, playing: bool, cx: &mut Context<Self>) {
        self.is_playing = playing;
        #[cfg(feature = "audio")]
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            if playing {
                backend.play();
            } else {
                backend.pause();
            }
        }
        cx.notify();
    }

    pub fn toggle_playing(&mut self, cx: &mut Context<Self>) {
        self.is_playing = !self.is_playing;
        #[cfg(feature = "audio")]
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            if self.is_playing {
                backend.play();
            } else {
                backend.pause();
            }
        }
        cx.notify();
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted
    }

    pub fn set_muted(&mut self, muted: bool, cx: &mut Context<Self>) {
        self.is_muted = muted;
        #[cfg(feature = "audio")]
        self.apply_volume();
        cx.notify();
    }

    pub fn toggle_muted(&mut self, cx: &mut Context<Self>) {
        self.is_muted = !self.is_muted;
        #[cfg(feature = "audio")]
        self.apply_volume();
        cx.notify();
    }

    pub fn current_time(&self) -> f32 {
        self.current_time
    }

    pub fn progress_class(&self) -> &'static str {
        audio_fraction_class(self.progress_percentage())
    }

    pub fn set_current_time(&mut self, time: f32, cx: &mut Context<Self>) {
        self.current_time = if time.is_finite() {
            time.clamp(0.0, self.duration)
        } else {
            0.0
        };
        cx.notify();
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_duration(&mut self, duration: f32, cx: &mut Context<Self>) {
        if !duration.is_finite() || duration < 0.0 {
            return;
        }
        self.duration = duration;
        self.current_time = self.current_time.min(duration);
        cx.notify();
    }

    pub fn volume(&self) -> f32 {
        self.volume
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
        #[cfg(feature = "audio")]
        self.apply_volume();
        cx.notify();
    }

    #[cfg(feature = "audio")]
    fn apply_volume(&self) {
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            let effective_vol = if self.is_muted { 0.0 } else { self.volume };
            backend.set_volume(effective_vol);
        }
    }

    pub fn effective_volume(&self) -> f32 {
        if self.is_muted { 0.0 } else { self.volume }
    }

    pub fn volume_class(&self) -> &'static str {
        audio_fraction_class(self.effective_volume())
    }

    pub fn playback_speed(&self) -> PlaybackSpeed {
        self.playback_speed
    }

    pub fn speed_key(&self) -> &'static str {
        self.playback_speed.to_text()
    }

    pub fn set_playback_speed(&mut self, speed: PlaybackSpeed, cx: &mut Context<Self>) {
        self.playback_speed = speed;
        #[cfg(feature = "audio")]
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            backend.set_speed(speed.value());
        }
        cx.notify();
    }

    pub fn cycle_playback_speed(&mut self, cx: &mut Context<Self>) {
        self.playback_speed = self.playback_speed.next();
        #[cfg(feature = "audio")]
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            backend.set_speed(self.playback_speed.value());
        }
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.is_playing = false;
        self.current_time = 0.0;
        #[cfg(feature = "audio")]
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            backend.stop();
        }
        cx.notify();
    }

    #[cfg(feature = "audio")]
    pub fn is_finished(&self) -> bool {
        if let Some(ref backend) = self.backend
            && let Ok(backend) = backend.try_borrow()
        {
            return backend.is_empty();
        }
        false
    }

    #[cfg(not(feature = "audio"))]
    pub fn is_finished(&self) -> bool {
        self.current_time >= self.duration
    }

    fn progress_percentage(&self) -> f32 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        (self.current_time / self.duration).clamp(0.0, 1.0)
    }

    fn update_progress_from_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let track_width = self.progress_bounds.size.width;
        if track_width <= px(0.0) {
            return;
        }
        let relative_x = (position.x - self.progress_bounds.left()).clamp(px(0.0), track_width);
        let percentage = (relative_x / track_width).clamp(0.0, 1.0);
        self.current_time = percentage * self.duration;
        cx.notify();
    }

    fn update_volume_from_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let track_width = self.volume_bounds.size.width;
        if track_width <= px(0.0) {
            return;
        }
        let relative_x = (position.x - self.volume_bounds.left()).clamp(px(0.0), track_width);
        let volume = (relative_x / track_width).clamp(0.0, 1.0);
        self.set_volume(volume, cx);
    }

    #[cfg(feature = "audio")]
    pub fn is_audio_loaded(&self) -> bool {
        self.backend.is_some() && self.source_path.is_some()
    }

    #[cfg(not(feature = "audio"))]
    pub fn is_audio_loaded(&self) -> bool {
        false
    }

    pub fn has_source_path(&self) -> bool {
        self.source_path.is_some()
    }

    pub fn to_text(&self) -> String {
        format!(
            "audio player state: playing {}, muted {}, progress {}, volume {}, speed {}, source {}, loaded {}, progress dragging {}, volume dragging {}",
            self.is_playing,
            self.is_muted,
            self.progress_class(),
            self.volume_class(),
            self.speed_key(),
            self.has_source_path(),
            self.is_audio_loaded(),
            self.progress_dragging,
            self.volume_dragging
        )
    }
}

impl Focusable for AudioPlayerState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AudioPlayerState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn format_time(seconds: f32) -> String {
    let total_seconds = seconds.max(0.0) as u32;
    let minutes = total_seconds / 60;
    let secs = total_seconds % 60;
    format!("{:02}:{:02}", minutes, secs)
}

fn handle_seek_accessibility_action(
    request: &AccessibilityActionRequest,
    state: &Entity<AudioPlayerState>,
    handler: &Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (current, duration) = {
        let state = state.read(cx);
        (state.current_time as f64, state.duration.max(0.0) as f64)
    };
    let Some(next) =
        crate::util::accessibility_adjusted_value(request, current, 0.0, duration, 5.0)
    else {
        return;
    };
    state.update(cx, |state, cx| {
        state.set_current_time(next as f32, cx);
        if let Some(handler) = handler {
            handler(state.current_time, window, cx);
        }
    });
}

fn handle_volume_accessibility_action(
    request: &AccessibilityActionRequest,
    state: &Entity<AudioPlayerState>,
    handler: &Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    window: &mut Window,
    cx: &mut App,
) {
    let current = state.read(cx).volume as f64;
    let Some(next) = crate::util::accessibility_adjusted_value(request, current, 0.0, 1.0, 0.05)
    else {
        return;
    };
    state.update(cx, |state, cx| state.set_volume(next as f32, cx));
    if let Some(handler) = handler {
        handler(next as f32, window, cx);
    }
}

#[derive(IntoElement)]
pub struct AudioPlayer {
    id: ElementId,
    state: Entity<AudioPlayerState>,
    size: AudioPlayerSize,
    disabled: bool,
    title: Option<SharedString>,
    on_play: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    on_pause: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    on_seek: Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    on_volume_change: Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    on_speed_change: Option<Rc<dyn Fn(PlaybackSpeed, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl AudioPlayer {
    #[track_caller]
    pub fn new(state: Entity<AudioPlayerState>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "audio-player:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            size: AudioPlayerSize::Full,
            disabled: false,
            title: None,
            on_play: None,
            on_pause: None,
            on_seek: None,
            on_volume_change: None,
            on_speed_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn size(mut self, size: AudioPlayerSize) -> Self {
        self.size = size;
        self
    }

    pub fn size_key(&self) -> &'static str {
        self.size.to_text()
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    pub fn handler_count(&self) -> usize {
        [
            self.on_play.is_some(),
            self.on_pause.is_some(),
            self.on_seek.is_some(),
            self.on_volume_change.is_some(),
            self.on_speed_change.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }

    pub fn to_text(&self) -> String {
        format!(
            "audio player: size {}, disabled {}, title {}, handlers {}",
            self.size_key(),
            self.is_disabled(),
            self.has_title(),
            self.handler_count()
        )
    }

    pub fn compact(mut self) -> Self {
        self.size = AudioPlayerSize::Compact;
        self
    }

    pub fn full(mut self) -> Self {
        self.size = AudioPlayerSize::Full;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
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

    pub fn on_seek(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
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

    pub fn on_speed_change(
        mut self,
        handler: impl Fn(PlaybackSpeed, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_speed_change = Some(Rc::new(handler));
        self
    }
}

fn audio_fraction_class(value: f32) -> &'static str {
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use kael::TestAppContext;

    #[::core::prelude::v1::test]
    fn audio_player_state_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(AudioPlayerState::new));

        let summary = cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.load_file("/Users/person/private-audio.wav", cx);
                state.set_duration(120.0, cx);
                state.set_current_time(64.0, cx);
                state.set_volume(0.4, cx);
                state.set_playback_speed(PlaybackSpeed::OneAndHalf, cx);
                state.set_playing(true, cx);
                state.to_text()
            })
        });

        assert!(summary.contains("playing true"));
        assert!(summary.contains("progress medium"));
        assert!(summary.contains("volume medium"));
        assert!(summary.contains("speed one-and-half"));
        assert!(summary.contains("source true"));
        assert!(!summary.contains("private-audio"));
        assert!(!summary.contains("64"));
        assert!(!summary.contains("0.4"));
    }

    #[::core::prelude::v1::test]
    fn audio_player_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(AudioPlayerState::new));
        let summary = AudioPlayer::new(state)
            .compact()
            .title("Secret Podcast")
            .on_play(|_, _| {})
            .on_seek(|_, _, _| {})
            .to_text();

        assert!(summary.contains("size compact"));
        assert!(summary.contains("title true"));
        assert!(summary.contains("handlers 2"));
        assert!(!summary.contains("Secret Podcast"));
    }

    #[::core::prelude::v1::test]
    fn audio_player_state_rejects_non_finite_values() {
        let cx = TestAppContext::single();
        let state = cx.update(|cx| cx.new(AudioPlayerState::new));

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_duration(120.0, cx);
                state.set_current_time(f32::NAN, cx);
                assert_eq!(state.current_time(), 0.0);

                state.set_duration(f32::INFINITY, cx);
                assert_eq!(state.duration(), 120.0);

                state.set_volume(f32::NAN, cx);
                assert_eq!(state.volume(), 0.0);
            });
        });
    }
}

impl Styled for AudioPlayer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AudioPlayer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let is_playing = state.is_playing;
        let is_muted = state.is_muted;
        let current_time = state.current_time;
        let duration = state.duration;
        let volume = state.volume;
        let progress_percentage = state.progress_percentage();
        let playback_speed = state.playback_speed;
        let progress_focus_handle = state.progress_focus_handle.clone();
        let volume_focus_handle = state.volume_focus_handle.clone();
        let user_style = self.style.clone();

        let (padding, gap, button_size, icon_size, track_height, thumb_size) = match self.size {
            AudioPlayerSize::Compact => (px(8.0), px(8.0), px(32.0), px(16.0), px(4.0), px(12.0)),
            AudioPlayerSize::Full => (px(16.0), px(12.0), px(40.0), px(20.0), px(6.0), px(16.0)),
        };

        let play_icon = if is_playing { "pause" } else { "play" };
        let volume_icon = if is_muted || volume == 0.0 {
            "volume-x"
        } else if volume < 0.5 {
            "volume-1"
        } else {
            "volume-2"
        };

        let accessibility_label = self
            .title
            .clone()
            .unwrap_or_else(|| "Audio player".into())
            .to_string();
        let accessibility_state = if self.disabled {
            AccessibilityState::DISABLED
        } else {
            AccessibilityState::NONE
        };
        let base = div()
            .id(self.id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(accessibility_label)
                    .states(accessibility_state),
            )
            .flex()
            .items_center()
            .gap(gap)
            .p(padding)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .when(self.disabled, |this| this.opacity(0.5));

        match self.size {
            AudioPlayerSize::Compact => base
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .child(self.render_play_button(
                    window,
                    theme,
                    play_icon,
                    is_playing,
                    button_size,
                    icon_size,
                ))
                .child(self.render_progress_bar(
                    window,
                    theme,
                    progress_percentage,
                    current_time,
                    duration,
                    progress_focus_handle.clone(),
                    track_height,
                    thumb_size,
                ))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.tokens.muted_foreground)
                        .font_family(theme.tokens.font_mono.clone())
                        .child(format!(
                            "{} / {}",
                            format_time(current_time),
                            format_time(duration)
                        )),
                ),

            AudioPlayerSize::Full => base
                .flex_col()
                .gap(px(12.0))
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .when_some(self.title.clone(), |this, title| {
                    this.child(
                        div()
                            .w_full()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.tokens.foreground)
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(StyledText::new(title).accessibility_hidden(true)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .w_full()
                        .child(self.render_progress_bar(
                            window,
                            theme,
                            progress_percentage,
                            current_time,
                            duration,
                            progress_focus_handle,
                            track_height,
                            thumb_size,
                        ))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .w_full()
                                .text_size(px(12.0))
                                .font_family(theme.tokens.font_mono.clone())
                                .text_color(theme.tokens.muted_foreground)
                                .child(format_time(current_time))
                                .child(format_time(duration)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(div().flex().items_center().gap(px(8.0)).child(
                            self.render_play_button(
                                window,
                                theme,
                                play_icon,
                                is_playing,
                                button_size,
                                icon_size,
                            ),
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(self.render_speed_button(window, theme, playback_speed))
                                .child(self.render_mute_button(
                                    window,
                                    theme,
                                    volume_icon,
                                    is_muted || volume == 0.0,
                                    px(28.0),
                                    px(14.0),
                                ))
                                .child(self.render_volume_slider(
                                    window,
                                    theme,
                                    volume,
                                    volume_focus_handle,
                                    px(80.0),
                                    px(4.0),
                                    px(12.0),
                                )),
                        ),
                ),
        }
    }
}

impl AudioPlayer {
    fn render_play_button(
        &self,
        window: &mut Window,
        _theme: &crate::theme::Theme,
        icon_name: &str,
        is_playing: bool,
        button_size: Pixels,
        icon_size: Pixels,
    ) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let on_play = self.on_play.clone();
        let on_pause = self.on_pause.clone();
        let disabled = self.disabled;

        IconButton::new(icon_name)
            .id(ElementId::NamedChild(
                Box::new(self.id.clone()),
                "play".into(),
            ))
            .label(if is_playing {
                "Pause audio"
            } else {
                "Play audio"
            })
            .variant(ButtonVariant::Default)
            .size(button_size)
            .icon_size(icon_size)
            .disabled(disabled)
            .rounded_full()
            .on_click(window.listener_for(&state, move |state, _, window, cx| {
                state.toggle_playing(cx);
                if state.is_playing {
                    if let Some(ref handler) = on_play {
                        handler(window, cx);
                    }
                } else if let Some(ref handler) = on_pause {
                    handler(window, cx);
                }
            }))
    }

    fn render_progress_bar(
        &self,
        window: &mut Window,
        theme: &crate::theme::Theme,
        percentage: f32,
        current_time: f32,
        duration: f32,
        focus_handle: FocusHandle,
        track_height: Pixels,
        thumb_size: Pixels,
    ) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let on_seek = self.on_seek.clone();
        let disabled = self.disabled;

        let state_for_key = state.clone();
        let on_seek_key = on_seek.clone();
        let focus_on_mouse = focus_handle.clone();
        let mut accessibility = AccessibilityAttributes::progress_bar(
            "Playback position",
            current_time as f64,
            0.0,
            duration.max(0.0) as f64,
        );
        accessibility.role = Some(AccessibilityRole::Slider);
        if disabled {
            accessibility = accessibility.states(AccessibilityState::DISABLED);
        } else {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Increment,
                AccessibilityAction::Decrement,
                AccessibilityAction::SetValue,
            ]);
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(self.id.clone()),
                "progress".into(),
            ))
            .accessibility(accessibility)
            .when(!disabled, |track| {
                track.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .flex_1()
            .h(thumb_size)
            .flex()
            .items_center()
            .relative()
            .child(
                canvas_with_prepaint(
                    {
                        let state = state.clone();
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
                    .w_full()
                    .h(track_height)
                    .rounded_full()
                    .bg(theme.tokens.muted)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(percentage))
                            .bg(theme.tokens.primary),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(percentage))
                    .top(px(0.0))
                    .ml(-(thumb_size / 2.0))
                    .size(thumb_size)
                    .rounded_full()
                    .bg(theme.tokens.primary)
                    .border_2()
                    .border_color(theme.tokens.background)
                    .shadow_sm()
                    .when(!disabled, |this| this.cursor(CursorStyle::PointingHand)),
            )
            .when(!disabled, |this| {
                let state_increment = state.clone();
                let on_seek_increment = on_seek.clone();
                let state_decrement = state.clone();
                let on_seek_decrement = on_seek.clone();
                let state_set_value = state.clone();
                let on_seek_set_value = on_seek.clone();
                let state_down = state.clone();
                let on_seek_down = on_seek.clone();
                let state_move = state.clone();
                let on_seek_move = on_seek.clone();
                let state_up = state.clone();

                this.on_accessibility_action(
                    AccessibilityAction::Increment,
                    move |request, window, cx| {
                        handle_seek_accessibility_action(
                            request,
                            &state_increment,
                            &on_seek_increment,
                            window,
                            cx,
                        );
                    },
                )
                .on_accessibility_action(
                    AccessibilityAction::Decrement,
                    move |request, window, cx| {
                        handle_seek_accessibility_action(
                            request,
                            &state_decrement,
                            &on_seek_decrement,
                            window,
                            cx,
                        );
                    },
                )
                .on_accessibility_action(
                    AccessibilityAction::SetValue,
                    move |request, window, cx| {
                        handle_seek_accessibility_action(
                            request,
                            &state_set_value,
                            &on_seek_set_value,
                            window,
                            cx,
                        );
                    },
                )
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(
                        &state_down,
                        move |state, e: &MouseDownEvent, window, cx| {
                            window.focus(&focus_on_mouse);
                            state.progress_dragging = true;
                            state.update_progress_from_position(e.position, cx);
                            if let Some(ref handler) = on_seek_down {
                                handler(state.current_time, window, cx);
                            }
                        },
                    ),
                )
                .on_mouse_move(window.listener_for(
                    &state_move,
                    move |state, e: &MouseMoveEvent, window, cx| {
                        if state.progress_dragging {
                            state.update_progress_from_position(e.position, cx);
                            if let Some(ref handler) = on_seek_move {
                                handler(state.current_time, window, cx);
                            }
                        }
                    },
                ))
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state_up, move |state, _: &MouseUpEvent, _, _| {
                        state.progress_dragging = false;
                    }),
                )
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let delta = match event.keystroke.key.as_str() {
                        "left" | "down" => Some(-5.0),
                        "right" | "up" => Some(5.0),
                        "home" => Some(f32::NEG_INFINITY),
                        "end" => Some(f32::INFINITY),
                        _ => None,
                    };
                    if let Some(delta) = delta {
                        state_for_key.update(cx, |state, cx| {
                            let next = if delta == f32::NEG_INFINITY {
                                0.0
                            } else if delta == f32::INFINITY {
                                state.duration
                            } else {
                                state.current_time + delta
                            };
                            state.set_current_time(next, cx);
                            if let Some(ref handler) = on_seek_key {
                                handler(state.current_time, window, cx);
                            }
                        });
                        cx.stop_propagation();
                    }
                })
            })
    }

    fn render_mute_button(
        &self,
        window: &mut Window,
        _theme: &crate::theme::Theme,
        icon_name: &str,
        is_muted: bool,
        button_size: Pixels,
        icon_size: Pixels,
    ) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let disabled = self.disabled;

        IconButton::new(icon_name)
            .id(ElementId::NamedChild(
                Box::new(self.id.clone()),
                "mute".into(),
            ))
            .label(if is_muted {
                "Unmute audio"
            } else {
                "Mute audio"
            })
            .variant(ButtonVariant::Ghost)
            .size(button_size)
            .icon_size(icon_size)
            .disabled(disabled)
            .on_click(window.listener_for(&state, move |state, _, _, cx| {
                state.toggle_muted(cx);
            }))
    }

    fn render_volume_slider(
        &self,
        window: &mut Window,
        theme: &crate::theme::Theme,
        volume: f32,
        focus_handle: FocusHandle,
        width: Pixels,
        track_height: Pixels,
        thumb_size: Pixels,
    ) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let on_volume_change = self.on_volume_change.clone();
        let disabled = self.disabled;

        let state_for_key = state.clone();
        let on_volume_key = on_volume_change.clone();
        let focus_on_mouse = focus_handle.clone();
        let mut accessibility =
            AccessibilityAttributes::slider("Volume", volume as f64, 0.0, 1.0, Some(0.05));
        if disabled {
            accessibility = accessibility.states(AccessibilityState::DISABLED);
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(self.id.clone()),
                "volume".into(),
            ))
            .accessibility(accessibility)
            .when(!disabled, |slider| {
                slider.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .w(width)
            .h(thumb_size)
            .flex()
            .items_center()
            .relative()
            .child(
                canvas_with_prepaint(
                    {
                        let state = state.clone();
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
                    .w_full()
                    .h(track_height)
                    .rounded_full()
                    .bg(theme.tokens.muted)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(volume))
                            .bg(theme.tokens.muted_foreground),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(volume))
                    .top(px(0.0))
                    .ml(-(thumb_size / 2.0))
                    .size(thumb_size)
                    .rounded_full()
                    .bg(theme.tokens.foreground)
                    .border_2()
                    .border_color(theme.tokens.background)
                    .shadow_sm()
                    .when(!disabled, |this| this.cursor(CursorStyle::PointingHand)),
            )
            .when(!disabled, |this| {
                let state_increment = state.clone();
                let on_volume_increment = on_volume_change.clone();
                let state_decrement = state.clone();
                let on_volume_decrement = on_volume_change.clone();
                let state_set_value = state.clone();
                let on_volume_set_value = on_volume_change.clone();
                let state_down = state.clone();
                let on_vol_down = on_volume_change.clone();
                let state_move = state.clone();
                let on_vol_move = on_volume_change.clone();
                let state_up = state.clone();

                this.on_accessibility_action(
                    AccessibilityAction::Increment,
                    move |request, window, cx| {
                        handle_volume_accessibility_action(
                            request,
                            &state_increment,
                            &on_volume_increment,
                            window,
                            cx,
                        );
                    },
                )
                .on_accessibility_action(
                    AccessibilityAction::Decrement,
                    move |request, window, cx| {
                        handle_volume_accessibility_action(
                            request,
                            &state_decrement,
                            &on_volume_decrement,
                            window,
                            cx,
                        );
                    },
                )
                .on_accessibility_action(
                    AccessibilityAction::SetValue,
                    move |request, window, cx| {
                        handle_volume_accessibility_action(
                            request,
                            &state_set_value,
                            &on_volume_set_value,
                            window,
                            cx,
                        );
                    },
                )
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(
                        &state_down,
                        move |state, e: &MouseDownEvent, window, cx| {
                            window.focus(&focus_on_mouse);
                            state.volume_dragging = true;
                            state.update_volume_from_position(e.position, cx);
                            if let Some(ref handler) = on_vol_down {
                                handler(state.volume, window, cx);
                            }
                        },
                    ),
                )
                .on_mouse_move(window.listener_for(
                    &state_move,
                    move |state, e: &MouseMoveEvent, window, cx| {
                        if state.volume_dragging {
                            state.update_volume_from_position(e.position, cx);
                            if let Some(ref handler) = on_vol_move {
                                handler(state.volume, window, cx);
                            }
                        }
                    },
                ))
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state_up, move |state, _: &MouseUpEvent, _, _| {
                        state.volume_dragging = false;
                    }),
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
                        state_for_key.update(cx, |state, cx| state.set_volume(next, cx));
                        if let Some(ref handler) = on_volume_key {
                            handler(next, window, cx);
                        }
                        cx.stop_propagation();
                    }
                })
            })
    }

    fn render_speed_button(
        &self,
        window: &mut Window,
        _theme: &crate::theme::Theme,
        speed: PlaybackSpeed,
    ) -> impl IntoElement + use<> {
        let state = self.state.clone();
        let on_speed_change = self.on_speed_change.clone();
        let disabled = self.disabled;

        Button::new(
            ElementId::NamedChild(Box::new(self.id.clone()), "speed".into()),
            speed.label(),
        )
        .variant(ButtonVariant::Ghost)
        .size(ButtonSize::Sm)
        .disabled(disabled)
        .tooltip("Change playback speed")
        .on_click(window.listener_for(&state, move |state, _, window, cx| {
            state.cycle_playback_speed(cx);
            if let Some(ref handler) = on_speed_change {
                handler(state.playback_speed, window, cx);
            }
        }))
    }
}
