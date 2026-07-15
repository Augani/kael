//! Layer component - ASTRYX-style positioned layer primitive and API types.

use crate::overlays::toast::{ToastPosition, ToastViewport};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayerPlacement {
    #[default]
    Fill,
    Center,
    Top,
    Bottom,
    Above,
    Below,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayerAlignment {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayerMode {
    #[default]
    Context,
    Fixed,
}

#[derive(Clone, Debug)]
pub struct ContextRenderProps {
    pub placement: LayerPlacement,
    pub alignment: LayerAlignment,
}

impl Default for ContextRenderProps {
    fn default() -> Self {
        Self {
            placement: LayerPlacement::Above,
            alignment: LayerAlignment::Center,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FixedRenderProps {
    pub x: Pixels,
    pub y: Pixels,
}

impl FixedRenderProps {
    pub fn new(x: impl Into<Pixels>, y: impl Into<Pixels>) -> Self {
        Self {
            x: x.into(),
            y: y.into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContextLayerOptions {
    pub light_dismiss: bool,
    pub is_open: bool,
}

#[derive(Clone, Debug)]
pub struct FixedLayerOptions {
    pub light_dismiss: bool,
    pub is_open: bool,
    pub x: Pixels,
    pub y: Pixels,
}

impl Default for FixedLayerOptions {
    fn default() -> Self {
        Self {
            light_dismiss: false,
            is_open: false,
            x: px(0.0),
            y: px(0.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContextLayerReturn {
    pub anchor_id: SharedString,
    pub id: SharedString,
    pub is_open: bool,
}

#[derive(Clone, Debug)]
pub struct FixedLayerReturn {
    pub id: SharedString,
    pub is_open: bool,
    pub x: Pixels,
    pub y: Pixels,
}

#[derive(Clone, Debug)]
pub struct LayerToastConfig {
    pub position: ToastPosition,
    pub max_visible: usize,
    pub inset: Pixels,
}

impl Default for LayerToastConfig {
    fn default() -> Self {
        Self {
            position: ToastPosition::BottomEnd,
            max_visible: 5,
            inset: px(0.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerContextValue {
    pub toast_config: LayerToastConfig,
    pub is_provider: bool,
}

pub fn use_layer_context() -> Option<LayerContextValue> {
    None
}

#[allow(non_snake_case)]
pub fn useLayerContext() -> Option<LayerContextValue> {
    use_layer_context()
}

pub fn use_layer(options: ContextLayerOptions) -> ContextLayerReturn {
    ContextLayerReturn {
        anchor_id: "--astryx-layer-kael".into(),
        id: "astryx-layer-kael".into(),
        is_open: options.is_open,
    }
}

#[allow(non_snake_case)]
pub fn useLayer(options: ContextLayerOptions) -> ContextLayerReturn {
    use_layer(options)
}

#[derive(IntoElement)]
pub struct LayerProvider {
    toast: LayerToastConfig,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

pub type LayerProviderProps = LayerProvider;

impl LayerProvider {
    pub fn new() -> Self {
        Self {
            toast: LayerToastConfig::default(),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn toast(mut self, toast: LayerToastConfig) -> Self {
        self.toast = toast;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for LayerProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for LayerProvider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LayerProvider {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        self.children
            .into_iter()
            .fold(
                ToastViewport::new()
                    .position(self.toast.position)
                    .max_visible(self.toast.max_visible)
                    .inset(self.toast.inset),
                |viewport, child| viewport.child(child),
            )
            .map(|this| {
                let mut viewport = this;
                viewport.style().refine(&user_style);
                viewport
            })
    }
}

#[derive(IntoElement)]
pub struct Layer {
    placement: LayerPlacement,
    alignment: LayerAlignment,
    surface: bool,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Layer {
    pub fn new() -> Self {
        Self {
            placement: LayerPlacement::Fill,
            alignment: LayerAlignment::Center,
            surface: false,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn placement(mut self, placement: LayerPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn alignment(mut self, alignment: LayerAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn surface(mut self, surface: bool) -> Self {
        self.surface = surface;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Layer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Layer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let placement = match self.placement {
            LayerPlacement::Above => LayerPlacement::Top,
            LayerPlacement::Below => LayerPlacement::Bottom,
            LayerPlacement::Start => LayerPlacement::Top,
            LayerPlacement::End => LayerPlacement::Bottom,
            placement => placement,
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .when(placement == LayerPlacement::Fill, |this| {
                this.items_start().justify_start()
            })
            .when(placement == LayerPlacement::Center, |this| {
                this.items_center().justify_center()
            })
            .when(placement == LayerPlacement::Top, |this| {
                this.items_start().justify_center()
            })
            .when(placement == LayerPlacement::Bottom, |this| {
                this.items_end().justify_center()
            })
            .child(
                div()
                    .when(placement == LayerPlacement::Fill, |this| this.size_full())
                    .when(self.alignment == LayerAlignment::Start, |this| {
                        this.justify_self_start()
                    })
                    .when(self.alignment == LayerAlignment::Center, |this| {
                        this.justify_self_center()
                    })
                    .when(self.alignment == LayerAlignment::End, |this| {
                        this.justify_self_end()
                    })
                    .when(self.surface, |this| {
                        this.bg(theme.tokens.popover)
                            .border_1()
                            .border_color(theme.tokens.border)
                            .rounded(theme.tokens.radius_lg)
                            .shadow(theme.tokens.shadow_lg.to_vec())
                    })
                    .children(self.children),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
