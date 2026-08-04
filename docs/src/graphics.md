# Canvas & Graphics

Beyond the element tree, Kael gives you direct GPU drawing: an immediate-mode canvas, a vector path builder, gradients, backdrop blur, SVG, and optional Lottie playback. Everything renders through the same per-platform pipeline (Metal / DirectX 11 / Vulkan) with device-pixel snapping for crisp output at any DPI.

## Visual escape-hatch ladder

When designing a graphics-heavy workflow or giving an AI agent a rendering task,
choose the lowest rung that solves the problem:

| Need | Use today | Notes |
| --- | --- | --- |
| Product UI, dashboards, tool chrome | styled `div()` / `kael_ui` | Best memory and startup profile |
| Charts, timelines, waveform views, custom controls | `canvas(...)`, `paint_quad`, `paint_path`, `PathBuilder` | Native immediate-mode drawing |
| Icons, diagrams, generated vector assets | `svg()` / `PathBuilder` | Keep assets inspectable and themeable |
| Motion graphics and loaders | `lottie(...)` with feature `lottie` | Decodes off the UI path |
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

For most custom graphics, use the immediate-mode `canvas(size, draw)` form. It
records native draw commands for the current pass and lets generated code
inspect composition before the commands flush into the window:

```rust
use kael::{canvas, point, px, size, stroke, Bounds};

canvas(size(px(320.0), px(180.0)), |draw, _window, _app| {
    draw.fill_rect(
        Bounds::new(point(px(0.0), px(0.0)), draw.size()),
        kael::rgb(0x1e1e1e),
    );
    draw.stroke_rect(
        Bounds::new(point(px(24.0), px(24.0)), size(px(120.0), px(64.0))),
        stroke(px(2.0), kael::rgb(0xffffff)),
    );

    tracing::info!(summary = draw.to_text(), "canvas draw");
})
```

`DrawContext::to_text()` reports queued command count, path count, quad count,
filled/stroked quad counts, text count, image count, saved-state depth, and
canvas size without logging text, image data, colors, or drawing coordinates.
Use `command_count()`, `path_count()`, `quad_count()`, `filled_quad_count()`,
`stroked_quad_count()`, `text_count()`, `image_count()`, `state_stack_depth()`,
and `is_empty()` when agents or tests need to verify generated chart, timeline,
waveform, canvas editor, or game HUD drawing.

`canvas` also supports the lower-level two-closure form — a prepaint pass
(compute layout/state, returns a value) and a paint pass (draw into the bounds).
Inside paint you call `window.paint_quad` and `window.paint_path`:

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

Use `cached(child)` when a subtree is expensive but only depends on tracked
state, `deferred(child)` when a subtree should keep layout in-tree but paint
after ancestors, and `effect_layer(child)` when a subtree needs native
CSS-style content blur or drop shadow. Use `LayerStack` with `LayerOptions`
when the app needs native in-window modal, fullscreen, or anchored overlay
composition instead of a WebView-hosted DOM overlay:

```rust
use kael::{cached, deferred, effect_layer, LayerOptions, px};

let preview = cached(render_preview()).id("preview-cache");
tracing::info!(summary = preview.to_text(), "cached subtree");

let overlay = deferred(effect_layer(render_panel()).content_blur(px(8.))).with_priority(120);
tracing::info!(summary = overlay.to_text(), "deferred overlay");

let modal = LayerOptions::modal();
tracing::info!(summary = modal.to_text(), "native layer options");
```

Inspect `Cached::to_text()`, `Deferred::to_text()`, and
`EffectLayer::to_text()` in generated graphics, overlays, previews, and
inspectors. Inspect `LayerAnchor::to_text()`, `LayerOptions::to_text()`, and
`LayerStack::to_text()` before generated modal/popover/fullscreen layer flows.
These helpers report child presence, explicit cache-key presence, draw
priority/class, effect combination, blur class, shadow presence, placement,
backdrop/dismissal policy, and active layer counts without logging cache ids,
child contents, colors, coordinates, margins, blur radii, shadow offsets, shadow
colors, or geometry.

## SVG

`svg()` renders a vector asset; `text_color` fills monochrome SVGs and `with_transformation` applies rotation/scale:

```rust
use kael::{svg, Transformation, px, rgb, size};

let icon = svg()
    .path("icons/logo.svg")
    .with_transformation(Transformation::scale(size(1.25, 1.25)))
    .size(px(24.0))
    .text_color(rgb(0x2563eb));

tracing::info!(summary = icon.to_text(), "svg");
```

Use `Svg::to_text()`, `Transformation::to_text()`, `has_path()`,
`path_len_bytes()`, `has_transformation()`, and `transformation_key()` when
generated icon, diagram, or vector-asset UI needs diagnostics. Summaries report
path presence/byte length and coarse transform kind without logging SVG paths,
asset names, transform coordinates, scale values, or rotation values.

## Images and Surfaces

`img(source)` renders URL, embedded, file-path, cached, decoded, and custom
loader image sources with native object-fit behavior:

```rust
use kael::{img, ObjectFit, StyledImage};

let poster = img("https://cdn.example.com/poster.png")
    .object_fit(ObjectFit::Cover)
    .with_fallback(|| fallback_art().into_any_element());

tracing::info!(summary = poster.to_text(), "image");
```

Use `ImageSource::to_text()`, `ImageStyle::to_text()`, and `Img::to_text()` for
asset-heavy generated UI. The helpers expose source kind, resource identifier
byte length, grayscale state, object-fit key, loading/fallback hook presence,
and explicit cache binding without logging URLs, file paths, embedded asset
names, raw bytes, decoded image IDs, pixel dimensions, or child contents.

For native image caching, scope cache providers around the subtree that owns the
asset working set:

```rust
use kael::{image_cache, lru, retain_all};

let cache = lru("gallery-cache", 64);
tracing::info!(summary = cache.to_text(), "image cache policy");

image_cache(cache).child(gallery)
```

Use `retain_all(id)` for bounded asset sets and `lru(id, max_images)` for
feeds, galleries, maps, and other churning image sets. Inspect
`RetainAllImageCacheProvider::to_text()`, `LruImageCacheProvider::to_text()`,
`ImageCacheElement::to_text()`, `RetainAllImageCache::to_text()`,
`LruImageCache::to_text()`, and `ImageCacheItem::to_text()` when generated UI or
agents need cache policy, entry counts, loading/loaded/error counts, capacity,
capacity class, and scoped child count without logging resource identifiers,
element ids, image ids, image bytes, error details, or asset names.

`surface(source)` renders platform-native external image buffers, such as
CoreVideo pixel buffers on macOS. Use `SurfaceSource::to_text()` and
`Surface::to_text()` to report source class and object-fit key without logging
pixel contents or dimensions.

## Lottie

Enable Kael's `lottie` feature to add the native decoder and renderer:

```toml
[dependencies]
kael = { version = "0.3", features = ["lottie"] }
```

`lottie()` plays Lottie/dotLottie animations, decoding frames on a background thread so the UI stays responsive:

```rust
use kael::{lottie, LoopMode};

lottie("animations/loader.json")
    .autoplay()
    .loop_forever()            // or .loop_mode(LoopMode::Loop) / .ping_pong()
```

Builders: `.autoplay()`, `.loop_forever()`, `.loop_mode(LoopMode)`, `.ping_pong()`, `.object_fit(ObjectFit)`, `.prefetch_frames(n)`, `.with_loading(|| element)`, `.with_fallback(|| element)`.

See the Astryx showcase's media and visual-effects sections for complete,
runnable compositions.
