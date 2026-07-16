use std::time::Duration;

use crate::{
    AnyElement, Context, IntoElement, ParentElement, Render, SharedString, Styled, Timer,
    WeakEntity, Window, WindowAppearance, div, hsla, px,
};

/// Position where toasts appear on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastPosition {
    /// Top-right corner of the window.
    #[default]
    TopRight,
    /// Bottom-right corner of the window.
    BottomRight,
    /// Top-center of the window.
    TopCenter,
}

impl ToastPosition {
    /// Stable text key for diagnostics and generated tests.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::TopRight => "top_right",
            Self::BottomRight => "bottom_right",
            Self::TopCenter => "top_center",
        }
    }
}

/// Configuration for a single toast notification.
#[derive(Clone)]
pub struct Toast {
    title: SharedString,
    body: Option<SharedString>,
    duration: Duration,
    position: ToastPosition,
}

impl Toast {
    /// Create a new toast with the given title.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: None,
            duration: Duration::from_secs(3),
            position: ToastPosition::default(),
        }
    }

    /// Set the body text of the toast.
    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set how long the toast should be displayed before auto-dismissing.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set the screen position where the toast appears.
    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// Returns true when body text is configured.
    pub fn has_body(&self) -> bool {
        self.body.is_some()
    }

    /// Length of the configured title in bytes, without exposing title text.
    pub fn title_len_bytes(&self) -> usize {
        self.title.len()
    }

    /// Length of the configured body in bytes, without exposing body text.
    pub fn body_len_bytes(&self) -> usize {
        self.body.as_ref().map_or(0, |body| body.len())
    }

    /// Coarse duration class for content-safe diagnostics.
    pub fn duration_class(&self) -> &'static str {
        match self.duration {
            duration if duration.is_zero() => "instant",
            duration if duration <= Duration::from_secs(2) => "short",
            duration if duration <= Duration::from_secs(8) => "normal",
            _ => "long",
        }
    }

    /// Configured toast position.
    pub fn position_key(&self) -> &'static str {
        self.position.to_text()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "toast(title_len_bytes={}, has_body={}, body_len_bytes={}, duration_class={}, position={})",
            self.title_len_bytes(),
            self.has_body(),
            self.body_len_bytes(),
            self.duration_class(),
            self.position_key()
        )
    }
}

struct ToastEntry {
    toast: Toast,
}

/// A stack of toast notifications that manages display and auto-dismissal.
///
/// Create a `ToastStack` as a GPUI entity and render it as part of your
/// window's view tree. Use [`ToastStack::push`] to add new toasts.
pub struct ToastStack {
    toasts: Vec<ToastEntry>,
    position: ToastPosition,
}

impl ToastStack {
    /// Create a new empty toast stack with the default position.
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            position: ToastPosition::default(),
        }
    }

    /// Set the default position for toasts in this stack.
    pub fn with_position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// Push a new toast onto the stack and schedule its auto-dismissal.
    pub fn push(&mut self, toast: Toast, window: &Window, cx: &mut Context<Self>) {
        let duration = toast.duration;
        self.toasts.push(ToastEntry { toast });
        cx.notify();

        let index = self.toasts.len() - 1;
        cx.spawn_in(window, async move |this: WeakEntity<Self>, cx| {
            Timer::after(duration).await;
            this.update(cx, |stack, cx| {
                if index < stack.toasts.len() {
                    stack.toasts.remove(index);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Remove all toasts from the stack.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.toasts.clear();
        cx.notify();
    }

    fn is_dark_appearance(window: &Window) -> bool {
        matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        )
    }
}

impl Render for ToastStack {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let is_dark = Self::is_dark_appearance(window);
        let position = self.position;

        let mut container = div().flex().flex_col().gap_2().p_4().max_w(px(360.0));

        match position {
            ToastPosition::TopRight => {
                container = container.absolute().top_0().right_0();
            }
            ToastPosition::BottomRight => {
                container = container.absolute().bottom_0().right_0();
            }
            ToastPosition::TopCenter => {
                container = container.absolute().top_0().left_auto().right_auto();
            }
        }

        let children: Vec<AnyElement> = self
            .toasts
            .iter()
            .map(|entry| render_toast_item(&entry.toast, is_dark))
            .collect();

        for child in children {
            container = container.child(child);
        }

        container
    }
}

fn render_toast_item(toast: &Toast, is_dark: bool) -> AnyElement {
    let bg_color = if is_dark {
        hsla(0.0, 0.0, 0.1, 0.92)
    } else {
        hsla(0.0, 0.0, 0.0, 0.85)
    };

    let text_color = if is_dark {
        hsla(0.0, 0.0, 0.95, 1.0)
    } else {
        hsla(0.0, 0.0, 1.0, 1.0)
    };

    let secondary_text_color = if is_dark {
        hsla(0.0, 0.0, 0.7, 1.0)
    } else {
        hsla(0.0, 0.0, 0.85, 1.0)
    };

    let title = toast.title.clone();
    let body = toast.body.clone();

    let mut toast_div = div()
        .flex()
        .flex_col()
        .gap_1()
        .py(px(12.0))
        .px(px(16.0))
        .rounded(px(8.0))
        .bg(bg_color)
        .shadow_lg()
        .max_w(px(320.0))
        .min_w(px(200.0))
        .text_color(text_color)
        .text_sm()
        .child(div().font_weight(crate::FontWeight::SEMIBOLD).child(title));

    if let Some(body_text) = body {
        toast_div = toast_div.child(
            div()
                .text_xs()
                .text_color(secondary_text_color)
                .child(body_text),
        );
    }

    toast_div.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_summary_is_content_safe() {
        let toast = Toast::new("Secret sync complete")
            .body("Private workspace finished")
            .duration(Duration::from_secs(12))
            .position(ToastPosition::BottomRight);

        assert_eq!(ToastPosition::BottomRight.to_text(), "bottom_right");
        assert!(toast.has_body());
        assert_eq!(toast.title_len_bytes(), "Secret sync complete".len());
        assert_eq!(toast.body_len_bytes(), "Private workspace finished".len());
        assert_eq!(toast.duration_class(), "long");
        assert_eq!(toast.position_key(), "bottom_right");

        let summary = toast.to_text();
        assert!(summary.contains("title_len_bytes=20"));
        assert!(summary.contains("has_body=true"));
        assert!(summary.contains("body_len_bytes=26"));
        assert!(summary.contains("duration_class=long"));
        assert!(summary.contains("position=bottom_right"));
        assert!(!summary.contains("Secret"));
        assert!(!summary.contains("Private"));
        assert!(!summary.contains("12"));
    }
}
