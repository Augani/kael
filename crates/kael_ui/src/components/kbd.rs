use crate::theme::use_theme;
use kael::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KBDSize {
    Sm,
    #[default]
    Md,
    Lg,
}

pub struct KBD {
    keys: SharedString,
    size: KBDSize,
    style: StyleRefinement,
}

pub type Kbd = KBD;
pub type KbdProps = KBD;

impl KBD {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            keys: label.into(),
            size: KBDSize::default(),
            style: StyleRefinement::default(),
        }
    }

    pub fn keys(mut self, keys: impl Into<SharedString>) -> Self {
        self.keys = keys.into();
        self
    }

    pub fn size(mut self, size: KBDSize) -> Self {
        self.size = size;
        self
    }

    fn key_parts(&self) -> Vec<SharedString> {
        self.keys
            .split('+')
            .map(|key| display_key(key.trim()))
            .collect()
    }
}

impl Styled for KBD {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for KBD {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let parts = self.key_parts();
        let user_style = self.style;

        let (text_size, px_val, height, min_w) = match self.size {
            KBDSize::Sm => (px(11.0), px(4.0), px(18.0), px(16.0)),
            KBDSize::Md => (px(12.0), px(4.0), px(20.0), px(20.0)),
            KBDSize::Lg => (px(13.0), px(6.0), px(24.0), px(24.0)),
        };

        let mut el = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .flex_shrink_0()
            .children(parts.into_iter().map(|part| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px_val)
                    .h(height)
                    .min_w(min_w)
                    .bg(theme.tokens.secondary)
                    .border_b_2()
                    .border_color(theme.tokens.input)
                    .rounded(theme.tokens.radius_sm)
                    .text_size(text_size)
                    .font_family(theme.tokens.font_family.clone())
                    .text_color(theme.tokens.muted_foreground)
                    .font_weight(FontWeight::MEDIUM)
                    .line_height(px(20.0))
                    .child(part)
            }));
        el.style().refine(&user_style);
        el
    }
}

fn display_key(key: &str) -> SharedString {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "mod" | "cmd" | "command" => "⌘".into(),
        "ctrl" | "control" => "⌃".into(),
        "alt" | "option" => "⌥".into(),
        "shift" => "⇧".into(),
        "enter" | "return" => "↵".into(),
        "backspace" => "⌫".into(),
        "escape" | "esc" => "Esc".into(),
        "tab" => "⇥".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "plus" => "+".into(),
        "" => "".into(),
        _ => key.to_uppercase().into(),
    }
}
