//! ImageViewer/Lightbox component for displaying images in a fullscreen overlay.

use kael::{prelude::FluentBuilder as _, *};
use std::{rc::Rc, sync::Arc};

use crate::components::button::{Button, ButtonColors, ButtonSize};
use crate::theme::Theme;

actions!(
    image_viewer,
    [
        ImageViewerClose,
        ImageViewerNext,
        ImageViewerPrev,
        ImageViewerZoomIn,
        ImageViewerZoomOut,
        ImageViewerResetZoom
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LightboxMediaType {
    #[default]
    Image,
    Video,
}

pub type LightboxMedia = ImageItem;

#[derive(Clone)]
pub struct ImageItem {
    pub src: SharedString,
    pub alt: Option<SharedString>,
    pub caption: Option<SharedString>,
    pub media_type: LightboxMediaType,
    pub has_auto_play: bool,
}

impl ImageItem {
    pub fn new(src: impl Into<SharedString>) -> Self {
        Self {
            src: src.into(),
            alt: None,
            caption: None,
            media_type: LightboxMediaType::Image,
            has_auto_play: false,
        }
    }

    pub fn alt(mut self, alt: impl Into<SharedString>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    pub fn media_type(mut self, media_type: LightboxMediaType) -> Self {
        self.media_type = media_type;
        self
    }

    #[allow(non_snake_case)]
    pub fn mediaType(self, media_type: LightboxMediaType) -> Self {
        self.media_type(media_type)
    }

    pub fn image(self) -> Self {
        self.media_type(LightboxMediaType::Image)
    }

    pub fn video(self) -> Self {
        self.media_type(LightboxMediaType::Video)
    }

    pub fn has_auto_play(mut self, has_auto_play: bool) -> Self {
        self.has_auto_play = has_auto_play;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasAutoPlay(self, has_auto_play: bool) -> Self {
        self.has_auto_play(has_auto_play)
    }

    pub fn lightbox_image(src: impl Into<SharedString>, alt: impl Into<SharedString>) -> Self {
        Self::new(src).alt(alt).image()
    }

    pub fn lightbox_video(src: impl Into<SharedString>, alt: impl Into<SharedString>) -> Self {
        Self::new(src).alt(alt).video()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ImageViewerSize {
    Auto,
    #[default]
    Contain,
    Cover,
    Custom(f32),
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 5.0;
const ZOOM_STEP: f32 = 0.25;

pub struct ImageViewerState {
    images: Vec<ImageItem>,
    current_index: usize,
    zoom: f32,
    pan_offset: Point<Pixels>,
    _is_panning: bool,
    _last_mouse_pos: Point<Pixels>,
    _loading: bool,
    show_thumbnails: bool,
    _fit_mode: ImageViewerSize,
    has_zoom: bool,
    has_auto_play: bool,
    on_index_change: Option<Rc<dyn Fn(usize, &mut App)>>,
}

impl ImageViewerState {
    pub fn new(images: Vec<ImageItem>) -> Self {
        Self {
            images,
            current_index: 0,
            zoom: 1.0,
            pan_offset: point(px(0.0), px(0.0)),
            _is_panning: false,
            _last_mouse_pos: point(px(0.0), px(0.0)),
            _loading: false,
            show_thumbnails: true,
            _fit_mode: ImageViewerSize::default(),
            has_zoom: false,
            has_auto_play: false,
            on_index_change: None,
        }
    }

    pub fn set_images(&mut self, images: Vec<ImageItem>) {
        self.images = images;
        self.current_index = 0;
        self.reset_view();
    }

    pub fn go_to(&mut self, index: usize) {
        if index < self.images.len() {
            self.current_index = index;
            self.reset_view();
        }
    }

    pub fn go_to_with_notify(&mut self, index: usize, cx: &mut App) {
        if index < self.images.len() {
            self.current_index = index;
            self.reset_view();
            self.notify_index_change(cx);
        }
    }

    pub fn next(&mut self) {
        if self.current_index < self.images.len().saturating_sub(1) {
            self.current_index += 1;
            self.reset_view();
        }
    }

    pub fn next_with_notify(&mut self, cx: &mut App) {
        if self.current_index < self.images.len().saturating_sub(1) {
            self.current_index += 1;
            self.reset_view();
            self.notify_index_change(cx);
        }
    }

    pub fn prev(&mut self) {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.reset_view();
        }
    }

    pub fn prev_with_notify(&mut self, cx: &mut App) {
        if self.current_index > 0 {
            self.current_index -= 1;
            self.reset_view();
            self.notify_index_change(cx);
        }
    }

    pub fn zoom_in(&mut self) {
        if !self.has_zoom {
            return;
        }
        self.zoom = (self.zoom + ZOOM_STEP).min(MAX_ZOOM);
    }

    pub fn zoom_out(&mut self) {
        if !self.has_zoom {
            return;
        }
        self.zoom = (self.zoom - ZOOM_STEP).max(MIN_ZOOM);
    }

    pub fn reset_zoom(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = point(px(0.0), px(0.0));
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        if self.has_zoom {
            self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    pub fn toggle_thumbnails(&mut self) {
        self.show_thumbnails = !self.show_thumbnails;
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = point(px(0.0), px(0.0));
    }

    pub fn current_image(&self) -> Option<&ImageItem> {
        self.images.get(self.current_index)
    }

    pub fn has_next(&self) -> bool {
        self.current_index < self.images.len().saturating_sub(1)
    }

    pub fn has_prev(&self) -> bool {
        self.current_index > 0
    }

    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn is_zoomed(&self) -> bool {
        (self.zoom - 1.0).abs() > 0.01
    }

    pub fn has_zoom(&mut self, has_zoom: bool) {
        self.has_zoom = has_zoom;
        if !has_zoom {
            self.reset_zoom();
        }
    }

    pub fn has_auto_play(&mut self, has_auto_play: bool) {
        self.has_auto_play = has_auto_play;
    }

    pub fn on_index_change<F>(&mut self, handler: F)
    where
        F: Fn(usize, &mut App) + 'static,
    {
        self.on_index_change = Some(Rc::new(handler));
    }

    fn notify_index_change(&self, cx: &mut App) {
        if let Some(handler) = &self.on_index_change {
            handler(self.current_index, cx);
        }
    }
}

pub struct ImageViewer {
    focus_handle: FocusHandle,
    state: Entity<ImageViewerState>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    close_on_backdrop_click: bool,
    close_on_escape: bool,
    show_controls: bool,
    show_counter: bool,
    show_thumbnails: bool,
    is_open: bool,
    has_zoom: bool,
    has_auto_play: bool,
    style: StyleRefinement,
}

impl ImageViewer {
    pub fn new(state: Entity<ImageViewerState>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            state,
            on_close: None,
            close_on_backdrop_click: true,
            close_on_escape: true,
            show_controls: true,
            show_counter: true,
            show_thumbnails: true,
            is_open: true,
            has_zoom: false,
            has_auto_play: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    pub fn close_on_backdrop_click(mut self, close: bool) -> Self {
        self.close_on_backdrop_click = close;
        self
    }

    pub fn close_on_escape(mut self, close: bool) -> Self {
        self.close_on_escape = close;
        self
    }

    pub fn show_controls(mut self, show: bool) -> Self {
        self.show_controls = show;
        self
    }

    pub fn show_counter(mut self, show: bool) -> Self {
        self.show_counter = show;
        self
    }

    pub fn show_thumbnails(mut self, show: bool) -> Self {
        self.show_thumbnails = show;
        self
    }

    pub fn is_open(mut self, is_open: bool) -> Self {
        self.is_open = is_open;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOpen(self, is_open: bool) -> Self {
        self.is_open(is_open)
    }

    pub fn has_zoom(mut self, has_zoom: bool) -> Self {
        self.has_zoom = has_zoom;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasZoom(self, has_zoom: bool) -> Self {
        self.has_zoom(has_zoom)
    }

    pub fn has_auto_play(mut self, has_auto_play: bool) -> Self {
        self.has_auto_play = has_auto_play;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasAutoPlay(self, has_auto_play: bool) -> Self {
        self.has_auto_play(has_auto_play)
    }

    fn handle_close(&self, window: &mut Window, cx: &mut App) {
        if let Some(handler) = &self.on_close {
            (handler)(window, cx);
        }
    }
}

impl Styled for ImageViewer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Focusable for ImageViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<()> for ImageViewer {}

impl Render for ImageViewer {
    #[allow(refining_impl_trait)]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.is_open {
            return div().into_any_element();
        }
        cx.update_entity(&self.state, |state, _| {
            state.has_zoom(self.has_zoom);
            state.has_auto_play(self.has_auto_play);
        });
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let current_image = state.current_image().cloned();
        let current_index = state.current_index();
        let image_count = state.image_count();
        let zoom = state.zoom();
        let has_prev = state.has_prev();
        let has_next = state.has_next();
        let _pan_offset = state.pan_offset;
        let has_auto_play = self.has_auto_play;

        let viewer_entity = cx.entity().clone();
        let state_entity = self.state.clone();

        window.focus(&self.focus_handle);

        let close_on_escape = self.close_on_escape;
        let close_on_backdrop = self.close_on_backdrop_click;
        let on_dark_button = ButtonColors {
            background: kael::transparent_black(),
            foreground: kael::white(),
            border: kael::transparent_black(),
            hover_background: kael::white().opacity(0.12),
            hover_foreground: kael::white(),
            has_shadow: false,
            has_border: false,
        };

        div()
            .id("image-viewer-overlay")
            .key_context("ImageViewer")
            .track_focus(&self.focus_handle)
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(kael::black().opacity(0.78))
            .on_action({
                let viewer_entity = viewer_entity.clone();
                move |_: &ImageViewerClose, window, cx| {
                    if close_on_escape {
                        cx.update_entity(&viewer_entity, |viewer, cx| {
                            viewer.handle_close(window, cx);
                        });
                    }
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerNext, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| state.next_with_notify(cx));
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerPrev, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| state.prev_with_notify(cx));
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerZoomIn, _, cx| {
                    cx.update_entity(&state_entity, |state, _| state.zoom_in());
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerZoomOut, _, cx| {
                    cx.update_entity(&state_entity, |state, _| state.zoom_out());
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerResetZoom, _, cx| {
                    cx.update_entity(&state_entity, |state, _| state.reset_zoom());
                }
            })
            .when(self.show_counter && image_count > 1, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(12.0))
                        .left(px(12.0))
                        .text_size(px(14.0))
                        .line_height(px(20.0))
                        .text_color(kael::white())
                        .font_family(theme.tokens.font_family.clone())
                        .child(format!("{} / {}", current_index + 1, image_count)),
                )
            })
            .when(self.show_controls, |this| {
                let viewer_entity = viewer_entity.clone();
                this.child(
                    div().absolute().top(px(12.0)).right(px(12.0)).child(
                        Button::new("close-viewer", "")
                            .colors(on_dark_button)
                            .size(ButtonSize::Icon)
                            .icon("x")
                            .on_click(move |_, window, cx| {
                                cx.update_entity(&viewer_entity, |viewer, cx| {
                                    viewer.handle_close(window, cx);
                                });
                            }),
                    ),
                )
            })
            .child(
                div()
                    .id("image-viewer-content")
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .relative()
                    .overflow_hidden()
                    .when(close_on_backdrop, |this| {
                        let viewer_entity = viewer_entity.clone();
                        this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.update_entity(&viewer_entity, |viewer, cx| {
                                viewer.handle_close(window, cx);
                            });
                        })
                    })
                    .when(self.show_controls && has_prev, |this| {
                        let state_entity = state_entity.clone();
                        this.child(
                            div()
                                .id("prev-button")
                                .absolute()
                                .left(px(12.0))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .items_center()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    Button::new("prev-image", "")
                                        .colors(on_dark_button)
                                        .size(ButtonSize::Icon)
                                        .icon("chevron-left")
                                        .on_click(move |_, _, cx| {
                                            cx.update_entity(&state_entity, |state, cx| {
                                                state.prev_with_notify(cx)
                                            });
                                        }),
                                ),
                        )
                    })
                    .when(self.show_controls && has_next, |this| {
                        let state_entity = state_entity.clone();
                        this.child(
                            div()
                                .id("next-button")
                                .absolute()
                                .right(px(12.0))
                                .top_0()
                                .bottom_0()
                                .flex()
                                .items_center()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    Button::new("next-image", "")
                                        .colors(on_dark_button)
                                        .size(ButtonSize::Icon)
                                        .icon("chevron-right")
                                        .on_click(move |_, _, cx| {
                                            cx.update_entity(&state_entity, |state, cx| {
                                                state.next_with_notify(cx)
                                            });
                                        }),
                                ),
                        )
                    })
                    .when_some(current_image.clone(), |this, image| {
                        let media_element = match image.media_type {
                            LightboxMediaType::Image => img(image.src.clone())
                                .max_w(relative(1.0 * zoom))
                                .max_h(relative(1.0 * zoom))
                                .object_fit(ObjectFit::Contain)
                                .into_any_element(),
                            LightboxMediaType::Video => {
                                kael::video(Arc::<str>::from(image.src.as_ref()))
                                    .object_fit(ObjectFit::Contain)
                                    .when(image.has_auto_play || has_auto_play, |this| {
                                        this.autoplay()
                                    })
                                    .into_any_element()
                            }
                        };

                        this.child(
                            div()
                                .id("media-group")
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .max_w(relative(1.0))
                                .max_h(relative(1.0))
                                .overflow_hidden()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    div()
                                        .id("image-container")
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .overflow_hidden()
                                        .child(media_element),
                                )
                                .when_some(image.caption.clone(), |this, caption| {
                                    this.child(
                                        div()
                                            .id("image-caption")
                                            .pt(px(8.0))
                                            .px(px(12.0))
                                            .max_w(px(600.0))
                                            .text_size(px(18.0))
                                            .line_height(px(28.0))
                                            .text_color(kael::white())
                                            .font_family(theme.tokens.font_family.clone())
                                            .text_center()
                                            .child(caption),
                                    )
                                }),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub fn init_image_viewer(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", ImageViewerClose, Some("ImageViewer")),
        KeyBinding::new("left", ImageViewerPrev, Some("ImageViewer")),
        KeyBinding::new("right", ImageViewerNext, Some("ImageViewer")),
        KeyBinding::new("up", ImageViewerZoomIn, Some("ImageViewer")),
        KeyBinding::new("down", ImageViewerZoomOut, Some("ImageViewer")),
        KeyBinding::new("0", ImageViewerResetZoom, Some("ImageViewer")),
        KeyBinding::new("+", ImageViewerZoomIn, Some("ImageViewer")),
        KeyBinding::new("-", ImageViewerZoomOut, Some("ImageViewer")),
        KeyBinding::new("=", ImageViewerZoomIn, Some("ImageViewer")),
    ]);
}

pub fn init_lightbox(cx: &mut App) {
    init_image_viewer(cx);
}
