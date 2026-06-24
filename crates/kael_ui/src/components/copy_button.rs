use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

pub struct CopyButtonState {
    copied: bool,
    text: SharedString,
}

impl CopyButtonState {
    pub fn new(text: SharedString) -> Self {
        Self {
            copied: false,
            text,
        }
    }

    pub fn set_text(&mut self, text: SharedString) {
        self.text = text;
    }

    pub fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.text.to_string()));
        self.copied = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            smol::Timer::after(Duration::from_secs(2)).await;
            let _ = this.update(cx, |state, cx| {
                state.copied = false;
                cx.notify();
            });
        })
        .detach();
    }
}

#[derive(IntoElement)]
pub struct CopyButton {
    state: Entity<CopyButtonState>,
    id: ElementId,
    style: StyleRefinement,
}

impl CopyButton {
    pub fn new(id: impl Into<ElementId>, state: Entity<CopyButtonState>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
        }
    }
}

impl RenderOnce for CopyButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let copied = self.state.read(cx).copied;
        let state = self.state.clone();
        let user_style = self.style;

        div()
            .id(self.id)
            .flex()
            .items_center()
            .justify_center()
            .h(px(24.0))
            .px(px(8.0))
            .rounded(theme.tokens.radius_sm)
            .cursor_pointer()
            .text_size(px(12.0))
            .text_color(theme.tokens.muted_foreground)
            .transition(theme.tokens.transition_fast)
            .hover(|s| s.bg(theme.tokens.muted))
            .active(|s| s.opacity(0.7))
            .when(copied, |el| el.text_color(theme.tokens.success))
            .on_click(move |_, window, cx| {
                state.update(cx, |s, cx| s.copy(window, cx));
            })
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .child(if copied { "\u{2713}" } else { "\u{1F4CB}" })
    }
}

impl Styled for CopyButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
