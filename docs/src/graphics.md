# Canvas & Graphics

Beyond the element tree, Kael gives you direct GPU drawing: an immediate-mode canvas, a vector path builder, gradients, backdrop blur, SVG, and optional Lottie playback. Everything renders through the same per-platform pipeline (Metal / DirectX 11 / Vulkan / browser WebGL2) with device-pixel snapping for crisp output at any DPI.

## Visual escape-hatch ladder

When designing a graphics-heavy workflow or giving an AI agent a rendering task,
choose the lowest rung that solves the problem:

| Need | Use today | Notes |
| --- | --- | --- |
| Product UI, dashboards, tool chrome | styled `div()` / `kael_ui` | Best memory and startup profile |
| Charts, timelines, waveform views, custom controls | `canvas(...)`, `paint_quad`, `paint_path`, `PathBuilder` | Native immediate-mode drawing |
| Game worlds, whiteboards, and large retained 2D surfaces | `PortableScene2d` / `portable_scene(...)` | Same bounded retained commands on native and browser renderers |
| Icons, diagrams, generated vector assets | `svg()` / `PathBuilder` | Keep assets inspectable and themeable |
| Motion graphics and loaders | `lottie(...)` with feature `lottie` | Decodes off the UI path |
| Frosted or filtered subtrees | `backdrop_blur(...)` / `effect_layer(...)` | Effect layers are partial CSS-filter coverage, not arbitrary shaders |
| Run a Kael canvas in a browser | `kael` / `kael_ui` feature `browser` | Same retained Scene through the WebGL2 renderer |
| External or hosted browser content | `webview(id, url)` | Native composition island on desktop; sandboxed iframe island in the wasm backend, with documented cross-origin limits |
| Golden-image or benchmark evidence | `HeadlessRenderer` / `golden` | Off-screen rendering is for tests and measurements |
| Public custom render target or custom shader | roadmap | The backend renderer is not yet a public arbitrary-shader API |

The public `graphics_capability_report()` API exposes this same truth for
readiness checks and agent planning. It reports full cross-backend coverage for
styled elements, canvas, the portable retained 2D surface, paths, gradients,
SVG, and Lottie; partial coverage for clip
shapes, effect layers, and headless rendering; WebView coverage for browser
graphics fallback; and roadmap status for public render targets/custom shaders.

## Display density and text

Kael lays out in logical pixels and updates the backing scale whenever a native
window or browser canvas moves between displays. On macOS, glyph masks use
display-independent grayscale antialiasing and baselines are snapped to device
pixels. Windows also uses grayscale DirectWrite coverage instead of caching
panel-specific ClearType RGB stripes. This avoids stale-resolution text and
RGB/BGR subpixel color fringing on scaled, rotated, or differently ordered
external panels, while reducing glyph-atlas storage relative to four-channel
subpixel masks.

Embed application fonts when typography is part of the product identity.
`kael_ui::init` already registers its bundled Inter and JetBrains Mono faces.
Font family, weight, layout scale, and backing resolution remain stable across
screens; small rasterization differences between operating-system and browser
text engines are still expected.

## Canvas

For most custom graphics, use the immediate-mode `canvas(size, draw)` form. It
records native draw commands for the current pass and lets generated code
inspect composition before the commands flush into the window:

```rust
use kael::{canvas, point, px, size, stroke, Bounds};

canvas(size(px(320.0), px(180.0)), |draw, _window, _app| {
    draw.reserve_commands(6);
    draw.fill_rect(
        Bounds::new(point(px(0.0), px(0.0)), draw.size()),
        kael::rgb(0x1e1e1e),
    );
    draw.fill_rects([
        (
            Bounds::new(point(px(24.0), px(112.0)), size(px(48.0), px(40.0))),
            kael::rgb(0x3b82f6).into(),
        ),
        (
            Bounds::new(point(px(80.0), px(88.0)), size(px(48.0), px(64.0))),
            kael::rgb(0x60a5fa).into(),
        ),
    ]);
    draw.fill_circles([
        (point(px(232.0), px(64.0)), px(12.0), kael::rgb(0xf59e0b).into()),
        (point(px(268.0), px(64.0)), px(12.0), kael::rgb(0xfbbf24).into()),
    ]);
    draw.stroke_rect(
        Bounds::new(point(px(24.0), px(24.0)), size(px(120.0), px(64.0))),
        stroke(px(2.0), kael::rgb(0xffffff)),
    );

    tracing::info!(summary = draw.to_text(), "canvas draw");
})
```

For stable real-time workloads, call `reserve_commands` with the expected mixed
command count before drawing, and use `fill_rects` or `fill_circles` for batches.
The batch helpers reserve from the iterator's size hint. Circles are emitted as
rounded quads, reusing the renderer's quad fast path instead of tessellating a
vector path; this is the preferred route for particle systems, graph nodes, and
game sprites that are geometrically circular.

`DrawContext::to_text()` reports queued command count, path count, quad count,
filled/stroked quad counts, text count, image count, saved-state depth, and
canvas size without logging text, image data, colors, or drawing coordinates.
Use `command_count()`, `path_count()`, `quad_count()`, `filled_quad_count()`,
`stroked_quad_count()`, `text_count()`, `image_count()`, `state_stack_depth()`,
and `is_empty()` when agents or tests need to verify generated chart, timeline,
waveform, canvas editor, or game HUD drawing.

For a scene that persists across frames, use `PortableScene2d`. It accepts
bounded batches of solid or rounded quads, decoded-image sprites, pre-tessellated
filled paths, and triangles, with affine transforms, rectangular clips,
source-over opacity, typed limit failures, and transactional rollback. The
default public ceilings are 100,000 commands/objects, 1,000,000 path vertices,
256 decoded image frames, 64 MiB of decoded image data, and 128 MiB of estimated
retained payload. Static path transforms are baked when recorded rather than
recomputed on every frame.

```rust
use std::sync::Arc;
use kael::{Bounds, PortableScene2d, PortableSolidQuad, point, portable_scene,
           px, rgb, size};

let mut scene = PortableScene2d::new();
scene.try_reserve_commands(100_000)?;
let quads = (0..100_000).map(|index| {
    let x = (index % 500) as f32 * 3.0;
    let y = (index / 500) as f32 * 3.0;
    PortableSolidQuad::new(
        Bounds::new(point(px(x), px(y)), size(px(2.0), px(2.0))),
        rgb(0x60a5fa),
    )
}).collect::<Vec<_>>();
scene.push_solid_quads(&quads)?;

let surface = portable_scene(size(px(1_500.0), px(600.0)), Arc::new(scene));
# Ok::<_, kael::PortableSceneError>(surface)
```

This is the portable game/creative-app escape hatch, not raw GPU access.
Custom blend modes, user shaders, compute, depth-tested 3D, and public renderer
handles return or report `Unsupported` and remain explicit roadmap work.

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

## High-fidelity pointer input and retained scenes

Use `on_pointer_event` for one drawing path across mouse, touch, and pen. Browser
events include stable pointer id/type, primary state, changed and held buttons,
pressure, tangential pressure, tilt, twist, contact geometry, cancellation, and
up to 256 coalesced samples. Pointer sequences remain routed to the element that
received the down event, including independent simultaneous touches. Give a
surface a stable `.id(...)` when it rerenders during a stroke or drag so its
capture set persists across frames:

```rust
use kael::{div, InteractiveElement as _, PointerPhase};

div().id("drawing-surface").on_pointer_event(|event, _window, _app| {
    if matches!(event.phase, PointerPhase::Down | PointerPhase::Move) {
        for sample in event.stroke_samples() {
            tracing::trace!(
                pointer = event.pointer_id.get(),
                pressure = sample.pressure,
                tilt_x = sample.tilt_x,
                tilt_y = sample.tilt_y,
                "stroke sample"
            );
        }
    }
})
```

Existing mouse callbacks remain source compatible. Legacy desktop mouse streams
are promoted to `PointerInputEvent` with a stable mouse id. Windows WM_POINTER
provides simultaneous touch plus pen pressure, tilt, rotation, contact geometry,
cancellation, and bounded chronological history. AppKit provides tablet
identity/proximity, pressure, tangential pressure, tilt, rotation, buttons, and
timestamps (but no macOS desktop touchscreen or contact ellipse). Wayland
`wl_touch` and X11 XI2.2 provide simultaneous contacts and cancellation;
Wayland also reports oriented contact geometry, while tablet pressure/tilt on
Linux remains compositor/device-protocol dependent. `CapabilityReport` exposes
these per-platform boundaries. Browser touch and pen expose the full Pointer
Events shape.

For large whiteboards and game scenes, `SpatialIndex` uses a bounded spatial
hash instead of scanning every entry. `SceneGraph::hit_test` and
`SceneGraph::visible_in_rect` reuse a cached index while preserving topmost
order. `move_node` patches only the moved entry's old and new spatial cells;
structural changes, visibility edits through `get_mut`, and hierarchy changes
retain the safe lazy full-rebuild fallback. Use
`spatial_incremental_update_count`, `spatial_full_rebuild_count`, and
`last_spatial_candidate_count` to verify dynamic-scene behavior without
inspecting content. Pair culling with `TileDamageTracker`: invalidate old and
new object bounds, repaint the sorted tiles returned by `take`, and retain every
other tile. Pathological regions promote explicitly to `TileDamage::Full`
instead of allocating without bound.

`Window::export_frame_png` returns real encoded PNG bytes at device-pixel
resolution from browser WebGL2, macOS Metal, Windows Direct3D 11, and the Blade
renderer used by Linux and optional macOS Blade builds. GPU readback validates
dimensions, row pitch, channel order, alpha representation, and a 256 MiB
allocation ceiling. It honors checked content protection and returns typed
`WindowCaptureError` variants rather than silently dropping WebView overlays or
live surfaces. Platform/compositor chrome and the system cursor are outside the
scene. Blade capture renders into a bounded app-owned texture before copying to
shared memory, so it does not depend on swapchain copy support; it returns a
typed backend error if the selected surface format is not one of the supported
8-bit RGBA/BGRA formats. Capability support remains partial because hosted/live
surfaces and operating-system chrome are intentionally outside the retained
scene, not because the release gate substitutes a headless renderer.

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

Common application formats—including PNG, JPEG, GIF, WebP, TIFF, BMP, ICO,
TGA, HDR, PNM, farbfeld, DDS, and QOI—are enabled without pulling parallel
image-processing dependencies into every Kael application. Enable the heavier
formats only when the product needs them:

```toml
[dependencies]
kael = { version = "0.4", features = ["image-avif", "image-exr"] }
```

AVIF decoding uses the native libdav1d library and pkg-config discovery.
Install both through the platform package manager, or also provide Git, Meson,
and Ninja so the binding can build libdav1d from source.

The built-in resource loader rejects empty or larger-than-64-MiB encoded
sources, raster or SVG dimensions above 16,384 pixels per axis, more than 256
MiB of decoded frame data, and animations above 10,000 frames. HTTP failures do
not retain response bodies or expose resource locations in error messages. Use
a custom `ImageSource` loader that returns a validated `RenderImage` when a
controlled workload intentionally needs a different budget.

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
kael = { version = "0.4", features = ["lottie"] }
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
