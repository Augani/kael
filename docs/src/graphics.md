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
| Procedural graphics and custom fragment effects | feature `custom-shaders`, `ShaderHandle`, `RenderTarget`, `render_target(...)` | GPU execution on Metal, DX11, Blade, and WebGL2; checked interfaces, formats, ownership, and budgets |
| GPU image/data kernels and multi-pass effects | `ComputeHandle`, `GpuBuffer`, `GpuRenderGraph` | Native compute/storage on Metal, DX11 feature level 11 and Blade; fragment graphs also execute on WebGL2 |

The public `graphics_capability_report()` API exposes this same truth for
readiness checks and agent planning. It reports full cross-backend coverage for
styled elements, canvas, the portable retained 2D surface, paths, gradients,
SVG, and Lottie; partial coverage for clip
shapes, effect layers, and headless rendering; WebView coverage for browser
graphics fallback. With `custom-shaders` enabled, public targets, fragment shaders, native compute and GPU graph execution report partial coverage. Host capabilities and browser HDR extensions are checked at runtime; WebGL2 has no compute stage. Without that feature these public GPU APIs report roadmap status.

## Display density and text

Kael lays out in logical pixels and updates the backing scale whenever a native
window or browser canvas moves between displays. On macOS, glyph masks use
display-independent grayscale antialiasing and baselines are snapped to device
pixels. Windows also uses grayscale DirectWrite coverage instead of caching
panel-specific ClearType RGB stripes. This avoids stale-resolution text and
RGB/BGR subpixel color fringing on scaled, rotated, or differently ordered
external panels, while reducing glyph-atlas storage relative to four-channel
subpixel masks.

Native atlas sprites clamp filtered samples to the owned tile's texel centers,
preserving interior interpolation while excluding adjacent packed glyphs or
images. Metal uploads atlas pixels through bounded, queue-ordered staging
buffers. Destination pages, staging and transient rasters share atlas admission, and unfinished
uploads retain their resources until completion. Upload timeout stops admission
and progress wakes; drawable acquisition also has a finite timeout.

The Metal renderer allocates path/MSAA and cached-subtree scratch textures only
when those features are drawn. Resizing a window that only draws quads, text,
and images does not allocate these full-window targets. Once used, each target
reuses its own largest required dimensions across frames and resizes; a zero
drawable size releases the scratch textures. Backdrop-blur targets are also
allocated on use. Actual driver allocation depends on the GPU; the nominal
BGRA8 path target, 4x MSAA target, and cache target together would otherwise
require 24 bytes per device pixel (about 190 MiB at 3840 × 2160).

DirectX also allocates its path/MSAA, backdrop-blur and cached-subtree scratch
groups only when drawn. Resize releases those groups, and plain scenes leave them
unallocated. Memory pressure releases optional scratch, sheds dead custom targets
and pipeline caches, and shrinks grown instance buffers; later draws rebuild the
needed resources. Live public GPU target handles retain their storage.

Native Metal and DirectX source-over blending preserves coverage alpha for
translucent layers, including paths and cached subtree targets: two layers
with 50% opacity produce 75% coverage. Metal capture validates staging and
decoded-pixel byte limits before allocating or rendering offscreen targets, so
rejected captures do not allocate large scratch textures.

Backdrop blur retains premultiplied RGB between filtering passes and
applies tint and saturation without multiplying coverage alpha twice. Its
samples preserve the backdrop's viewport coordinates and clamp to captured
texel centers at the viewport boundary. Capture bounds round their absolute
endpoints outward, preserving texels covered by fractional element bounds.
The final pass applies the element's own rounded corners, its rectangular
content mask, and the innermost screen-space rounded ancestor clip before
source-over composition over the target.

Browser WebGL2 uses two GPU Gaussian passes and compact capture textures.
The RGBA8 scratch pair allocates on first use, reuses capacity, and has a
separate 64 MiB payload ceiling checked before allocation. Resize and memory
pressure release optional scratch; context loss discards its generation.
Changed scenes containing blur redraw their dependent backdrop, while an
unchanged scene can retain the existing frame.

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

This is the retained 2D game/creative-app API. Enable `custom-shaders` for typed
GPU targets, custom fragment programs, native compute and multi-pass graphs as
described below. Custom blend modes and depth-tested 3D remain separate roadmap
work.

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
feeds, galleries, maps, and other churning image sets. The LRU default also limits
decoded frames to 64 MiB and unfinished loads to eight; completed errors count
toward the entry limit. Use `lru_with_limits` for an explicit policy:

```rust
use kael::{image_cache, lru_with_limits, ImageCacheLimits};

image_cache(lru_with_limits("gallery-cache", ImageCacheLimits {
    max_images: 64,
    max_decoded_bytes: 32 * 1024 * 1024,
    max_pending_loads: 4,
})).child(gallery)
```

Declared raster/SVG dimensions are checked against the byte limit before pixel
allocation; animated images additionally count every decoded frame. Four built-in
image jobs may perform I/O or decoding concurrently across all image caches.
Pending admission errors can be retried when a load completes; rendering helpers
wake waiting views automatically. Clearing, removing, dropping a cache entity,
or critical memory pressure cancels its owned image loads and releases residency.
`retain_all` keeps its explicit unbounded completed-retention policy between
clears/pressure events, with eight pending loads.

The default shared asset cache retains at most 256 completed entries and 64 MiB
of reported output bytes. Configure it through
`App::set_completed_asset_cache_limits`; custom heap-backed assets should override
`Asset::cache_bytes`. Framework asset helpers use `App::fetch_asset_checked`, which
checks a separate pending limit before calling the loader (64 by default).
`App::set_pending_asset_limit` configures that limit. The legacy `App::fetch_asset`
allows explicit unbounded admission. Removing the cache's ownership does not
cancel a shared load still owned by another caller; it cancels when the final
owner releases it. Critical pressure releases both completed outputs and cache
ownership of unfinished shared assets. Window rendering helpers keep lightweight
wake tokens rather than independent owners of those tasks. These are retention
and concurrency budgets, not a hard cap
on the process's total memory or codec temporary allocations.

Inspect
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

## Custom GPU targets, fragment and compute shaders

Enable `kael`'s `custom-shaders` feature. Register WGSL once through
`App::register_fragment_shader(ShaderDescriptor::fragment(label, source, entry))`,
create a physical-pixel target with `Window::create_render_target`, and execute
with `Window::render_shader`. `render_target(target.clone())` is a styled surface
element that samples the GPU texture directly; display performs no CPU readback
or image re-upload. The non-media `custom_shader` example shows a complete window:

```sh
cargo run -p kael --example custom_shader --features custom-shaders
```

The fragment entry returns `@location(0) vec4<f32>` with straight linear RGB and
coverage alpha. Optional inputs are `@location(0) vec2<f32>` UV and
`@builtin(position) vec4<f32>` physical target coordinates, both with a top-left
origin. Kael supplies a full-target triangle and stores associated RGB by applying
coverage alpha once into a cleared target. Linear filtering therefore preserves
translucent edges. Sampling another target returns associated linear RGB: a
copy pass must unassociate its nonzero-alpha sample before returning the straight
fragment output. Compute/storage operations use stored data directly.

Group zero supports sparse authored bindings for uniform blocks, sampled 2D
float textures, and samplers. Supply values with `ShaderBindings::new().with(...)`;
all declared resources must be present with the reflected type and uniform byte
count. `ShaderHandle::resources()` exposes recursive member offsets, sizes,
alignment and strides. The portable uniform contract accepts only layouts whose
WGSL and std140 bytes match; a Rust `repr(C)` struct alone does not prove this.
Texture inputs are reusable `RenderTarget` handles; `ShaderSampler` selects linear
or nearest edge-clamped filtering. Framework-generated `kael_` identifiers and
`Kael` type names are reserved.

Targets support linear `Rgba8Unorm`, sRGB RGBA/BGRA, linear `Rgba16Float`, and
`R8Unorm` scalar fields. sRGB attachments encode on storage and decode on sampling.
WebGL2 implements BGRA requests with equivalent RGBA sRGB storage and requires
`EXT_color_buffer_float` for HDR rendering. R8 displays as opaque grayscale.
`read_render_target` is an explicit tightly packed CPU export with native target
encoding: RGBA channel order, encoded sRGB bytes, little-endian binary16 HDR, or
one R8 byte per pixel. UI composition respects masks, rounded corners, opacity,
transforms, and color filters; nested ancestor rounded clipping follows the
existing innermost screen-space clip contract.

A renderer retains at most 64 live target and buffer allocations combined, 64
fragment/format pipelines and 64 native compute pipelines. User-owned GPU data
defaults to one shared 256 MiB per-window budget, adjustable with
`set_render_target_byte_budget`; one target is capped at 256 MiB, one storage
buffer at 128 MiB, and targets at 1..=16384 pixels per dimension.
Driver alignment is accounted by Metal; other
backends account payload bytes. In-flight driver resources, uniforms, pipeline
objects, and explicit readback staging are additional bounded allocations.
A clone pins its target or buffer through memory pressure. Dropping the last clone permits
reclamation, and a dropped/lost renderer invalidates its handles. Targets cannot
cross windows or devices, even when two windows use the same physical GPU;
replace targets when dimensions change. Rejected binding, ownership, feedback,
and admission requests do not submit GPU work.

Metal and Blade cap custom submissions, retain pending resources until GPU
completion, and use a ten-second deadline for explicit readback/reuse waits.
DX11 explicit target/buffer readback polls without blocking Map beyond the same
deadline. Metal retains at most eight completed uniform buffers of 64 KiB each;
large upload/readback staging is released after completion.
Blade scene waits also make one bounded attempt. Failed waits invalidate owned
handles and stop new scene work; resize/pressure retain pending allocations until
completion, and a timed-out teardown preserves the device/raw allocations.
WebGL2 readback is a browser-synchronous operation. Shader validation establishes
language and interface correctness; it is not an execution-time guarantee for
arbitrary authored GPU code. Native GTK4/GSK and mock/headless platform windows
currently return a typed unsupported error for this public execution API; the
native X11/Wayland Blade hosts support it. Metal, DX11, and Blade's internal GPU
regression adapters also exercise the same target contract offscreen.

`Window::write_render_target` uploads exactly the packed encoding returned by
`read_render_target`, with top-left pixels first. This enables decoded images,
HDR data and scalar fields without a fragment readback loop. The caller supplies
associated RGB and the selected format's storage encoding; uploads perform no
implicit color conversion. Wrong byte lengths are rejected before submission.

Register native WGSL kernels with
`App::register_compute_shader(ComputeDescriptor::new(label, source, entry))`.
Use `Window::create_gpu_buffer`, `write_gpu_buffer` and `read_gpu_buffer` for
zero-initialized storage buffers. Buffer sizes and write ranges must be nonzero
multiples of four (empty writes are allowed), with checked offsets and the same
window ownership/budget as textures. Reflected storage layouts check minimum
bytes and final runtime-array offset/stride; writable/read aliases and sampled
input/output aliases are rejected. `ComputeBindings` accepts uniforms, sampled
textures, samplers, storage buffers and write-only `Rgba8Unorm`/`Rgba16Float`
storage images. Compute stores already-associated linear colors directly.

```rust
use kael::{App, ComputeBinding, ComputeBindings, ComputeDescriptor,
           RenderTarget, RenderTargetDescriptor, Window};

fn gpu_tile(window: &mut Window, cx: &mut App)
    -> Result<RenderTarget, Box<dyn std::error::Error>>
{
    let shader = cx.register_compute_shader(ComputeDescriptor::new("UV tile", r#"
        @group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
        @compute @workgroup_size(8, 8)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            let dimensions = textureDimensions(output);
            if any(id.xy >= dimensions) { return; }
            let uv = vec2<f32>(id.xy) / vec2<f32>(dimensions);
            textureStore(output, vec2<i32>(id.xy), vec4<f32>(uv.x, 0.25, uv.y, 1.0));
        }
    "#, "main"))?;
    let target = window.create_render_target(RenderTargetDescriptor::rgba8(640, 480))?;
    let bindings = ComputeBindings::new()
        .with(1, ComputeBinding::StorageTexture(target.clone()));
    window.dispatch_compute(&shader, &bindings, [80, 60, 1])?;
    Ok(target)
}
```

Native workgroups use the portable guaranteed floor: at most 128 invocations,
x/y dimensions at most 128, z at most 64, and 16 KiB shared workgroup memory.
Dispatches accept 1..=65535 groups per axis and at most 16,777,216 total groups.
Metal also checks the compiled pipeline's actual thread limit; DirectX compute
requires feature level 11.0. WebGL2 returns a typed unsupported error for compute
and storage buffers; fragment execution and texture uploads remain available.

Fragment programs and compute kernels without synchronized loops share a
65,536 loop-body execution counter per invocation, across nested loops and
helper functions. Exhaustion exits remaining loops and continues ordinary
control flow, so larger algorithms must be split across passes. Compute programs
containing both loops and workgroup/storage barriers preserve authored loop and
synchronization semantics: `ComputeHandle::loop_body_limit()` returns `None`.
Use `ComputeDescriptor::require_loop_bound()` to reject those kernels explicitly.
A private counter must not remove only some workitems from a workgroup barrier.
Neither mode provides a GPU wall-clock deadline. CPU registration limits apply
across fragment and compute together: 128 live programs, 16 MiB retained source
and translations, and 2 MiB per program.

The current evidence and platform runtime limits are recorded in
[the shader completion review](https://github.com/Augani/kael/blob/main/docs/reviews/2026-10-02-shader-completion.md).

## GPU graphs with native compute

With `custom-shaders`, `kael::gpu_graph` exposes resource/pass declarations and
the lifetime planner. `GpuRenderGraph::new` describes a fragment graph;
`new_with_resources` accepts typed texture and storage-buffer descriptors for
mixed graphs. Use `GpuGraphPass::Fragment(GpuFragmentPass)` or
`GpuGraphPass::Compute(GpuComputePass)` with `Window::execute_gpu_graph`.
The native `compute_graph` example creates an HDR image in compute, tone maps it
in a fragment pass and displays the resulting GPU target:

```sh
cargo run --locked -p kael --example compute_graph --features custom-shaders
```

Declare every sampled/read-only resource as a pass read and every output as a
write. Storage textures bind through `GpuGraphBinding::Texture`, storage
buffers through `GpuGraphBinding::Buffer`. Reflection must exactly match the
declared accesses, uniform sizes, storage formats and buffer minimum/stride.
All imports are validated against the executing window before allocation.
Duplicate physical imports and resource feedback are rejected.

Exports remain live through the execution tail. Compatible transient resources
reuse slots only after their inclusive lifetimes end; allocation classes must
agree on exact dimensions, format or buffer size. Cache hits compare program,
workgroup count, uniform bytes, sampler settings and actual input/output
identities and revisions. Aliased storage also requires the intended logical
resource to remain resident. Kernels with writable storage buffers always run,
since read/write kernels can depend on previous buffer contents. Kernels must
initialize transient outputs before reading them, including reused slots.

Graphs are limited to 256 resources/passes, eight outputs per pass, 64 physical
slots and 16 MiB of uniform signatures per execution. The graph's 256 MiB
default payload limit and the window's shared live texture/buffer budget both
apply. `set_byte_budget`, `clear_cache` and `handle_memory_pressure` let an app
release retained graph work; caller-owned exports remain pinned. A backend
submission failure can leave an executed prefix and invalidates pass caches.
Submission uses ordered native renderer commands; the compiler's barriers and
lifetimes remain available through `compiled()`.

WebGL2 supports fragment graphs and returns a typed unsupported error for
compute before graph allocation. Native backend capabilities still apply.
Actual Metal and Blade graph regressions cover buffer → image → fragment pixels, cache
invalidation, exported lifetimes, alias residency, foreign imports, invalid
workgroup admission and invalidated output recovery. The completion review
tracks other platform runtime evidence independently. DirectX uses the same six
graph contracts; its native Windows runtime result is separate from compilation.

## Native GPU timing

`Window::set_gpu_frame_timing_enabled(true)` enables bounded native timing when
supported and returns `false` on other backends. Metal reports command-buffer
GPU start/end timestamps and drawable presentation callbacks. Drain ready
records with `take_gpu_frame_timings`; onscreen records wait for both callbacks.
The collector retains at most 64 pending or ready records, drops the oldest under
pressure, and discards the session when disabled. Callbacks use weak ownership
and introduce no polling timer or target readback.

Timestamps use the native monotonic host clock in seconds. An absent presentation
timestamp means no display time was reported, including an offscreen or dropped
frame. Rust `Instant`, CPU submission duration, GPU execution duration and actual
drawable presentation are distinct measurements. Other backends currently report
unsupported timing rather than supplying estimated GPU times.
