//! Text component - Typography with theming and semantic variants.

use std::sync::Arc;

use crate::theme::{use_theme, Theme, ThemeTokens};
use kael::{prelude::FluentBuilder as _, *};

/// Text variants for semantic typography
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextVariant {
    /// Extra large heading (32px, bold)
    H1,
    /// Large heading (28px, semibold)
    H2,
    /// Medium heading (24px, semibold)
    H3,
    /// Small heading (20px, semibold)
    H4,
    /// Extra small heading (18px, medium)
    H5,
    /// Tiny heading (16px, medium)
    H6,
    /// Body text - large (16px, regular)
    BodyLarge,
    /// Body text - default (14px, regular)
    Body,
    /// Body text - small (13px, regular)
    BodySmall,
    /// Caption text (12px, regular)
    Caption,
    /// Label text (14px, medium)
    Label,
    /// Label text - small (12px, medium)
    LabelSmall,
    /// Code/monospace text (14px, mono font)
    Code,
    /// Code/monospace - small (12px, mono font)
    CodeSmall,
    /// Display headline 1 (42px, regular)
    Display1,
    /// Display headline 2 (35px, regular)
    Display2,
    /// Display headline 3 (29px, regular)
    Display3,
    /// Custom - use size(), weight(), etc. for full control
    Custom,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextType {
    #[default]
    Body,
    Large,
    Label,
    Supporting,
    Code,
    Display1,
    Display2,
    Display3,
    Inherit,
}

impl TextType {
    fn variant(self) -> TextVariant {
        match self {
            Self::Body | Self::Supporting | Self::Inherit => TextVariant::Body,
            Self::Large => TextVariant::BodyLarge,
            Self::Label => TextVariant::Label,
            Self::Code => TextVariant::Code,
            Self::Display1 => TextVariant::Display1,
            Self::Display2 => TextVariant::Display2,
            Self::Display3 => TextVariant::Display3,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextSize {
    FourXs,
    ThreeXs,
    TwoXs,
    Xsm,
    Sm,
    Base,
    Lg,
    Xl,
    TwoXl,
    ThreeXl,
    FourXl,
}

impl TextSize {
    fn pixels(self) -> Pixels {
        match self {
            Self::FourXs => px(10.0),
            Self::ThreeXs => px(11.0),
            Self::TwoXs => px(12.0),
            Self::Xsm => px(13.0),
            Self::Sm => px(14.0),
            Self::Base => px(16.0),
            Self::Lg => px(18.0),
            Self::Xl => px(20.0),
            Self::TwoXl => px(24.0),
            Self::ThreeXl => px(30.0),
            Self::FourXl => px(36.0),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextWeight {
    Normal,
    Medium,
    Semibold,
    Bold,
}

impl TextWeight {
    fn font_weight(self) -> FontWeight {
        match self {
            Self::Normal => FontWeight::NORMAL,
            Self::Medium => FontWeight::MEDIUM,
            Self::Semibold => FontWeight::SEMIBOLD,
            Self::Bold => FontWeight::BOLD,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextColor {
    #[default]
    Primary,
    Secondary,
    Disabled,
    Placeholder,
    Active,
    Inherit,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextDisplay {
    #[default]
    Inline,
    Block,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum WordBreak {
    #[default]
    BreakWord,
    BreakAll,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextWrap {
    #[default]
    Wrap,
    Nowrap,
    Balance,
    Pretty,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextJustify {
    #[default]
    Start,
    Center,
    End,
}

impl TextVariant {
    /// Get the text size for this variant
    pub fn size(&self) -> Pixels {
        match self {
            Self::H1 => px(24.0),
            Self::H2 => px(20.0),
            Self::H3 => px(17.0),
            Self::H4 => px(14.0),
            Self::H5 => px(12.0),
            Self::H6 => px(11.0),
            Self::BodyLarge => px(17.0),
            Self::Body => px(14.0),
            Self::BodySmall => px(13.0),
            Self::Caption => px(12.0),
            Self::Label => px(14.0),
            Self::LabelSmall => px(12.0),
            Self::Code => px(14.0),
            Self::CodeSmall => px(12.0),
            Self::Display1 => px(42.0),
            Self::Display2 => px(35.0),
            Self::Display3 => px(29.0),
            Self::Custom => px(14.0), // Default for custom
        }
    }

    /// Get the font weight for this variant
    pub fn weight(&self) -> FontWeight {
        match self {
            Self::H1 | Self::H2 | Self::H3 | Self::H4 | Self::H5 | Self::H6 => FontWeight::SEMIBOLD,
            Self::Label | Self::LabelSmall => FontWeight::MEDIUM,
            Self::BodyLarge | Self::Body | Self::BodySmall | Self::Caption => FontWeight::NORMAL,
            Self::Code | Self::CodeSmall => FontWeight::NORMAL,
            Self::Display1 | Self::Display2 | Self::Display3 => FontWeight::NORMAL,
            Self::Custom => FontWeight::NORMAL,
        }
    }

    /// Check if this variant uses monospace font
    pub fn is_mono(&self) -> bool {
        matches!(self, Self::Code | Self::CodeSmall)
    }

    /// Get line height multiplier for this variant
    pub fn line_height(&self) -> f32 {
        match self {
            Self::H1 => 1.3333,
            Self::H2 => 1.4,
            Self::H3 => 1.4118,
            Self::H4 => 1.4286,
            Self::H5 => 1.6667,
            Self::H6 => 1.6,
            Self::BodyLarge => 1.4118,
            Self::Body | Self::BodySmall => 1.4286,
            Self::Caption | Self::Label | Self::Code | Self::CodeSmall => 1.4286,
            Self::LabelSmall => 1.6667,
            Self::Display1 => 1.2381,
            Self::Display2 => 1.2571,
            Self::Display3 => 1.2414,
            Self::Custom => 1.5,
        }
    }
}

/// Text component with automatic theming and typography
#[derive(IntoElement)]
pub struct Text {
    content: SharedString,
    variant: TextVariant,
    size: Option<Pixels>,
    weight: Option<FontWeight>,
    color: Option<Hsla>,
    semantic_color: Option<TextColor>,
    font: Option<SharedString>,
    line_height: Option<f32>,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    wrap: bool,
    truncate: bool,
    display: TextDisplay,
    word_break: WordBreak,
    text_wrap: TextWrap,
    justify: TextJustify,
    tabular_numbers: bool,
    max_lines: Option<usize>,
    style: StyleRefinement,
}

impl Text {
    /// Create new text with content
    pub fn new<S: Into<SharedString>>(content: S) -> Self {
        Self {
            content: content.into(),
            variant: TextVariant::Body,
            size: None,
            weight: None,
            color: None,
            semantic_color: None,
            font: None,
            line_height: None,
            italic: false,
            underline: false,
            strikethrough: false,
            wrap: true,
            truncate: false,
            display: TextDisplay::Inline,
            word_break: WordBreak::default(),
            text_wrap: TextWrap::Wrap,
            justify: TextJustify::Start,
            tabular_numbers: false,
            max_lines: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set the text variant (heading, body, etc.)
    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the ASTRYX semantic text type.
    pub fn text_type(mut self, text_type: TextType) -> Self {
        self.variant = text_type.variant();
        if text_type == TextType::Supporting {
            self.semantic_color = Some(TextColor::Secondary);
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn textType(self, text_type: TextType) -> Self {
        self.text_type(text_type)
    }

    /// Set custom font size (overrides variant size)
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = Some(size);
        self
    }

    /// Set the ASTRYX token size override.
    pub fn text_size(mut self, size: TextSize) -> Self {
        self.size = Some(size.pixels());
        self
    }

    #[allow(non_snake_case)]
    pub fn textSize(self, size: TextSize) -> Self {
        self.text_size(size)
    }

    /// Set custom font weight (overrides variant weight)
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Set the ASTRYX semantic font weight.
    pub fn text_weight(mut self, weight: TextWeight) -> Self {
        self.weight = Some(weight.font_weight());
        self
    }

    #[allow(non_snake_case)]
    pub fn textWeight(self, weight: TextWeight) -> Self {
        self.text_weight(weight)
    }

    /// Set text color (overrides theme foreground)
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the ASTRYX semantic text color.
    pub fn text_color(mut self, color: TextColor) -> Self {
        self.semantic_color = Some(color);
        self
    }

    #[allow(non_snake_case)]
    pub fn textColor(self, color: TextColor) -> Self {
        self.text_color(color)
    }

    /// Set custom font family (overrides theme font)
    pub fn font(mut self, font: impl Into<SharedString>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Set custom line height multiplier
    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = Some(line_height);
        self
    }

    /// Make text italic
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Add underline
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Add strikethrough
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    pub fn has_strikethrough(self, has_strikethrough: bool) -> Self {
        if has_strikethrough {
            self.strikethrough()
        } else {
            self
        }
    }

    #[allow(non_snake_case)]
    pub fn hasStrikethrough(self, has_strikethrough: bool) -> Self {
        self.has_strikethrough(has_strikethrough)
    }

    pub fn has_tabular_numbers(mut self, tabular_numbers: bool) -> Self {
        self.tabular_numbers = tabular_numbers;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasTabularNumbers(self, tabular_numbers: bool) -> Self {
        self.has_tabular_numbers(tabular_numbers)
    }

    pub fn display(mut self, display: TextDisplay) -> Self {
        self.display = display;
        self
    }

    pub fn word_break(mut self, word_break: WordBreak) -> Self {
        self.word_break = word_break;
        self
    }

    #[allow(non_snake_case)]
    pub fn wordBreak(self, word_break: WordBreak) -> Self {
        self.word_break(word_break)
    }

    pub fn text_wrap(mut self, text_wrap: TextWrap) -> Self {
        self.text_wrap = text_wrap;
        if matches!(text_wrap, TextWrap::Nowrap) {
            self.wrap = false;
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn textWrap(self, text_wrap: TextWrap) -> Self {
        self.text_wrap(text_wrap)
    }

    pub fn justify(mut self, justify: TextJustify) -> Self {
        self.justify = justify;
        self
    }

    /// Disable text wrapping (single line)
    pub fn no_wrap(mut self) -> Self {
        self.wrap = false;
        self
    }

    /// Enable text truncation with ellipsis
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self.wrap = false; // Truncate requires no wrap
        self
    }

    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        if max_lines == 1 {
            self = self.truncate();
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn maxLines(self, max_lines: usize) -> Self {
        self.max_lines(max_lines)
    }

    /// Get the effective text size
    fn effective_size(&self) -> Pixels {
        self.size.unwrap_or_else(|| self.variant.size())
    }

    /// Get the effective font weight
    fn effective_weight(&self) -> FontWeight {
        self.weight.unwrap_or_else(|| self.variant.weight())
    }

    /// Get the effective line height
    fn effective_line_height(&self) -> f32 {
        self.line_height
            .unwrap_or_else(|| self.variant.line_height())
    }

    fn effective_color(&self, tokens: &ThemeTokens) -> Hsla {
        if let Some(color) = self.color {
            return color;
        }

        match self.semantic_color.unwrap_or(TextColor::Primary) {
            TextColor::Primary | TextColor::Inherit => tokens.foreground,
            TextColor::Secondary => tokens.muted_foreground,
            TextColor::Disabled => tokens.muted_foreground.opacity(0.55),
            TextColor::Placeholder => tokens.muted_foreground.opacity(0.72),
            TextColor::Active => tokens.primary,
        }
    }
}

impl Styled for Text {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Text {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;

        let size = self.effective_size();
        let weight = self.effective_weight();
        let line_height = self.effective_line_height();
        let text_color = self.effective_color(tokens);

        let font_family = if let Some(font) = self.font {
            font
        } else if self.variant.is_mono() {
            tokens.font_mono.clone()
        } else {
            tokens.font_family.clone()
        };

        let mut base = div();
        *base.style() = self.style;

        let needs_highlights = self.italic || self.strikethrough;
        let styled_text = if needs_highlights {
            let mut highlight_style = HighlightStyle::default();

            if self.italic {
                highlight_style.font_style = Some(FontStyle::Italic);
            }
            if self.strikethrough {
                highlight_style.strikethrough = Some(StrikethroughStyle {
                    color: Some(text_color),
                    thickness: px(1.0),
                });
            }

            let text_len = self.content.len();
            StyledText::new(self.content.clone())
                .with_highlights(vec![(0..text_len, highlight_style)])
        } else {
            StyledText::new(self.content.clone())
        };

        base.font_family(font_family.clone())
            .text_size(size)
            .font_weight(weight)
            .text_color(text_color)
            .line_height(relative(line_height))
            .when(self.display == TextDisplay::Block, |this| this.block())
            .when(self.underline, |this| this.underline())
            .when(self.tabular_numbers, |this| {
                this.font(Font {
                    family: font_family.clone(),
                    features: FontFeatures(Arc::new(vec![("tnum".into(), 1)])),
                    fallbacks: None,
                    weight,
                    style: FontStyle::default(),
                })
            })
            .when(self.justify == TextJustify::Center, |this| {
                this.text_center()
            })
            .when(self.justify == TextJustify::End, |this| this.text_right())
            .when(!self.wrap, |this| this.whitespace_nowrap())
            .when(matches!(self.text_wrap, TextWrap::Nowrap), |this| {
                this.whitespace_nowrap()
            })
            .when_some(self.max_lines.filter(|lines| *lines > 1), |this, lines| {
                this.line_clamp(lines)
            })
            .when(self.truncate, |this| this.overflow_hidden().text_ellipsis())
            .child(styled_text)
    }
}

/// Create heading 1 text
pub fn h1<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H1)
}

/// Create heading 2 text
pub fn h2<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H2)
}

/// Create heading 3 text
pub fn h3<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H3)
}

/// Create heading 4 text
pub fn h4<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H4)
}

/// Create heading 5 text
pub fn h5<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H5)
}

/// Create heading 6 text
pub fn h6<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::H6)
}

/// Create body text (default)
pub fn body<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::Body)
}

/// Create large body text
pub fn body_large<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::BodyLarge)
}

/// Create small body text
pub fn body_small<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::BodySmall)
}

/// Create caption text
pub fn caption<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::Caption)
}

/// Create label text
pub fn label<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::Label)
}

/// Create small label text
pub fn label_small<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::LabelSmall)
}

/// Create code/monospace text
pub fn code<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::Code)
}

/// Create small code text
pub fn code_small<S: Into<SharedString>>(content: S) -> Text {
    Text::new(content).variant(TextVariant::CodeSmall)
}

/// Create muted text (secondary color)
pub fn muted<S: Into<SharedString>>(content: S) -> Text {
    let theme = use_theme();
    Text::new(content)
        .variant(TextVariant::Body)
        .color(theme.tokens.muted_foreground)
}

/// Create muted small text
pub fn muted_small<S: Into<SharedString>>(content: S) -> Text {
    let theme = use_theme();
    Text::new(content)
        .variant(TextVariant::BodySmall)
        .color(theme.tokens.muted_foreground)
}
