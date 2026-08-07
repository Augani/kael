use crate::{
    self as kael, AbsoluteLength, AlignContent, AlignItems, AlignSelf, Background, BlendMode,
    BorderStyle, DefiniteLength, Display, Fill, FlexDirection, FlexWrap, Font, FontStyle,
    FontWeight, GridAutoFlow, GridPlacement, GridTrack, Hsla, JustifyContent, Length, Pixels,
    SharedString, StrikethroughStyle, StyleRefinement, TextAlign, TextOverflow, TextShadow,
    TextStyleRefinement, UnderlineStyle, WhiteSpace, point, px, relative, rems,
};
pub use kael_macros::{
    border_style_methods, box_shadow_style_methods, cursor_style_methods, margin_style_methods,
    overflow_style_methods, padding_style_methods, position_style_methods,
    visibility_style_methods,
};

const ELLIPSIS: SharedString = SharedString::new_static("…");

/// A trait for elements that can be styled.
/// Use this to opt-in to a utility CSS-like styling API.
#[cfg_attr(
    any(feature = "inspector", debug_assertions),
    kael_macros::derive_inspector_reflection
)]
pub trait Styled: Sized {
    /// Returns a reference to the style memory of this element.
    fn style(&mut self) -> &mut StyleRefinement;

    kael_macros::style_helpers!();
    kael_macros::visibility_style_methods!();
    kael_macros::margin_style_methods!();
    kael_macros::padding_style_methods!();
    kael_macros::position_style_methods!();
    kael_macros::overflow_style_methods!();
    kael_macros::cursor_style_methods!();
    kael_macros::border_style_methods!();
    kael_macros::box_shadow_style_methods!();

    /// Sets the display type of the element to `block`.
    /// [Docs](https://tailwindcss.com/docs/display)
    fn block(mut self) -> Self {
        self.style().display = Some(Display::Block);
        self
    }

    /// Sets the display type of the element to `flex`.
    /// [Docs](https://tailwindcss.com/docs/display)
    fn flex(mut self) -> Self {
        self.style().display = Some(Display::Flex);
        self
    }

    /// Sets the display type of the element to `grid`.
    /// [Docs](https://tailwindcss.com/docs/display)
    fn grid(mut self) -> Self {
        self.style().display = Some(Display::Grid);
        self
    }

    /// Sets the whitespace of the element to `normal`.
    /// [Docs](https://tailwindcss.com/docs/whitespace#normal)
    fn whitespace_normal(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .white_space = Some(WhiteSpace::Normal);
        self
    }

    /// Sets the whitespace of the element to `nowrap`.
    /// [Docs](https://tailwindcss.com/docs/whitespace#nowrap)
    fn whitespace_nowrap(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .white_space = Some(WhiteSpace::Nowrap);
        self
    }

    /// Sets the truncate overflowing text with an ellipsis (…) if needed.
    /// [Docs](https://tailwindcss.com/docs/text-overflow#ellipsis)
    fn text_ellipsis(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_overflow = Some(TextOverflow::Truncate(ELLIPSIS));
        self
    }

    /// Sets the text overflow behavior of the element.
    fn text_overflow(mut self, overflow: TextOverflow) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_overflow = Some(overflow);
        self
    }

    /// Set the text alignment of the element.
    fn text_align(mut self, align: TextAlign) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_align = Some(align);
        self
    }

    /// Sets the text alignment to left
    fn text_left(mut self) -> Self {
        self.text_align(TextAlign::Left)
    }

    /// Sets the text alignment to center
    fn text_center(mut self) -> Self {
        self.text_align(TextAlign::Center)
    }

    /// Sets the text alignment to right
    fn text_right(mut self) -> Self {
        self.text_align(TextAlign::Right)
    }

    /// Sets the truncate to prevent text from wrapping and truncate overflowing text with an ellipsis (…) if needed.
    /// [Docs](https://tailwindcss.com/docs/text-overflow#truncate)
    fn truncate(mut self) -> Self {
        self.overflow_hidden().whitespace_nowrap().text_ellipsis()
    }

    /// Sets number of lines to show before truncating the text.
    /// [Docs](https://tailwindcss.com/docs/line-clamp)
    fn line_clamp(mut self, lines: usize) -> Self {
        let mut text_style = self.text_style().get_or_insert_with(Default::default);
        text_style.line_clamp = Some(lines);
        self.overflow_hidden()
    }

    /// Sets the letter spacing (tracking) for text.
    /// [Docs](https://tailwindcss.com/docs/letter-spacing)
    fn letter_spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .letter_spacing = Some(spacing.into());
        self
    }

    /// Sets letter spacing to -0.5px (tighter tracking).
    fn tracking_tighter(self) -> Self {
        self.letter_spacing(px(-0.5))
    }

    /// Sets letter spacing to -0.25px (tight tracking).
    fn tracking_tight(self) -> Self {
        self.letter_spacing(px(-0.25))
    }

    /// Resets letter spacing to 0px (normal tracking).
    fn tracking_normal(self) -> Self {
        self.letter_spacing(px(0.0))
    }

    /// Sets letter spacing to 0.5px (wide tracking).
    fn tracking_wide(self) -> Self {
        self.letter_spacing(px(0.5))
    }

    /// Sets letter spacing to 1.0px (wider tracking).
    fn tracking_wider(self) -> Self {
        self.letter_spacing(px(1.0))
    }

    /// Sets letter spacing to 2.0px (widest tracking).
    fn tracking_widest(self) -> Self {
        self.letter_spacing(px(2.0))
    }

    /// Enables continuous (squircle) corner rounding instead of circular.
    /// This is the default for new `Style` instances since it matches SwiftUI's
    /// `RoundedRectangle.fill()` shape on macOS; call this explicitly only when
    /// overriding a style that has been switched to circular corners.
    fn continuous_corners(mut self) -> Self {
        self.style().continuous_corners = Some(true);
        self
    }

    /// Forces pure quarter-circle corner rounding instead of the default squircle.
    /// Use this only when matching a design that explicitly requires circular corners;
    /// for parity with SwiftUI/AppKit you almost always want the default continuous corners.
    fn circular_corners(mut self) -> Self {
        self.style().continuous_corners = Some(false);
        self
    }

    /// Sets the blend mode for this element's background rendering.
    fn blend_mode(mut self, mode: BlendMode) -> Self {
        self.style().blend_mode = Some(mode);
        self
    }

    /// Applies a backdrop blur to content rendered behind this element.
    fn backdrop_blur(mut self, radius: impl Into<AbsoluteLength>) -> Self {
        self.style().backdrop_blur = Some(radius.into());
        self
    }

    /// Adjusts the saturation applied to this element's blurred backdrop.
    fn backdrop_saturate(mut self, saturation: f32) -> Self {
        self.style().backdrop_saturate = Some(saturation.max(0.0));
        self
    }

    /// Applies an inset (inner) shadow — useful for pressed, inset, or neumorphic surfaces.
    fn shadow_inner(
        mut self,
        color: impl Into<Hsla>,
        blur_radius: impl Into<Pixels>,
        spread_radius: impl Into<Pixels>,
    ) -> Self {
        self.style().box_shadow = Some(smallvec::smallvec![crate::BoxShadow {
            color: color.into(),
            offset: point(px(0.), px(2.)),
            blur_radius: blur_radius.into(),
            spread_radius: spread_radius.into(),
            inset: true,
        }]);
        self
    }

    /// Applies a soft, offset-free outer glow of the given color and radius.
    fn glow(mut self, color: impl Into<Hsla>, radius: impl Into<Pixels>) -> Self {
        self.style().box_shadow = Some(smallvec::smallvec![crate::BoxShadow {
            color: color.into(),
            offset: point(px(0.), px(0.)),
            blur_radius: radius.into(),
            spread_radius: px(0.),
            inset: false,
        }]);
        self
    }

    /// Apply a reusable style preset built once as a [`StyleRefinement`]
    /// (e.g. `StyleRefinement::default().bg(...).rounded(...)`) and shared across many
    /// elements, so a custom look stays DRY instead of being re-typed per element.
    ///
    /// The preset's set properties are written onto this element; call it as a base
    /// before any element-specific style methods, which then override the preset.
    fn refine_style(mut self, preset: &StyleRefinement) -> Self {
        refineable::Refineable::refine(self.style(), preset);
        self
    }

    /// Applies a text shadow with custom parameters.
    fn text_shadow(mut self, shadow: TextShadow) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_shadow = Some(shadow);
        self
    }

    /// Applies a small text shadow (1px offset, 2px blur).
    fn text_shadow_sm(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_shadow = Some(TextShadow {
            color: Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0.2,
            },
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(2.0),
        });
        self
    }

    /// Applies a medium text shadow (2px offset, 4px blur).
    fn text_shadow_md(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_shadow = Some(TextShadow {
            color: Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0.25,
            },
            offset: point(px(0.0), px(2.0)),
            blur_radius: px(4.0),
        });
        self
    }

    /// Applies a large text shadow (3px offset, 6px blur).
    fn text_shadow_lg(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .text_shadow = Some(TextShadow {
            color: Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0.3,
            },
            offset: point(px(0.0), px(3.0)),
            blur_radius: px(6.0),
        });
        self
    }

    /// Sets the flex direction of the element to `column`.
    /// [Docs](https://tailwindcss.com/docs/flex-direction#column)
    fn flex_col(mut self) -> Self {
        self.style().flex_direction = Some(FlexDirection::Column);
        self
    }

    /// Sets the flex direction of the element to `column-reverse`.
    /// [Docs](https://tailwindcss.com/docs/flex-direction#column-reverse)
    fn flex_col_reverse(mut self) -> Self {
        self.style().flex_direction = Some(FlexDirection::ColumnReverse);
        self
    }

    /// Sets the flex direction of the element to `row`.
    /// [Docs](https://tailwindcss.com/docs/flex-direction#row)
    fn flex_row(mut self) -> Self {
        self.style().flex_direction = Some(FlexDirection::Row);
        self
    }

    /// Sets the flex direction of the element to `row-reverse`.
    /// [Docs](https://tailwindcss.com/docs/flex-direction#row-reverse)
    fn flex_row_reverse(mut self) -> Self {
        self.style().flex_direction = Some(FlexDirection::RowReverse);
        self
    }

    /// Sets the element to allow a flex item to grow and shrink as needed, ignoring its initial size.
    /// [Docs](https://tailwindcss.com/docs/flex#flex-1)
    fn flex_1(mut self) -> Self {
        self.style().flex_grow = Some(1.);
        self.style().flex_shrink = Some(1.);
        self.style().flex_basis = Some(relative(0.).into());
        self
    }

    /// Sets the element to allow a flex item to grow and shrink, taking into account its initial size.
    /// [Docs](https://tailwindcss.com/docs/flex#auto)
    fn flex_auto(mut self) -> Self {
        self.style().flex_grow = Some(1.);
        self.style().flex_shrink = Some(1.);
        self.style().flex_basis = Some(Length::Auto);
        self
    }

    /// Sets the element to allow a flex item to shrink but not grow, taking into account its initial size.
    /// [Docs](https://tailwindcss.com/docs/flex#initial)
    fn flex_initial(mut self) -> Self {
        self.style().flex_grow = Some(0.);
        self.style().flex_shrink = Some(1.);
        self.style().flex_basis = Some(Length::Auto);
        self
    }

    /// Sets the element to prevent a flex item from growing or shrinking.
    /// [Docs](https://tailwindcss.com/docs/flex#none)
    fn flex_none(mut self) -> Self {
        self.style().flex_grow = Some(0.);
        self.style().flex_shrink = Some(0.);
        self
    }

    /// Sets the initial size of flex items for this element.
    /// [Docs](https://tailwindcss.com/docs/flex-basis)
    fn flex_basis(mut self, basis: impl Into<Length>) -> Self {
        self.style().flex_basis = Some(basis.into());
        self
    }

    /// Sets the element to allow a flex item to grow to fill any available space.
    /// [Docs](https://tailwindcss.com/docs/flex-grow)
    fn flex_grow(mut self) -> Self {
        self.style().flex_grow = Some(1.);
        self
    }

    /// Sets the element to allow a flex item to shrink if needed.
    /// [Docs](https://tailwindcss.com/docs/flex-shrink)
    fn flex_shrink(mut self) -> Self {
        self.style().flex_shrink = Some(1.);
        self
    }

    /// Sets the element to prevent a flex item from shrinking.
    /// [Docs](https://tailwindcss.com/docs/flex-shrink#dont-shrink)
    fn flex_shrink_0(mut self) -> Self {
        self.style().flex_shrink = Some(0.);
        self
    }

    /// Sets the element to allow flex items to wrap.
    /// [Docs](https://tailwindcss.com/docs/flex-wrap#wrap-normally)
    fn flex_wrap(mut self) -> Self {
        self.style().flex_wrap = Some(FlexWrap::Wrap);
        self
    }

    /// Sets the element wrap flex items in the reverse direction.
    /// [Docs](https://tailwindcss.com/docs/flex-wrap#wrap-reversed)
    fn flex_wrap_reverse(mut self) -> Self {
        self.style().flex_wrap = Some(FlexWrap::WrapReverse);
        self
    }

    /// Sets the element to prevent flex items from wrapping, causing inflexible items to overflow the container if necessary.
    /// [Docs](https://tailwindcss.com/docs/flex-wrap#dont-wrap)
    fn flex_nowrap(mut self) -> Self {
        self.style().flex_wrap = Some(FlexWrap::NoWrap);
        self
    }

    /// Sets the element to align flex items to the start of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-items#start)
    fn items_start(mut self) -> Self {
        self.style().align_items = Some(AlignItems::FlexStart);
        self
    }

    /// Sets the element to align flex items to the end of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-items#end)
    fn items_end(mut self) -> Self {
        self.style().align_items = Some(AlignItems::FlexEnd);
        self
    }

    /// Sets the element to align flex items along the center of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-items#center)
    fn items_center(mut self) -> Self {
        self.style().align_items = Some(AlignItems::Center);
        self
    }

    /// Stretches flex items across the container's cross axis.
    fn items_stretch(mut self) -> Self {
        self.style().align_items = Some(AlignItems::Stretch);
        self
    }

    /// Sets the element to align flex items along the baseline of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-items#baseline)
    fn items_baseline(mut self) -> Self {
        self.style().align_items = Some(AlignItems::Baseline);
        self
    }

    /// Sets the element to justify flex items against the start of the container's main axis.
    /// [Docs](https://tailwindcss.com/docs/justify-content#start)
    fn justify_start(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::Start);
        self
    }

    /// Sets the element to justify flex items against the end of the container's main axis.
    /// [Docs](https://tailwindcss.com/docs/justify-content#end)
    fn justify_end(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::End);
        self
    }

    /// Sets the element to justify flex items along the center of the container's main axis.
    /// [Docs](https://tailwindcss.com/docs/justify-content#center)
    fn justify_center(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::Center);
        self
    }

    /// Sets the element to justify flex items along the container's main axis
    /// such that there is an equal amount of space between each item.
    /// [Docs](https://tailwindcss.com/docs/justify-content#space-between)
    fn justify_between(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::SpaceBetween);
        self
    }

    /// Sets the element to justify items along the container's main axis such
    /// that there is an equal amount of space on each side of each item.
    /// [Docs](https://tailwindcss.com/docs/justify-content#space-around)
    fn justify_around(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::SpaceAround);
        self
    }

    /// Sets the element to distribute items so every gap, including the gaps at
    /// the container edges, has the same size.
    /// [Docs](https://tailwindcss.com/docs/justify-content#space-evenly)
    fn justify_evenly(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::SpaceEvenly);
        self
    }

    /// Aligns this flex or grid item to the start of its container's cross axis.
    fn self_start(mut self) -> Self {
        self.style().align_self = Some(AlignSelf::Start);
        self
    }

    /// Centers this flex or grid item on its container's cross axis.
    fn self_center(mut self) -> Self {
        self.style().align_self = Some(AlignSelf::Center);
        self
    }

    /// Aligns this flex or grid item to the end of its container's cross axis.
    fn self_end(mut self) -> Self {
        self.style().align_self = Some(AlignSelf::End);
        self
    }

    /// Stretches this flex or grid item across its container's cross axis.
    fn self_stretch(mut self) -> Self {
        self.style().align_self = Some(AlignSelf::Stretch);
        self
    }

    /// Sets the element to pack content items in their default position as if no align-content value was set.
    /// [Docs](https://tailwindcss.com/docs/align-content#normal)
    fn content_normal(mut self) -> Self {
        self.style().align_content = None;
        self
    }

    /// Sets the element to pack content items in the center of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-content#center)
    fn content_center(mut self) -> Self {
        self.style().align_content = Some(AlignContent::Center);
        self
    }

    /// Sets the element to pack content items against the start of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-content#start)
    fn content_start(mut self) -> Self {
        self.style().align_content = Some(AlignContent::FlexStart);
        self
    }

    /// Sets the element to pack content items against the end of the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-content#end)
    fn content_end(mut self) -> Self {
        self.style().align_content = Some(AlignContent::FlexEnd);
        self
    }

    /// Sets the element to pack content items along the container's cross axis
    /// such that there is an equal amount of space between each item.
    /// [Docs](https://tailwindcss.com/docs/align-content#space-between)
    fn content_between(mut self) -> Self {
        self.style().align_content = Some(AlignContent::SpaceBetween);
        self
    }

    /// Sets the element to pack content items along the container's cross axis
    /// such that there is an equal amount of space on each side of each item.
    /// [Docs](https://tailwindcss.com/docs/align-content#space-around)
    fn content_around(mut self) -> Self {
        self.style().align_content = Some(AlignContent::SpaceAround);
        self
    }

    /// Sets the element to pack content items along the container's cross axis
    /// such that there is an equal amount of space between each item.
    /// [Docs](https://tailwindcss.com/docs/align-content#space-evenly)
    fn content_evenly(mut self) -> Self {
        self.style().align_content = Some(AlignContent::SpaceEvenly);
        self
    }

    /// Sets the element to allow content items to fill the available space along the container's cross axis.
    /// [Docs](https://tailwindcss.com/docs/align-content#stretch)
    fn content_stretch(mut self) -> Self {
        self.style().align_content = Some(AlignContent::Stretch);
        self
    }

    /// Sets the background color of the element.
    fn bg<F>(mut self, fill: F) -> Self
    where
        F: Into<Fill>,
        Self: Sized,
    {
        self.style().background = Some(fill.into());
        self
    }

    /// Sets the border style of the element.
    fn border_dashed(mut self) -> Self {
        self.style().border_style = Some(BorderStyle::Dashed);
        self
    }

    /// Sets a gradient border for the element. When set, it takes precedence over
    /// any solid border color at paint time. Combine with a `border_*` width method.
    fn border_gradient(mut self, gradient: impl Into<Background>) -> Self {
        self.style().border_gradient = Some(gradient.into());
        self
    }

    /// Returns a mutable reference to the text style that has been configured on this element.
    fn text_style(&mut self) -> &mut Option<TextStyleRefinement> {
        let style: &mut StyleRefinement = self.style();
        &mut style.text
    }

    /// Sets the text color of this element.
    ///
    /// This value cascades to its child elements.
    fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.text_style().get_or_insert_with(Default::default).color = Some(color.into());
        self
    }

    /// Sets the font weight of this element
    ///
    /// This value cascades to its child elements.
    fn font_weight(mut self, weight: FontWeight) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_weight = Some(weight);
        self
    }

    /// Sets the background color of this element.
    ///
    /// This value cascades to its child elements.
    fn text_bg(mut self, bg: impl Into<Hsla>) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .background_color = Some(bg.into());
        self
    }

    /// Sets the text size of this element.
    ///
    /// This value cascades to its child elements.
    fn text_size(mut self, size: impl Into<AbsoluteLength>) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(size.into());
        self
    }

    /// Sets the text size to 'extra small'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_xs(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(0.75).into());
        self
    }

    /// Sets the text size to 'small'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_sm(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(0.875).into());
        self
    }

    /// Sets the text size to 'base'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_base(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(1.0).into());
        self
    }

    /// Sets the text size to 'large'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_lg(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(1.125).into());
        self
    }

    /// Sets the text size to 'extra large'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_xl(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(1.25).into());
        self
    }

    /// Sets the text size to 'extra extra large'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_2xl(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(1.5).into());
        self
    }

    /// Sets the text size to 'extra extra extra large'.
    /// [Docs](https://tailwindcss.com/docs/font-size#setting-the-font-size)
    fn text_3xl(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_size = Some(rems(1.875).into());
        self
    }

    /// Sets the font style of the element to italic.
    /// [Docs](https://tailwindcss.com/docs/font-style#italicizing-text)
    fn italic(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_style = Some(FontStyle::Italic);
        self
    }

    /// Sets the font style of the element to normal (not italic).
    /// [Docs](https://tailwindcss.com/docs/font-style#displaying-text-normally)
    fn not_italic(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_style = Some(FontStyle::Normal);
        self
    }

    /// Sets the text decoration to underline.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-line#underling-text)
    fn underline(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        style.underline = Some(UnderlineStyle {
            thickness: px(1.),
            ..Default::default()
        });
        self
    }

    /// Sets the decoration of the text to have a line through it.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-line#adding-a-line-through-text)
    fn line_through(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        style.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            ..Default::default()
        });
        self
    }

    /// Removes the text decoration on this element.
    ///
    /// This value cascades to its child elements.
    fn text_decoration_none(mut self) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .underline = None;
        self
    }

    /// Sets the color for the underline on this element
    fn text_decoration_color(mut self, color: impl Into<Hsla>) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.color = Some(color.into());
        self
    }

    /// Sets the text decoration style to a solid line.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-style)
    fn text_decoration_solid(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.wavy = false;
        self
    }

    /// Sets the text decoration style to a wavy line.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-style)
    fn text_decoration_wavy(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.wavy = true;
        self
    }

    /// Sets the text decoration to be 0px thick.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-thickness)
    fn text_decoration_0(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.thickness = px(0.);
        self
    }

    /// Sets the text decoration to be 1px thick.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-thickness)
    fn text_decoration_1(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.thickness = px(1.);
        self
    }

    /// Sets the text decoration to be 2px thick.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-thickness)
    fn text_decoration_2(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.thickness = px(2.);
        self
    }

    /// Sets the text decoration to be 4px thick.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-thickness)
    fn text_decoration_4(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.thickness = px(4.);
        self
    }

    /// Sets the text decoration to be 8px thick.
    /// [Docs](https://tailwindcss.com/docs/text-decoration-thickness)
    fn text_decoration_8(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        let underline = style.underline.get_or_insert_with(Default::default);
        underline.thickness = px(8.);
        self
    }

    /// Sets the font family of this element and its children.
    fn font_family(mut self, family_name: impl Into<SharedString>) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .font_family = Some(family_name.into());
        self
    }

    /// Sets the font of this element and its children.
    fn font(mut self, font: Font) -> Self {
        let Font {
            family,
            features,
            fallbacks,
            weight,
            style,
        } = font;

        let text_style = self.text_style().get_or_insert_with(Default::default);
        text_style.font_family = Some(family);
        text_style.font_features = Some(features);
        text_style.font_weight = Some(weight);
        text_style.font_style = Some(style);
        text_style.font_fallbacks = fallbacks;

        self
    }

    /// Sets the line height of this element and its children.
    fn line_height(mut self, line_height: impl Into<DefiniteLength>) -> Self {
        self.text_style()
            .get_or_insert_with(Default::default)
            .line_height = Some(line_height.into());
        self
    }

    /// Sets the opacity of this element and its children.
    fn opacity(mut self, opacity: f32) -> Self {
        self.style().opacity = Some(opacity);
        self
    }

    /// Sets clockwise rotation in degrees.
    fn rotate(mut self, angle_degrees: f32) -> Self {
        self.style().rotate = Some(angle_degrees.to_radians());
        self
    }

    /// Sets uniform scale factor.
    fn scale(mut self, factor: f32) -> Self {
        self.style().scale = Some(point(factor, factor));
        self
    }

    /// Sets non-uniform scale factors for x and y axes.
    fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.style().scale = Some(point(x, y));
        self
    }

    /// Sets the transform origin as a fraction of element size (0.0-1.0).
    /// Default is center (0.5, 0.5).
    fn transform_origin(mut self, x: f32, y: f32) -> Self {
        self.style().transform_origin = Some(point(x, y));
        self
    }

    /// Sets a translation transform along both axes, in pixels.
    fn translate(mut self, x: impl Into<Pixels>, y: impl Into<Pixels>) -> Self {
        self.style().translate = Some(point(x.into(), y.into()));
        self
    }

    /// Sets a horizontal translation transform, in pixels.
    fn translate_x(mut self, x: impl Into<Pixels>) -> Self {
        let current_y = self
            .style()
            .translate
            .map(|translate| translate.y)
            .unwrap_or_default();
        self.style().translate = Some(point(x.into(), current_y));
        self
    }

    /// Sets a vertical translation transform, in pixels.
    fn translate_y(mut self, y: impl Into<Pixels>) -> Self {
        let current_x = self
            .style()
            .translate
            .map(|translate| translate.x)
            .unwrap_or_default();
        self.style().translate = Some(point(current_x, y.into()));
        self
    }

    /// Sets a skew transform along the x axis, in degrees.
    fn skew_x(mut self, angle_degrees: f32) -> Self {
        let current_y = self.style().skew.map(|skew| skew.y).unwrap_or(0.0);
        self.style().skew = Some(point(angle_degrees.to_radians(), current_y));
        self
    }

    /// Sets a skew transform along the y axis, in degrees.
    fn skew_y(mut self, angle_degrees: f32) -> Self {
        let current_x = self.style().skew.map(|skew| skew.x).unwrap_or(0.0);
        self.style().skew = Some(point(current_x, angle_degrees.to_radians()));
        self
    }

    /// Desaturates this element and its subtree toward luminance. `0.0` leaves color
    /// untouched, `1.0` produces fully grayscale output.
    fn grayscale(mut self, amount: f32) -> Self {
        let mut filter = self.style().color_filter.unwrap_or_default();
        filter.grayscale = amount;
        self.style().color_filter = Some(filter);
        self
    }

    /// Adjusts the color saturation of this element and its subtree. `1.0` leaves
    /// saturation unchanged, `0.0` produces grayscale, values above `1.0` oversaturate.
    fn saturate(mut self, amount: f32) -> Self {
        let mut filter = self.style().color_filter.unwrap_or_default();
        filter.saturate = amount;
        self.style().color_filter = Some(filter);
        self
    }

    /// Adjusts the brightness of this element and its subtree. `1.0` leaves brightness
    /// unchanged, values below `1.0` darken and above `1.0` brighten.
    fn brightness(mut self, amount: f32) -> Self {
        let mut filter = self.style().color_filter.unwrap_or_default();
        filter.brightness = amount;
        self.style().color_filter = Some(filter);
        self
    }

    /// Adjusts the contrast of this element and its subtree around mid-gray. `1.0` leaves
    /// contrast unchanged, values below `1.0` reduce and above `1.0` increase contrast.
    fn contrast(mut self, amount: f32) -> Self {
        let mut filter = self.style().color_filter.unwrap_or_default();
        filter.contrast = amount;
        self.style().color_filter = Some(filter);
        self
    }

    /// Sets the grid columns of this element.
    fn grid_cols(mut self, cols: u16) -> Self {
        self.style().grid_cols = Some(cols);
        self
    }

    /// Sets the grid rows of this element.
    fn grid_rows(mut self, rows: u16) -> Self {
        self.style().grid_rows = Some(rows);
        self
    }

    /// Sets the column start of this element.
    fn col_start(mut self, start: i16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column.start = GridPlacement::Line(start);
        self
    }

    /// Sets the column start of this element to auto.
    fn col_start_auto(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column.start = GridPlacement::Auto;
        self
    }

    /// Sets the column end of this element.
    fn col_end(mut self, end: i16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column.end = GridPlacement::Line(end);
        self
    }

    /// Sets the column end of this element to auto.
    fn col_end_auto(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column.end = GridPlacement::Auto;
        self
    }

    /// Sets the column span of this element.
    fn col_span(mut self, span: u16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column = GridPlacement::Span(span)..GridPlacement::Span(span);
        self
    }

    /// Sets the row span of this element.
    fn col_span_full(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.column = GridPlacement::Line(1)..GridPlacement::Line(-1);
        self
    }

    /// Sets the row start of this element.
    fn row_start(mut self, start: i16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row.start = GridPlacement::Line(start);
        self
    }

    /// Sets the row start of this element to "auto"
    fn row_start_auto(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row.start = GridPlacement::Auto;
        self
    }

    /// Sets the row end of this element.
    fn row_end(mut self, end: i16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row.end = GridPlacement::Line(end);
        self
    }

    /// Sets the row end of this element to "auto"
    fn row_end_auto(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row.end = GridPlacement::Auto;
        self
    }

    /// Sets the row span of this element.
    fn row_span(mut self, span: u16) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row = GridPlacement::Span(span)..GridPlacement::Span(span);
        self
    }

    /// Sets the row span of this element.
    fn row_span_full(mut self) -> Self {
        let grid_location = self.style().grid_location_mut();
        grid_location.row = GridPlacement::Line(1)..GridPlacement::Line(-1);
        self
    }

    /// Sets the preferred aspect ratio (width divided by height) for this element.
    /// When one axis is definite and the other is automatic, the missing axis is
    /// derived from this ratio.
    /// [Docs](https://tailwindcss.com/docs/aspect-ratio)
    fn aspect_ratio(mut self, ratio: f32) -> Self {
        self.style().aspect_ratio = Some(ratio);
        self
    }

    /// Sets a 1:1 (square) aspect ratio.
    fn aspect_square(self) -> Self {
        self.aspect_ratio(1.0)
    }

    /// Sets a 16:9 (widescreen video) aspect ratio.
    fn aspect_video(self) -> Self {
        self.aspect_ratio(16.0 / 9.0)
    }

    /// Sets explicit column tracks for a grid container (CSS `grid-template-columns`).
    /// Takes precedence over [`Styled::grid_cols`] when non-empty.
    fn grid_template_columns(mut self, tracks: impl Into<Vec<GridTrack>>) -> Self {
        self.style().grid_template_columns = Some(tracks.into());
        self
    }

    /// Sets explicit row tracks for a grid container (CSS `grid-template-rows`).
    /// Takes precedence over [`Styled::grid_rows`] when non-empty.
    fn grid_template_rows(mut self, tracks: impl Into<Vec<GridTrack>>) -> Self {
        self.style().grid_template_rows = Some(tracks.into());
        self
    }

    /// Sets the grid auto-placement flow (CSS `grid-auto-flow`).
    fn grid_auto_flow(mut self, flow: GridAutoFlow) -> Self {
        self.style().grid_auto_flow = Some(flow);
        self
    }

    /// Flows grid auto-placed items into rows (CSS `grid-auto-flow: row`).
    fn grid_flow_row(self) -> Self {
        self.grid_auto_flow(GridAutoFlow::Row)
    }

    /// Flows grid auto-placed items into columns (CSS `grid-auto-flow: column`).
    fn grid_flow_col(self) -> Self {
        self.grid_auto_flow(GridAutoFlow::Column)
    }

    /// Flows grid auto-placed items into rows using the dense packing algorithm.
    fn grid_flow_row_dense(self) -> Self {
        self.grid_auto_flow(GridAutoFlow::RowDense)
    }

    /// Flows grid auto-placed items into columns using the dense packing algorithm.
    fn grid_flow_col_dense(self) -> Self {
        self.grid_auto_flow(GridAutoFlow::ColumnDense)
    }

    /// Aligns all grid items in the inline (row) axis within their grid areas (CSS `justify-items`).
    fn justify_items(mut self, value: AlignItems) -> Self {
        self.style().justify_items = Some(value);
        self
    }

    /// Packs grid items toward the inline-axis start of their grid areas.
    fn justify_items_start(self) -> Self {
        self.justify_items(AlignItems::Start)
    }

    /// Centers grid items in the inline axis of their grid areas.
    fn justify_items_center(self) -> Self {
        self.justify_items(AlignItems::Center)
    }

    /// Packs grid items toward the inline-axis end of their grid areas.
    fn justify_items_end(self) -> Self {
        self.justify_items(AlignItems::End)
    }

    /// Stretches grid items to fill the inline axis of their grid areas.
    fn justify_items_stretch(self) -> Self {
        self.justify_items(AlignItems::Stretch)
    }

    /// Aligns this grid item in the inline (row) axis within its grid area (CSS `justify-self`).
    fn justify_self(mut self, value: AlignItems) -> Self {
        self.style().justify_self = Some(value);
        self
    }

    /// Packs this grid item toward the inline-axis start of its grid area.
    fn justify_self_start(self) -> Self {
        self.justify_self(AlignItems::Start)
    }

    /// Centers this grid item in the inline axis of its grid area.
    fn justify_self_center(self) -> Self {
        self.justify_self(AlignItems::Center)
    }

    /// Packs this grid item toward the inline-axis end of its grid area.
    fn justify_self_end(self) -> Self {
        self.justify_self(AlignItems::End)
    }

    /// Stretches this grid item to fill the inline axis of its grid area.
    fn justify_self_stretch(self) -> Self {
        self.justify_self(AlignItems::Stretch)
    }

    /// Draws a debug border around this element.
    #[cfg(debug_assertions)]
    fn debug(mut self) -> Self {
        self.style().debug = Some(true);
        self
    }

    /// Draws a debug border on all conforming elements below this element.
    #[cfg(debug_assertions)]
    fn debug_below(mut self) -> Self {
        self.style().debug_below = Some(true);
        self
    }
}
