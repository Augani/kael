//! Separator component - Visual dividers for content sections.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

/// Orientation of the separator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeparatorOrientation {
    /// Horizontal separator (default)
    #[default]
    Horizontal,
    /// Vertical separator
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeparatorVariant {
    #[default]
    Subtle,
    Strong,
}

pub type Divider = Separator;
pub type DividerVariant = SeparatorVariant;

/// Visual divider for separating content
///
/// # Example
///
/// ```rust,ignore
/// // Horizontal separator
/// Separator::new()
///
/// // Vertical separator
/// Separator::new()
///     .orientation(SeparatorOrientation::Vertical)
///
/// // With label
/// Separator::new()
///     .label("OR")
/// ```
#[derive(IntoElement)]
pub struct Separator {
    orientation: SeparatorOrientation,
    variant: SeparatorVariant,
    label: Option<SharedString>,
    color: Option<Hsla>,
    decorative: bool,
    full_bleed: bool,
    style: StyleRefinement,
}

impl Separator {
    /// Create a new horizontal separator
    pub fn new() -> Self {
        Self {
            orientation: SeparatorOrientation::default(),
            variant: SeparatorVariant::default(),
            label: None,
            color: None,
            decorative: true,
            full_bleed: false,
            style: StyleRefinement::default(),
        }
    }

    /// Create a horizontal separator
    pub fn horizontal() -> Self {
        Self::new()
    }

    /// Create a vertical separator
    pub fn vertical() -> Self {
        Self::new().orientation(SeparatorOrientation::Vertical)
    }

    /// Set the orientation
    pub fn orientation(mut self, orientation: SeparatorOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Set visual divider weight.
    pub fn variant(mut self, variant: SeparatorVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set an optional label to display on the separator
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set custom color for the separator line
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Set whether this is a decorative separator (for accessibility)
    /// Decorative separators are hidden from screen readers
    pub fn decorative(mut self, decorative: bool) -> Self {
        self.decorative = decorative;
        self
    }

    /// Extend the divider to the edges of its container.
    pub fn full_bleed(mut self, full_bleed: bool) -> Self {
        self.full_bleed = full_bleed;
        self
    }

    #[allow(non_snake_case)]
    pub fn isFullBleed(self, full_bleed: bool) -> Self {
        self.full_bleed(full_bleed)
    }
}

impl Styled for Separator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Separator {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let border = tokens.border;
        let foreground = tokens.foreground;
        let muted_foreground = tokens.muted_foreground;
        let line_color = self.color.unwrap_or(match self.variant {
            SeparatorVariant::Subtle => border,
            SeparatorVariant::Strong => foreground.opacity(0.32),
        });
        let user_style = self.style;
        let is_horizontal = self.orientation == SeparatorOrientation::Horizontal;
        let has_label = self.label.is_some();
        let line = || {
            div()
                .flex_1()
                .when(is_horizontal, |this| this.h(px(1.0)))
                .when(!is_horizontal, |this| this.w(px(1.0)))
                .bg(line_color)
        };

        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .when(is_horizontal, |this| {
                this.w_full().when(!has_label, |this| this.h(px(1.0)))
            })
            .when(!is_horizontal, |this| {
                this.flex_col()
                    .h_full()
                    .when(!has_label, |this| this.w(px(1.0)))
            })
            .when(self.full_bleed && is_horizontal, |this| {
                this.ml(px(-24.0)).mr(px(-24.0))
            })
            .when(self.full_bleed && !is_horizontal, |this| {
                this.mt(px(-24.0)).mb(px(-24.0))
            })
            .child(line())
            .when_some(self.label, |this, label| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .when(is_horizontal, |this| this.px(px(12.0)))
                        .when(!is_horizontal, |this| this.py(px(12.0)))
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(muted_foreground)
                        .font_family(tokens.font_family.clone())
                        .child(label),
                )
                .child(line())
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}
