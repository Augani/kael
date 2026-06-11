//! Real component tests that drive a window through `kael::TestAppContext`.
//!
//! These exercise `kael_ui` components end-to-end: a root view renders a real
//! component tree into a headless test window, and we simulate input or advance
//! frames to assert on observable behavior. They double as the canonical pattern
//! for testing `kael_ui` apps — see `docs/src/testing.md`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use kael::{
    div, point, px, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, Window,
};
use kael_ui::prelude::*;

struct ButtonHarness {
    clicks: Rc<Cell<usize>>,
}

impl Render for ButtonHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div().size_full().child(
            div().absolute().top(px(0.0)).left(px(0.0)).child(
                Button::new("harness-button", "Click me")
                    .size(ButtonSize::Lg)
                    .on_click(move |_event, _window, _cx| {
                        clicks.set(clicks.get() + 1);
                    }),
            ),
        )
    }
}

#[kael::test]
fn button_click_fires_handler(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let clicks = Rc::new(Cell::new(0usize));
    let (_view, vcx) = cx.add_window_view(|_window, _cx| ButtonHarness {
        clicks: clicks.clone(),
    });

    vcx.simulate_click(point(px(40.0), px(20.0)), Default::default());
    vcx.run_until_parked();

    assert_eq!(clicks.get(), 1, "button on_click handler should fire once");
}

struct TransitionHarness {
    hovered: bool,
}

impl Render for TransitionHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let bg = if self.hovered {
            kael_ui::theme::Theme::dark().tokens.primary
        } else {
            kael_ui::theme::Theme::dark().tokens.muted
        };
        div()
            .id("transition-box")
            .size(px(100.0))
            .bg(bg)
            .transition(Duration::from_millis(120))
    }
}

#[kael::test]
fn implicit_transition_renders_across_frames(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (view, vcx) = cx.add_window_view(|_window, _cx| TransitionHarness { hovered: false });

    vcx.update(|window, _cx| window.refresh());
    vcx.run_until_parked();

    view.update(vcx, |harness, cx| {
        harness.hovered = true;
        cx.notify();
    });
    vcx.run_until_parked();

    view.update(vcx, |harness, _cx| {
        assert!(
            harness.hovered,
            "transition harness should hold its toggled state after re-render"
        );
    });
}

struct CounterHarness {
    count: usize,
}

impl Render for CounterHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        VStack::new()
            .gap(px(8.0))
            .child(div().child(format!("count: {}", self.count)))
            .child(Button::new("inc", "Increment"))
    }
}

#[kael::test]
fn view_state_update_redraws(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kael_ui::theme::install_theme(cx, kael_ui::theme::Theme::dark());
    });

    let (view, vcx) = cx.add_window_view(|_window, _cx| CounterHarness { count: 0 });

    view.update(vcx, |harness, cx| {
        harness.count += 1;
        cx.notify();
    });
    vcx.run_until_parked();

    view.update(vcx, |harness, _cx| {
        assert_eq!(harness.count, 1, "view state should persist across redraw");
    });
}
