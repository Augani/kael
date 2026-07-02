# Canvas & Graphics

Beyond the element tree, Kael gives you direct GPU drawing: an immediate-mode canvas, a vector path builder, gradients, backdrop blur, SVG, and Lottie playback. Everything renders through the same per-platform pipeline (Metal / DirectX 11 / Vulkan) with device-pixel snapping for crisp output at any DPI.

## Visual escape-hatch ladder

When porting an Electron app or giving an AI agent a graphics task, choose the
lowest rung that solves the problem:

| Need | Use today | Notes |
| --- | --- | --- |
| Product UI, dashboards, tool chrome | styled `div()` / `kael_ui` | Best memory and startup profile |
| Charts, timelines, waveform views, custom controls | `canvas(...)`, `paint_quad`, `paint_path`, `PathBuilder` | Native immediate-mode drawing |
| Icons, diagrams, generated vector assets | `svg()` / `PathBuilder` | Keep assets inspectable and themeable |
| Motion graphics and loaders | `lottie(...)` | Decodes off the UI path |
| Frosted or filtered subtrees | `backdrop_blur(...)` / `effect_layer(...)` | Effect layers are partial CSS-filter coverage, not arbitrary shaders |
| Browser-only graphics such as WebGL/WebGPU demos | `webview(id, url)` | Treat as a WebView island with native Kael chrome around it |
| Golden-image or benchmark evidence | `HeadlessRenderer` / `golden` | Off-screen rendering is for tests and measurements |
| Native custom render target or custom shader | roadmap | Do not claim WebGL/WebGPU parity yet |

The public `graphics_capability_report()` API exposes this same truth for
readiness checks and agent planning. It reports full native coverage for styled
elements, canvas, paths, gradients, SVG, and Lottie; partial coverage for clip
shapes, effect layers, and headless rendering; WebView coverage for browser
graphics fallback; and roadmap status for public render targets/custom shaders.

## Canvas

`canvas` takes two closures — a prepaint pass (compute layout/state, returns a value) and a paint pass (draw into the bounds). Inside paint you call `window.paint_quad` and `window.paint_path`:

```rust
use kael::{canvas, fill, quad, px, rgb, Bounds, Pixels, Window, App};

canvas(
    move |_bounds: Bounds<Pixels>, _window: &mut Window, _app: &mut App| {
        // prepaint: return any state the paint pass needs
    },
    move |bounds: Bounds<Pixels>, _state, window: &mut Window, _app: &mut App| {
        window.paint_quad(fill(bounds, rgb(0x1e1e1e)));
        // window.paint_path(path, color);
    },
)
.size_full()
```

## Vector paths

Build filled or stroked paths with `PathBuilder`, then hand the result to `window.paint_path`:

```rust
use kael::{PathBuilder, point, px};

let mut builder = PathBuilder::fill();        // or PathBuilder::stroke(px(2.0))
builder.move_to(point(px(50.0), px(50.0)));
builder.line_to(point(px(130.0), px(50.0)));
builder.curve_to(point(px(130.0), px(130.0)), point(px(160.0), px(90.0))); // quadratic
builder.close();
let path = builder.build()?;
```

Segment methods: `move_to`, `line_to`, `curve_to` (quadratic Bézier), `cubic_curve_to`, `arc_to`, and `close`. Stroked builders also accept `dash_array` / `dash_offset`.

## Gradients

Gradients are backgrounds you pass to `.bg(...)`:

```rust
use kael::{linear_gradient, linear_color_stop, rgb};

div().bg(linear_gradient(
    45.0,
    linear_color_stop(rgb(0xff0080), 0.0),
    linear_color_stop(rgb(0x7928ca), 1.0),
))
```

Also available: `multi_stop_linear_gradient(angle, &[stops])`, `radial_gradient(cx, cy, radius, &[stops])`, and `conic_gradient(cx, cy, angle_offset, &[stops])`.

## Backdrop blur & frosted glass

`backdrop_blur` blurs whatever is painted behind an element — combine it with a translucent background for a frosted-glass panel:

```rust
use kael::{px, rgba};

div()
    .backdrop_blur(px(20.0))
    .bg(rgba(0xffffff20))
    .rounded_xl()
```

## SVG

`svg()` renders a vector asset; `text_color` fills monochrome SVGs and `with_transformation` applies rotation/scale:

```rust
use kael::{svg, px, rgb};

svg().path("icons/logo.svg").size(px(24.0)).text_color(rgb(0x2563eb))
```

## Lottie

`lottie()` plays Lottie/dotLottie animations, decoding frames on a background thread so the UI stays responsive:

```rust
use kael::{lottie, LoopMode};

lottie("animations/loader.json")
    .autoplay()
    .loop_forever()            // or .loop_mode(LoopMode::Loop) / .ping_pong()
```

Builders: `.autoplay()`, `.loop_forever()`, `.loop_mode(LoopMode)`, `.ping_pong()`, `.object_fit(ObjectFit)`, `.prefetch_frames(n)`, `.with_loading(|| element)`, `.with_fallback(|| element)`.

See `examples/painting.rs`, `gradient.rs`, `pattern.rs`, `shadow.rs`, `svg/main.rs`, and `gif_viewer.rs`.
