//! Heading component - semantic ASTRYX heading typography.

use crate::components::text::{Text, TextDisplay, TextJustify, TextVariant, TextWrap, WordBreak};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HeadingLevel {
    #[default]
    H2,
    H1,
    H3,
    H4,
    H5,
    H6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingType {
    Display1,
    Display2,
    Display3,
}

impl HeadingLevel {
    fn variant(self) -> TextVariant {
        match self {
            Self::H1 => TextVariant::H1,
            Self::H2 => TextVariant::H2,
            Self::H3 => TextVariant::H3,
            Self::H4 => TextVariant::H4,
            Self::H5 => TextVariant::H5,
            Self::H6 => TextVariant::H6,
        }
    }

    fn number(self) -> usize {
        match self {
            Self::H1 => 1,
            Self::H2 => 2,
            Self::H3 => 3,
            Self::H4 => 4,
            Self::H5 => 5,
            Self::H6 => 6,
        }
    }
}

#[derive(IntoElement)]
pub struct Heading {
    content: SharedString,
    level: HeadingLevel,
    heading_type: Option<HeadingType>,
    color: Option<Hsla>,
    truncate: bool,
    display: TextDisplay,
    max_lines: Option<usize>,
    word_break: WordBreak,
    text_wrap: TextWrap,
    justify: TextJustify,
    has_strikethrough: bool,
    style: StyleRefinement,
}

impl Heading {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            level: HeadingLevel::default(),
            heading_type: None,
            color: None,
            truncate: false,
            display: TextDisplay::Block,
            max_lines: None,
            word_break: WordBreak::default(),
            text_wrap: TextWrap::Wrap,
            justify: TextJustify::Start,
            has_strikethrough: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn level(mut self, level: HeadingLevel) -> Self {
        self.level = level;
        self
    }

    pub fn heading_type(mut self, heading_type: HeadingType) -> Self {
        self.heading_type = Some(heading_type);
        self
    }

    #[allow(non_snake_case)]
    pub fn headingType(self, heading_type: HeadingType) -> Self {
        self.heading_type(heading_type)
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn truncate(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    pub fn display(mut self, display: TextDisplay) -> Self {
        self.display = display;
        self
    }

    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        if max_lines == 1 {
            self.truncate = true;
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn maxLines(self, max_lines: usize) -> Self {
        self.max_lines(max_lines)
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

    pub fn has_strikethrough(mut self, has_strikethrough: bool) -> Self {
        self.has_strikethrough = has_strikethrough;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasStrikethrough(self, has_strikethrough: bool) -> Self {
        self.has_strikethrough(has_strikethrough)
    }
}

impl Styled for Heading {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Heading {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let semantic_level = self.level.number();
        let mut text = Text::new(self.content)
            .variant(
                self.heading_type
                    .map(|heading_type| match heading_type {
                        HeadingType::Display1 => TextVariant::Display1,
                        HeadingType::Display2 => TextVariant::Display2,
                        HeadingType::Display3 => TextVariant::Display3,
                    })
                    .unwrap_or_else(|| self.level.variant()),
            )
            .display(self.display)
            .word_break(self.word_break)
            .text_wrap(self.text_wrap)
            .justify(self.justify)
            .semantic_heading_level(semantic_level)
            .has_strikethrough(self.has_strikethrough);
        if let Some(color) = self.color {
            text = text.color(color);
        }
        if let Some(max_lines) = self.max_lines {
            text = text.max_lines(max_lines);
        }
        if self.truncate {
            text = text.truncate();
        }
        text.map(|this| {
            let mut text = this;
            text.style().refine(&self.style);
            text
        })
    }
}
