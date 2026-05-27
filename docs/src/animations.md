# Animations

Kael drives animations from its render-on-demand loop: an animating element requests frames only while it is in flight, then the window returns to idle (0% CPU). There are two layers — explicit, time-driven animations you attach to any element, and the framework's built-in motion such as elastic scrolling.

## Animating an element

Bring the `AnimationExt` trait into scope and call `with_animation` on any element. You give it a stable id, an `Animation` describing the timeline, and an animator closure that receives the element and the eased progress `delta` in `0.0..=1.0`:

```rust
use std::time::Duration;
use kael::{Animation, AnimationExt as _, Transformation, bounce, ease_in_out, percentage, svg};

svg()
    .size_20()
    .path(ARROW_CIRCLE_SVG)
    .with_animation(
        "spinner",
        Animation::new(Duration::from_secs(2))
            .repeat_forever()
            .with_easing(bounce(ease_in_out)),
        |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
    )
```

## The `Animation` timeline

```rust
use kael::{Animation, Easing, Repeat};

Animation::new(Duration::from_millis(400))
    .delay(Duration::from_millis(100))   // wait before starting
    .easing(Easing::EaseInOut)           // pick a curve (see below)
    .repeat(Repeat::Count(3));           // Once | Count(n) | Forever
```

`repeat_forever()` is shorthand for `repeat(Repeat::Forever)`, and `with_easing(f)` accepts any `Fn(f32) -> f32` (including the helpers `ease_in_out`, `ease_out_quint()`, and `bounce(inner)`).

## Easing curves

The `Easing` enum covers the common curves plus a physical spring:

| Variant | Curve |
|---------|-------|
| `Easing::Linear` | constant rate |
| `Easing::EaseIn` / `EaseOut` / `EaseInOut` | quadratic |
| `Easing::CubicBezier(x1, y1, x2, y2)` | CSS-style cubic Bézier |
| `Easing::Spring { stiffness, damping, mass }` | damped spring |
| `Easing::Custom(Rc<dyn Fn(f32) -> f32>)` | your own |

```rust
Animation::new(Duration::from_millis(600))
    .easing(Easing::Spring { stiffness: 180.0, damping: 12.0, mass: 1.0 });
```

## Keyframes and sequences

For multi-stop transitions across common styled properties, build a `Keyframes` set and attach it with `with_keyframes`:

```rust
use kael::{Animation, AnimationExt as _, Keyframes};

div().with_keyframes(
    "pulse",
    Keyframes::new()
        .at(0.0, |k| k.opacity(0.4))
        .at(0.5, |k| k.opacity(1.0))
        .at(1.0, |k| k.opacity(0.4)),
    Animation::new(Duration::from_secs(1)).repeat_forever(),
)
```

Chain whole animations with `AnimationSequence::new().then(...).then_for(duration).with_overlap(...)` and drive them with `with_animation_sequence`. For animations you may need to interrupt, `with_cancellable_animation` returns an `(element, AnimationHandle)`; call `handle.cancel()` to jump to the final state.

## Elastic scrolling

Scrollable regions — `overflow_*_scroll()` containers, `uniform_list`, and `list` (via `ListState`) — get native rubber-band overscroll automatically on macOS: content stretches past its bounds on a trackpad pull and springs back on release. Use a `ScrollHandle` to read or set the offset programmatically:

```rust
use kael::{ScrollHandle, point, px};

let scroll = ScrollHandle::new();
scroll.set_offset(point(px(-360.0), px(0.0)));
let current = scroll.offset();        // Point<Pixels>
let max = scroll.max_offset();        // Size<Pixels>
```

See `examples/animation.rs` and `examples/elastic_scrolling.rs` for runnable demos.
