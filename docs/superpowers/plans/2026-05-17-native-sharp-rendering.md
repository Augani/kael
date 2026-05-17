# Native-Sharp Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Kael-rendered apps visually indistinguishable from native platform apps (Telegram Desktop) by adopting platform text rendering, integer pixel math, pre-baked effects, and precise border handling.

**Architecture:** Five independent workstreams: (1) LCD subpixel text rendering on macOS via CoreText compositing into BGRA atlas tiles, (2) integer-pixel layout spine that eliminates float→round artifacts, (3) pre-baked shadow/corner texture cache, (4) exact 1px border rendering at all scale factors, (5) premultiplied-alpha audit across the pipeline. Each workstream produces a testable improvement.

**Tech Stack:** Rust, Core Graphics/Core Text (macOS), Metal shaders, WGSL shaders (blade), font-kit

---

## File Map

| Responsibility | File(s) |
|---|---|
| Glyph rasterization (macOS) | `crates/kael/src/platform/mac/text_system.rs` |
| Text system types & constants | `crates/kael/src/text_system.rs` |
| Glyph painting & sprite creation | `crates/kael/src/window.rs:3380-3480` |
| Pixel coordinate types & snapping | `crates/kael/src/geometry.rs` |
| Scene primitives (Shadow, Quad) | `crates/kael/src/scene.rs` |
| Shadow/effect caching | `crates/kael/src/cache.rs` (extend) |
| Metal renderer | `crates/kael/src/platform/mac/metal_renderer.rs` |
| Metal shaders | `crates/kael/src/platform/mac/shaders.metal` |
| Blade renderer | `crates/kael/src/platform/blade/blade_renderer.rs` |
| WGSL shaders | `crates/kael/src/platform/blade/shaders.wgsl` |
| Atlas system | `crates/kael/src/platform/blade/blade_atlas.rs` |

---

## Phase 1: LCD Subpixel Text Rendering (Highest Impact)

### Task 1: Switch glyph rasterization from grayscale to BGRA LCD subpixel

The current implementation uses `kCGImageAlphaOnly` (single-channel grayscale) for non-emoji glyphs. This discards subpixel color information. We need to render into a BGRA context with font smoothing enabled, producing 4-byte-per-pixel glyph bitmaps where R/G/B channels carry independent coverage.

**Files:**
- Modify: `crates/kael/src/platform/mac/text_system.rs:343-429`
- Modify: `crates/kael/src/text_system.rs:44-51` (constants)

- [ ] **Step 1: Add a `SubpixelMode` enum and configuration**

In `crates/kael/src/text_system.rs`, after line 51, add:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GlyphRasterMode {
    Grayscale,
    Subpixel,
}
```

- [ ] **Step 2: Add `raster_mode` field to `RenderGlyphParams`**

In `crates/kael/src/text_system.rs`, modify `RenderGlyphParams`:

```rust
pub(crate) struct RenderGlyphParams {
    pub(crate) font_id: FontId,
    pub(crate) glyph_id: GlyphId,
    pub(crate) font_size: Pixels,
    pub(crate) subpixel_variant: Point<u8>,
    pub(crate) scale_factor: f32,
    pub(crate) is_emoji: bool,
    pub(crate) raster_mode: GlyphRasterMode,
}
```

Update the `Hash` impl to include `raster_mode`.

- [ ] **Step 3: Modify `rasterize_glyph` to support BGRA subpixel rendering**

In `crates/kael/src/platform/mac/text_system.rs`, modify the non-emoji branch of `rasterize_glyph` (lines 374-385):

```rust
} else if params.raster_mode == GlyphRasterMode::Subpixel {
    bytes = vec![0; bitmap_size.width.0 as usize * 4 * bitmap_size.height.0 as usize];
    cx = CGContext::create_bitmap_context(
        Some(bytes.as_mut_ptr() as *mut _),
        bitmap_size.width.0 as usize,
        bitmap_size.height.0 as usize,
        8,
        bitmap_size.width.0 as usize * 4,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little, // BGRA
    );
    cx.set_font_smoothing_style(16); // LCD subpixel smoothing
    cx.set_should_smooth_fonts(true);
    cx.set_rgb_fill_color(1.0, 1.0, 1.0, 1.0);
} else {
    // existing grayscale path
    bytes = vec![0; bitmap_size.width.0 as usize * bitmap_size.height.0 as usize];
    cx = CGContext::create_bitmap_context(
        Some(bytes.as_mut_ptr() as *mut _),
        bitmap_size.width.0 as usize,
        bitmap_size.height.0 as usize,
        8,
        bitmap_size.width.0 as usize,
        &CGColorSpace::create_device_gray(),
        kCGImageAlphaOnly,
    );
}
```

Also change line 402 for subpixel mode — use `set_rgb_fill_color(1.0, 1.0, 1.0, 1.0)` instead of `set_gray_fill_color(0.0, 1.0)` (white on black background gives LCD fringe data).

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build -p kael 2>&1 | head -30`
Expected: Compiles (may have warnings, no errors)

- [ ] **Step 5: Commit**

```bash
git add crates/kael/src/platform/mac/text_system.rs crates/kael/src/text_system.rs
git commit -m "feat(text): add BGRA subpixel glyph rasterization mode on macOS"
```

---

### Task 2: Update atlas to store subpixel glyphs as BGRA tiles

The atlas currently stores non-emoji glyphs as `R8Unorm` (monochrome). Subpixel glyphs need `Bgra8Unorm` storage — the same format used for emoji/polychrome sprites.

**Files:**
- Modify: `crates/kael/src/window.rs:3380-3427` (paint_glyph)
- Modify: `crates/kael/src/platform/blade/blade_atlas.rs` (tile allocation)
- Modify: `crates/kael/src/scene.rs` (use PolychromeSprite for subpixel glyphs)

- [ ] **Step 1: In `paint_glyph`, route subpixel glyphs to polychrome sprite path**

In `crates/kael/src/window.rs`, modify the `paint_glyph` method (~line 3381). When `raster_mode == Subpixel`, the glyph should be stored as a `PolychromeSprite` rather than a `MonochromeSprite`:

```rust
let raster_mode = GlyphRasterMode::Subpixel; // TODO: make configurable
let params = RenderGlyphParams {
    font_id,
    glyph_id,
    font_size,
    subpixel_variant,
    scale_factor,
    is_emoji: false,
    raster_mode,
};

let raster_bounds = self.text_system().raster_bounds(&params)?;
if !raster_bounds.is_zero() {
    let Some(tile) = self
        .sprite_atlas
        .get_or_insert_with(&params.clone().into(), &mut || {
            let (size, bytes) = self.text_system().rasterize_glyph(&params)?;
            Ok(Some((size, Cow::Owned(bytes))))
        })?
    else {
        return Ok(());
    };
    let bounds = Bounds {
        origin: glyph_origin.map(|px| px.floor()) + raster_bounds.origin.map(Into::into),
        size: tile.bounds.size.map(Into::into),
    };
    let content_mask = self.content_mask().scale(scale_factor);

    match raster_mode {
        GlyphRasterMode::Subpixel => {
            self.next_frame.scene.insert_primitive(PolychromeSprite {
                order: 0,
                bounds,
                content_mask,
                tile,
                grayscale: false,
                opacity: element_opacity,
            });
        }
        GlyphRasterMode::Grayscale => {
            self.next_frame.scene.insert_primitive(MonochromeSprite {
                order: 0,
                pad: 0,
                bounds,
                content_mask,
                color: color.opacity(element_opacity),
                tile,
                transformation,
            });
        }
    }
}
```

- [ ] **Step 2: Update `AtlasKey` to distinguish subpixel glyph tiles as polychrome**

Ensure that when `raster_mode == Subpixel`, the `AtlasKey` derived from `RenderGlyphParams` routes to the polychrome (BGRA) atlas texture rather than the monochrome (R8) texture.

- [ ] **Step 3: Run `cargo build` and verify**

Run: `cargo build -p kael 2>&1 | head -30`
Expected: Compiles cleanly

- [ ] **Step 4: Commit**

```bash
git add crates/kael/src/window.rs crates/kael/src/platform/blade/blade_atlas.rs crates/kael/src/scene.rs
git commit -m "feat(text): route subpixel glyphs through polychrome sprite pipeline"
```

---

### Task 3: Add subpixel-aware text compositing shader

Subpixel text needs a specialized blend mode: per-channel alpha from the glyph texture multiplied by the text color, then standard premultiplied-alpha blend with the background. The current monochrome path uses a single alpha channel. The polychrome path uses straight texture colors. We need a third mode.

**Files:**
- Modify: `crates/kael/src/platform/mac/shaders.metal` (add `subpixel_text_fragment`)
- Modify: `crates/kael/src/platform/blade/shaders.wgsl` (add equivalent)
- Modify: `crates/kael/src/platform/mac/metal_renderer.rs` (new pipeline state)
- Modify: `crates/kael/src/platform/blade/blade_renderer.rs` (new pipeline)

- [ ] **Step 1: Add Metal fragment shader for subpixel text**

In `shaders.metal`, add after the existing sprite shaders:

```metal
fragment float4 subpixel_sprite_fragment(
    SpriteFragmentInput input [[stage_in]],
    constant PolychromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(0)]]
) {
    PolychromeSprite sprite = sprites[input.sprite_id];
    float4 sample = atlas_texture.sample(sampler_t(), input.atlas_position);
    // sample.rgb contains per-channel coverage (white text on black bg)
    // Multiply by text color's RGB to tint, use max channel as alpha for blending
    float3 coverage = sample.rgb;
    float alpha = max(max(coverage.r, coverage.g), coverage.b);
    // Output premultiplied: color * coverage per channel
    return float4(sprite.color.rgb * coverage, alpha) * sprite.color.a;
}
```

Note: The exact shader will need dual-source blending or a two-pass approach (background read) for true subpixel AA. Start with the max-alpha approximation which is visually close and doesn't require framebuffer fetch.

- [ ] **Step 2: Add equivalent WGSL shader for blade**

In `shaders.wgsl`, add the matching fragment function.

- [ ] **Step 3: Register the new pipeline in Metal renderer**

In `metal_renderer.rs`, add a `subpixel_sprites_pipeline_state` field and initialize it using `"subpixel_sprite_vertex"` / `"subpixel_sprite_fragment"` entry points.

- [ ] **Step 4: Run `cargo build` and fix any issues**

Run: `cargo build -p kael 2>&1 | head -30`

- [ ] **Step 5: Commit**

```bash
git add crates/kael/src/platform/mac/shaders.metal crates/kael/src/platform/blade/shaders.wgsl crates/kael/src/platform/mac/metal_renderer.rs crates/kael/src/platform/blade/blade_renderer.rs
git commit -m "feat(text): add subpixel text compositing shader for LCD rendering"
```

---

## Phase 2: Integer Pixel Layout

### Task 4: Add `DevicePixelLayout` mode — compute layout in device-integer space

Currently layout computes in `Pixels` (f32 logical) and rounds at render time via `scale_and_snap`. This creates rounding inconsistencies where adjacent elements don't share exact boundaries. The fix: perform layout in `DevicePixels` (i32) when the scale factor is known, eliminating the round step.

**Files:**
- Modify: `crates/kael/src/geometry.rs:1671-1692`

- [ ] **Step 1: Add `scale_and_snap_floor_origin` variant**

The current `scale_and_snap` rounds both origin and far-edge. For pixel-perfect results, origin should **floor** and far-edge should **ceil** (guaranteeing sizes are always >= the logical size, never accidentally 0):

```rust
pub fn scale_and_snap_conservative(&self, factor: f32) -> Bounds<ScaledPixels> {
    let scaled_origin_x = self.origin.x.0 * factor;
    let scaled_origin_y = self.origin.y.0 * factor;
    let scaled_far_x = (self.origin.x.0 + self.size.width.0) * factor;
    let scaled_far_y = (self.origin.y.0 + self.size.height.0) * factor;

    let snapped_origin_x = scaled_origin_x.floor();
    let snapped_origin_y = scaled_origin_y.floor();
    let snapped_far_x = scaled_far_x.ceil();
    let snapped_far_y = scaled_far_y.ceil();

    Bounds {
        origin: point(
            ScaledPixels(snapped_origin_x),
            ScaledPixels(snapped_origin_y),
        ),
        size: size(
            ScaledPixels(snapped_far_x - snapped_origin_x),
            ScaledPixels(snapped_far_y - snapped_origin_y),
        ),
    }
}
```

- [ ] **Step 2: Use `scale_and_snap_conservative` for shadow and blur bounds**

In `crates/kael/src/window.rs`, find where shadows and blur_rects get their bounds snapped. Use the conservative variant to ensure shadow bounding boxes never clip content.

- [ ] **Step 3: Run tests**

Run: `cargo test -p kael -- --lib geometry 2>&1 | tail -20`
Expected: All existing geometry tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/kael/src/geometry.rs crates/kael/src/window.rs
git commit -m "feat(geometry): add conservative floor/ceil snapping for bounds"
```

---

### Task 5: Snap border widths to exact device pixels

A 1px logical border at 2x scale = 2 device pixels, which is correct. But at 1.5x scale, 1px = 1.5 device pixels → rounds to 2 → appears thicker than intended. The fix: snap border widths via `max(1, round(width * scale))` to ensure they're always at least 1 device pixel but never fractional.

**Files:**
- Modify: `crates/kael/src/geometry.rs` (add `Edges::scale_and_snap_widths`)
- Modify: `crates/kael/src/window.rs:3254` (use new snapping for border_widths)

- [ ] **Step 1: Add `scale_and_snap_widths` method to `Edges<Pixels>`**

```rust
impl Edges<Pixels> {
    pub fn scale_and_snap_widths(&self, factor: f32) -> Edges<ScaledPixels> {
        let snap = |px: Pixels| -> ScaledPixels {
            let scaled = px.0 * factor;
            if scaled == 0.0 {
                ScaledPixels(0.0)
            } else {
                ScaledPixels(scaled.round().max(1.0))
            }
        };
        Edges {
            top: snap(self.top),
            right: snap(self.right),
            bottom: snap(self.bottom),
            left: snap(self.left),
        }
    }
}
```

- [ ] **Step 2: Replace `border_widths.scale_and_snap(scale_factor)` usage in window.rs**

At line 3254 in `window.rs`:
```rust
border_widths: quad.border_widths.scale_and_snap_widths(scale_factor),
```

- [ ] **Step 3: Run `cargo build`**

Run: `cargo build -p kael 2>&1 | head -20`

- [ ] **Step 4: Commit**

```bash
git add crates/kael/src/geometry.rs crates/kael/src/window.rs
git commit -m "fix(borders): snap border widths to exact device pixels with 1px minimum"
```

---

## Phase 3: Pre-Baked Shadow Cache

### Task 6: Implement shadow texture cache

tdesktop pre-renders shadow corners as pixmaps at startup. We'll cache rendered shadow textures keyed by `(blur_radius, corner_radii, color, size_bucket)` in the atlas, re-using them across frames. This eliminates per-pixel gaussian computation in the fragment shader for repeated shadows.

**Files:**
- Create: `crates/kael/src/shadow_cache.rs`
- Modify: `crates/kael/src/lib.rs` (add module)
- Modify: `crates/kael/src/window.rs` (check cache before inserting Shadow primitive)

- [ ] **Step 1: Create `shadow_cache.rs` with the cache structure**

```rust
use crate::{
    AtlasTile, Bounds, Corners, DevicePixels, Hsla, PlatformAtlas, ScaledPixels, Size,
};
use collections::FxHashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ShadowCacheKey {
    blur_radius_bits: u32,
    corner_radii_bits: [u32; 4],
    color_bits: u64,
    width_bucket: u16,
    height_bucket: u16,
}

impl ShadowCacheKey {
    fn new(
        blur_radius: ScaledPixels,
        corner_radii: &Corners<ScaledPixels>,
        color: Hsla,
        size: Size<ScaledPixels>,
    ) -> Self {
        let bucket = |v: f32| -> u16 { (v / 4.0).ceil() as u16 * 4 };
        Self {
            blur_radius_bits: blur_radius.0.to_bits(),
            corner_radii_bits: [
                corner_radii.top_left.0.to_bits(),
                corner_radii.top_right.0.to_bits(),
                corner_radii.bottom_left.0.to_bits(),
                corner_radii.bottom_right.0.to_bits(),
            ],
            color_bits: unsafe { std::mem::transmute(color) },
            width_bucket: bucket(size.width.0),
            height_bucket: bucket(size.height.0),
        }
    }
}

pub(crate) struct ShadowCache {
    entries: FxHashMap<ShadowCacheKey, AtlasTile>,
}

impl ShadowCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }

    pub(crate) fn get_or_render(
        &mut self,
        blur_radius: ScaledPixels,
        corner_radii: &Corners<ScaledPixels>,
        color: Hsla,
        size: Size<ScaledPixels>,
        atlas: &Arc<dyn PlatformAtlas>,
        render_fn: impl FnOnce(Size<DevicePixels>) -> Vec<u8>,
    ) -> Option<AtlasTile> {
        let key = ShadowCacheKey::new(blur_radius, corner_radii, color, size);
        if let Some(tile) = self.entries.get(&key) {
            return Some(tile.clone());
        }
        let device_size = Size {
            width: DevicePixels(size.width.0.ceil() as i32),
            height: DevicePixels(size.height.0.ceil() as i32),
        };
        let bytes = render_fn(device_size);
        // Store in atlas and cache the tile
        // (actual atlas insertion will use the platform atlas API)
        None // placeholder — wire up in step 2
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
```

- [ ] **Step 2: Register the module and wire into the window draw cycle**

Add `mod shadow_cache;` to `lib.rs`. Add a `ShadowCache` field to the frame/window state. Before inserting a `Shadow` primitive, check the cache.

- [ ] **Step 3: CPU-side shadow rasterization function**

Add a function that renders a shadow into a BGRA pixel buffer using the same math as the Metal shader (gaussian blur over rounded-rect SDF). This runs once per unique shadow shape.

- [ ] **Step 4: Run `cargo build`**

Run: `cargo build -p kael 2>&1 | head -20`

- [ ] **Step 5: Commit**

```bash
git add crates/kael/src/shadow_cache.rs crates/kael/src/lib.rs crates/kael/src/window.rs
git commit -m "feat(shadows): add CPU-rendered shadow texture cache"
```

---

## Phase 4: Premultiplied Alpha Audit

### Task 7: Ensure all color paths use premultiplied alpha

tdesktop uses `Format_ARGB32_Premultiplied` exclusively. Kael already uses premultiplied in some paths (emoji conversion via `swap_rgba_pa_to_bgra`) but the shadow shader computes `hsla_to_rgba` and blends — verify all primitives output premultiplied color.

**Files:**
- Audit: `crates/kael/src/platform/mac/shaders.metal` (all fragment shaders)
- Audit: `crates/kael/src/platform/blade/shaders.wgsl`

- [ ] **Step 1: Audit Metal shaders for premultiplied output**

Check each fragment shader's return statement. Correct pattern:
```metal
float4 color = hsla_to_rgba(input_color);
return float4(color.rgb * color.a, color.a); // premultiplied
```

Incorrect pattern:
```metal
return hsla_to_rgba(input_color); // straight alpha — blending will be wrong
```

Search for `hsla_to_rgba` calls in fragment shaders and verify each multiplies rgb by alpha before return.

- [ ] **Step 2: Fix any non-premultiplied outputs**

For each fragment shader that returns straight alpha, multiply `.rgb` by `.a`.

- [ ] **Step 3: Verify Metal pipeline blend state expects premultiplied**

In `metal_renderer.rs`, check that render pipeline descriptors use:
```
sourceRGBBlendFactor = .one
destinationRGBBlendFactor = .oneMinusSourceAlpha
```

(This is the premultiplied-alpha blend equation.)

- [ ] **Step 4: Run `cargo build` and visual test**

Run: `cargo build -p kael 2>&1 | head -20`

- [ ] **Step 5: Commit**

```bash
git add crates/kael/src/platform/mac/shaders.metal crates/kael/src/platform/blade/shaders.wgsl
git commit -m "fix(render): ensure all shader outputs are premultiplied alpha"
```

---

## Phase 5: Glyph Origin Precision

### Task 8: Align glyph origins to device pixel grid consistently

Currently `glyph_origin.map(|px| px.floor())` floors the origin, but the subpixel_variant is computed from the fractional part. If two glyphs in the same line have origins that straddle a pixel boundary differently, text can appear uneven. The fix: compute glyph baseline in device pixels first, then extract the subpixel offset.

**Files:**
- Modify: `crates/kael/src/window.rs:3386-3413`

- [ ] **Step 1: Refactor glyph origin computation**

Replace the current logic at line 3386:

```rust
let scale_factor = self.scale_factor();
let glyph_origin = origin.scale(scale_factor);

let subpixel_variant = Point {
    x: (glyph_origin.x.0.fract() * SUBPIXEL_VARIANTS_X as f32).floor() as u8,
    y: (glyph_origin.y.0.fract() * SUBPIXEL_VARIANTS_Y as f32).floor() as u8,
};
```

With a version that explicitly separates the device-pixel integer part and fractional part:

```rust
let scale_factor = self.scale_factor();
let glyph_origin = origin.scale(scale_factor);

let device_x = glyph_origin.x.0;
let device_y = glyph_origin.y.0;
let fract_x = device_x - device_x.floor();
let fract_y = device_y - device_y.floor();

let subpixel_variant = Point {
    x: (fract_x * SUBPIXEL_VARIANTS_X as f32).floor() as u8,
    y: (fract_y * SUBPIXEL_VARIANTS_Y as f32).floor() as u8,
};
```

This is functionally identical but makes the intent explicit. The key change is in the bounds computation — use `floor()` consistently:

```rust
let bounds = Bounds {
    origin: point(
        ScaledPixels(device_x.floor()),
        ScaledPixels(device_y.floor()),
    ) + raster_bounds.origin.map(Into::into),
    size: tile.bounds.size.map(Into::into),
};
```

- [ ] **Step 2: Apply same fix to `paint_emoji` at line 3448**

The emoji path should use the same explicit floor logic for consistency.

- [ ] **Step 3: Run `cargo build`**

Run: `cargo build -p kael 2>&1 | head -20`

- [ ] **Step 4: Commit**

```bash
git add crates/kael/src/window.rs
git commit -m "fix(text): explicit device-pixel floor for glyph origin alignment"
```

---

## Phase 6: Validation & Integration Testing

### Task 9: Create a visual regression test harness

**Files:**
- Create: `crates/kael/examples/sharp_rendering_test.rs`

- [ ] **Step 1: Create a test app that renders common UI patterns**

```rust
use kael::prelude::*;

fn main() {
    App::new().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                bounds: Some(Bounds::centered(None, size(px(800.), px(600.)), cx)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| SharpTestView),
        )
        .unwrap();
    });
}

struct SharpTestView;

impl Render for SharpTestView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .m_4()
                    .p_4()
                    .rounded_lg()
                    .shadow_md()
                    .border_1()
                    .border_color(rgb(0xe0e0e0))
                    .child("Sharp text rendering test — The quick brown fox")
                    .child(
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(rgb(0x666666))
                            .child("12px subpixel text should have visible LCD color fringing on non-retina")
                    )
            )
    }
}
```

- [ ] **Step 2: Run the example and visually inspect**

Run: `cargo run --example sharp_rendering_test`

Check:
- Text has visible subpixel color fringing when zoomed in
- 1px borders are exactly 1 device pixel (not blurry)
- Shadows have no visible banding or clipping at edges
- Adjacent elements share pixel boundaries without gaps

- [ ] **Step 3: Commit**

```bash
git add crates/kael/examples/sharp_rendering_test.rs
git commit -m "test: add visual regression test for sharp rendering"
```

---

## Execution Order & Dependencies

```
Phase 1 (Tasks 1-3): LCD Text — independent, highest visual impact
Phase 2 (Tasks 4-5): Pixel Layout — independent of Phase 1
Phase 3 (Task 6): Shadow Cache — independent
Phase 4 (Task 7): Alpha Audit — independent
Phase 5 (Task 8): Glyph Precision — depends on Phase 1 (uses same raster_mode field)
Phase 6 (Task 9): Validation — depends on all above
```

Phases 1-4 can be developed in parallel. Phase 5 should follow Phase 1. Phase 6 is last.

---

## Risk Notes

1. **LCD subpixel on transparent backgrounds**: Subpixel rendering requires a known background color. When text is over transparency or complex backgrounds, fall back to grayscale. This is what macOS does natively — `CGContextSetShouldSmoothFonts` is ignored on transparent layers.

2. **Dark mode / light-on-dark text**: LCD subpixel rendering on dark backgrounds shows color fringing more aggressively. May need to detect background luminance and use grayscale for light-on-dark text.

3. **Shadow cache memory pressure**: Size-bucketing (4px increments) limits cache entries but large UIs with many unique shadow sizes could grow the atlas. Add an eviction policy if the cache exceeds a threshold.

4. **Dual-source blending availability**: True per-channel alpha blending requires `MTLBlendFactor.source1Color` which needs Metal 2.0+. The max-alpha approximation in Task 3 works everywhere but has slight color fringing loss at glyph edges.
