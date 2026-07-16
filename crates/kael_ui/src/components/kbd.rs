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

    fn accessibility_label(&self) -> String {
        let label = self
            .keys
            .split('+')
            .map(|key| accessible_key(key.trim()))
            .filter(|key| !key.is_empty())
            .collect::<Vec<_>>()
            .join(" + ");
        if label.is_empty() {
            "Keyboard shortcut".to_string()
        } else {
            label
        }
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
        let accessibility_label = self.accessibility_label();
        let user_style = self.style;

        let (text_size, px_val, height, min_w) = match self.size {
            KBDSize::Sm => (px(11.0), px(4.0), px(18.0), px(16.0)),
            KBDSize::Md => (px(12.0), px(4.0), px(20.0), px(20.0)),
            KBDSize::Lg => (px(13.0), px(6.0), px(24.0), px(24.0)),
        };

        let mut el = div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::StaticText)
                    .label(accessibility_label),
            )
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
                    .child(StyledText::new(part).accessibility_hidden(true))
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

fn accessible_key(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "mod" | "cmd" | "command" => "Command".to_string(),
        "ctrl" | "control" => "Control".to_string(),
        "alt" | "option" => "Option".to_string(),
        "shift" => "Shift".to_string(),
        "enter" | "return" => "Enter".to_string(),
        "backspace" => "Backspace".to_string(),
        "escape" | "esc" => "Escape".to_string(),
        "tab" => "Tab".to_string(),
        "up" => "Up arrow".to_string(),
        "down" => "Down arrow".to_string(),
        "left" => "Left arrow".to_string(),
        "right" => "Right arrow".to_string(),
        "plus" => "Plus".to_string(),
        "" => String::new(),
        key => key.to_ascii_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn shortcut_accessibility_uses_spoken_key_names() {
        assert_eq!(
            KBD::new("mod+shift+p").accessibility_label(),
            "Command + Shift + P"
        );
        assert_eq!(KBD::new(" ").accessibility_label(), "Keyboard shortcut");
        assert_eq!(accessible_key("left"), "Left arrow");
    }
}
