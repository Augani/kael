use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::theme::Theme;

pub struct TypeWriterState {
    full_text: SharedString,
    visible_count: usize,
    is_typing: bool,
    version: usize,
    speed: Duration,
    on_complete: Option<Box<dyn Fn(&mut App)>>,
}

impl TypeWriterState {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            full_text: text.into(),
            visible_count: 0,
            is_typing: false,
            version: 0,
            speed: Duration::from_millis(50),
            on_complete: None,
        }
    }

    pub fn with_speed(mut self, speed: Duration) -> Self {
        self.speed = speed;
        self
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.visible_count = 0;
        self.is_typing = true;
        self.version += 1;
        let version = self.version;
        let total_chars = self.full_text.chars().count();
        let speed = self.speed;

        if cx.reduce_motion() {
            self.visible_count = total_chars;
            self.is_typing = false;
            if let Some(cb) = self.on_complete.take() {
                cb(cx);
            }
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            for i in 1..=total_chars {
                cx.background_executor().timer(speed).await;
                let should_stop = this
                    .update(cx, |state, cx| {
                        if state.version != version {
                            return true;
                        }
                        state.visible_count = i;
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);

                if should_stop {
                    return;
                }
            }

            _ = this.update(cx, |state, cx| {
                if state.version != version {
                    return;
                }
                state.is_typing = false;
                if let Some(cb) = state.on_complete.take() {
                    cb(cx);
                }
                cx.notify();
            });
        })
        .detach();

        cx.notify();
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.version += 1;
        self.visible_count = 0;
        self.is_typing = false;
        cx.notify();
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.version += 1;
        self.full_text = text.into();
        self.visible_count = 0;
        self.is_typing = false;
        cx.notify();
    }

    pub fn on_complete(mut self, handler: impl Fn(&mut App) + 'static) -> Self {
        self.on_complete = Some(Box::new(handler));
        self
    }

    pub fn visible_text(&self) -> &str {
        let byte_end = self
            .full_text
            .char_indices()
            .nth(self.visible_count)
            .map(|(i, _)| i)
            .unwrap_or(self.full_text.len());
        &self.full_text[..byte_end]
    }

    pub fn full_text(&self) -> &str {
        &self.full_text
    }

    pub fn is_typing(&self) -> bool {
        self.is_typing
    }

    pub fn is_complete(&self) -> bool {
        self.visible_count >= self.full_text.chars().count()
    }
}

#[derive(IntoElement)]
pub struct TypeWriter {
    id: ElementId,
    base: Div,
    state: Entity<TypeWriterState>,
    show_cursor: bool,
    cursor_char: char,
}

impl TypeWriter {
    pub fn new(id: impl Into<ElementId>, state: Entity<TypeWriterState>) -> Self {
        Self {
            id: id.into(),
            base: div(),
            state,
            show_cursor: true,
            cursor_char: '|',
        }
    }

    pub fn cursor(mut self, show: bool) -> Self {
        self.show_cursor = show;
        self
    }

    pub fn cursor_char(mut self, ch: char) -> Self {
        self.cursor_char = ch;
        self
    }
}

impl Styled for TypeWriter {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for TypeWriter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let foreground = Theme::of(cx).tokens.foreground;
        let state = self.state.read(cx);
        let text = String::from(state.visible_text());
        let accessible_text = if state.full_text().trim().is_empty() {
            "Typewriter text".to_string()
        } else {
            state.full_text().to_string()
        };
        let is_typing = state.is_typing();
        let cursor_str = SharedString::from(String::from(self.cursor_char));

        self.base
            .id(self.id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::StaticText).label(accessible_text),
            )
            .flex()
            .flex_row()
            .text_color(foreground)
            .child(StyledText::new(text).accessibility_hidden(true))
            .when(
                self.show_cursor && (is_typing || !state.is_complete()),
                |el| {
                    el.child(
                        div()
                            .id(ElementId::NamedChild(Box::new(self.id), "cursor".into()))
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Group)
                                    .states(AccessibilityState::HIDDEN),
                            )
                            .child(cursor_str)
                            .with_animation(
                                "cursor-blink",
                                Animation::new(Duration::from_millis(530))
                                    .repeat_forever()
                                    .with_easing(kael::linear),
                                |el, delta| {
                                    if delta < 0.5 {
                                        el.opacity(1.0)
                                    } else {
                                        el.opacity(0.0)
                                    }
                                },
                            ),
                    )
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn reduced_motion_reveals_text_without_typing_animation() {
        let mut cx = TestAppContext::single();
        cx.set_reduce_motion(true);
        let state = cx.new(|_| TypeWriterState::new("Accessible motion"));

        cx.update(|cx| {
            state.update(cx, |state, cx| state.start(cx));
            let state = state.read(cx);
            assert_eq!(state.visible_text(), "Accessible motion");
            assert!(state.is_complete());
            assert!(!state.is_typing());
        });
    }
}
