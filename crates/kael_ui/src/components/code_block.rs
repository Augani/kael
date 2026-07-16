use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;

use crate::{
    components::copy_button::{CopyButton, CopyButtonState},
    theme::Theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeBlockCopyState {
    #[default]
    Idle,
    Copied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Keyword,
    StringLiteral,
    Comment,
    Number,
    Plain,
}

#[derive(IntoElement)]
pub struct CodeBlock {
    id: ElementId,
    base: Div,
    code: SharedString,
    language: Option<SharedString>,
    show_line_numbers: bool,
    show_copy_button: bool,
    highlight_lines: Vec<usize>,
    max_height: Option<Pixels>,
}

impl CodeBlock {
    #[track_caller]
    pub fn new(code: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "code-block:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            base: div(),
            code: code.into(),
            language: None,
            show_line_numbers: true,
            show_copy_button: true,
            highlight_lines: Vec::new(),
            max_height: None,
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn language(mut self, lang: impl Into<SharedString>) -> Self {
        self.language = Some(lang.into());
        self
    }

    pub fn show_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    #[allow(non_snake_case)]
    pub fn showLineNumbers(self, show: bool) -> Self {
        self.show_line_numbers(show)
    }

    pub fn highlight_lines(mut self, lines: Vec<usize>) -> Self {
        self.highlight_lines = lines.into_iter().filter(|line| *line > 0).collect();
        self.highlight_lines.sort_unstable();
        self.highlight_lines.dedup();
        self
    }

    #[allow(non_snake_case)]
    pub fn highlightLines(self, lines: Vec<usize>) -> Self {
        self.highlight_lines(lines)
    }

    pub fn max_height(mut self, height: Pixels) -> Self {
        let value = f32::from(height);
        if value.is_finite() && value > 0.0 {
            self.max_height = Some(height);
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn maxHeight(self, height: Pixels) -> Self {
        self.max_height(height)
    }

    pub fn show_copy_button(mut self, show: bool) -> Self {
        self.show_copy_button = show;
        self
    }

    #[allow(non_snake_case)]
    pub fn showCopyButton(self, show: bool) -> Self {
        self.show_copy_button(show)
    }
}

impl RenderOnce for CodeBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let lines: Vec<&str> = self.code.split('\n').collect();
        let is_rust = self
            .language
            .as_ref()
            .map(|l| l.as_ref() == "rust" || l.as_ref() == "rs")
            .unwrap_or(false);
        let language_label = self
            .language
            .as_ref()
            .filter(|language| !language.trim().is_empty())
            .map(|language| match language.as_ref() {
                "rs" | "rust" => "Rust".to_string(),
                other => other.to_string(),
            });
        let group_label = language_label
            .map(|language| format!("{language} code block"))
            .unwrap_or_else(|| "Code block".to_string());
        let accessible_code = if self.code.trim().is_empty() {
            "Empty code block".to_string()
        } else {
            self.code.to_string()
        };

        let keyword_color = theme.tokens.primary;
        let string_color = hsla(0.4, 0.7, 0.5, 1.0);
        let comment_color = theme.tokens.muted_foreground;
        let number_color = hsla(0.08, 0.7, 0.6, 1.0);
        let plain_color = theme.tokens.foreground;
        let line_number_color = theme.tokens.muted_foreground;
        let highlight_bg = theme.tokens.muted.opacity(0.5);

        let gutter_width = px(40.0);

        let code_for_copy = self.code.clone();
        let show_copy = self.show_copy_button;
        let id = self.id.clone();

        let mut outer = self
            .base
            .id(id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label(group_label),
            )
            .relative()
            .bg(theme.tokens.muted.opacity(0.3))
            .rounded(theme.tokens.radius_md)
            .font_family(theme.tokens.font_mono.clone())
            .text_size(px(13.0))
            .overflow_hidden();

        let max_h = self.max_height;

        if show_copy {
            let copy_state_id = ElementId::NamedChild(Box::new(id.clone()), "copy-state".into());
            let copy_state = window.use_keyed_state(copy_state_id, cx, |_, _| {
                CopyButtonState::new(code_for_copy.clone())
            });
            copy_state.update(cx, |state, _| state.set_text(code_for_copy.clone()));
            let copy_btn = CopyButton::new(
                ElementId::NamedChild(Box::new(id.clone()), "copy".into()),
                copy_state,
            )
            .absolute()
            .top(px(8.0))
            .right(px(8.0))
            .h(px(28.0));
            outer = outer.child(copy_btn);
        }

        let mut content = div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .states(AccessibilityState::HIDDEN),
            )
            .flex()
            .flex_col()
            .py(px(12.0));

        for (idx, line_text) in lines.iter().enumerate() {
            let line_num = idx + 1;
            let is_highlighted = self.highlight_lines.contains(&line_num);

            let mut row = div().flex().flex_row().px(px(12.0));

            if is_highlighted {
                row = row.bg(highlight_bg);
            }

            if self.show_line_numbers {
                row = row.child(
                    div()
                        .w(gutter_width)
                        .flex_shrink_0()
                        .text_color(line_number_color)
                        .text_size(px(12.0))
                        .text_right()
                        .pr(px(12.0))
                        .child(format!("{}", line_num)),
                );
            }

            let mut code_row = div().flex().flex_row().flex_1().min_w_0();
            let tokens = tokenize(line_text, is_rust);

            for (kind, text) in tokens {
                let color = match kind {
                    TokenKind::Keyword => keyword_color,
                    TokenKind::StringLiteral => string_color,
                    TokenKind::Comment => comment_color,
                    TokenKind::Number => number_color,
                    TokenKind::Plain => plain_color,
                };
                code_row = code_row.child(div().text_color(color).child(text.to_string()));
            }

            row = row.child(code_row);
            content = content.child(row);
        }

        outer.child(
            div()
                .id(ElementId::NamedChild(Box::new(id), "scroll".into()))
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::StaticText)
                        .label(accessible_code),
                )
                .overflow_x_scroll()
                .when_some(max_h, |this, height| this.max_h(height).overflow_y_scroll())
                .child(content),
        )
    }
}

fn tokenize(line: &str, is_rust: bool) -> Vec<(TokenKind, &str)> {
    let mut tokens = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        if pos + 1 < len && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            tokens.push((TokenKind::Comment, &line[pos..]));
            return tokens;
        }

        if bytes[pos] == b'"' {
            let start = pos;
            pos += 1;
            while pos < len && bytes[pos] != b'"' {
                if bytes[pos] == b'\\' && pos + 1 < len {
                    pos += 1;
                }
                pos += 1;
            }
            if pos < len {
                pos += 1;
            }
            tokens.push((TokenKind::StringLiteral, &line[start..pos]));
            continue;
        }

        if bytes[pos] == b'\'' && is_rust {
            let start = pos;
            pos += 1;
            if pos < len && bytes[pos] == b'\\' && pos + 1 < len {
                pos += 2;
            } else if pos < len {
                pos += 1;
            }
            if pos < len && bytes[pos] == b'\'' {
                pos += 1;
                tokens.push((TokenKind::StringLiteral, &line[start..pos]));
                continue;
            }
            pos = start + 1;
            tokens.push((TokenKind::Plain, &line[start..start + 1]));
            continue;
        }

        if bytes[pos].is_ascii_digit()
            || (bytes[pos] == b'-' && pos + 1 < len && bytes[pos + 1].is_ascii_digit())
        {
            let start = pos;
            if bytes[pos] == b'-' {
                pos += 1;
            }
            while pos < len
                && (bytes[pos].is_ascii_digit() || bytes[pos] == b'.' || bytes[pos] == b'_')
            {
                pos += 1;
            }
            if pos < len && (bytes[pos] == b'e' || bytes[pos] == b'E') {
                pos += 1;
                if pos < len && (bytes[pos] == b'+' || bytes[pos] == b'-') {
                    pos += 1;
                }
                while pos < len && bytes[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            tokens.push((TokenKind::Number, &line[start..pos]));
            continue;
        }

        if bytes[pos].is_ascii_alphabetic() || bytes[pos] == b'_' {
            let start = pos;
            while pos < len && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'_') {
                pos += 1;
            }
            let word = &line[start..pos];
            if is_rust && is_rust_keyword(word) {
                tokens.push((TokenKind::Keyword, word));
            } else {
                tokens.push((TokenKind::Plain, word));
            }
            continue;
        }

        if bytes[pos] == b' ' {
            let start = pos;
            while pos < len && bytes[pos] == b' ' {
                pos += 1;
            }
            tokens.push((TokenKind::Plain, &line[start..pos]));
            continue;
        }

        let start = pos;
        pos += 1;
        tokens.push((TokenKind::Plain, &line[start..pos]));
    }

    tokens
}

fn is_rust_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "let"
            | "mut"
            | "pub"
            | "struct"
            | "enum"
            | "impl"
            | "use"
            | "mod"
            | "if"
            | "else"
            | "for"
            | "while"
            | "match"
            | "return"
            | "self"
            | "Self"
            | "crate"
            | "super"
            | "true"
            | "false"
            | "async"
            | "await"
            | "move"
            | "ref"
            | "where"
            | "type"
            | "trait"
            | "const"
            | "static"
            | "loop"
            | "break"
            | "continue"
            | "in"
            | "as"
            | "unsafe"
            | "dyn"
            | "extern"
    )
}

impl Styled for CodeBlock {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for CodeBlock {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for CodeBlock {}

impl ParentElement for CodeBlock {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn code_block_normalizes_highlights_and_invalid_height() {
        let block = CodeBlock::new("one\ntwo")
            .highlight_lines(vec![2, 0, 2, 1])
            .max_height(px(f32::NAN));
        assert_eq!(block.highlight_lines, vec![1, 2]);
        assert_eq!(block.max_height, None);

        let block = block.max_height(px(-12.0)).max_height(px(180.0));
        assert_eq!(block.max_height, Some(px(180.0)));
    }
}
