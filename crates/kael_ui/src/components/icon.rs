//! Icon component - SVG icon rendering with named icon support.

use crate::components::icon_source::IconSource;
use crate::icon_config::resolve_icon_path;
use crate::theme::use_theme;
use kael::{prelude::*, *};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;
use std::sync::RwLock;

pub type IconName = String;
pub type IconType = IconSource;
pub type IconRegistry = BTreeMap<IconName, IconSource>;

static ICON_REGISTRY: Lazy<RwLock<IconRegistry>> = Lazy::new(|| RwLock::new(default_icons()));

fn default_icons() -> IconRegistry {
    [
        ("close", "x"),
        ("chevronDown", "chevron-down"),
        ("chevronLeft", "chevron-left"),
        ("chevronRight", "chevron-right"),
        ("check", "check"),
        ("success", "check-circle"),
        ("error", "x-circle"),
        ("warning", "alert-circle"),
        ("info", "info"),
        ("calendar", "calendar"),
        ("clock", "clock"),
        ("externalLink", "external-link"),
        ("menu", "menu"),
        ("moreHorizontal", "more-horizontal"),
        ("search", "search"),
        ("arrowUp", "arrow-up"),
        ("arrowDown", "arrow-down"),
        ("arrowsUpDown", "arrow-up-down"),
        ("funnel", "filter"),
        ("eyeSlash", "eye-off"),
        ("viewColumns", "columns"),
        ("copy", "copy"),
        ("checkDouble", "badge-check"),
        ("wrench", "wrench"),
        ("stop", "square"),
        ("microphone", "mic"),
    ]
    .into_iter()
    .map(|(name, icon)| (name.to_string(), IconSource::from(icon)))
    .collect()
}

pub fn register_icons(icons: IconRegistry) {
    let mut registry = ICON_REGISTRY.write().expect("icon registry poisoned");
    registry.extend(icons);
}

#[allow(non_snake_case)]
pub fn registerIcons(icons: IconRegistry) {
    register_icons(icons);
}

pub fn get_icon_registry() -> IconRegistry {
    ICON_REGISTRY
        .read()
        .expect("icon registry poisoned")
        .clone()
}

#[allow(non_snake_case)]
pub fn getIconRegistry() -> IconRegistry {
    get_icon_registry()
}

pub fn get_icon(name: impl AsRef<str>) -> Option<IconSource> {
    ICON_REGISTRY
        .read()
        .expect("icon registry poisoned")
        .get(name.as_ref())
        .cloned()
}

#[allow(non_snake_case)]
pub fn getIcon(name: impl AsRef<str>) -> Option<IconSource> {
    get_icon(name)
}

pub fn reset_icons() {
    let mut registry = ICON_REGISTRY.write().expect("icon registry poisoned");
    *registry = default_icons();
}

#[allow(non_snake_case)]
pub fn resetIcons() {
    reset_icons();
}

/// Icon variant - currently for API compatibility, not yet affecting rendering
/// TODO: Implement different icon styles or remove if not needed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconVariant {
    #[default]
    Regular,
    Solid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconColor {
    Primary,
    Secondary,
    Tertiary,
    Disabled,
    Accent,
    Success,
    Error,
    Warning,
    #[default]
    Inherit,
    Blue,
    Red,
    Green,
    Gray,
    Cyan,
    Teal,
    Yellow,
    Orange,
    Pink,
    Purple,
}

/// Icon size variants - supports named sizes and custom pixel values
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum IconSize {
    /// Extra small: 12px (size_3 in Kael)
    XSmall,
    /// Small: 14px (size_3p5 in Kael)
    Small,
    /// ASTRYX small: 16px
    Sm,
    /// Medium: 16px (size_4 in Kael) - Default
    #[default]
    Medium,
    /// ASTRYX medium: 20px
    Md,
    /// Large: 24px (size_6 in Kael)
    Large,
    /// ASTRYX large: 24px
    Lg,
    /// ASTRYX extra small: 12px
    Xsm,
    /// Custom pixel size
    Custom(Pixels),
}

impl From<Pixels> for IconSize {
    fn from(pixels: Pixels) -> Self {
        Self::Custom(pixels)
    }
}

impl From<f32> for IconSize {
    fn from(value: f32) -> Self {
        Self::Custom(px(value))
    }
}

impl IconSize {
    /// Convert IconSize to Pixels
    pub fn to_pixels(&self) -> Pixels {
        match self {
            IconSize::XSmall => px(12.0),
            IconSize::Small => px(14.0),
            IconSize::Sm => px(16.0),
            IconSize::Medium => px(16.0),
            IconSize::Md => px(20.0),
            IconSize::Large => px(24.0),
            IconSize::Lg => px(24.0),
            IconSize::Xsm => px(12.0),
            IconSize::Custom(pixels) => *pixels,
        }
    }
}

fn icon_path_from_name(name: &str) -> String {
    match get_icon(name) {
        Some(IconSource::FilePath(path)) => path.to_string(),
        Some(IconSource::Named(name)) => resolve_icon_path(&name),
        None => resolve_icon_path(name),
    }
}

pub struct Icon {
    source: IconSource,
    variant: IconVariant,
    size: IconSize,
    color: Option<Hsla>,
    semantic_color: IconColor,
    clickable: bool,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App) + Send + Sync + 'static>>,
    focus_handle: Option<FocusHandle>,
    style: StyleRefinement,
    rotation: Option<Radians>,
}

impl Icon {
    pub fn new(source: impl Into<IconSource>) -> Self {
        Self {
            source: source.into(),
            variant: IconVariant::default(),
            size: IconSize::default(),
            color: None,
            semantic_color: IconColor::Inherit,
            clickable: false,
            disabled: false,
            on_click: None,
            focus_handle: None,
            style: StyleRefinement::default(),
            rotation: None,
        }
    }

    pub fn variant(mut self, variant: IconVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn icon_color(mut self, color: IconColor) -> Self {
        self.semantic_color = color;
        self
    }

    pub fn color_variant(self, color: IconColor) -> Self {
        self.icon_color(color)
    }

    pub fn clickable(mut self, clickable: bool) -> Self {
        self.clickable = clickable;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_click = Some(Box::new(f));
        self.clickable = true;
        self
    }

    /// Rotate the icon by the given angle in radians
    pub fn rotate(mut self, radians: impl Into<Radians>) -> Self {
        self.rotation = Some(radians.into());
        self
    }

    fn get_svg_path(&self) -> Option<SharedString> {
        match &self.source {
            IconSource::FilePath(path) => Some(path.clone()),
            IconSource::Named(name) => Some(SharedString::from(icon_path_from_name(name))),
        }
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn semantic_icon_color(color: IconColor, theme: &crate::theme::Theme) -> Hsla {
    match color {
        IconColor::Primary => theme.tokens.foreground,
        IconColor::Secondary | IconColor::Tertiary => theme.tokens.muted_foreground,
        IconColor::Disabled => theme.tokens.muted_foreground.opacity(0.5),
        IconColor::Accent => theme.tokens.accent_foreground,
        IconColor::Success | IconColor::Green => hsla(142.0 / 360.0, 0.71, 0.45, 1.0),
        IconColor::Error | IconColor::Red => theme.tokens.destructive,
        IconColor::Warning | IconColor::Yellow => hsla(48.0 / 360.0, 0.96, 0.53, 1.0),
        IconColor::Inherit => theme.tokens.foreground,
        IconColor::Blue => hsla(217.0 / 360.0, 0.91, 0.6, 1.0),
        IconColor::Gray => theme.tokens.muted_foreground,
        IconColor::Cyan => hsla(188.0 / 360.0, 0.86, 0.53, 1.0),
        IconColor::Teal => hsla(173.0 / 360.0, 0.8, 0.4, 1.0),
        IconColor::Orange => hsla(25.0 / 360.0, 0.95, 0.53, 1.0),
        IconColor::Pink => hsla(330.0 / 360.0, 0.81, 0.6, 1.0),
        IconColor::Purple => hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
    }
}

impl IntoElement for Icon {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let color = self
            .color
            .unwrap_or_else(|| semantic_icon_color(self.semantic_color, &theme));
        let svg_content = self.get_svg_path();

        // For non-clickable icons, return minimal wrapper
        if !self.clickable {
            let mut base = svg();
            *base.style() = self.style;

            return base
                .flex_shrink_0()
                .when_some(svg_content, |this, svg_string| this.path(svg_string))
                .size(self.size.to_pixels())
                .text_color(if self.disabled {
                    theme.tokens.muted_foreground
                } else {
                    color
                })
                .when_some(self.rotation, |this, rotation| {
                    this.with_transformation(Transformation::rotate(rotation))
                })
                .into_any_element();
        }

        // For clickable icons, wrap in interactive Div
        let on_click = self.on_click;
        let disabled = self.disabled;

        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .cursor(if disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .when_some(self.focus_handle, |div, handle| div.track_focus(&handle))
            .when(!disabled && on_click.is_some(), |div| {
                div.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if let Some(ref cb) = on_click {
                        cb(window, cx);
                    }
                })
            })
            .when(!disabled, |div| {
                div.hover(|mut style| {
                    style.opacity = Some(0.7);
                    style
                })
            })
            .when_some(svg_content, |div, svg_string| {
                let mut icon_svg = svg();
                *icon_svg.style() = self.style.clone();

                div.child(
                    icon_svg
                        .path(svg_string)
                        .size(self.size.to_pixels())
                        .text_color(if disabled {
                            theme.tokens.muted_foreground
                        } else {
                            color
                        })
                        .when_some(self.rotation, |this, rotation| {
                            this.with_transformation(Transformation::rotate(rotation))
                        }),
                )
            })
            .into_any_element()
    }
}

pub fn icon(source: impl Into<IconSource>) -> Icon {
    Icon::new(source)
}

pub fn icon_button<F>(source: impl Into<IconSource>, on_click: F) -> Icon
where
    F: Fn(&mut Window, &mut App) + Send + Sync + 'static,
{
    Icon::new(source).clickable(true).on_click(on_click)
}

pub fn render_icon_slot(
    source: impl Into<IconSource>,
    size: impl Into<IconSize>,
    color: IconColor,
) -> Icon {
    Icon::new(source).size(size).icon_color(color)
}

#[allow(non_snake_case)]
pub fn renderIconSlot(
    source: impl Into<IconSource>,
    size: impl Into<IconSize>,
    color: IconColor,
) -> Icon {
    render_icon_slot(source, size, color)
}
