//! ImageViewer/Lightbox component for displaying images in a fullscreen overlay.

use kael::{prelude::FluentBuilder as _, *};
use std::{rc::Rc, sync::Arc};

use crate::{
    components::{
        button::ButtonVariant,
        icon_button::IconButton,
        spinner::{Spinner, SpinnerSize},
        thumbnail::Thumbnail,
    },
    theme::Theme,
};

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
    is_panning: bool,
    last_mouse_pos: Point<Pixels>,
    loading: bool,
    show_thumbnails: bool,
    fit_mode: ImageViewerSize,
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
            is_panning: false,
            last_mouse_pos: point(px(0.0), px(0.0)),
            loading: false,
            show_thumbnails: true,
            fit_mode: ImageViewerSize::default(),
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
            self.zoom = if zoom.is_finite() {
                zoom.clamp(MIN_ZOOM, MAX_ZOOM)
            } else {
                1.0
            };
            if !self.is_zoomed() {
                self.pan_offset = point(px(0.0), px(0.0));
                self.is_panning = false;
            }
        }
    }

    pub fn fit_mode(&self) -> ImageViewerSize {
        self.fit_mode
    }

    pub fn set_fit_mode(&mut self, fit_mode: ImageViewerSize) {
        self.fit_mode = match fit_mode {
            ImageViewerSize::Custom(scale) if !scale.is_finite() || scale <= 0.0 => {
                ImageViewerSize::Contain
            }
            fit_mode => fit_mode,
        };
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub fn begin_pan(&mut self, position: Point<Pixels>) {
        if self.has_zoom && self.is_zoomed() {
            self.is_panning = true;
            self.last_mouse_pos = position;
        }
    }

    pub fn update_pan(&mut self, position: Point<Pixels>) -> bool {
        if !self.is_panning {
            return false;
        }
        let delta = position - self.last_mouse_pos;
        self.pan_offset.x += delta.x;
        self.pan_offset.y += delta.y;
        self.last_mouse_pos = position;
        true
    }

    pub fn end_pan(&mut self) {
        self.is_panning = false;
    }

    pub fn pan_offset(&self) -> Point<Pixels> {
        self.pan_offset
    }

    pub fn is_panning(&self) -> bool {
        self.is_panning
    }

    pub fn toggle_thumbnails(&mut self) {
        self.show_thumbnails = !self.show_thumbnails;
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan_offset = point(px(0.0), px(0.0));
        self.is_panning = false;
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
    fit_mode: ImageViewerSize,
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
            fit_mode: ImageViewerSize::Contain,
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

    pub fn fit_mode(mut self, fit_mode: ImageViewerSize) -> Self {
        self.fit_mode = fit_mode;
        self
    }

    pub fn size(self, fit_mode: ImageViewerSize) -> Self {
        self.fit_mode(fit_mode)
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
            state.set_fit_mode(self.fit_mode);
        });
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let current_image = state.current_image().cloned();
        let current_index = state.current_index();
        let image_count = state.image_count();
        let zoom = state.zoom();
        let is_zoomed = state.is_zoomed();
        let has_prev = state.has_prev();
        let has_next = state.has_next();
        let pan_offset = state.pan_offset();
        let is_panning = state.is_panning();
        let fit_mode = state.fit_mode();
        let is_loading = state.is_loading();
        let show_thumbnails = self.show_thumbnails && state.show_thumbnails;
        let images = state.images.clone();
        let has_auto_play = self.has_auto_play;

        let viewer_entity = cx.entity().clone();
        let state_entity = self.state.clone();
        let viewer_id: ElementId = ("image-viewer", viewer_entity.entity_id()).into();

        if !self.focus_handle.contains_focused(window, cx) {
            window.focus(&self.focus_handle);
        }

        let close_on_escape = self.close_on_escape;
        let close_on_backdrop = self.close_on_backdrop_click;
        div()
            .id(viewer_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Dialog).label("Image viewer"),
            )
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
                    cx.update_entity(&state_entity, |state, cx| {
                        state.next_with_notify(cx);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerPrev, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| {
                        state.prev_with_notify(cx);
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerZoomIn, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| {
                        state.zoom_in();
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerZoomOut, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| {
                        state.zoom_out();
                        cx.notify();
                    });
                }
            })
            .on_action({
                let state_entity = state_entity.clone();
                move |_: &ImageViewerResetZoom, _, cx| {
                    cx.update_entity(&state_entity, |state, cx| {
                        state.reset_zoom();
                        cx.notify();
                    });
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
            .when(is_loading, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new().size(SpinnerSize::Lg).color(kael::white())),
                )
            })
            .when(self.show_controls, |this| {
                let viewer_entity = viewer_entity.clone();
                let zoom_out_state = state_entity.clone();
                let reset_zoom_state = state_entity.clone();
                let zoom_in_state = state_entity.clone();
                this.child(
                    div()
                        .absolute()
                        .top(px(12.0))
                        .right(px(12.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .when(self.has_zoom, |this| {
                            this.child(
                                IconButton::new("minus")
                                    .id(ElementId::NamedChild(
                                        Box::new(viewer_id.clone()),
                                        "zoom-out".into(),
                                    ))
                                    .label("Zoom out")
                                    .variant(ButtonVariant::Ghost)
                                    .no_background(true)
                                    .text_color(kael::white())
                                    .disabled(zoom <= MIN_ZOOM + f32::EPSILON)
                                    .on_click(move |_, _, cx| {
                                        cx.update_entity(&zoom_out_state, |state, cx| {
                                            state.zoom_out();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                IconButton::new("rotate-ccw")
                                    .id(ElementId::NamedChild(
                                        Box::new(viewer_id.clone()),
                                        "reset-zoom".into(),
                                    ))
                                    .label("Reset zoom")
                                    .variant(ButtonVariant::Ghost)
                                    .no_background(true)
                                    .text_color(kael::white())
                                    .disabled(!is_zoomed)
                                    .on_click(move |_, _, cx| {
                                        cx.update_entity(&reset_zoom_state, |state, cx| {
                                            state.reset_zoom();
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(48.0))
                                    .text_center()
                                    .text_size(px(12.0))
                                    .text_color(kael::white())
                                    .child(format!("{:.0}%", zoom * 100.0)),
                            )
                            .child(
                                IconButton::new("plus")
                                    .id(ElementId::NamedChild(
                                        Box::new(viewer_id.clone()),
                                        "zoom-in".into(),
                                    ))
                                    .label("Zoom in")
                                    .variant(ButtonVariant::Ghost)
                                    .no_background(true)
                                    .text_color(kael::white())
                                    .disabled(zoom >= MAX_ZOOM - f32::EPSILON)
                                    .on_click(move |_, _, cx| {
                                        cx.update_entity(&zoom_in_state, |state, cx| {
                                            state.zoom_in();
                                            cx.notify();
                                        });
                                    }),
                            )
                        })
                        .child(
                            IconButton::new("x")
                                .id(ElementId::NamedChild(
                                    Box::new(viewer_id.clone()),
                                    "close".into(),
                                ))
                                .label("Close image viewer")
                                .variant(ButtonVariant::Ghost)
                                .no_background(true)
                                .text_color(kael::white())
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
                    .id(ElementId::NamedChild(
                        Box::new(viewer_id.clone()),
                        "content".into(),
                    ))
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
                                .id(ElementId::NamedChild(
                                    Box::new(viewer_id.clone()),
                                    "previous-region".into(),
                                ))
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
                                    IconButton::new("chevron-left")
                                        .id(ElementId::NamedChild(
                                            Box::new(viewer_id.clone()),
                                            "previous".into(),
                                        ))
                                        .label("Previous image")
                                        .variant(ButtonVariant::Ghost)
                                        .no_background(true)
                                        .text_color(kael::white())
                                        .on_click(move |_, _, cx| {
                                            cx.update_entity(&state_entity, |state, cx| {
                                                state.prev_with_notify(cx);
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                    })
                    .when(self.show_controls && has_next, |this| {
                        let state_entity = state_entity.clone();
                        this.child(
                            div()
                                .id(ElementId::NamedChild(
                                    Box::new(viewer_id.clone()),
                                    "next-region".into(),
                                ))
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
                                    IconButton::new("chevron-right")
                                        .id(ElementId::NamedChild(
                                            Box::new(viewer_id.clone()),
                                            "next".into(),
                                        ))
                                        .label("Next image")
                                        .variant(ButtonVariant::Ghost)
                                        .no_background(true)
                                        .text_color(kael::white())
                                        .on_click(move |_, _, cx| {
                                            cx.update_entity(&state_entity, |state, cx| {
                                                state.next_with_notify(cx);
                                                cx.notify();
                                            });
                                        }),
                                ),
                        )
                    })
                    .when_some(current_image.clone(), |this, image| {
                        let custom_scale = match fit_mode {
                            ImageViewerSize::Custom(scale) => scale,
                            _ => 1.0,
                        };
                        let effective_scale = zoom * custom_scale;
                        let media_label = image
                            .alt
                            .clone()
                            .or_else(|| image.caption.clone())
                            .unwrap_or_else(|| "Displayed media".into())
                            .to_string();
                        let media_role = match image.media_type {
                            LightboxMediaType::Image => AccessibilityRole::Image,
                            LightboxMediaType::Video => AccessibilityRole::Group,
                        };
                        let media_element =
                            match image.media_type {
                                LightboxMediaType::Image => {
                                    let image = img(image.src.clone()).size_full().object_fit(
                                        match fit_mode {
                                            ImageViewerSize::Auto => ObjectFit::ScaleDown,
                                            ImageViewerSize::Contain
                                            | ImageViewerSize::Custom(_) => ObjectFit::Contain,
                                            ImageViewerSize::Cover => ObjectFit::Cover,
                                        },
                                    );
                                    image.into_any_element()
                                }
                                LightboxMediaType::Video => {
                                    kael::video(Arc::<str>::from(image.src.as_ref()))
                                        .object_fit(match fit_mode {
                                            ImageViewerSize::Auto => ObjectFit::ScaleDown,
                                            ImageViewerSize::Contain
                                            | ImageViewerSize::Custom(_) => ObjectFit::Contain,
                                            ImageViewerSize::Cover => ObjectFit::Cover,
                                        })
                                        .when(image.has_auto_play || has_auto_play, |this| {
                                            this.autoplay()
                                        })
                                        .into_any_element()
                                }
                            };
                        let pan_start_state = state_entity.clone();
                        let pan_move_state = state_entity.clone();
                        let pan_end_state = state_entity.clone();
                        let pan_cancel_state = state_entity.clone();

                        this.child(
                            div()
                                .id(ElementId::NamedChild(
                                    Box::new(viewer_id.clone()),
                                    "media".into(),
                                ))
                                .accessibility(
                                    AccessibilityAttributes::new(media_role).label(media_label),
                                )
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .w(relative(0.86))
                                .h(relative(if show_thumbnails { 0.72 } else { 0.84 }))
                                .overflow_hidden()
                                .cursor(if is_panning {
                                    CursorStyle::ClosedHand
                                } else if effective_scale > 1.0 {
                                    CursorStyle::OpenHand
                                } else {
                                    CursorStyle::Arrow
                                })
                                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                    cx.update_entity(&pan_start_state, |state, cx| {
                                        state.begin_pan(event.position);
                                        cx.notify();
                                    });
                                    cx.stop_propagation();
                                })
                                .on_mouse_move(move |event, _, cx| {
                                    let moved = cx.update_entity(&pan_move_state, |state, cx| {
                                        let moved = state.update_pan(event.position);
                                        if moved {
                                            cx.notify();
                                        }
                                        moved
                                    });
                                    if moved {
                                        cx.stop_propagation();
                                    }
                                })
                                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                    cx.update_entity(&pan_end_state, |state, cx| {
                                        state.end_pan();
                                        cx.notify();
                                    });
                                    cx.stop_propagation();
                                })
                                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                                    cx.update_entity(&pan_cancel_state, |state, cx| {
                                        state.end_pan();
                                        cx.notify();
                                    });
                                })
                                .child(
                                    div()
                                        .id(ElementId::NamedChild(
                                            Box::new(viewer_id.clone()),
                                            "media-container".into(),
                                        ))
                                        .flex()
                                        .flex_1()
                                        .min_h(px(0.0))
                                        .w_full()
                                        .items_center()
                                        .justify_center()
                                        .overflow_hidden()
                                        .scale(effective_scale)
                                        .ml(pan_offset.x)
                                        .mt(pan_offset.y)
                                        .child(media_element),
                                )
                                .when_some(image.caption.clone(), |this, caption| {
                                    this.child(
                                        div()
                                            .id(ElementId::NamedChild(
                                                Box::new(viewer_id.clone()),
                                                "caption".into(),
                                            ))
                                            .pt(px(8.0))
                                            .px(px(12.0))
                                            .flex_none()
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
            .when(show_thumbnails && images.len() > 1, |this| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(16.0))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .id(ElementId::NamedChild(
                                    Box::new(viewer_id.clone()),
                                    "thumbnail-scroll".into(),
                                ))
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .max_w(relative(0.8))
                                .overflow_scroll()
                                .p(px(6.0))
                                .rounded(theme.tokens.radius_lg)
                                .bg(kael::black().opacity(0.45))
                                .children(images.into_iter().enumerate().map(|(index, image)| {
                                    let state_entity = state_entity.clone();
                                    let label =
                                        image.alt.clone().or(image.caption.clone()).unwrap_or_else(
                                            || format!("Media {}", index + 1).into(),
                                        );
                                    Thumbnail::new()
                                        .id(ElementId::NamedChild(
                                            Box::new(viewer_id.clone()),
                                            format!("thumbnail-{index}").into(),
                                        ))
                                        .when(
                                            image.media_type == LightboxMediaType::Image,
                                            |thumbnail| thumbnail.src(image.src.clone()),
                                        )
                                        .alt(label.clone())
                                        .label(label)
                                        .size(px(48.0))
                                        .show_label(false)
                                        .when(index == current_index, |thumbnail| {
                                            thumbnail.border_2().border_color(theme.tokens.primary)
                                        })
                                        .on_click(move |_, cx| {
                                            cx.update_entity(&state_entity, |state, cx| {
                                                state.go_to_with_notify(index, cx);
                                                cx.notify();
                                            });
                                        })
                                })),
                        ),
                )
            })
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

#[cfg(test)]
mod tests {
    use super::{ImageItem, ImageViewerSize, ImageViewerState};
    use kael::{point, px};

    #[test]
    fn panning_only_moves_a_zoomed_view() {
        let mut state = ImageViewerState::new(vec![ImageItem::new("photo.png")]);
        state.begin_pan(point(px(10.0), px(10.0)));
        assert!(!state.update_pan(point(px(20.0), px(25.0))));
        assert_eq!(state.pan_offset(), point(px(0.0), px(0.0)));

        state.has_zoom(true);
        state.set_zoom(2.0);
        state.begin_pan(point(px(10.0), px(10.0)));
        assert!(state.update_pan(point(px(20.0), px(25.0))));
        assert_eq!(state.pan_offset(), point(px(10.0), px(15.0)));
        state.end_pan();
        assert!(!state.is_panning());
    }

    #[test]
    fn invalid_custom_fit_falls_back_to_contain() {
        let mut state = ImageViewerState::new(Vec::new());
        state.set_fit_mode(ImageViewerSize::Custom(f32::NAN));
        assert_eq!(state.fit_mode(), ImageViewerSize::Contain);
        state.set_fit_mode(ImageViewerSize::Custom(1.5));
        assert_eq!(state.fit_mode(), ImageViewerSize::Custom(1.5));
    }

    #[test]
    fn non_finite_zoom_resets_the_view_and_clears_pan() {
        let mut state = ImageViewerState::new(vec![ImageItem::new("photo.png")]);
        state.has_zoom(true);
        state.set_zoom(2.0);
        state.begin_pan(point(px(10.0), px(10.0)));
        assert!(state.update_pan(point(px(20.0), px(25.0))));

        state.set_zoom(f32::NAN);
        assert_eq!(state.zoom(), 1.0);
        assert_eq!(state.pan_offset(), point(px(0.0), px(0.0)));
        assert!(!state.is_panning());
    }
}
