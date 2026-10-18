//! Icon component - SVG icon rendering with named icon support.

use crate::components::icon_source::IconSource;
use crate::components::{button::ButtonVariant, icon_button::IconButton};
use crate::icon_config::resolve_icon_path;
use crate::theme::use_theme;
use kael::{prelude::*, *};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;
use std::{panic::Location, path::Path, sync::RwLock};

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
        ("success", "circle-check"),
        ("error", "circle-x"),
        ("warning", "triangle-alert"),
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

/// Icon rendering style. Solid variants resolve `<name>-solid` assets or
/// registered aliases when available, with a regular-icon fallback.
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

// Default identities are resolved only on the interactive path. Keep the full
// original Name key when it becomes public through a rendered IconButton.
// A private enum prevents an explicit CodeLocation override being mistaken for
// a default marker. No cache, interner or additional persistent owner exists.
enum IconIdentity {
    Default(&'static Location<'static>),
    Explicit(ElementId),
}

fn default_icon_element_id(file: &'static str, line: u32, column: u32) -> ElementId {
    ElementId::Name(format!("icon:{}:{}:{}", file, line, column).into())
}

impl IconIdentity {
    fn into_element_id(self) -> ElementId {
        match self {
            Self::Default(caller) => {
                default_icon_element_id(caller.file(), caller.line(), caller.column())
            }
            Self::Explicit(id) => id,
        }
    }
}

pub struct Icon {
    id: IconIdentity,
    source: IconSource,
    variant: IconVariant,
    size: IconSize,
    color: Option<Hsla>,
    semantic_color: IconColor,
    clickable: bool,
    disabled: bool,
    label: Option<SharedString>,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
    rotation: Option<Radians>,
}

impl Icon {
    #[track_caller]
    pub fn new(source: impl Into<IconSource>) -> Self {
        let caller = Location::caller();
        Self {
            id: IconIdentity::Default(caller),
            source: source.into(),
            variant: IconVariant::default(),
            size: IconSize::default(),
            color: None,
            semantic_color: IconColor::Inherit,
            clickable: false,
            disabled: false,
            label: None,
            on_click: None,
            style: StyleRefinement::default(),
            rotation: None,
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = IconIdentity::Explicit(id.into());
        self
    }

    /// Set the accessible name used when the icon is interactive.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
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
            IconSource::Named(name) => {
                if self.variant == IconVariant::Solid {
                    for candidate in [format!("{}Solid", name), format!("{}-solid", name)] {
                        if let Some(source) = get_icon(&candidate) {
                            return match source {
                                IconSource::FilePath(path) => Some(path),
                                IconSource::Named(name) => {
                                    Some(SharedString::from(resolve_icon_path(&name)))
                                }
                            };
                        }
                    }

                    let solid_path = resolve_icon_path(&format!("{}-solid", name));
                    if Path::new(&solid_path).exists() {
                        return Some(solid_path.into());
                    }
                }
                Some(SharedString::from(icon_path_from_name(name)))
            }
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
        let label = self.label.unwrap_or_else(|| match &self.source {
            IconSource::Named(name) => name.replace(['-', '_'], " ").into(),
            IconSource::FilePath(_) => "Icon action".into(),
        });
        let icon_size = self.size.to_pixels();
        let rotation = self.rotation;
        let mut button = IconButton::new(svg_content.unwrap_or_else(|| "".into()))
            .id(self.id.into_element_id())
            .label(label)
            .variant(ButtonVariant::Ghost)
            .no_background(true)
            .size(icon_size.max(px(32.0)))
            .icon_size(icon_size)
            .disabled(disabled)
            .text_color(if disabled {
                theme.tokens.muted_foreground
            } else {
                color
            })
            .when_some(rotation, |this, rotation| this.rotate(rotation));
        button.style().refine(&self.style);
        if let Some(callback) = on_click {
            button = button.on_click(move |_, window, cx| callback(window, cx));
        }
        button.into_any_element()
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

#[cfg(test)]
mod tests {
    use super::{IconSource, default_icons};

    #[test]
    fn semantic_aliases_resolve_to_bundled_icon_names() {
        let icons = default_icons();

        assert_eq!(
            icons.get("success"),
            Some(&IconSource::from("circle-check"))
        );
        assert_eq!(
            icons.get("warning"),
            Some(&IconSource::from("triangle-alert"))
        );
        assert_eq!(icons.get("error"), Some(&IconSource::from("circle-x")));
    }
}
#[cfg(test)]
fn inspected_icon_identity_for_regression(icon: Icon) -> ElementId {
    icon.id.into_element_id()
}

#[cfg(test)]
fn inspected_callsite_identity_for_regression(
    file: &'static str,
    line: u32,
    column: u32,
) -> ElementId {
    default_icon_element_id(file, line, column)
}

#[cfg(test)]
mod default_identity_regressions {
    use super::*;
    use core::prelude::v1::test;
    use std::hash::{Hash, Hasher};

    #[track_caller]
    fn icon_and_location() -> (Icon, &'static Location<'static>) {
        (Icon::new("chevron-right"), Location::caller())
    }

    fn same_callsite() -> Icon {
        Icon::new("chevron-right")
    }

    fn hash_id(id: &ElementId) -> u64 {
        let mut state = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut state);
        state.finish()
    }

    #[test]
    fn clickable_default_preserves_original_name_display_hash_and_conversion() {
        let (icon, caller) = icon_and_location();
        let id = inspected_icon_identity_for_regression(icon.clickable(true));
        let expected_name = format!(
            "icon:{}:{}:{}",
            caller.file(),
            caller.line(),
            caller.column()
        );
        let expected = ElementId::Name(expected_name.clone().into());
        assert_eq!(id, expected);
        assert_eq!(hash_id(&id), hash_id(&expected));
        assert_eq!(id.to_string(), expected_name);
        let name: SharedString = id.try_into().expect("default remains a Name");
        assert_eq!(name.as_ref(), expected_name);
    }

    #[test]
    fn same_constructor_callsite_remains_stable() {
        let first = inspected_icon_identity_for_regression(same_callsite());
        let second = inspected_icon_identity_for_regression(same_callsite());
        assert_eq!(first, second);
        assert_eq!(hash_id(&first), hash_id(&second));
    }

    #[test]
    fn distinct_actual_lines_and_columns_do_not_alias() {
        let first = Icon::new("chevron-right");
        let second = Icon::new("chevron-right");
        assert_ne!(
            inspected_icon_identity_for_regression(first),
            inspected_icon_identity_for_regression(second)
        );
        let (first, second) = (Icon::new("chevron-right"), Icon::new("chevron-right"));
        assert_ne!(
            inspected_icon_identity_for_regression(first),
            inspected_icon_identity_for_regression(second)
        );
    }

    #[test]
    fn file_line_column_fields_and_icon_namespace_remain_exact() {
        let id = inspected_callsite_identity_for_regression("file-a.rs", 123, 456);
        assert_eq!(id, ElementId::Name("icon:file-a.rs:123:456".into()));
        assert_ne!(
            id,
            inspected_callsite_identity_for_regression("file-b.rs", 123, 456)
        );
        assert_ne!(
            id,
            inspected_callsite_identity_for_regression("file-a.rs", 124, 456)
        );
        assert_ne!(
            id,
            inspected_callsite_identity_for_regression("file-a.rs", 123, 457)
        );
        assert_ne!(
            inspected_callsite_identity_for_regression("file-a.rs", 0, u32::MAX),
            inspected_callsite_identity_for_regression("file-a.rs", 1, 0)
        );
        assert_ne!(id, ElementId::Name("button:file-a.rs:123:456".into()));
        assert_ne!(
            id,
            ElementId::NamedInteger(SharedString::new_static("file-a.rs"), (123_u64 << 32) | 456)
        );
        assert_eq!(
            inspected_callsite_identity_for_regression("file-a.rs", u32::MAX, u32::MAX).to_string(),
            "icon:file-a.rs:4294967295:4294967295"
        );
    }

    #[test]
    fn explicit_name_and_code_location_overrides_remain_exact() {
        let explicit = ElementId::Name("explicit-icon".into());
        assert_eq!(
            inspected_icon_identity_for_regression(Icon::new("chevron-right").id(explicit.clone())),
            explicit
        );
        let explicit = ElementId::CodeLocation(*Location::caller());
        assert_eq!(
            inspected_icon_identity_for_regression(
                Icon::new("chevron-right")
                    .id(explicit.clone())
                    .clickable(true)
            ),
            explicit
        );
    }

    #[test]
    fn explicit_override_and_clickable_toggling_preserve_the_same_identity() {
        let explicit = ElementId::NamedInteger(SharedString::new_static("user-icon"), u64::MAX);
        let icon = Icon::new("chevron-right")
            .clickable(true)
            .id(explicit.clone())
            .clickable(false)
            .clickable(true);
        assert_eq!(inspected_icon_identity_for_regression(icon), explicit);
    }

    struct MountedIdentityHost {
        explicit: Option<ElementId>,
        activations: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Render for MountedIdentityHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let activations = self.activations.clone();
            let icon = Icon::new("chevron-right")
                .clickable(true)
                .label("Identity activation")
                .on_click(move |_, _| {
                    activations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                });
            let icon = if let Some(id) = self.explicit.clone() {
                icon.id(id)
            } else {
                icon
            };
            div().w(px(100.0)).h(px(100.0)).child(icon)
        }
    }

    #[kael::test]
    fn mounted_clickable_default_and_explicit_ids_keep_semantic_identity_and_activation(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral_dark()));
        let cases = [
            None,
            Some(ElementId::Name("explicit-icon".into())),
            Some(ElementId::CodeLocation(*Location::caller())),
        ];
        for explicit in cases {
            let activations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let (view, window) = cx.add_window_view({
                let activations = activations.clone();
                move |_, _| MountedIdentityHost {
                    explicit,
                    activations,
                }
            });
            let mut window = window.clone();
            window.update(|window, cx| window.draw(cx).clear());
            let first = window.update(|window, _| {
                let tree = window.accessibility_tree();
                let node = tree
                    .nodes
                    .values()
                    .find(|node| {
                        node.role == AccessibilityRole::Button
                            && node.label.as_deref() == Some("Identity activation")
                    })
                    .expect("mounted clickable icon button");
                assert!(node.actions.contains(&AccessibilityAction::Focus));
                assert!(node.actions.contains(&AccessibilityAction::Click));
                (node.id, node.bounds.expect("mounted button bounds"))
            });
            assert!(first.1.width > 0.0 && first.1.height > 0.0);
            window.update(|window, cx| {
                view.update(cx, |_, cx| cx.notify());
                window.draw(cx).clear();
            });
            let second = window.update(|window, _| {
                window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .find(|node| {
                        node.role == AccessibilityRole::Button
                            && node.label.as_deref() == Some("Identity activation")
                    })
                    .expect("button after redraw")
                    .id
            });
            assert_eq!(first.0, second, "same rendered identity survives redraw");
            let center = point(
                px((first.1.x + first.1.width / 2.0) as f32),
                px((first.1.y + first.1.height / 2.0) as f32),
            );
            window.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
            window.update(|window, cx| window.draw(cx).clear());
            window.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
            assert_eq!(
                activations.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "one synthetic click activates once"
            );
            window.update(|window, _| window.remove_window());
            drop(view);
            drop(window);
        }
    }
}
