# kael Gap Analysis — Can it be the go-to Electron alternative for huge, beautiful, complex apps?

*Lead framework architect assessment. All findings trace to a verified audit (10 dimensions, adversarial verdicts). Refuted gaps are dropped; partially-confirmed gaps are softened to match their corrections.*

---

## 1. Executive summary

**Short answer: not yet — but the bones are unusually good for a young framework, and the path to "yes" is concrete and mostly additive.**

kael inherits GPUI's genuinely fast immediate-mode-over-retained batcher, ships a deep token system with live hot-reload, has best-in-class-for-native motion primitives (FLIP, spring integrator, ~30 easing curves), and has production plumbing most young frameworks lack (fail-closed signed auto-update, 3-OS secrets, real installers, native IME). For an AI-driven era it has a real edge: a 211-line `llms.txt`, per-page "Copy for LLM" buttons, and a 4-rung component-authoring ladder. A competent agent **can** produce a beautiful kael screen today.

But against the bar of "build ANY huge, beautiful, complex app the way you can on the web," four structural walls stand between kael and the goal, and they map almost exactly onto the owner's stated priorities:

1. **The canvas is fragmented and incomplete (priority #3).** There is no single `CanvasRenderingContext2D`. Two disjoint "canvas" surfaces exist — `DrawContext` (thin, ~14 methods, *zero real callers*) and `kael_engines/canvas.rs` (a serde data model with *zero consumers*). Neither does `drawImage`, pixel access, blend modes, arbitrary-path clip, or >4-stop gradients. You cannot build a Figma, a pixel editor, or an additive particle system. *(canvas_draw.rs, kael_engines/src/canvas.rs)*

2. **Styling/component freedom is shallow — apps WILL trend toward looking the same (priority #2).** There is no headless/unstyled primitive layer; ~85% of components cannot have their hover/active/focus restyled (the slot is overwritten and `debug_assert!`s in debug); variants are closed enums; grid is a facade (N equal columns only); no clip-path/mask, no real mix-blend-mode, no z-index on Style, gradients capped at 4 stops, no sticky/fixed, no RTL.

3. **Performance ceilings on large *static* UIs (priority #1).** No damage/dirty-region rendering — every vsync clears the full drawable and re-encodes the whole scene; the taffy layout tree is fully rebuilt every frame; no viewport culling outside virtualized lists; the already-built `kael_render_graph` is wired only to media, not the UI compositor.

4. **Memory eviction is written but unplugged (priority #1).** Four complete, tested memory subsystems (`GpuMemoryManager`, `ShadowLruTracker`, `kael_cache::MemoryCache`, `TileCache`) have **zero non-test call sites**. The glyph/shadow atlas never evicts, so a long-running app with churning glyphs leaks GPU memory for hours.

**What is genuinely excellent today:** type-batched instanced rendering with a naga-verified shader-layout guard; analytic erf-based GPU box shadows (Figma-grade); two-pass separable backdrop blur; the motion system (FLIP + spring + broad easing); auto-update integrity; the `llms.txt`/docs/templates surface; and a real in-app inspector.

**The good news:** most of the highest-impact gaps are *additive and already half-built*. The render graph exists. The spring integrator exists. The memory managers exist. The aspect-ratio field is wired. Much of Wave 1–2 is "connect what's already there," not "invent from scratch."

---

## 2. Maturity scorecard

| Dimension | Score | Verdict |
|---|---|---|
| Rendering performance & frame pipeline | 6/10 | Fast for the common case; no damage/dirty-region is the dominant ceiling for big static UIs. |
| Memory management & per-frame allocations | 5/10 | Good per-frame discipline, but all eviction machinery is unplugged — GPU atlas leaks over hours. |
| Styling expressiveness & "fluid div" parity | 6/10 | Strong Tailwind subset; walls at masks, blend modes, layered/multi-stop gradients, sticky, RTL. |
| Canvas / immediate-mode 2D parity | 4/10 | No unified Context2D; two dead surfaces; no drawImage/pixels/blend/clip — biggest priority-3 gap. |
| Animation & motion system | 6/10 | Above-average motion; spring not in the declarative path, no velocity-preserving interrupts, no reduced-motion. |
| Component freedom (anti-sameness) & theming | 4/10 | Deep tokens + hot-reload, but no headless layer and locked interaction styling drive sameness. |
| Layout engine vs CSS | 5/10 | Flexbox at parity; grid is a facade; no intrinsic sizing, sticky, calc, or RTL. |
| Visual polish primitives (premium look) | 6/10 | Excellent shadows/blur; gamut-clipped to sRGB, 4-stop gradients, no true gradient text. |
| Production-readiness breadth (ship gates) | 5/10 | Broad plumbing; Windows a11y is Name-only, a11y tree carries no geometry, macOS-only runtime verification. |
| Developer & AI-agent ergonomics | 6/10 | Great docs/`llms.txt`; killed by no hot reload + an undiscoverable scaffolder + a dead-twin dev API. |

---

## 3. The two headline questions, answered directly

### 3a. "Can you build ANY design like web divs, or will all kael apps look the same?"

**Honest answer: today, kael apps will trend toward looking the same — not because the palette is fixed (it isn't; theming is genuinely deep) but because the *structure* of restyling is shallow and there is no escape hatch into unstyled-but-correct components.** The owner's fear is well-founded.

What *works* and is real:
- A Tailwind-flavored CSS subset: flexbox at full parity, per-corner radii, linear/radial/conic gradients (sRGB/Oklab), 2D transforms, analytic box shadows, backdrop blur+saturate, a 4-channel color filter, declarative transitions + FLIP. *(div.rs, color.rs, style.rs)*
- A ~50-token `ThemeTokens` system with `Theme::custom`, live `install_theme`, and a TOML/JSON file bridge for designer hot-reload. *(theme/tokens.rs, theme/theme.rs)*
- A consistently-applied base-box override: 121 sites apply user `StyleRefinement` last via `.refine(&user_style)`.

The walls that produce sameness (all confirmed):

| Capability | Status | Why it matters |
|---|---|---|
| Headless/unstyled primitive layer (Radix-style) | **missing** | Root cause of sameness. To restyle radically you must rewrite the whole component (re-implementing focus/keyboard/state) on raw `div()`. There is no "correct behavior, my visuals" path. *(no `headless` module in kael_ui)* |
| Hover/active/focus override on most components | **missing (~85%)** | The hover slot is *overwritten* by the component default and `debug_assert!(hover_style.is_none())` fires in debug — a user `.hover()` is silently clobbered or panics. Two apps share identical interaction choreography. *(button.rs:291-306, div.rs:1289)* |
| Open/extensible variants | **missing** | Closed enums (6 button recipes, 3 input). No `Custom` recipe, no registry. A 7th look means hand-rolling from `div()`. *(button.rs:31-39)* |
| Per-component / subtree-scoped theming | **missing** | One flat global. Can't give a sidebar its own accent without forking components. *(theme.rs:237)* |
| Token-driven density/sizing | **missing** | Heights/paddings are hardcoded px literals; the spacing scale is populated but never consumed. A theme swap changes color, never proportion — a strong same-shape signal. *(button.rs:178-183)* |
| Style overrides reach internals | **missing** | Overrides hit only the outer box; inner label/field/option are private. The "inside" of every component looks the same. *(button.rs:331-357)* |

The most important **CSS capabilities still missing** for free-form design:
- **CSS Grid track sizing** — `grid_cols` is `Option<u16>` always compiling to `repeat(N, minmax(0,1fr))`. No `fr`/`minmax`/`auto`/`repeat(auto-fill)`/template-areas — so no sidebar+content+aside shell, bento, or responsive card grid declaratively. *(taffy.rs:302)*
- **clip-path / mask** — clipping is rectangle + rounded-rect only. No circle/polygon/alpha-mask. Every clipped surface is a rounded rect. *(style.rs:619-691)*
- **Real `mix-blend-mode`** — a correctness bug, not just a gap: the shader squares/screens the element's *own* color and never samples the backdrop, on all three backends. Duotone/multiply/difference are impossible and silently wrong. *(shaders.wgsl:636-678; correct math exists in `kael_render_graph` but the styling path never reaches it)*
- **Multi-stop & layered backgrounds** — gradients hard-capped at 4 stops (`[LinearColorStop;4]`), background is a single `Option<Fill>`. No aurora/mesh in one fill. *(color.rs:697, style.rs:241)*
- **position: sticky / fixed** — `Position` is only `Relative|Absolute`; the shipped data_table fakes sticky headers by hoisting a sibling out of the scroll region. *(style.rs:1399)*
- **Intrinsic sizing** (`min/max/fit-content`) — absent from the `Length` type; chips/tags can't hug content the CSS way. *(geometry.rs:3577)*
- **RTL / logical properties** — text shapes BiDi but the box model never mirrors. Shipping to RTL markets requires authoring two layouts. *(style.rs:189)*
- **Wide-gamut/HDR** — surface hardcoded BGRA8 + sRGB; saturated brand colors render duller than Safari/SwiftUI on the same P3 display. *(metal_renderer.rs:48,189)*
- **True gradient/masked text** — `GradientText` lerps one flat color *per glyph*, visibly stepped; no `background-clip:text`. *(gradient_text.rs:84)*

Two claims to *not* over-state (per verdicts): a declarative layering escape hatch **does** exist (`deferred()`/`anchored()`/`layer()` with priorities — popovers/menus/selects all use it), so "no z-index" is an API-ergonomics gap (no `.z(n)` on Style + no auto stacking contexts), not a missing capability. And `aspect-ratio` is fully wired to Taffy — it just lacks a one-line builder.

**Verdict:** you can build most flat/material/Tailwind UIs beautifully. You cannot reproduce Figma/Framer-grade compositions (masks, blend, layered/mesh backgrounds, sticky, distinctive interaction skins) without dropping to raw `div()`. The fix is a headless layer + open variants + the missing CSS primitives — see Wave 1.

### 3b. "Is there one canvas that does everything the web canvas does?"

**No. There is no `CanvasRenderingContext2D`, and the canvas story is actively fragmented into two dead-ends.** This is the single largest gap against priority #3.

- **`DrawContext`** (canvas_draw.rs) is the immediate-mode surface but is so thin it has **zero real callers** — even `examples/painting.rs` bypasses it for the legacy low-level `canvas(prepaint, paint)` form with raw `window.paint_path`.
- **`kael_engines/canvas.rs`** (`VectorPath`/`TileCache`/`CanvasExport`) *looks* like a real canvas with serde + export, but has **zero consumers anywhere in the tree** and never bridges to rendering. `CanvasExport` only validates config; it has no encoder.

They never interoperate, so a developer has no single "use this for all 2D graphics" answer — a guaranteed wall the moment an app outgrows `fill_rect`/`fill_circle`.

Canvas2D features still missing (from the parity table):

| Canvas2D feature | Status |
|---|---|
| Unified `getContext('2d')` object | **missing** (two disjoint surfaces) |
| `drawImage` / image compositing | **missing** — DrawContext has no image method at all; `window.paint_image` isn't reachable through it and ignores the transform stack |
| `getImageData`/`putImageData` (pixel access) | **missing** — no public read/write path; blocks editors, filters, procedural textures |
| `globalCompositeOperation` / blend on paths | **missing** — Path primitive has no blend field; vector content is alpha-over only (no additive glow) |
| Multi-stop gradient fills | **partial** — render on paths but capped at 4 stops, silently truncated |
| `clip()` arbitrary path | **missing** — rect-only; path fills can't even take the existing rounded clip |
| `isPointInPath` / hit-testing | **missing** — only `Bounds.contains()`; every interactive canvas hand-rolls point-in-polygon |
| `arc(center,angles)`/`ellipse`/`roundRect` + web method names | **partial/missing** — only SVG-style `arc_to`; `fill_circle` hand-builds arcs |
| flat `save()/restore()` + `ctx.translate/rotate/scale` | **partial** — closures only, forcing closure pyramids in loops/recursion |
| `textAlign`/`textBaseline`/multiline `fillText` | **missing** — always top-left+ascent, single pre-shaped line |

What's genuinely strong underneath: real lyon path tessellation cached once and replayed; **path stroking with dashes/caps/joins already exists and works** (`PathBuilder::stroke`, `DrawContext::stroke_path`, `kael::stroke()` — the audit's "no stroke" gap was *refuted*); gradient fills genuinely render on paths; a working rAF loop.

**Verdict:** the GPU machinery for a great canvas exists, but there is no coherent web-`<canvas>` API on top. The fix (Wave 1): pick `DrawContext` as THE canvas, grow it into a Context2D (`draw_image`, `with_blend`, `with_clip_path`, pixels, flat `save/restore`, web-named path ops), and demote `kael_engines/canvas.rs` to a document model that *compiles into* DrawContext commands.

---

## 4. Performance & memory deep-dive (ordered by impact)

**P0 — No damage / dirty-region rendering (critical).** Every live present path clears the full drawable and re-encodes the entire scene: Metal `MTLLoadAction::Clear` (metal_renderer.rs:1070-1076), Blade `InitOp::Clear` (blade_renderer.rs:821-834), DirectX clears the full RTV with scissor disabled (directx_renderer.rs:201,1657). Dirtiness is a single window-level bool (window.rs:135). A blinking caret in a 5,000-element dashboard re-encodes all 5,000 elements every vsync. The `structural_checksum` that could early-out (scene.rs:60) is wired only into the headless harness. **Fix:** scene-diff by checksum + per-primitive bounds → union damage rect → `Load` + scissor; early-out present when checksum is unchanged. Expose `Window::set_damage_tracking(bool)`. *(XL)*

**P0 — Layout tree fully rebuilt every frame (high).** `taffy.clear()` rebuilds the whole tree from scratch each frame (window.rs:2263, taffy.rs:49); per-frame layout cost scales with total node count even when nothing structural changed. **Fix:** retain the `TaffyTree`, reuse node ids for elements whose style+children hash is unchanged, call taffy partial relayout. Pairs with dirty-region. *(XL)*

**P0 — GPU memory eviction is unplugged (critical, memory).** `GpuMemoryManager` (gpu.rs:29-153, full LRU + `evict_to_budget`) and `GpuMemoryBudget::query` (works on Metal/DXGI/Vulkan) have **zero non-test callers**. The glyph/shadow atlas (metal_atlas.rs:37-90) only inserts; the only `remove` callers are image-drop, `CachedSurface::drop`, and lottie — never glyphs or shadows. `raster_bounds` (text_system.rs:309) is insert-only. `ShadowLruTracker` (shadow_cache.rs:230) is never instantiated outside tests. An editor with zoom, an i18n app, or anything animating font-size accumulates atlas textures forever. **Fix:** register every atlas texture with `GpuMemoryManager`, touch on hit, `evict_to_budget()` at end-of-frame seeded from `GpuMemoryBudget::query()`, add `last_used_frame` LRU to glyph/shadow tiles, expose `App::set_gpu_budget(bytes)`. *(L)*

**P1 — Render graph unused by the UI compositor (high).** `kael_render_graph` (transient aliasing, `changed_passes`, `assign_transient_memory`) is consumed only by the media engine; the window compositor hand-rolls `draw_scene_with_encoder` (metal_renderer.rs:1128) and cannot skip an unchanged blur pass or alias path/blur intermediates. **Fix:** route the UI path/blur/cached-surface passes through the graph. *(L)*

**P1 — No viewport/occlusion culling outside virtualized lists (high).** Only fully-clipped *primitives* are dropped (scene.rs:101); a generic off-screen-but-mounted subtree still runs `request_layout`+`prepaint`+`paint` every frame. Figma/Framer-style free-form boards with many off-screen nodes degrade unless you manually virtualize. **Fix:** in prepaint, skip paint+children when computed bounds don't intersect the viewport/nearest content_mask. *(L)*

**P1 — Full instance buffer re-uploaded every frame (medium, memory).** Every frame memcpy's all quads/sprites/shadows into a `StorageModeManaged` buffer and `did_modify_range`s the whole used span (metal_renderer.rs:1121). Cheaper on Apple unified memory than the discrete-GPU framing implies, but still wasted copy + coherency. **Fix:** split static (private, blit-once) vs a small triple-buffered dynamic ring; combine with scene diffing. *(L)*

**P2 — Path/BlurRects encoder restarts (medium).** Each Paths/BlurRects batch ends and reopens the main encoder and clears a full-viewport intermediate (metal_renderer.rs:1172). Heavy chart/canvas screens pay extra passes proportional to interleaved batches. **Fix:** size the intermediate to batch union bounds (scissor), group consecutive batches, route through the render graph. *(M)*

**P2 — Backdrop-blur kernel clamped to 16 taps at full res (high, polish/perf).** `radius = min(ceil(sigma*3), 16)` (shaders.metal:497) under-samples any blur beyond ~5.3px sigma, and each blur rect runs its own blit + 2 full-res passes with no downsampling/batching. Real >5px box/material blurs (styled_ext, overlays) are truncated. **Fix:** downsampled mip-pyramid / dual-Kawase; batch rects sharing a capture region. *(L)*

**P2 — `recycling_list` rebuilds an O(item_count) heights Vec + throwaway SumTree every frame (medium).** Defeats virtualization for 100k-row lists. **Fix:** cache heights in element state, recompute only on count change; lazy per-range estimation. *(M)*

**P2 — Canvas path deep-clone per draw command (high).** `fill_path` clones the entire tessellated vertex `Vec` and `DrawState` per command; `commands: Vec::new()` is reallocated every paint; `transformed()` clones the vertices a *second* time. Thousands of paths/frame = large transient churn — directly undercutting the canvas priority. **Fix:** store paths by `Arc`, intern `DrawState` with a `u32` index, retain+`clear()` the commands Vec across frames. *(M)*

**P3 — Quick wins:** orphaned `kael_cache`/`TileCache` (unbounded `HashMap<TileCoord, Vec<u8>>`) — integrate or delete (M); `RetainAllImageCache` is unbounded despite an LRU doc-comment — add a real bounded cache or fix the comment (M); no OS memory-pressure hook (M); `structural_checksum` skip-identical-frames as a cheap standalone win (S); `noise`/`grain` is hundreds of CPU quads not a shader (M).

---

## 5. Per-dimension findings (confirmed gaps only)

### Rendering performance & frame pipeline — 6/10
**Verdict:** Excellent batcher and virtualization; the per-frame full-redraw model is the wall for big static UIs. **Strengths:** type-batched instanced draws, O(n log n) bounds-tree z-order, analytic GPU shadows, naga-verified shader layouts, instance-buffer pooling.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | No damage/dirty-region present | Full clear + full re-encode every vsync on static screens | Scene-diff + damage rect + Load/scissor; checksum early-out | XL |
| High | Layout rebuilt every frame | Per-frame cost ∝ total nodes | Retain TaffyTree, partial relayout | XL |
| High | Render graph unused by UI | No transient aliasing/pass-skip | Route path/blur/cache passes through `kael_render_graph` | L |
| High | No viewport culling | Off-screen subtrees still paint | Cull in prepaint by bounds∩viewport | L |
| Medium | Full instance-buffer re-upload | Per-frame copy+coherency | Static private buffer + dynamic ring | L |
| Medium | Path/blur encoder restarts | Extra passes on chart/canvas | Sized intermediate + batch grouping | M |
| Low | Checksum not used to skip frames | Re-encodes identical frames | Cache last checksum, skip present | S |

*(Refuted/down-ranked: 60Hz pacing — present cadence is actually hardware-driven via CVDisplayLink/DWM/X11; only the `missed_display_intervals` diagnostic hardcodes 16.667ms. Reduced to a metrics nit, optional S.)*

### Memory management — 5/10
**Verdict:** Best-in-class per-frame discipline (arena double-buffer, line-layout cache, virtualized lists) sitting on top of completely unplugged eviction. **Strengths:** element Arena reset-in-place, double-buffered `LineLayoutCache` with 10k LRU cap, deterministic atlas reclaim on drop.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | GpuMemoryManager orphaned | No system-wide GPU ceiling | Wire into AtlasState, `evict_to_budget` end-of-frame, `App::set_gpu_budget` | L |
| Critical | Glyph/shadow atlas never evicts | GPU memory climbs for hours | `last_used_frame` LRU; instantiate `ShadowLruTracker`; cap `raster_bounds` | L |
| High | Canvas path deep-clone | Per-frame heap churn | Arc paths, interned state, retained Vec | M |
| High | Orphaned cache crates | Unbounded TileCache for infinite-canvas apps | Integrate `MemoryCache` LRU + byte cap, or delete | M |
| Medium | Image cache retain-all (misleading LRU doc) | Image-heavy apps never evict | `RetainLruImageCache(max_bytes)` + fix doc | M |
| Medium | recycling_list O(n) heights/frame | Defeats virtualization at scale | Cache heights, lazy per-range estimation | M |
| Medium | No memory-pressure hook | Can't shed caches under OS pressure | `App::on_memory_pressure(level)` from NSProcessInfo/DXGI/PSI | M |
| Low | CPU shadow rasterizer dead on Metal | Misleads future memory work | Delete or wire tracker + document | S |

### Styling expressiveness — 6/10
**Verdict:** Strong Tailwind subset; hard walls at masks, real blend, layered/multi-stop gradients, sticky, RTL.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| High | Blend modes never sample backdrop (correctness bug) | `.blend_mode(Multiply)` silently wrong | dst-read blend (offscreen/framebuffer-fetch) + W3C formula + `isolation()`; or rename honestly | L |
| High | No clip-path/mask | Only rounded rects; samey clipped surfaces | `clip_path(ClipPath{Circle/Polygon/Path})` + `mask(MaskSource)` in shader | XL |
| High | No position sticky/fixed | Sticky headers hand-rolled everywhere | `Position::Sticky/Fixed` against scroll handle | L |
| Medium | Transitions exclude translate/skew/filter/layout | Common slide-in snaps (FLIP covers layout move only) | Extend `ImplicitVisualStyle` w/ translate/skew/filter; `transition_layout()` opt-in | M |
| Medium | Element filters 4-channel only | No hue-rotate/invert/sepia on content | Add matrices to `ColorFilter`; `Styled::blur()` wraps existing `effect_layer` | M |
| Medium | Gradients 4-stop, single bg | No aurora/mesh/layered bg in one fill | Variable stop buffer + `SmallVec<[Fill;1]>` painted back-to-front | M |
| Medium | aspect-ratio has no builder | Wired but unreachable fluently | One-liner `aspect_ratio()`/`aspect_video()`/`aspect_square()` | S |
| Medium | Fixed pseudo-state enum | disabled/selected/checked hand-wired | `disabled(bool,f)`/`selected(bool,f)` helpers; peer via GroupHitboxes | M |
| Low | No style-level tokens / 3D transforms / pseudo-elements | Niche premium/Framer effects | Lower priority; `StyleVar<T>`, 4x4 transform, `before/after` helpers | M–XL |

*(Down-ranked per verdict: no-z-index → declarative `deferred()`/`layer()` priorities already solve overlays; the gap is just a `.z(n)` on Style. Severity low/medium.)*

### Canvas / 2D parity — 4/10
**Verdict:** GPU machinery is there; no coherent Context2D on top.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | Two disjoint canvases, no Context2D | No single 2D answer; guaranteed wall | Grow `DrawContext` into THE Context2D; demote engines/canvas to doc model | L |
| Critical | No drawImage | Photo/sprite/particle apps blocked | `draw_image(img, dst)` + `draw_image_src` carrying DrawState | M |
| High | No blend modes on paths | No additive glow/multiply for vector content | blend_mode on Path primitive + `with_blend(mode, ...)` | M |
| High | Gradient 4-stop cap | Multi-hue ramps silently truncated | Storage-buffer variable-length stops (WGSL+Metal) | M |
| High | clip() rect-only | No shaped/path clip; path fills lack even rounded clip | `with_clip_path(&Path)` + wire DrawContext to `with_rounded_clip` | L |
| Medium | No getImageData/putImageData | Editors/filters impossible | `draw_pixels` (putImageData) now; `read_canvas_region` via GPU readback later | L |
| Medium | No isPointInPath | Interactive canvas hand-rolls hit-test | `Path::contains()` via lyon `hit_test_path`; `DrawContext::hit_test` | M |
| Medium | Path API not web-shaped | Porting needs rewrites | `arc(center,angles)`, `ellipse`, `round_rect`, web-named aliases | S |
| Medium | save/restore closures only | Closure pyramids in loops | Flat `save()/restore()` + `translate/rotate/scale` mutators | S |
| Medium | No textAlign/baseline/multiline | Chart labels tedious | `fill_text(&str, origin, TextStyle{align,baseline})` + wrapped variant | M |
| Low | engines/canvas dead code | Misleads readers | Delete or make it the doc model that compiles to DrawContext | M |

*(Down-ranked: no-shadows-on-canvas → `effect_layer().drop_shadow()` already gives silhouette shadows; the gap is per-primitive `ctx.shadowBlur`.)*

### Animation & motion — 6/10
**Verdict:** Above-average; physics and accessibility are the gaps.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | Spring not in declarative path | Default motion reads as time-easing, not physics | `Motion{Timed,Spring}` enum + `with_spring`/`transition_spring(preset)` over `spring.rs` | L |
| High | No velocity-preserving interruption | Rapid toggles "restart"-stutter | Spring-back `ImplicitStyleAnimationState`; or estimate velocity from last 2 frames | L |
| High | No prefers-reduced-motion | Accessibility/production-gate failure | Read NSWorkspace/SPI_GETCLIENTAREAANIMATION/gtk; `cx.reduced_motion()`, fold into `animations_enabled()` | M |
| Medium | Single global transition config | No per-property timing | Map keyed by `AnimatableProperty`; `transition_property(prop,dur,easing)` | M |
| Medium | Keyframe builder opacity/scale/rotate only | Slides/colors drop to manual closures | Add translate/color/blur/corner to `StyledKeyframe` | M |
| Medium | Gesture→motion bespoke | Every draggable re-derives ~120 lines | `.draggable().on_release_spring(...)` modifier over PanGesture+SpringPoint | M |
| Low | Shared-element manual / no parallel tracks | Hero transitions tedious | `shared_element(layout_id)` registry; `MotionSet` for concurrent tracks | L |

### Component freedom & theming — 4/10
**Verdict:** Deep tokens + hot-reload, but shallow restyling = sameness.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | No headless primitive layer | Root cause of sameness | `kael_ui::headless` controllers (Select/Dropdown/Toggle) yielding state+props; ship styled defaults over them | XL |
| Critical | Hover/active/focus locked (~85%) | Identical interaction feel; user overrides clobbered | Per-component `hover_style/active_style/focus_style` merged AFTER defaults | L |
| High | Closed variant enums | Finite identical look set | `ButtonVariant::Custom(ButtonColors)` + `.colors()`; mirror Input | M |
| High | No per-component/subtree theming | Whole app shares one token set | Component token overlays + `ThemeScope` provider element | L |
| High | Hardcoded dimensions, not tokens | Theme swap can't change proportion | Density/control_height tokens consumed by components | M |
| Medium | Overrides can't reach internals | Inner parts look identical | Per-part style hooks (`field_style`, `option_style`) / slot map | M |
| Medium | Off-token hardcoded colors/shadows | Custom themes leak default greys/greens | Route literals through `ThemeTokens` | S |
| Low | No reusable style presets | Custom look not DRY → fall back to defaults | `StylePreset` + `apply_preset()`; ship alternate skins | S |

### Layout engine — 5/10
**Verdict:** Flexbox at parity; grid/intrinsic/sticky/calc/RTL are the gaps.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | Grid is N-equal-columns facade | No app shells/bento/responsive grids declaratively | `GridTracks` (px/fr/auto/minmax/repeat) + template-areas + auto-flow; Taffy already models these | L |
| High | No intrinsic sizing | Can't hug content the CSS way | `Length::{MinContent,MaxContent,FitContent}` → Taffy via CompactLength; `w_fit()` helpers | M |
| High | No position sticky | Sticky headers faked by sibling-hoist | `Position::Sticky` clamped against scroll container | L |
| High | No RTL/logical properties | RTL markets need two layouts | Per-subtree `LayoutDirection` + logical edges resolved at `to_taffy` | XL |
| Medium | No declarative z-index | `.z(n)` missing (overlays already work via `deferred`/`layer`) | `Style::z_index` + sort within stacking context | L |
| Medium | aspect-ratio unexposed | Wired but unreachable | One-line builder | S |
| Medium | No calc() | Can't do "fill minus 64px" directly | `DefiniteLength::Calc` + min/max/clamp (resolve against parent) | M |
| Low | No container queries | Component can't adapt to its own size | `with_measured_size(|size,cx|...)` over measured-layout path | L |

### Visual polish — 6/10
**Verdict:** Excellent shadows/blur; gamut, gradients, gradient-text are the gaps.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| High | No wide-gamut/HDR surface | Saturated brand colors look duller than browser | Configurable layer colorspace; opt-in RGBA16F + P3/EDR; `with_color_gamut()` (16F plumbing exists offscreen) | L |
| High | Gradient 4-stop cap | No aurora/mesh single fill | Variable stop buffer (WGSL+Metal) | L |
| High | Backdrop-blur 16-tap clamp + per-rect full-res passes | Large frost truncated; glass-heavy = perf wall | Mip-pyramid/dual-Kawase + batch capture | L |
| High | No gradient/masked text | Stepped per-glyph; no Framer hero headings | Glyph alpha-mask filled by Background; alpha-mask ContentMask | L |
| Medium | Noise/grain is CPU quads | Coarse, static grain | Procedural noise fragment effect `.grain()` | M |
| Low | ColorFilter lacks hue/invert/sepia/blur sugar | CSS filter chains incomplete | Add matrices + `.blur()` over `effect_layer` | S |
| Low | Inner-shadow no preset | Neumorphism awkward | `shadow_inner_*()` presets (engine already supports inset) | S |

*(Down-ranked per verdict: native vibrancy → `WindowBackgroundAppearance::Blurred` already gives real NSVisualEffectView/Acrylic/Wayland desktop vibrancy; gap is just a selectable material palette + `with_material()`, medium→lower. Glow → `glow_shadow()` + soft erf shadow falloff already exist; gap is a `.glow()` one-liner + showcase components not using it, low. Path-stroke gap **refuted** — full dashed/caps/joins stroking ships.)*

### Production-readiness — 5/10
**Verdict:** Broad gates; a11y and verification honesty are the weak pillars.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | Windows a11y Name-only | Every UIA pattern returns Err; per-element bounds zero → Section 508/EN 301 549 fail | Replace bespoke provider with `accesskit_windows::SubclassingAdapter` (deletes ~450 lines) | L |
| Critical | A11y nodes carry no geometry (all platforms) | VoiceOver/Orca can't draw focus or hit-test | Set `node.bounds` from laid-out bounds before `register_accessibility_node` (bounds in scope at div.rs:2406) | S |
| High | Live-region announcements discarded | Toasts/errors/status inaudible to AT | Drain buffer to `PlatformWindow::announce` → AccessKit Live update; add `role=alert` | M |
| High | Only macOS runtime-verified | Green build hides Win/Linux GPU/installer/UIA regressions | GPU-backed Win/Linux CI: launch sample, golden frame per backend, build+install, AccessKit snapshot; per-feature "verified on HW" badge | XL |
| Medium | No SemVer/deprecation policy | Can't build multi-year product pre-1.0 | `SEMVER.md`, minimize pub surface, `#[non_exhaustive]` on growing enums | L |
| Medium | Native crash opt-in + unsymbolicated | GPU/FFI crashes lost by default; poor triage | Make `diagnostics` native capture default; populate OsInfo; minidump/module map; `App::install_crash_reporting(endpoint)` | L |
| Low | Hand-rolled appcast XML parser | Valid feeds can wedge updates (integrity still safe) | Use quick-xml/roxmltree | S |
| Low | No OS drag-OUT | Can't drag rendered assets to Finder (NLE target) | `Window::start_drag` over NSDraggingSource/DoDragDrop/wl_data_device | L |

### Developer & agent ergonomics — 6/10
**Verdict:** Great reference surface; killed by the edit-compile loop and a hidden front door.

| Sev | Gap | Impact | Recommendation | Effort |
|---|---|---|---|---|
| Critical | No code hot reload | Order-of-magnitude slower than web HMR; guts trial-and-error agents | `kael::dev::watch_styles()` for tokens now; `subsecond`-style hot-patch behind `dev` feature + `cargo kael dev` | XL |
| High | Scaffolder undiscoverable + uninstallable | Front door hidden; docs say `cargo new` by hand; xtask is `publish=false` and loads templates from repo path | Publish `cargo install kael-cli` with `kael new --template ...`; embed templates via `include_dir`; document in README/getting-started/llms.txt | M |
| High | dev_tools.rs dead twin | Agents grep two `ElementInspector`s; inert one wastes iterations | Wire into real inspector or `pub(crate)` it; keep only FrameTimeline public; point to `kael_ui::devtools::install_inspector` | M |
| Medium | llms.txt is reference, not recipes | Agents reassemble whole screens → generic, error-prone | `recipes/` + `llms-recipes.txt` with 10–15 full screens lifted from the existing templates; optional `kael recipes` enum | M |
| Medium | Missing-id foot-gun | on_click/transition silently no-op without `.id()` | Debug warning naming file:line (inspector tracks source_location); typed `button()` wrappers; document as "common agent mistakes" | S |
| Low | Quickstart API drift | Two on_click idioms on page one | Standardize on `cx.listener(...)`; align README/getting-started/llms.txt | S |

---

## 6. Prioritized roadmap

### Wave 1 — Unblock web-like freedom & the unified canvas
*The owner's #2 and #3 priorities. This is what stops apps looking the same and gives one real canvas.*

1. **Unify the canvas into one Context2D (L).** Grow `DrawContext` into THE 2D API; demote `kael_engines/canvas.rs` to a doc model that compiles into DrawContext commands. *Why:* removes fragmentation, the root priority-3 wall. *API:* `canvas(size, |ctx, w, cx| { ctx.save(); ctx.translate(..); ctx.fill_path(&p, grad); ctx.draw_image(img, dst); ctx.restore(); })`.
2. **`draw_image` + `with_blend` + `with_clip_path` + pixels on DrawContext (M+M+L+L).** `fn draw_image(&mut self, img: Arc<RenderImage>, dst: Bounds<Pixels>)`, `fn draw_image_src(img, src, dst)`, `fn with_blend(BlendMode, impl FnOnce(&mut Self))`, `fn with_clip_path(&Path<Pixels>, ...)`, `fn draw_pixels(&ImageData, dst)`. *Why:* photo/editor/particle/masked apps.
3. **Headless primitive layer (XL).** `kael_ui::headless` controllers (`SelectController<T>`, `DropdownController`, `ToggleController`) owning focus/keyboard/state, yielding `render_with(|state| impl IntoElement)`. Ship current styled components as thin skins. *Why:* the single biggest anti-sameness lever.
4. **Per-component interaction styling + open variants (L+M).** `Button::hover_style/active_style/focus_style` merged AFTER defaults (fixes the clobber+`debug_assert`); `ButtonVariant::Custom(ButtonColors{...})` + `.colors()`. *Why:* distinctive look without forking.
5. **Real CSS Grid (L).** `fn grid_cols(self, impl Into<GridTracks>)` accepting px/rem/fr/auto/minmax/repeat(auto-fill); add `grid_template_areas`/`grid_auto_flow`. Keep `grid_cols_uniform(u16)`. *Why:* app shells, bento, responsive grids.
6. **clip-path/mask + true mix-blend-mode (XL+L).** `clip_path(ClipPath{Circle,Ellipse,Polygon,Path})`, `mask(MaskSource)`; dst-read blend + W3C formula + `isolation()` (port the correct math already in `kael_render_graph`). *Why:* Figma/Framer compositions + fixes a correctness bug.
7. **Variable-length gradient stops + layered backgrounds (M).** Stop buffer in WGSL+Metal; `Style.background: SmallVec<[Fill;1]>` back-to-front. *Why:* aurora/mesh/hero.
8. **position: sticky (L)** and **aspect-ratio builder (S).** `Position::Sticky` clamped against scroll handle; one-line `aspect_ratio()`.
9. **Spring in the declarative path + reduced-motion (L+M).** `transition_spring(preset)` over `spring.rs`; OS reduce-motion read + `cx.reduced_motion()`. *Why:* premium feel + accessibility gate.

### Wave 2 — Performance & memory hardening
*The owner's #1 priority — make HUGE apps stay fast and bounded.*

1. **Damage/dirty-region rendering (XL).** Checksum + per-primitive bounds → union damage rect → `Load`+scissor; early-out on unchanged frame. `Window::set_damage_tracking(bool)`.
2. **Wire GpuMemoryManager + atlas LRU + memory-pressure hook (L+L+M).** Register textures, `evict_to_budget()` end-of-frame from `GpuMemoryBudget::query()`, `last_used_frame` glyph/shadow eviction, `App::set_gpu_budget(bytes)`, `App::on_memory_pressure(level)`.
3. **Incremental layout (XL).** Retain `TaffyTree`, reuse node ids on unchanged style+children, partial relayout.
4. **Route UI compositor through `kael_render_graph` (L)** + **viewport culling in prepaint (L).**
5. **Canvas path Arc-clone fix (M)** + **static/dynamic instance-buffer split (L)** + **downsampled backdrop blur (L).**
6. **Cache/list bounding (M each):** integrate `TileCache`+`MemoryCache`, bounded image cache, cached recycling-list heights.

### Wave 3 — Production gates & polish
1. **Accessibility (S+L+M):** populate node geometry (the S fix — bounds already in scope), swap Windows to `accesskit_windows`, forward live-region announcements.
2. **GPU-backed Win/Linux CI with golden frames + install smoke (XL)** so "compiles" ≠ "works."
3. **Wide-gamut/HDR surface (L)** + **gradient/masked text (L)** + **default symbolicated native crash capture (L).**
4. **SemVer policy + `#[non_exhaustive]` discipline (L)** before 1.0.

### Wave 4 — DX & agent ergonomics
1. **Code hot reload (XL):** `kael::dev::watch_styles()` near-term; `subsecond` hot-patch + `cargo kael dev` medium-term. Biggest iteration-speed lever.
2. **Publish `cargo install kael-cli` with embedded-template `kael new` (M);** document the front door everywhere.
3. **Recipe corpus in `llms.txt` from the existing templates (M);** fix the `dev_tools.rs` dead twin (M); add missing-id debug diagnostic (S).

---

## 7. What to build first — this quarter's shortlist

Ordered. The theme is "connect what already exists, fix what is silently wrong, ship the two-line wins."

1. **A11y node geometry (S).** One change at div.rs:2406 — bounds are already in scope. Fixes VoiceOver/Orca focus on every platform. Highest impact-per-effort in the entire audit.
2. **Wire GpuMemoryManager + atlas LRU (L).** Stops the multi-hour GPU memory leak (priority #1). The manager and budget query already exist and are tested — this is pure plumbing.
3. **Per-component interaction styling + open variants (L+M).** Fixes the clobber/`debug_assert` bug and the closed-enum sameness — the most visible anti-sameness win short of the full headless layer.
4. **Unify DrawContext into a Context2D with `draw_image` (L+M).** Resolves the canvas fragmentation and unblocks the entire image-compositing app category (priority #3).
5. **Real CSS Grid track sizing (L).** Unblocks every app-shell/bento/responsive layout — the most-felt layout gap; Taffy already supports the model.
6. **Variable-length gradient stops + aspect-ratio builder + true blend-mode fix (M+S+L).** Three high-ROI styling wins; the blend-mode fix corrects a silently-wrong API.
7. **`kael::dev::watch_styles()` token hot-reload (M, scoped slice of the XL).** Extend the existing theme `FileWatcher` so spacing/color/typography iterate live without recompiling — the cheapest meaningful dent in the #1 DX problem while full hot-patch is scoped.
8. **Publish `kael-cli` with `kael new` (M).** The scaffolder already works; embed templates via `include_dir`, publish, and document. Gives agents and humans a real front door.

Everything above is additive, traces to confirmed audit findings, and leans on machinery kael already built. Land Wave 1 + the top of Wave 2 and kael moves from "a fast native toolkit with great docs" to "a credible Electron alternative for beautiful, complex, agent-built apps."

---

## 8. Implementation status — 2026-06-24 iteration

This section records what was actually implemented against the analysis, and the **specific, verified blocker** for everything that was not (so the remaining work is a precise execution plan, not a re-audit).

### Shipped — 30+ gaps bridged on `main` (each with a TDD test, `cargo build`, and `cargo clippy` green; full suite 952+ kael lib tests + the kael_ui suite passing)

- **A11y:** accessibility node geometry populated from layout bounds at all 13 registration sites; `prefers-reduced-motion` honored (`Platform::should_reduce_motion`, real macOS `NSWorkspace` read, folded into `Window::animations_enabled`).
- **Canvas / one-Context2D (priority #3):** flat `save`/`restore` state stack + `translate`/`rotate`/`scale`/`set_global_alpha`/`clip_rect`; `draw_image`; `fill_ellipse`/`stroke_ellipse`; `Path::contains` (isPointInPath, lyon hit-test); `draw_text_aligned` (textAlign); `stroke_rect`/`stroke_rounded_rect`.
- **Styling & layout:** `aspect_ratio`/`aspect_square`/`aspect_video`; **real CSS Grid track sizing** (`GridTrack` px/rem/fr/auto/min-content/max-content/fraction/minmax/repeat + `grid_template_columns`/`grid_template_rows`/`grid_auto_flow`, mapped to Taffy 0.9); `glow`/`shadow_inner` presets; `refine_style` reusable style presets.
- **Component freedom (priority #2):** open `Custom(Colors)` variants + `.colors()`/`.color()` for Button, IconButton, Input, Textarea, Alert, Progress, CircularProgress, AnimatedProgress, and Spinner; reusable style presets (`Styled::refine_style`); and the **start of the headless primitive layer** (the report's #1 anti-sameness lever) — `kael_ui::headless` with 8 unstyled-but-correct state machines (Disclosure, Toggle, Select, Tabs, Slider, Combobox, Accordion, Pagination), so apps can render any visual over correct interaction logic instead of forking styled components.
- **Memory (priority #1):** `TileCache` bounded with an LRU byte budget; `App::gpu_memory_budget()` exposes the real per-platform GPU memory query (Metal/DXGI/Vulkan); corrected the misleading `RetainAllImageCache` "LRU" doc.
- **Motion:** keyframe `translate`; implicit `.transition()` now animates translate, skew, and the 4-channel color filter; spring presets (`SpringPreset` + `transition_spring`); `Animation::stagger`.

### Not bridged — verified blockers and what each needs to unblock

| Gap (report ref) | Verified blocker | Prerequisite to unblock |
|---|---|---|
| Intrinsic sizing (`min/max/fit-content` width/height) | Taffy 0.9 `Dimension` only impls `TaffyZero`+`TaffyAuto` — no content sizing. | Taffy upgrade/fork exposing content sizing on `Dimension`. |
| `calc()` lengths | Taffy can't resolve `100% − 64px` statically. | A measured-layout pass, or Taffy calc support. |
| GPU atlas LRU eviction (#1/#2) | Evicting an in-use atlas tile risks **visual corruption/panic**; not validatable without on-device GPU stress testing. | GPU golden-image CI + last-used-frame atlas tracking; manager (`GpuMemoryManager`) is already a complete, tested primitive. |
| Multi-stop gradients, blend-mode dst-read, clip-path/mask, wide-gamut, gradient text, dual-Kawase blur | Shader work; not pixel-verifiable on this machine. | A golden-image GPU test harness (per-backend tolerance). |
| Windows AccessKit `SubclassingAdapter` swap | Windows-only; compile-checkable but never runtime-verifiable here. | Real Windows hardware / GPU-backed Windows CI. |
| `kael-cli` (`cargo install` + `kael new`) | crates.io publish-gated. | crates.io access + `include_dir` template embedding. |
| Semantic `success`/`warning` theme tokens | `ThemeTokens` has no `Default` and ~18 presets are full struct literals → ~36–72 brittle edits. | Refactor presets onto a `Default` base (`..ThemeTokens::base()`) first, then add tokens. |
| Headless-component layer (full) | Core state-machine controllers are **landed** (8 controllers, tested); remaining work is reskinning the 100+ styled components on top of them. | Incremental per-component refactor (not gated). |
| Damage/dirty-region rendering, incremental layout, code hot-reload | XL multi-week rewrites; the first two also carry corruption risk unverifiable here. | Dedicated multi-week workstreams. |

**Bottom line:** Wave 1's verifiable, non-gated surface is substantially landed. The remainder is a multi-engineer, multi-week program gated on a Taffy upgrade, a GPU golden-image CI, real Windows hardware, and crates.io — not on missing design.