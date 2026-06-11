# Authoring Components

Most of Kael's UI is built by composing `div()` and the widgets in `kael_ui`. When a piece of UI repeats, or earns a name, you promote it to a component. This guide walks the ladder from the simplest approach to the most powerful — stop climbing the moment your needs are met.

## Rung 1: compose in `Render`

The plainest reuse is a helper method that returns `impl IntoElement`. No new types, no traits — just split a large `render` into named pieces:

```rust
impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .child(self.header())
            .child(self.body(cx))
    }
}

impl Dashboard {
    fn header(&self) -> impl IntoElement {
        div().h(px(48.0)).child(self.title.clone())
    }
}
```

Reach for a real component only when the markup needs to live outside this view — used by more than one screen, or configured through its own builder.

## Rung 2: a stateless component with `RenderOnce`

A `RenderOnce` component is a recipe: build it, configure it through chained methods, render it once. Derive `IntoElement` so it drops into any `.child()` like a built-in widget.

Here is a complete `Badge` — a small colored pill — modeled on the shape every simple `kael_ui` component shares:

```rust
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    subtle: bool,
    style: StyleRefinement,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            subtle: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn subtle(mut self, subtle: bool) -> Self {
        self.subtle = subtle;
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let user_style = self.style;

        let (bg, fg) = if self.subtle {
            (tokens.muted, tokens.muted_foreground)
        } else {
            (tokens.primary, tokens.primary_foreground)
        };

        div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(tokens.radius_sm)
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .font_family(tokens.font_family.clone())
            .child(self.label)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
```

Use it anywhere:

```rust
div().child(Badge::new("New")).child(Badge::new("Beta").subtle(true))
```

Four conventions are doing the work here, and every `kael_ui` component follows them:

1. **Builder methods take `mut self` and return `Self`** so calls chain.
2. **`#[derive(IntoElement)]` + `impl RenderOnce`** turns the struct into something `.child()` accepts.
3. **Read the theme with `Theme::of(cx)`** — never hardcode colors. Pull `tokens` once, then style from `tokens.primary`, `tokens.radius_sm`, and friends so the component tracks the active theme and any live theme switch.
4. **Carry a `StyleRefinement` and apply it last** via `impl Styled` plus the closing `.map(...)`, so callers can override your defaults with `.bg(...)`, `.w(...)`, and the rest.

`RenderOnce` receives `cx: &mut App` (not `Context<Self>`): a recipe has no persistent identity, so it cannot `notify` itself. Wire interactivity through callbacks (`on_click`, `on_change`) that update an entity the caller owns — see how `kael_ui`'s controls thread an `Entity` through their handlers.

## Rung 3: a stateful component with `Render`

When a component owns state that changes over its lifetime — an open/closed flag, a scroll position, a cached value — make it an `Entity` with `impl Render`. Now `render` takes `&mut self` and `Context<Self>`, so it can mutate itself and call `cx.notify()`:

```rust
struct Disclosure {
    open: bool,
    title: SharedString,
}

impl Render for Disclosure {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();

        div()
            .id("disclosure")
            .transition(Theme::of(cx).tokens.transition_fast)
            .child(self.title.clone())
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    this.open = !this.open;
                    cx.notify();
                });
            })
            .when(self.open, |this| this.child("…details…"))
    }
}
```

The caller creates it with `cx.new(|_| Disclosure { open: false, title: "Advanced".into() })` and holds the `Entity<Disclosure>`. See [State Management](state-management.md) for the full story on entities, observation, and shared state.

### Transition conventions

Whenever a component restyles on `hover`, `active`, or a state change, ease it. Add `.transition(duration)` to the element — it must have a stable `.id(...)`, since unkeyed elements snap. Match `kael_ui`'s convention:

- **`tokens.transition_fast`** for color-only changes — background, text, border tint.
- **`tokens.transition_base`** for changes that move or resize — shadow steps, a hover lift, scale.

```rust
div()
    .id("row")
    .transition(Theme::of(cx).tokens.transition_fast)
    .hover(|style| style.bg(Theme::of(cx).tokens.accent))
```

[Animations](animations.md) covers the full set of interpolated properties, explicit `with_animation` timelines, and springs.

## Rung 4: custom drawing with `canvas`

For visuals that styled divs cannot express — waveforms, charts, custom paint — drop to the canvas. `canvas(prepaint, paint)` and `canvas_with_prepaint(prepaint, paint)` hand you the element's `Bounds` and the `Window`, and you paint quads, paths, and shadows directly. The prepaint closure runs first and can return data the paint closure reuses, keeping per-frame work cheap.

The `Waveform` component in `crates/kael_ui/src/components/waveform.rs` is the pattern to copy: it builds a `paint_data` struct in prepaint, then a free `paint_waveform(bounds, &data, window)` function does the drawing. Wrap the canvas in a positioned `div()` so layout still owns sizing:

```rust
div()
    .relative()
    .h(px(48.0))
    .child(
        canvas_with_prepaint(
            move |_bounds, _window, _cx| paint_data,
            move |bounds, data, window, _cx| paint_waveform(bounds, &data, window),
        )
        .absolute()
        .inset_0()
        .size_full(),
    )
```

## You rarely need the raw `Element` trait

Beneath everything is the `Element` trait with its `request_layout` / `prepaint` / `paint` lifecycle. `RenderOnce`, `Render`, and `canvas` are all built on top of it, and they cover the overwhelming majority of components. Implement `Element` by hand only when you need to control layout participation itself — a custom layout container, or an element that measures children and positions them manually. If you are reaching for it to draw or to hold state, one of the rungs above is the better fit.
