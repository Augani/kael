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
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use util::ResultExt;
use web_time::Instant;

const DEFAULT_PREFETCH_FRAMES: usize = 5;

// Limit element-owned CPU work across windows, independently of the number of
// elements or retired requests. Each batch admits at most 256 MiB of output;
// prepared assets, renderer scratch, ready results and frame caches are extra.
// Two workers deliberately trade some throughput for bounded concurrent work.
static RENDER_SLOTS: LazyLock<Arc<async_lock::Semaphore>> =
    LazyLock::new(|| Arc::new(async_lock::Semaphore::new(2)));

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

struct LottieState {
    player: Option<LottiePlayer>,
    autoplay_started: bool,
    started_loading: Option<(Instant, Task<()>)>,
    pending_batch: Option<PendingFrameBatch>,
    frame_cache: LottieFrameCache,
    render_lane: Arc<async_lock::Semaphore>,
}

impl Default for LottieState {
    fn default() -> Self {
        Self {
            player: None,
            autoplay_started: false,
            started_loading: None,
            pending_batch: None,
            frame_cache: LottieFrameCache::default(),
            render_lane: Arc::new(async_lock::Semaphore::new(1)),
        }
    }
}

struct PendingFrameBatch {
    render_size: crate::Size<DevicePixels>,
    requested_frames: Vec<usize>,
    result: Arc<Mutex<Option<Result<LottieRenderBatch, LottieError>>>>,
    cancelled: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for PendingFrameBatch {
    fn drop(&mut self) {
        // Task cancellation alone cannot preempt a background poll already
        // rendering synchronously. Signal that poll before dropping its owner.
        self.cancelled.store(true, Ordering::Release);
    }
}

impl PendingFrameBatch {
    fn spawn(
        animation: Arc<LottieAnimation>,
        render_size: crate::Size<DevicePixels>,
        requested_frames: Vec<usize>,
        render_lane: Arc<async_lock::Semaphore>,
        render_slots: Arc<async_lock::Semaphore>,
        cx: &App,
        on_ready: impl FnOnce(&mut App) + 'static,
    ) -> Self {
        Self::spawn_with_renderer(
            animation,
            render_size,
            requested_frames,
            render_lane,
            render_slots,
            cx,
            on_ready,
            |animation, size, frames, cancelled| {
                animation.render_batch_cancellable(size, frames, cancelled)
            },
        )
    }

    fn spawn_with_renderer(
        animation: Arc<LottieAnimation>,
        render_size: crate::Size<DevicePixels>,
        requested_frames: Vec<usize>,
        render_lane: Arc<async_lock::Semaphore>,
        render_slots: Arc<async_lock::Semaphore>,
        cx: &App,
        on_ready: impl FnOnce(&mut App) + 'static,
        render: impl FnOnce(
            &LottieAnimation,
            crate::Size<DevicePixels>,
            &[usize],
            &AtomicBool,
        ) -> Result<Option<LottieRenderBatch>, LottieError>
        + Send
        + 'static,
    ) -> Self {
        let result = Arc::new(Mutex::new(None));
        let result_slot = Arc::downgrade(&result);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let task_cancelled = cancelled.clone();
        let frames = requested_frames.clone();
        let task = cx.spawn(async move |cx| {
            let rendered = cx
                .background_executor()
                .spawn(async move {
                    // Keep both permits inside the worker, including while its
                    // synchronous frame is finishing after cancellation. A new
                    // size/source waits without blocking the UI thread.
                    let _lane = render_lane.acquire_arc().await;
                    if worker_cancelled.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let _slot = render_slots.acquire_arc().await;
                    render(&animation, render_size, &frames, &worker_cancelled)
                })
                .await;
            if task_cancelled.load(Ordering::Acquire) {
                return;
            }
            let rendered = match rendered {
                Ok(Some(batch)) => Ok(batch),
                Ok(None) => return,
                Err(error) => Err(error),
            };
            // The worker never owns the output slot. Retiring the element or
            // request immediately drops any ready batch and prevents publishing.
            if let Some(result_slot) = result_slot.upgrade()
                && let Ok(mut guard) = result_slot.lock()
            {
                *guard = Some(rendered);
                drop(guard);
                let _ = cx.update(on_ready);
            }
        });
        Self {
            render_size,
            requested_frames,
            result,
            cancelled,
            _task: task,
        }
    }
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

    /// Set the upper bound on frames prefetched ahead of the current frame.
    /// Large rendered frames reduce the effective prefetch and cache count to
    /// fit the per-batch byte budget.
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

    /// Returns the configured prefetch upper bound ahead of playback.
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
                                    state.retire_animation();
                                    state.started_loading = None;
                                }
                            }
                            None => {
                                if let Some(state) = &mut state {
                                    state.retire_animation();
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
                        window.with_optional_element_state(
                            global_id,
                            |state: Option<Option<LottieState>>, _| {
                                let mut state = state.flatten();
                                if let Some(state) = &mut state {
                                    state.pending_batch = None;
                                    state.frame_cache.clear();
                                }
                                ((), state)
                            },
                        );
                        return;
                    }

                    let render_size = scaled_render_size(draw_bounds.size, window.scale_factor());
                    let maybe_image = window.with_optional_element_state(
                        global_id,
                        |state: Option<Option<LottieState>>, window| {
                            let mut state = state.map(|state| state.unwrap_or_default());
                            let image = if let Some(state) = &mut state {
                                let admitted_frames =
                                    animation.max_admitted_frame_count(render_size).unwrap_or(0);
                                let prefetch_frames = state.admitted_prefetch_frames(
                                    self.prefetch_frames,
                                    admitted_frames,
                                );
                                let animations_enabled = window.animations_enabled();
                                state.sync_animation(
                                    animation.clone(),
                                    self.autoplay,
                                    animations_enabled,
                                    self.loop_mode,
                                    prefetch_frames,
                                    Instant::now(),
                                );
                                state.frame_cache.bind_atlas(window.sprite_atlas().clone());
                                state.frame_cache.set_max_frames(prefetch_frames);
                                state.set_render_size(render_size);
                                state.poll_pending_batch();

                                let now = Instant::now();
                                let (current_frame, requested_frames, should_animate) =
                                    state.plan_frames(now, prefetch_frames);

                                state.schedule_missing_frames(
                                    animation.clone(),
                                    requested_frames,
                                    render_size,
                                    window,
                                    cx,
                                );

                                if should_animate {
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
    fn retire_animation(&mut self) {
        self.pending_batch = None;
        self.player = None;
        self.autoplay_started = false;
        self.frame_cache.clear();
    }

    fn admitted_prefetch_frames(&mut self, configured: usize, admitted: usize) -> usize {
        let effective = configured.max(1).min(admitted);
        self.frame_cache.set_max_frames(effective);
        effective
    }

    fn plan_frames(&mut self, now: Instant, prefetch_frames: usize) -> (usize, Vec<usize>, bool) {
        let player = self.player.as_mut().expect("player initialized above");
        let current_frame = player.update(now);
        let is_animating = player.is_animating();
        let requested_frames = if prefetch_frames == 0 {
            Vec::new()
        } else if is_animating {
            player.upcoming_frames(now, prefetch_frames)
        } else {
            vec![current_frame]
        };
        (
            current_frame,
            requested_frames,
            is_animating && prefetch_frames > 0,
        )
    }

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

    fn set_render_size(&mut self, render_size: crate::Size<DevicePixels>) {
        if self
            .pending_batch
            .as_ref()
            .is_some_and(|pending| pending.render_size != render_size)
        {
            self.pending_batch = None;
        }
        self.frame_cache.set_render_size(render_size);
    }

    fn poll_pending_batch(&mut self) {
        // Also reject a ready stale batch if a caller already updated the cache
        // geometry. The cache must never be reset to an obsolete request size.
        if self
            .pending_batch
            .as_ref()
            .is_some_and(|pending| self.frame_cache.render_size != Some(pending.render_size))
        {
            self.pending_batch = None;
            return;
        }
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

        let current_view = window.current_view();
        self.pending_batch = Some(PendingFrameBatch::spawn(
            animation,
            render_size,
            missing_frames,
            self.render_lane.clone(),
            RENDER_SLOTS.clone(),
            cx,
            move |cx| cx.notify(current_view),
        ));
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
    use super::{LottieState, PendingFrameBatch, lottie};
    use crate::{
        DevicePixels, IntoElement, LoopMode, LottieAnimation, ObjectFit, ParentElement,
        TestAppContext, div, size,
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };
    use web_time::Instant;

    const SIMPLE_LOTTIE: &str = r#"{
        "v":"5.7.6",
        "fr":30,
        "ip":0,
        "op":30,
        "w":64,
        "h":32,
        "layers":[]
    }"#;

    fn tiny_animation() -> Arc<LottieAnimation> {
        Arc::new(
            LottieAnimation::from_json_str(r#"{"fr":30,"ip":0,"op":30,"w":1,"h":1,"layers":[]}"#)
                .unwrap(),
        )
    }

    #[test]
    fn dropped_element_cancels_queued_work_and_releases_source() {
        let cx = TestAppContext::single();
        let slots = Arc::new(async_lock::Semaphore::new(1));
        let blocker = slots.try_acquire_arc().unwrap();
        let animation = tiny_animation();
        let source = Arc::downgrade(&animation);
        let notified = Arc::new(AtomicBool::new(false));
        let mut state = LottieState::default();
        let ready = notified.clone();
        state.pending_batch = Some(cx.update(|cx| {
            PendingFrameBatch::spawn(
                animation,
                size(DevicePixels(1), DevicePixels(1)),
                vec![0, 1],
                state.render_lane.clone(),
                slots.clone(),
                cx,
                move |_| ready.store(true, Ordering::SeqCst),
            )
        }));
        let result = Arc::downgrade(&state.pending_batch.as_ref().unwrap().result);
        cx.executor().run_until_parked();
        assert!(source.upgrade().is_some());
        assert!(!notified.load(Ordering::SeqCst));
        drop(state);
        assert!(result.upgrade().is_none());
        cx.executor().run_until_parked();
        assert!(source.upgrade().is_none());
        drop(blocker);
        cx.executor().run_until_parked();
        assert!(!notified.load(Ordering::SeqCst));
        assert!(slots.try_acquire_arc().is_some());
    }

    #[test]
    fn source_and_size_replacements_retire_work_before_current_geometry_is_polled() {
        let cx = TestAppContext::single();
        let slots = Arc::new(async_lock::Semaphore::new(1));
        let blocker = slots.try_acquire_arc().unwrap();
        let mut state = LottieState::default();
        let lane = state.render_lane.clone();
        let old = tiny_animation();
        let old_source = Arc::downgrade(&old);
        let old_size = size(DevicePixels(1), DevicePixels(1));
        let new_size = size(DevicePixels(2), DevicePixels(2));
        state.sync_animation(old.clone(), true, true, LoopMode::Loop, 2, Instant::now());
        state.set_render_size(old_size);
        state.pending_batch = Some(cx.update(|cx| {
            PendingFrameBatch::spawn(
                old,
                old_size,
                vec![0, 1],
                lane.clone(),
                slots.clone(),
                cx,
                |_| {},
            )
        }));
        cx.executor().run_until_parked();
        let old_cancelled = state.pending_batch.as_ref().unwrap().cancelled.clone();
        let current = tiny_animation();
        state.sync_animation(
            current.clone(),
            true,
            true,
            LoopMode::Loop,
            2,
            Instant::now(),
        );
        assert!(old_cancelled.load(Ordering::Acquire));
        assert!(Arc::ptr_eq(&lane, &state.render_lane));
        cx.executor().run_until_parked();
        assert!(old_source.upgrade().is_none());
        state.pending_batch = Some(cx.update(|cx| {
            PendingFrameBatch::spawn(
                current.clone(),
                old_size,
                vec![0, 1],
                lane.clone(),
                slots.clone(),
                cx,
                |_| {},
            )
        }));
        cx.executor().run_until_parked();
        let old_cancelled = state.pending_batch.as_ref().unwrap().cancelled.clone();
        state.set_render_size(new_size);
        assert!(old_cancelled.load(Ordering::Acquire));
        assert!(state.pending_batch.is_none());
        cx.executor().run_until_parked();
        let notified = Arc::new(AtomicUsize::new(0));
        let ready = notified.clone();
        state.pending_batch = Some(cx.update(|cx| {
            PendingFrameBatch::spawn(current, new_size, vec![2, 3], lane, slots, cx, move |_| {
                ready.fetch_add(1, Ordering::SeqCst);
            })
        }));
        drop(blocker);
        cx.executor().run_until_parked();
        assert_eq!(notified.load(Ordering::SeqCst), 1);
        state.poll_pending_batch();
        assert_eq!(state.frame_cache.render_size, Some(new_size));
        assert_eq!(state.frame_cache.get(2).unwrap().size(0), new_size);
        assert_eq!(state.frame_cache.get(3).unwrap().size(0), new_size);
    }

    #[test]
    fn shared_slots_bound_workers_without_blocking_foreground_or_starving_waiters() {
        let cx = TestAppContext::single();
        let slots = Arc::new(async_lock::Semaphore::new(2));
        let first = slots.try_acquire_arc().unwrap();
        let second = slots.try_acquire_arc().unwrap();
        assert!(slots.try_acquire_arc().is_none());
        let notified = Arc::new(AtomicUsize::new(0));
        let mut pending = (0..3)
            .map(|frame| {
                let ready = notified.clone();
                cx.update(|cx| {
                    PendingFrameBatch::spawn(
                        tiny_animation(),
                        size(DevicePixels(1), DevicePixels(1)),
                        vec![frame],
                        Arc::new(async_lock::Semaphore::new(1)),
                        slots.clone(),
                        cx,
                        move |_| {
                            ready.fetch_add(1, Ordering::SeqCst);
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        let foreground_progress = Arc::new(AtomicBool::new(false));
        let progress = foreground_progress.clone();
        let ui = cx.foreground_executor().spawn(async move {
            progress.store(true, Ordering::SeqCst);
        });
        cx.executor().run_until_parked();
        assert!(foreground_progress.load(Ordering::SeqCst));
        assert_eq!(notified.load(Ordering::SeqCst), 0);
        assert!(
            pending
                .iter()
                .all(|work| work.result.lock().unwrap().is_none())
        );
        drop(pending.remove(0));
        cx.executor().run_until_parked();
        // One released slot must service both remaining requests, even though
        // the other is still occupied. A ready result does not hoard admission.
        drop(first);
        cx.executor().run_until_parked();
        assert_eq!(notified.load(Ordering::SeqCst), 2);
        for work in &pending {
            let batch = work.result.lock().unwrap().take().unwrap().unwrap();
            assert_eq!(batch.frames.len(), 1);
            assert_eq!(
                batch.frames[0].image.size(0),
                size(DevicePixels(1), DevicePixels(1))
            );
        }
        drop((second, ui));
        assert!(slots.try_acquire_arc().is_some());
    }

    #[test]
    fn running_retired_worker_holds_admission_until_its_current_frame_returns() {
        let cx = TestAppContext::single();
        let executor = cx.executor();
        let slots = Arc::new(async_lock::Semaphore::new(1));
        let lane = Arc::new(async_lock::Semaphore::new(1));
        let animation = tiny_animation();
        let source = Arc::downgrade(&animation);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let notified = Arc::new(AtomicBool::new(false));
        let ready = notified.clone();
        let pending = cx.update(|cx| {
            PendingFrameBatch::spawn_with_renderer(
                animation,
                size(DevicePixels(1), DevicePixels(1)),
                vec![0, 1, 2],
                lane.clone(),
                slots.clone(),
                cx,
                move |_| ready.store(true, Ordering::SeqCst),
                move |animation, size, frames, cancelled| {
                    animation.render_batch_with_control(size, frames, 12, |completed| {
                        if completed == 1 {
                            started_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                        !cancelled.load(Ordering::Acquire)
                    })
                },
            )
        });
        let output = Arc::downgrade(&pending.result);
        assert!(executor.tick()); // Foreground owner starts the background task.
        let worker_executor = executor.clone();
        let worker = std::thread::spawn(move || assert!(worker_executor.tick()));
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(slots.try_acquire_arc().is_none());
        assert!(lane.try_acquire_arc().is_none());
        drop(pending);
        // Run the task cancellation cleanup while its synchronous frame is
        // genuinely inside a background poll. Neither permit may be released.
        executor.run_until_parked();
        assert!(output.upgrade().is_none());
        assert!(source.upgrade().is_some());
        assert!(slots.try_acquire_arc().is_none());
        assert!(lane.try_acquire_arc().is_none());
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        executor.run_until_parked();
        assert!(source.upgrade().is_none());
        assert!(!notified.load(Ordering::SeqCst));
        assert!(slots.try_acquire_arc().is_some());
        assert!(lane.try_acquire_arc().is_some());
    }

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
    fn admitted_prefetch_clips_real_plans_and_cache_while_frames_advance() {
        let animation = Arc::new(
            LottieAnimation::from_json_str(r#"{"fr":30,"ip":0,"op":30,"w":1,"h":1,"layers":[]}"#)
                .unwrap(),
        );
        let render_size = size(DevicePixels(1), DevicePixels(1));
        let admitted = animation
            .max_admitted_frame_count_with_budget(render_size, 8)
            .unwrap();
        assert_eq!(admitted, 2);
        let start = Instant::now();
        let mut state = LottieState::default();
        state.sync_animation(animation.clone(), true, true, LoopMode::Loop, 1000, start);
        for (elapsed_ms, expected) in [(0, 0), (40, 1), (980, 29), (1040, 1)] {
            let effective = state.admitted_prefetch_frames(1000, admitted);
            let (current, requested, animating) =
                state.plan_frames(start + Duration::from_millis(elapsed_ms), effective);
            assert!(animating);
            assert_eq!(current, expected);
            assert_eq!(requested.len(), 2);
            assert_eq!(requested[0], current);
            let batch = animation
                .render_batch_with_budget(render_size, &requested, 8)
                .unwrap();
            state.frame_cache.insert_batch(batch);
            assert_eq!(state.frame_cache.max_frames, 2);
            assert!(state.frame_cache.frames.len() <= 2);
            assert!(state.frame_cache.get(current).is_some());
        }
        let effective = state.admitted_prefetch_frames(1000, 0);
        let (_, requested, should_animate) =
            state.plan_frames(start + Duration::from_millis(1080), effective);
        assert!(requested.is_empty());
        assert!(!should_animate);
        assert!(state.player.as_ref().unwrap().is_animating());
        let effective = state.admitted_prefetch_frames(1000, admitted);
        let (current, requested, should_animate) =
            state.plan_frames(start + Duration::from_millis(1120), effective);
        assert_eq!(current, 3);
        assert_eq!(requested[0], current);
        assert_eq!(requested.len(), 2);
        assert!(should_animate);
    }

    #[test]
    fn ready_batch_for_previous_geometry_is_discarded() {
        let animation =
            LottieAnimation::from_json_str(r#"{"fr":30,"ip":0,"op":30,"w":1,"h":1,"layers":[]}"#)
                .unwrap();
        let old_size = size(DevicePixels(1), DevicePixels(1));
        let current_size = size(DevicePixels(2), DevicePixels(2));
        let batch = animation.render_batch(old_size, &[0, 1]).unwrap();
        let old_image = Arc::downgrade(&batch.frames[0].image);
        let mut state = LottieState::default();
        state.frame_cache.set_render_size(current_size);
        state.pending_batch = Some(super::PendingFrameBatch {
            render_size: old_size,
            requested_frames: vec![0, 1],
            result: Arc::new(std::sync::Mutex::new(Some(Ok(batch)))),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _task: crate::Task::ready(()),
        });
        state.poll_pending_batch();
        assert_eq!(state.frame_cache.render_size, Some(current_size));
        assert!(state.frame_cache.frames.is_empty());
        assert!(old_image.upgrade().is_none());
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
