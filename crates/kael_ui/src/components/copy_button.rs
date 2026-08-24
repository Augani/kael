use crate::{components::icon::Icon, theme::Theme};
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
            Timer::after(Duration::from_secs(2)).await;
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
        let render_theme = theme.clone();
        let label: SharedString = if copied { "Copied" } else { "Copy" }.into();

        button(self.id)
            .label(label.clone())
            .on_click(move |_, window, cx| {
                state.update(cx, |s, cx| s.copy(window, cx));
            })
            .render_with(move |render_state, _, _| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(render_theme.tokens.radius_md)
                    .border_1()
                    .border_color(render_theme.tokens.border)
                    .bg(render_theme.tokens.card)
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if copied {
                        render_theme.tokens.success
                    } else {
                        render_theme.tokens.foreground
                    })
                    .transition(render_theme.tokens.transition_fast)
                    .hover(|style| style.bg(render_theme.tokens.accent))
                    .when(render_state.focused, |this| {
                        this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                            render_theme.tokens.ring,
                        )])
                    })
                    .child(
                        Icon::new(if copied { "check" } else { "copy" })
                            .size(px(14.0))
                            .color(if copied {
                                render_theme.tokens.success
                            } else {
                                render_theme.tokens.muted_foreground
                            }),
                    )
                    .child(StyledText::new(label.clone()).accessibility_hidden(true))
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div.into_any_element()
                    })
            })
    }
}

impl Styled for CopyButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
