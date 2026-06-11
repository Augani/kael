# Animations

Kael drives animations from its render-on-demand loop: an animating element requests frames only while it is in flight, then the window returns to idle (0% CPU). There are three layers — implicit transitions that ease style changes automatically, explicit time-driven animations you attach to any element, and the framework's built-in motion such as elastic scrolling.

## Implicit transitions

The web's "soft" feel comes from `transition: all 150ms ease`; Kael's equivalent is `.transition(duration)` on any element with a stable id. Whenever the element's computed style changes — hover, active, focus, or a state-driven restyle — the change is interpolated instead of snapping:

```rust
use std::time::Duration;

div()
    .id("cta")
    .bg(theme.tokens.primary)
    .rounded(px(10.))
    .transition(Duration::from_millis(150))
    .hover(|style| style.bg(theme.tokens.accent).rounded(px(16.)))
    .active(|style| style.scale(0.97))
```

Animated properties: background (including gradients with matching stop counts), border color, text color, opacity, corner radii, box shadows, rotation, and scale. `transition_with(duration, easing)` takes an explicit easing curve; transitions interrupt cleanly, retargeting from the current visual state. kael_ui's controls ship with this wired to the `transition_fast` theme token.

## Layout (FLIP) animation

`.animate_layout(duration)` makes a keyed element glide to its new position when layout moves it — list reorders, grid changes, sidebar toggles:

```rust
div().id(item.id).animate_layout(Duration::from_millis(350))
```

Avoid it on children of containers that scroll mid-animation; scrolling moves the element and restarts the glide. See `examples/soft_ui.rs`.

## Springs and gestures

For physics-driven motion, `kael_ui` provides `SpringValue`/`SpringPoint` (real spring integration with velocity, presets from `SpringPreset`) and `DraggableSpring`, a container you can drag and throw: on release, the pan gesture's velocity hands off to the spring, which settles to the nearest snap point. See `crates/kael_ui/examples/drag_spring.rs`.

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

`kael::Easing` is the single, canonical easing vocabulary for the workspace. It
covers the full standard curve set in named variants:

| Variant | Curve |
|---------|-------|
| `Easing::Linear` | constant rate |
| `Easing::EaseIn` / `EaseOut` / `EaseInOut` | quadratic |
| `Easing::EaseInCubic` / `EaseOutCubic` / `EaseInOutCubic` | cubic |
| `Easing::EaseInQuart` / `EaseOutQuart` / `EaseInOutQuart` | quartic |
| `Easing::EaseInQuint` / `EaseOutQuint` / `EaseInOutQuint` | quintic |
| `Easing::EaseInExpo` / `EaseOutExpo` / `EaseInOutExpo` | exponential |
| `Easing::EaseInCirc` / `EaseOutCirc` / `EaseInOutCirc` | circular |
| `Easing::EaseInBack(overshoot)` / `EaseOutBack(overshoot)` / `EaseInOutBack(overshoot)` | backing overshoot |
| `Easing::EaseInElastic` / `EaseOutElastic` / `Elastic` | elastic |
| `Easing::Steps(n)` | `n` discrete steps |
| `Easing::CubicBezier(x1, y1, x2, y2)` | CSS-style cubic Bézier |
| `Easing::Custom(Rc<dyn Fn(f32) -> f32>)` | your own |

```rust
Animation::new(Duration::from_millis(600))
    .easing(Easing::EaseOutBack(1.70158));
```

For a smooth physical spring, `kael_ui` provides `SpringValue`/`SpringPoint`
(see [Springs and gestures](#springs-and-gestures)).

### `kael_ui` compatibility shims

`kael_ui::animations::easings` exposes the same curves as free `fn(f32) -> f32`
functions (`ease_out_cubic`, `ease_in_back`, `steps(n)`, …). These are
compatibility shims that delegate to the `Easing` variants above; prefer the
`Easing` variants directly in new code. The `spring`, `smooth_spring`, and
`cubic_bezier` helpers have no `Easing` variant and remain defined in that
module.

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
