# macOS Crispness Plan

Goal: apps built with Kael should feel as sharp, smooth, and native as first-party macOS apps. That means Retina-correct pixels, stable text, native-feeling motion, color correctness, and default UI styling that avoids the softer look common in cross-platform renderers.

## Current State

- The macOS window path reads `NSScreen::backingScaleFactor` and updates the Metal layer `contentsScale`.
- Window resize updates `CAMetalLayer.drawableSize` in device pixels.
- Most paint primitives scale to device pixels and snap bounds, clips, radii, and border widths before insertion into the scene.
- Text rasterization uses CoreGraphics/CoreText with antialiasing and subpixel positioning enabled.
- The resize regression from CPU shadow rasterization has been fixed by routing box shadows through the GPU `Shadow` primitive.

These are the right foundations. The remaining work is mostly about removing small sources of softness and making polished defaults the easy path.

## Priority Fixes

### 1. Use sRGB-aware render targets

Several Metal textures use `BGRA8Unorm`. That is fast, but it can make colors, gradients, and antialiasing look subtly unlike AppKit because blending happens without explicit sRGB semantics.

Audit whether the drawable, cached surfaces, blur textures, and polychrome atlas should use `BGRA8Unorm_sRGB` or explicit shader linearization. Pick one pipeline-wide color policy and document it.

Acceptance:
- White/gray text edges match native labels more closely.
- Gradients do not look muddy or over-bright.
- Cached surfaces and direct rendering produce the same visual color.

### 2. Protect 1 px strokes and borders

Kael snaps many bounds, but crisp macOS UI depends on consistent hairline rules:

- A 1 device-pixel stroke should land on exact device pixels.
- Filled rectangles should snap outward or inward consistently.
- Centered strokes need half-pixel logical offsets only when that maps to whole device pixels.
- Border radii should snap without changing the perceived corner shape.

Add a small `PixelSnapPolicy` helper for fills, strokes, clips, and text baselines instead of each paint path making slightly different choices.

Acceptance:
- 1 px dividers are never blurry at 1x, 2x, or 3x.
- Borders stay the same thickness while moving a window between Retina and non-Retina displays.
- The crispness showcase includes a hairline/stroke grid that visually catches regressions.

### 3. Make text baselines first-class

Text is where users feel "native" first. The current macOS rasterizer uses subpixel variants and CoreGraphics flags, but Kael still needs a stricter baseline policy.

Add tests/examples for:
- Baselines at fractional logical positions.
- Text in scrolled containers.
- Text inside cached surfaces.
- Text after moving between displays with different scale factors.

Acceptance:
- Text does not shimmer while scrolling slowly.
- Text inside cached surfaces is not softer than direct text.
- Glyph atlases are invalidated or separated when scale factor or raster mode changes.

### 4. Make cached surfaces Retina-safe

Cached surfaces are a common source of soft UI. They must be rendered and sampled at device-pixel size, never logical size, and copied without filtering when source and destination map 1:1.

Audit:
- Cached surface texture allocation.
- Snapshot source bounds.
- Destination bounds.
- Sampling mode in the Metal shader.

Acceptance:
- Cached and uncached versions of the same component are pixel-identical at 2x for static content.
- Scaling is explicit when intentionally zooming; accidental scaling is impossible.

### 5. Match native motion cadence

Smooth macOS feel is not only FPS. It is also frame pacing and not over-rendering.

Keep demand-driven polling as the default, but add instrumentation for:
- Frames requested during resize.
- Frames presented during resize.
- Missed display intervals.
- Drawable acquisition stalls.

Acceptance:
- Idle windows stop polling.
- Resize produces steady frame intervals.
- Long-running paint work is visible in debug logs or trace output.

## Design Defaults

Rendering can be technically crisp while the app still feels non-native. Kael should provide defaults that steer app authors toward macOS polish:

- Use the system font by default for UI, with correct weights and optical sizes where available.
- Prefer restrained radii, subtle borders, and real macOS spacing density.
- Avoid heavy blurred panels and oversized shadows as defaults.
- Provide native-feeling controls for common app surfaces: toolbar, sidebar, inspector, popover, sheet, command palette, and list rows.
- Use `NSVisualEffectView` or a carefully matched Metal material only where macOS would use material, not everywhere.

## Crispness Showcase Additions

Extend `crispness_showcase` into a visual regression target:

- Hairline grid at 1x/2x/3x.
- Text baseline rows with fractional origins.
- Cached vs uncached text and cards.
- sRGB gradient and alpha-blended edge samples.
- Scroll shimmer test.
- Resize stress panel with many shadows, blurs, and text.

## Recommended Order

1. Lock the shadow and resize fixes with regression tests or trace counters.
2. Add crispness showcase panels for hairlines, text, cached surfaces, and gradients.
3. Define the color-space policy for Metal textures and shaders.
4. Add a shared pixel snapping helper and migrate fills, borders, paths, and clips.
5. Audit cached surface sampling and text baseline behavior.
6. Add macOS-native design primitives so app authors get crisp defaults without hand-tuning.

