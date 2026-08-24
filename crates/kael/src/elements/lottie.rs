use super::img::LOADING_DELAY;
use crate::{
    AnyElement, App, AtlasKey, Bounds, DefiniteLength, DevicePixels, Element, ElementId,
    GlobalElementId, Hitbox, InspectorElementId, InteractiveElement, Interactivity, IntoElement,
    LayoutId, Length, LoopMode, LottieAnimation, LottieError, LottiePlayer, LottieRenderBatch,
    LottieSource, ObjectFit, Pixels, PlatformAtlas, RenderImage, RenderImageParams,
    StyleRefinement, Styled, Task, Window, px,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use util::ResultExt;
use web_time::Instant;

const DEFAULT_PREFETCH_FRAMES: usize = 5;

struct LottieStyle {
    object_fit: ObjectFit,
    loading: Option<Box<dyn Fn() -> AnyElement>>,
    fallback: Option<Box<dyn Fn() -> AnyElement>>,
}

impl Default for LottieStyle {
    fn default() -> Self {
        Self {
            object_fit: ObjectFit::Contain,
            loading: None,
            fallback: None,
        }
    }
}

#[derive(Default)]
struct LottieState {
    player: Option<LottiePlayer>,
    autoplay_started: bool,
    started_loading: Option<(Instant, Task<()>)>,
    pending_batch: Option<PendingFrameBatch>,
    frame_cache: LottieFrameCache,
}

struct PendingFrameBatch {
    render_size: crate::Size<DevicePixels>,
    requested_frames: Vec<usize>,
    result: Arc<Mutex<Option<Result<LottieRenderBatch, LottieError>>>>,
}

#[derive(Default)]
struct LottieFrameCache {
    atlas: Option<Arc<dyn PlatformAtlas>>,
    render_size: Option<crate::Size<DevicePixels>>,
    max_frames: usize,
    frames: VecDeque<CachedFrame>,
}

struct CachedFrame {
    frame_index: usize,
    image: Arc<RenderImage>,
}

/// Layout state retained while a Lottie element resolves fallback or loading content.
#[doc(hidden)]
pub struct LottieLayoutState {
    replacement: Option<AnyElement>,
}

/// A Lottie animation element.
pub struct Lottie {
    interactivity: Interactivity,
    source: LottieSource,
    style: LottieStyle,
    autoplay: bool,
    loop_mode: LoopMode,
    prefetch_frames: usize,
}

/// Create a Lottie animation element.
#[track_caller]
pub fn lottie(source: impl Into<LottieSource>) -> Lottie {
    Lottie {
        interactivity: Interactivity::new(),
        source: source.into(),
        style: LottieStyle::default(),
        autoplay: false,
        loop_mode: LoopMode::Once,
        prefetch_frames: DEFAULT_PREFETCH_FRAMES,
    }
}

impl Lottie {
    /// Start playback automatically once the animation has loaded.
    pub fn autoplay(mut self) -> Self {
        self.autoplay = true;
        self
    }

    /// Set the playback loop mode.
    pub fn loop_mode(mut self, loop_mode: LoopMode) -> Self {
        self.loop_mode = loop_mode;
        self
    }

    /// Restart from the first frame whenever the animation reaches the end.
    pub fn loop_forever(mut self) -> Self {
        self.loop_mode = LoopMode::Loop;
        self
    }

    /// Reverse direction at the ends of the timeline.
    pub fn ping_pong(mut self) -> Self {
        self.loop_mode = LoopMode::PingPong;
        self
    }

    /// Set how many frames should be prefetched ahead of the current frame.
    pub fn prefetch_frames(mut self, prefetch_frames: usize) -> Self {
        self.prefetch_frames = prefetch_frames.max(1);
        self
    }

    /// Set the object-fit policy used to place the rendered animation within its bounds.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.style.object_fit = object_fit;
        self
    }

    /// Render a replacement while the animation is still loading.
    pub fn with_loading(mut self, loading: impl Fn() -> AnyElement + 'static) -> Self {
        self.style.loading = Some(Box::new(loading));
        self
    }

    /// Render a replacement when the animation fails to load or decode.
    pub fn with_fallback(mut self, fallback: impl Fn() -> AnyElement + 'static) -> Self {
        self.style.fallback = Some(Box::new(fallback));
        self
    }

    /// Returns true when this element will start playback after load.
    pub fn is_autoplay(&self) -> bool {
        self.autoplay
    }

    /// Returns the configured loop policy.
    pub fn loop_policy(&self) -> LoopMode {
        self.loop_mode
    }

    /// Returns the number of frames this element prefetches ahead of playback.
    pub fn prefetch_frame_count(&self) -> usize {
        self.prefetch_frames
    }

    /// Returns true when loading replacement content is configured.
    pub fn has_loading(&self) -> bool {
        self.style.loading.is_some()
    }

    /// Returns true when fallback replacement content is configured.
    pub fn has_fallback(&self) -> bool {
        self.style.fallback.is_some()
    }

    /// Returns a content-safe summary of this Lottie element plan.
    pub fn to_text(&self) -> String {
        format!(
            "lottie element: {}, autoplay {}, loop {}, object_fit {}, prefetch_frames {}, loading {}, fallback {}",
            self.source.to_text(),
            self.autoplay,
            self.loop_mode.to_text(),
            object_fit_key(&self.style.object_fit),
            self.prefetch_frames,
            self.has_loading(),
            self.has_fallback()
        )
    }
}

impl Element for Lottie {
    type RequestLayoutState = LottieLayoutState;
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
        let mut layout_state = LottieLayoutState { replacement: None };

        window.with_optional_element_state(
            global_id,
            |state: Option<Option<LottieState>>, window| {
                let mut state = state.map(|state| state.unwrap_or_default());

                let layout_id = self.interactivity.request_layout(
                    global_id,
                    inspector_id,
                    window,
                    cx,
                    |mut style, window, cx| {
                        let mut replacement_id = None;

                        match self.source.use_animation(window, cx) {
                            Some(Ok(animation)) => {
                                if let Some(state) = &mut state {
                                    let animations_enabled = window.animations_enabled();
                                    state.sync_animation(
                                        animation.clone(),
                                        self.autoplay,
                                        animations_enabled,
                                        self.loop_mode,
                                        self.prefetch_frames,
                                        Instant::now(),
                                    );
                                    state.started_loading = None;
                                }

                                let animation_size = animation.size();
                                style.aspect_ratio =
                                    Some(animation_size.width / animation_size.height);

                                if let Length::Auto = style.size.width {
                                    style.size.width = match style.size.height {
                                        Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                                            let height_px =
                                                window.unscaled_ui_length_in_pixels(abs_length);
                                            Length::Definite(
                                                px(animation_size.width.0 * height_px.0
                                                    / animation_size.height.0)
                                                .into(),
                                            )
                                        }
                                        _ => Length::Definite(animation_size.width.into()),
                                    };
                                }

                                if let Length::Auto = style.size.height {
                                    style.size.height = match style.size.width {
                                        Length::Definite(DefiniteLength::Absolute(abs_length)) => {
                                            let width_px =
                                                window.unscaled_ui_length_in_pixels(abs_length);
                                            Length::Definite(
                                                px(animation_size.height.0 * width_px.0
                                                    / animation_size.width.0)
                                                .into(),
                                            )
                                        }
                                        _ => Length::Definite(animation_size.height.into()),
                                    };
                                }
                            }
                            Some(Err(_)) => {
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
                                    if let Some((started_loading, _)) =
                                        state.started_loading.as_ref()
                                    {
                                        if started_loading.elapsed() > LOADING_DELAY
                                            && let Some(loading) = self.style.loading.as_ref()
                                        {
                                            let mut element = loading();
                                            replacement_id =
                                                Some(element.request_layout(window, cx));
                                            layout_state.replacement = Some(element);
                                        }
                                    } else {
                                        let current_view = window.current_view();
                                        let task = window.spawn(cx, async move |cx| {
                                            cx.background_executor().timer(LOADING_DELAY).await;
                                            let _ = cx.update(|_, cx| {
                                                cx.notify(current_view);
                                            });
                                        });
                                        state.started_loading = Some((Instant::now(), task));
                                    }
                                }
                            }
                        }

                        window.request_layout(style, replacement_id, cx)
                    },
                );

                ((layout_id, layout_state), state)
            },
        )
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
            |style, window, cx| match source.get_animation(window, cx) {
                Some(Ok(animation)) => {
                    let natural_size = animation.native_pixel_size();
                    let draw_bounds = self.style.object_fit.get_bounds(bounds, natural_size);
                    if draw_bounds.size.width <= px(0.0) || draw_bounds.size.height <= px(0.0) {
                        return;
                    }

                    let render_size = scaled_render_size(draw_bounds.size, window.scale_factor());
                    let maybe_image = window.with_optional_element_state(
                        global_id,
                        |state: Option<Option<LottieState>>, window| {
                            let mut state = state.map(|state| state.unwrap_or_default());
                            let image = if let Some(state) = &mut state {
                                let animations_enabled = window.animations_enabled();
                                state.sync_animation(
                                    animation.clone(),
                                    self.autoplay,
                                    animations_enabled,
                                    self.loop_mode,
                                    self.prefetch_frames,
                                    Instant::now(),
                                );
                                state.frame_cache.bind_atlas(window.sprite_atlas().clone());
                                state.frame_cache.set_max_frames(self.prefetch_frames);
                                state.frame_cache.set_render_size(render_size);
                                state.poll_pending_batch();

                                let now = Instant::now();
                                let (current_frame, requested_frames, is_animating) = {
                                    let player =
                                        state.player.as_mut().expect("player initialized above");
                                    let current_frame = player.update(now);
                                    let requested_frames = if player.is_animating() {
                                        player.upcoming_frames(now, self.prefetch_frames)
                                    } else {
                                        vec![current_frame]
                                    };
                                    let is_animating = player.is_animating();
                                    (current_frame, requested_frames, is_animating)
                                };

                                state.schedule_missing_frames(
                                    animation.clone(),
                                    requested_frames,
                                    render_size,
                                    window,
                                    cx,
                                );

                                if is_animating {
                                    window.request_animation_frame();
                                }

                                state
                                    .frame_cache
                                    .get(current_frame)
                                    .unwrap_or_else(|| animation.poster_frame())
                            } else {
                                animation.poster_frame()
                            };

                            (image, state)
                        },
                    );

                    let corner_radii = window
                        .ui_corners_in_pixels(style.corner_radii)
                        .clamp_radii_for_quad_size(draw_bounds.size);
                    window
                        .paint_image(draw_bounds, corner_radii, maybe_image, 0, false)
                        .log_err();
                }
                Some(Err(_)) | None => {
                    if let Some(replacement) = &mut layout_state.replacement {
                        replacement.paint(window, cx);
                    }
                }
            },
        )
    }
}

impl IntoElement for Lottie {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Lottie {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Lottie {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl LottieState {
    fn sync_animation(
        &mut self,
        animation: Arc<LottieAnimation>,
        autoplay: bool,
        animations_enabled: bool,
        loop_mode: LoopMode,
        prefetch_frames: usize,
        now: Instant,
    ) {
        let needs_reset = self
            .player
            .as_ref()
            .is_none_or(|player| !Arc::ptr_eq(player.animation(), &animation));

        if needs_reset {
            self.player = Some(LottiePlayer::new(animation));
            self.autoplay_started = false;
            self.pending_batch = None;
            self.frame_cache.clear();
        }

        let player = self.player.as_mut().unwrap();
        player.set_loop_mode(loop_mode);
        self.frame_cache.set_max_frames(prefetch_frames);

        if !animations_enabled {
            player.stop();
            self.autoplay_started = false;
            return;
        }

        if autoplay && !self.autoplay_started {
            player.play_at(now);
            self.autoplay_started = true;
        }
    }

    fn poll_pending_batch(&mut self) {
        let ready = self.pending_batch.as_ref().and_then(|pending| {
            let mut guard = pending.result.lock().ok()?;
            let result = guard.take()?;
            Some((pending.render_size, result))
        });

        if let Some((render_size, result)) = ready {
            self.pending_batch = None;
            match result {
                Ok(batch) if batch.render_size == render_size => {
                    self.frame_cache.insert_batch(batch)
                }
                Ok(_) => {}
                Err(err) => {
                    log::error!("failed to render lottie frames: {err}");
                }
            }
        }
    }

    fn schedule_missing_frames(
        &mut self,
        animation: Arc<LottieAnimation>,
        requested_frames: Vec<usize>,
        render_size: crate::Size<DevicePixels>,
        window: &Window,
        cx: &App,
    ) {
        let mut missing_frames = requested_frames
            .into_iter()
            .filter(|frame_index| !self.frame_cache.contains(*frame_index))
            .collect::<Vec<_>>();

        if let Some(pending) = &self.pending_batch {
            if pending.render_size != render_size {
                self.pending_batch = None;
            } else {
                missing_frames
                    .retain(|frame_index| !pending.requested_frames.contains(frame_index));
            }
        }

        if missing_frames.is_empty() || self.pending_batch.is_some() {
            return;
        }

        let result = Arc::new(Mutex::new(None));
        let result_slot = result.clone();
        let requested_frames = missing_frames.clone();
        let current_view = window.current_view();
        window
            .spawn(cx, move |cx: &mut crate::AsyncWindowContext| {
                let animation = animation.clone();
                let mut async_cx = cx.clone();
                async move {
                    let render_task = async_cx.background_executor().spawn(async move {
                        animation.render_batch(render_size, &requested_frames)
                    });
                    let rendered = render_task.await;
                    if let Ok(mut guard) = result_slot.lock() {
                        *guard = Some(rendered);
                    }
                    let _ = async_cx.update(|_, cx: &mut App| {
                        cx.notify(current_view);
                    });
                }
            })
            .detach();

        self.pending_batch = Some(PendingFrameBatch {
            render_size,
            requested_frames: missing_frames,
            result,
        });
    }
}

impl LottieFrameCache {
    fn bind_atlas(&mut self, atlas: Arc<dyn PlatformAtlas>) {
        if self
            .atlas
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &atlas))
        {
            return;
        }

        self.clear();
        self.atlas = Some(atlas);
    }

    fn set_max_frames(&mut self, max_frames: usize) {
        self.max_frames = max_frames.max(1);
        self.trim();
    }

    fn set_render_size(&mut self, render_size: crate::Size<DevicePixels>) {
        if self.render_size != Some(render_size) {
            self.clear();
            self.render_size = Some(render_size);
        }
    }

    fn contains(&self, frame_index: usize) -> bool {
        self.frames
            .iter()
            .any(|frame| frame.frame_index == frame_index)
    }

    fn get(&mut self, frame_index: usize) -> Option<Arc<RenderImage>> {
        let position = self
            .frames
            .iter()
            .position(|frame| frame.frame_index == frame_index)?;
        let frame = self.frames.remove(position)?;
        let image = frame.image.clone();
        self.frames.push_back(frame);
        Some(image)
    }

    fn insert_batch(&mut self, batch: LottieRenderBatch) {
        self.set_render_size(batch.render_size);
        for frame in batch.frames {
            self.insert(frame.frame_index, frame.image);
        }
        self.trim();
    }

    fn clear(&mut self) {
        while let Some(frame) = self.frames.pop_front() {
            self.remove_image(&frame.image);
        }
    }

    fn insert(&mut self, frame_index: usize, image: Arc<RenderImage>) {
        if let Some(position) = self
            .frames
            .iter()
            .position(|frame| frame.frame_index == frame_index)
        {
            if let Some(frame) = self.frames.remove(position) {
                self.remove_image(&frame.image);
            }
        }

        self.frames.push_back(CachedFrame { frame_index, image });
    }

    fn trim(&mut self) {
        let max_frames = self.max_frames.max(1);
        while self.frames.len() > max_frames {
            if let Some(frame) = self.frames.pop_front() {
                self.remove_image(&frame.image);
            }
        }
    }

    fn remove_image(&self, image: &Arc<RenderImage>) {
        if let Some(atlas) = &self.atlas {
            atlas.remove(&AtlasKey::from(RenderImageParams {
                image_id: image.id,
                frame_index: 0,
            }));
        }
    }
}

impl Drop for LottieFrameCache {
    fn drop(&mut self) {
        self.clear();
    }
}

fn scaled_render_size(size: crate::Size<Pixels>, scale_factor: f32) -> crate::Size<DevicePixels> {
    crate::size(
        DevicePixels((size.width.0 * scale_factor).ceil().max(1.0) as i32),
        DevicePixels((size.height.0 * scale_factor).ceil().max(1.0) as i32),
    )
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

#[cfg(test)]
mod tests {
    use super::{LottieState, lottie};
    use crate::{IntoElement, LoopMode, LottieAnimation, ObjectFit, ParentElement, div};
    use std::{sync::Arc, time::Instant};

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
    fn sync_animation_stops_when_animations_are_disabled() {
        let animation = Arc::new(LottieAnimation::from_json_str(SIMPLE_LOTTIE).unwrap());
        let mut state = LottieState::default();
        let start = Instant::now();

        state.sync_animation(animation.clone(), true, true, LoopMode::Loop, 3, start);
        assert!(
            state
                .player
                .as_ref()
                .is_some_and(|player| player.is_animating())
        );

        state.sync_animation(animation, true, false, LoopMode::Loop, 3, start);
        assert!(
            state
                .player
                .as_ref()
                .is_some_and(|player| !player.is_animating())
        );
        assert!(!state.autoplay_started);
    }

    #[test]
    fn lottie_element_summary_is_content_safe() {
        let element = lottie("https://cdn.example.com/private/spinner.json")
            .autoplay()
            .ping_pong()
            .object_fit(ObjectFit::Cover)
            .prefetch_frames(8)
            .with_loading(|| div().child("Loading private spinner").into_any_element())
            .with_fallback(|| div().child("Broken private spinner").into_any_element());

        assert!(element.is_autoplay());
        assert_eq!(element.loop_policy(), LoopMode::PingPong);
        assert_eq!(element.prefetch_frame_count(), 8);
        assert!(element.has_loading());
        assert!(element.has_fallback());
        assert_eq!(
            element.to_text(),
            "lottie element: lottie source: kind uri, bytes none, decoded false, autoplay true, loop ping-pong, object_fit cover, prefetch_frames 8, loading true, fallback true"
        );
        assert!(!element.to_text().contains("cdn.example.com"));
        assert!(!element.to_text().contains("private"));
        assert!(!element.to_text().contains("Loading"));
        assert!(!element.to_text().contains("Broken"));
    }
}
