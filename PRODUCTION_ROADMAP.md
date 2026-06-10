# kael Production Roadmap
## From promising prototype to a production framework for any cross-platform desktop application

---

## 0. Direction — read this first

**Kael is a general-purpose desktop application framework.** That is the
product, and this roadmap is read through that lens. See [VISION.md](VISION.md)
for the full statement of direction.

This document was originally drafted with a video editor as the headline goal,
and it retains a deep, source-verified audit of what a professional NLE would
demand from the framework. That audit remains valuable — a video editor is the
single most demanding workload a desktop framework can host, and it is kept as
our stress-test lens — but the *priority ordering* is now explicitly:

1. **Lens A (general-app production gates) is the primary track.** Accessibility,
   packaging/signing, update integrity, crash reporting, and text correctness
   (Phase P3, plus the P-hotfix) block every serious app and come first.
2. **The GPU substrate work (P0-A/B/C/E) is re-scoped as public framework API.**
   Offscreen render targets, custom shaders, compute pipelines, the render
   graph, and GPU memory budgeting are the most-requested general capabilities
   in the GPUI ecosystem — they ship as documented, app-facing features for
   *all* applications, with the media stack as just one consumer.
3. **Lens B (the video-editor bar) is the optional media track.** The
   media-specific workstreams (P0-F/G/H, P1, P2, P4) continue as the layered,
   feature-gated `kael-media`/`kael_audio`/`kael_engines` stack — subordinate
   to, and never blocking, tracks 1 and 2. Nothing media-specific lands in the
   core `kael` crate.

Several items below have landed since this audit was written — among them the
auto-updater signature/hash verification (P-hotfix), radial and conic gradients
in the core styling API, the `kael_render_graph` and `kael_gpu_budget` crates,
`kael_secrets`, UAX#9 BiDi, and UAX#14 line breaking. Treat line-level claims as
a snapshot of the audit date; the architecture and sequencing analysis stands.

---

## 1. Executive summary

**Verdict:** kael is a genuinely competent **2D application-UI toolkit** — inherited from Zed/GPUI, with three real GPU backends (Metal/DirectX 11/Blade-Vulkan), a production-grade text-input/keybinding/focus stack, a mature widget set, and solid windowing on all three platforms. It is **usable today for a standard desktop app on macOS**, and **not production-ready** as an Electron replacement that "handles any load," nor remotely ready to host a professional GPU video editor. The maturity gradient is steep: rendering/input/state are 2–3/5 for general UI; media (video + audio), accessibility, and packaging/distribution are 1/5 and are largely facade — APIs that compile and test but execute almost nothing real. **For the video-editor lens specifically, the GPU substrate is effectively 0/5: it does not exist yet.**

**The single biggest architectural gap is the GPU/media core.** kael's renderer is a *fixed-function 2D primitive batcher* with a closed set of 8 primitive variants (`crates/kael/src/scene.rs:269-279`), drawn by a fixed ~10-pipeline list per backend (`crates/kael/src/platform/mac/metal_renderer.rs:135-152`). There are **no app-facing offscreen render targets, no compute pipelines (verified: 0 occurrences of `@compute`/`numthreads`/`MTLComputePipelineState` across the tree), no custom-shader hook, and no render-graph.** The only render-target pixel format is hardcoded 8-bit BGRA on every backend (`metal_renderer.rs:48` `BGRA8Unorm`; `directx_renderer.rs:30` `B8G8R8A8_UNORM`). The video-frame display path exists **only on macOS**, where `draw_surfaces` hard-`assert_eq!`s a single pixel format — NV12 *full-range* (`kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`, `metal_renderer.rs:1575`) — with one fixed YCbCr matrix in the shader (`shaders.metal:1057-1060`). On Windows `draw_surfaces` is a literal no-op that returns `Ok(())` after an empty-check (`directx_renderer.rs:803-809`); on Linux Blade there is no real surface path at all. The media engine is software-only FFmpeg that decodes whole files into a RAM `Vec` capped at 256 frames / 128 MB (`crates/kael-media/src/lib.rs:42-43`), with **no hardware decode** (no `hw_device_ctx`/`get_format` anywhere), backward seek implemented as `restart()`-to-frame-0 (`lib.rs:364`), and frame decode running on the paint thread (`crates/kael/src/media_playback.rs`). There is **no encoder or muxer of any kind** — the `VTCompressionSession` bindgen in `crates/media` (`crates/media/src/bindings.h:5`) is dead code consuming only capture buffers. Audio is a single-stream `rodio` player; `crates/kael_audio/src/effects.rs` is an 11-line volume clamp. The NLE `Timeline`/`TimelineTrack`/`TimelineClip` and `ThumbnailCache` types in `kael_engines/src/media.rs:45-160` are inert structs depended on by nothing. Until the renderer grows render-targets + a pass/shader/compute API and a real media engine is built on top, **every** video-editor capability — compositing, effects, transitions, color, export — is blocked.

**One live security hole is fixed before any phase.** The runtime auto-updater is a present, exploitable remote-code-execution path, not a future hardening item: `download_update` reads the package bytes, writes them to an attacker-guessable temp path (`temp_dir/gpui_update_{filename}`, `auto_updater.rs:243-251`), and sets `ReadyToInstall` with **no SHA-256 and no signature check**; `install_and_restart` then runs the installer on that file. The `sparkle:edSignature`/`sparkle:dsaSignature` attribute is parsed and immediately discarded (`auto_updater.rs:364-365`), and the verification primitive that already exists — `kael_release::update::verify_manifest` (`crates/kael_release/src/update.rs:144`, backed by `ed25519_dalek`) — is **never imported** by the updater. This ships as a standalone hotfix in week 1, decoupled from the entire media program (see §9 action 1).

**The plan:** A phased program that (P0) rebuilds the renderer foundation (offscreen targets, render-graph, custom GPU passes, a color-managed linear ≥16F pipeline decided up front, cross-platform zero-copy video surface) and the media-engine spine (hardware decode, frame-accurate streaming seek, encode/export, a sample-accurate audio mixer that owns the master clock) — then (P1) lands a **usable single-track-edit-and-export vertical slice** to validate the architecture end-to-end, then (P2–P5) builds the full NLE (multi-track compositor, effects/color/transitions, timeline UI, project model) in parallel with hardening the general-framework production gates: cross-platform parity, accessibility (via AccessKit), packaging/signing/notarization/auto-update with integrity verification, native crash reporting, real GPU profiling and golden-image testing, and performance-at-load. Native C/C++ integration is concentrated and deliberate: **FFmpeg (hardware-first, LGPL-pinned)** for codecs, **OpenColorIO** for color (staged), platform codec SDKs at the decode→texture boundary; everything else stays Rust. **Keep the three native GPU backends — do not migrate to wgpu or adopt Skia.**

**Two things the team must build on, not rebuild.** The audit's earlier draft was pessimistic in several places that source contradicts, and the roadmap below corrects them: a keyframe/easing primitive already exists (`animation.rs:126` `Easing`, `:159` `CubicBezier`, `:232` `Keyframes`, `:292` `keyframes()`) — media keyframes are an *extension*, not a from-scratch build; a transactional undo core exists (`app_runtime.rs:330` `UndoableChange`, `:346` `UndoTransaction` with grouped changes) — document-scale undo builds *on* it; 1D virtualization (`elements/uniform_list.rs`, `elements/recycling_list.rs`) and a tree-capable `VirtualDataSource` (`virtual_data.rs:125`) already ship; cross-platform `wry` webview exists for Windows **and** Linux (`platform/windows/webview.rs`, `platform/linux/webview.rs`) — macOS is the gap, not the reverse; Windows already implements GPU device-loss recovery (`directx_devices.rs` `try_to_recover_from_device_lost`, `events.rs:1231` `handle_device_lost`, `WM_GPUI_GPU_DEVICE_LOST`) — Metal/Blade are the gap; and `#![deny(missing_docs)]` is already enforced on the public crate (`kael.rs:2`), so the docs problem is examples/quality, not raw coverage.

**Calendar honesty.** The three highest-risk items — cross-platform zero-copy decode→texture interop, the offscreen/pass renderer re-architecture across three hand-written backends with three shader languages, and AccessKit accessibility from a geometry-less per-frame tree — are each *multiple* XLs, and P0→P1 is mostly serial, not overlapping. A realistic P0+P1 with a ~4–6-engineer team is **10–14 months, not the ~7–10 a naive read of the phase table implies**; the full program is multi-year. The opening interop and licensing spikes (§9) can move this range materially in either direction, so the calendar is stated as a range gated on those spike outcomes.

---

## 2. Where kael stands today

Maturity scale: **0** = absent · **1** = facade/stub (compiles, tests enum mappings, executes ~nothing real) · **2** = partial, not production · **3** = usable with gaps · **4** = production-grade with minor gaps · **5** = best-in-class. Sorted by risk (lowest maturity / highest blocking impact first). Where the two lenses diverge, both ratings are shown (General / Editor).

| Subsystem | Mat. (Gen/Ed) | Blocks general prod? | Blocks video editor? | One-line state |
|---|:--:|:--:|:--:|---|
| **Rendering & GPU pipeline** | **2 / 0** | Partial (8-bit SDR) | **YES — hard** | Solid 2D UI compositor; **no offscreen targets, no compute, no custom shaders, no render-graph**; 8-bit BGRA hardcoded as only RT format; for editing the substrate does not exist |
| **Media — Video** (decode/seek/NLE/composite/encode) | **1** | Partial (decode on UI thread) | **YES — hard** | Software FFmpeg, whole-file→256-frame/128 MB RAM, no HW decode, **no encode/export** (dead VT bindgen), `restart()`-to-0 seek, no compositor; NLE types inert |
| **Media — Audio** (mixer/clock/DSP) | **1** | Partial (recording) | **YES — hard** | Single-stream `rodio`, no mixer/bus/DSP (11-line "effects"), wall-clock not device-clock (A/V drift), no waveforms, no export mixdown |
| **Accessibility** | **1** | **YES (legal gate)** | YES | macOS NSAccessibility absent (role-stub conversions only); Linux AT-SPI is `dbus-send` theater; Windows UIA has no patterns; nodes have no geometry; per-frame tree rebuild; **AccessKit not even a dependency** |
| **Packaging / Distribution / Updates / CI** | **1** | **YES** | YES | Bundlers are stubs (.app dir+plist / installer.json / .desktop — no .dmg/MSI/.deb/AppImage); **CI packages `xtask` itself**, never a real app; **auto-updater is a live RCE** (no hash/sig verify before install); only Rust panics captured |
| **Layout & Text engine** | **2** | YES (i18n) | YES (per-frame layout) | Full Taffy rebuild every frame (no incremental); no BiDi/RTL, no UAX#14 line-breaking, no variable fonts; 3 divergent shapers (CoreText/DirectWrite/cosmic_text); **no video-export text/title path** |
| **Multi-process / IPC / Extensions / Security** | **2** | YES | YES | Real worker harness but JSON-over-socket only (no shared-mem/zero-copy); "WASM sandbox" doesn't exist; no OS sandboxing/resource limits |
| **DevTools / Testing / Diagnostics / Perf** | **2** | YES | **YES** | Good test harness; **no real GPU timing**, **CI perf gate runs `simulate_workload`/`simulate_frame` — never opens a window or touches the GPU yet gates merges**; no golden-image tests; no hot reload; devtools UI is dead code |
| **Ancillary crates** (storage/net/i18n/cache/doc/…) | **2** | Partial | Partial | `sum_tree`/`http_client` solid; `kael_net`/`kael_cache`/`kael_i18n` orphaned/toy; **no secure credential storage anywhere** (plaintext `kael_net::TokenStore`) |
| **State / Reactivity / Async runtime** | **3** | Partial | YES | Solid entity model; single `RefCell<App>` (`app.rs:67` `AppCell`) serializes all mutation; coarse reactivity; unbounded FG queue; **transactional undo core already exists** (`UndoableChange`/`UndoTransaction`) but not document-scale |
| **Input / Events / Gestures / Focus** | **3** | Partial | YES | Strong keybindings/IME (macOS); no pen/tablet, no raw multi-touch; scroll momentum/precise-delta macOS-only; no event coalescing |
| **Platform — Windows** | **3** | Partial | YES | Competent DX11 UI; **device-loss recovery present**; `draw_surfaces` no-op (no video); no HW codec; capture stubs; 8-bit SDR; first-adapter (iGPU risk) |
| **Platform — Linux** | **3** | Partial | YES | X11+Wayland usable for UI; **`wry` webview present**; no real surface path; capture backends fake-device stubs; no presentation-time/vblank feedback; 8-bit sRGB; no touch/tablet |
| **Elements / Widgets** | **3** | Partial (dock) | YES | Strong general widgets; **1D virtualization + tree data source exist**; genuine gaps: timeline canvas, color picker/wheels, curve/keyframe editor, scrub number field; dock system is data-only stub |
| **Platform — macOS** | **3** | Partial | YES | Most mature; **no media pipeline at all**; video surface 8-bit NV12-full-range + fixed matrix; no offscreen/export; **CVDisplayLink with documented occasional segfaults** (`display_link.rs:92`); no sandbox bookmarks |

**Reading the matrix:** the framework's *chrome-grade* layers (windowing, input, widgets, state) cluster at 3/5 — good enough to ship general apps with hardening, and several carry usable primitives the team should extend. The *media/graphics core and the production-ship gates* (video, audio, a11y, packaging, GPU-devtools) cluster at 1–2/5 — and for editing, the GPU substrate is 0/5 — and are where the multi-person-year investment lives.

---

## 3. The two target bars (condensed)

### 3a. Production-framework bar (Electron replacement, "handle any load")

| # | Category | P | Must-have (condensed) |
|---|---|:--:|---|
| 1 | Rendering/GPU & pacing | P0 | 3 backends + software fallback; **device-loss recovery (only Win exists today)**; vsync-correct pacing on mixed-refresh/VRR; **GPU-memory budget query + eviction**; idle-to-0fps |
| 2 | Text/i18n | P0 | HarfBuzz-class complex shaping; **BiDi (UAX#9)**; UAX#14 line-break; grapheme-correct editing; system font fallback + color emoji; full IME; RTL *layout* mirroring; locale format/plural |
| 3 | Accessibility | P0 | Real a11y tree on all 3 OS (**AccessKit**: UIA/NSAccessibility/AT-SPI); screen readers; high-contrast/reduce-motion/text-scale; WCAG 2.2 AA |
| 4 | Input | P0 | IME preedit geometry; layout-independent keys; precise/momentum scroll + gestures; OS DnD in/out; multi-format clipboard; pen/tablet |
| 5 | Windowing/HiDPI | P0 | Per-monitor fractional DPI (mid-drag); multi-window/popups; vibrancy/custom titlebar; tray/menu-bar; Wayland+X11; hotplug/sleep/RDP survival |
| 6 | Packaging/signing | P0 | **Signed+notarized installers, all 3 OS, automated in CI**: .dmg/.pkg+notarize+hardened-runtime, MSI/MSIX+Authenticode, AppImage/Flatpak/.deb/.rpm — **including bundled dylibs** |
| 7 | Auto-update/crash | P0 | **Signature+hash-verified before install** (live RCE today), rollback, staged rollout, delta; native+Rust+GPU-fault crash capture, symbolicated minidumps |
| 8 | Security/sandbox | P1 | App Sandbox/AppContainer/Flatpak confinement; capability-scoped access; audited `unsafe`; `cargo-deny`/SBOM |
| 9 | DevTools/test/profile | P0/P1 | Inspector; **GPU/frame profiler**; **golden/snapshot pixel tests**; headless UI test; **real (not simulated) perf gate** — pulled to P0; hot reload (P1) |
| 10 | Docs/DX | P1 | `missing_docs` already enforced — gap is **examples + `create-app` scaffolder**; signing/packaging/update recipes |
| 11 | Ecosystem/interop | P1 | Clean native FFI; **embeddable webview** (Win/Linux exist; add macOS); native dialogs/notifications/print; plugin architecture |
| 12 | API stability | P1 | Published SemVer/deprecation policy; deliberate `pub` surface; fork/upstream strategy |
| 13 | Perf/memory budgets | P0 | Steady 60/120fps, <2-frame latency; bounded memory over hours; fast cold start; 100k-row virtualized lists |

### 3b. Video-editor bar (CapCut/DaVinci/Premiere class)

| # | Pillar | P | Must-have (condensed) | Owner layer |
|---|---|:--:|---|---|
| V1 | **GPU compositing & effect graph** | P0 | Offscreen RTs (RGBA16F); multi-pass/render-to-texture; custom fragment **+ compute** shaders; node DAG evaluator with **time-varying cache invalidation**; blend modes on *video* layers; masks/mattes; transforms | Renderer + new graph crate |
| V2 | **Media engine** | P0 | HW decode→GPU texture (VideoToolbox/D3D11VA/VAAPI); **frame-accurate streaming seek**; bounded frame cache; **HW encode + mux + timeline export**; proxy media; **ingest robustness (VFR/rotation/multi-track/alpha/corrupt)** | `kael-media`/new `kael-codec` |
| V3 | **Color management** | P0/P1 | **Linear working space + ≥16F + bit-depth decided UP FRONT (P0)**; per-clip input transform (709/2020/Log); 1D/3D LUT; HDR (PQ/HLG, 10-bit); scopes (grading P1/P4) | Media + Renderer |
| V4 | **Audio engine** | P0 | Multi-track mixer→buses→master; sample-accurate device clock (A/V master); DSP/effects; resample to project rate; waveforms; offline mixdown; metering | `kael_audio` rebuild |
| V5 | **Timeline data model** | P1 | Tracks/clips/gaps; trim/ripple/roll/slip/slide; transitions; nesting; markers; **keyframe tracks w/ bezier (EXTEND existing `Keyframes`/`CubicBezier`)** | App over `sum_tree` |
| V6 | **Editor widgets** | P1 | 2D-virtualized timeline canvas; ruler/playhead/scrubber; **scrub number field, color wheels, curve editor** (each its own multi-week effort); dock manager; tree/outliner (extend `VirtualDataSource`) | Widgets |
| V7 | **Transport & pacing** | P0 | J-K-L/step/loop/in-out; decode+graph+present scheduled to vsync against media clock; **audio↔present clock contract co-designed in P0**; preview-vs-export quality | Renderer+Media+Audio |
| V8 | **Project/undo at scale** | P1 | Versioned/relinkable project format + **migration-test discipline**; autosave/crash recovery; **document-scale transactional undo (build on `UndoTransaction`)** | App (framework primitives) |
| V9 | **Perf at 4K/8K, many clips** | P0 | Zero-copy HW frames; tiered RAM/VRAM/disk frame cache w/ **GPU-memory-budget eviction**; dirty-subtree graph caching; background decode/proxy/export/**filmstrip+waveform** jobs; backpressure | Renderer+Media |
| V10 | **Export determinism** | P0 | **Choose canonical export path** (single-backend or tolerance-bounded multi-backend); cross-driver GPU output is NOT bit-identical, so "preview == export" must be *designed*, not assumed | Renderer + Media |

---

## 4. Critical architectural gaps (deep dive)

These five cross-cutting gaps are *foundational*: nearly every video-editor capability and several general-framework gates depend on closing them. They define the P0 work. (The auto-updater RCE in §1 is a sixth, but it is a same-week hotfix rather than an architecture program — see §9 action 1.)

### 4.1 The renderer is a closed 2D primitive batcher — the keystone blocker

**What exists:** `Scene` holds a closed enum of 8 primitive variants (`scene.rs:269-279`); each backend draws them with a fixed list of ~10 `RenderPipelineState`s (`metal_renderer.rs:135-152`); shaders are a build-time-compiled fixed set (`shaders.metal` is ~1.4k lines of fixed-function MSL, with HLSL and WGSL/Blade equivalents that must be kept in lockstep), validated via `naga` in `build.rs`. The only offscreen textures are *internal* (path-MSAA resolve, blur intermediates, cached-surface snapshots), all `pub(crate)`, all 8-bit BGRA/sRGB (`metal_renderer.rs:48`, `directx_renderer.rs:30`). There is **zero** compute pipeline (verified 0 hits) and **no** way for app code to register a shader, allocate a render target, or chain passes.

**Why it blocks everything:** An NLE composites by evaluating, *per output frame*, a DAG of GPU passes into offscreen buffers — decode a clip to a texture, apply its effect chain into intermediate targets, transform it, blend it over the track below, run a transition between two rendered sources, apply a color LUT, present. None of those operations have anywhere to execute. Adding a single new visual effect today requires hand-editing the enum + all 3 backends + all 3 shader files in lockstep. This is the dead-end that sank the original video-editor attempt. **Re-architecture realism:** re-expressing the existing UI + blur on a generic pass API while keeping current output pixel-perfect is itself a large, regression-prone project across three hand-written backends and three shader languages — it is multiple XLs, not one, and golden-image regression tests (P0, see §4.5) must guard it from day one.

**What must be built (P0):**
1. **Public `RenderTarget`/offscreen-texture API** — allocate typed targets (incl. `RGBA16F` for HDR/linear), render scenes/passes into them, sample them as inputs. Make the swapchain/RT pixel format configurable rather than the hardcoded BGRA8. Thread through Metal, DX11, Blade (multiple XLs).
2. **Generic pass/pipeline abstraction** — bind inputs (textures + uniform block) + output target + pipeline; run. Re-express the existing UI pass and blur on top of it under golden-image guard.
3. **Custom shader registration + compute pipelines** — app/framework-supplied MSL/HLSL/WGSL (or single WGSL → `naga` → backend) with a declared binding contract; add `MTLComputePipelineState`/CS/`@compute` paths. **Constrained** (fixed effect-pass library, not arbitrary user shaders, for 3-backend portability + security).
4. **Render-graph layer** (new crate, e.g. `kael-render-graph`) — declare passes + read/write resources; auto-allocate transient targets; schedule; insert barriers. **Cache-key design for time-varying inputs is the hard part and gets its own design owner** (a clip frame changes every frame; an effect param keyframes) — naïve sub-tree hashing is the usual source of NLE preview corruption, so the invalidation model (frame-PTS + param-hash + topology) is specified before caching is implemented.
5. **GPU-memory budget API** — query device/host budget and drive eviction; there is no app-facing allocator/budget layer today. This is the owning workstream for general-bar item #1 ("GPU memory budgeting") and feeds V9's tiered cache.
6. **Cross-platform zero-copy video-surface path** — see §4.4 (its own gap because it is the single most underestimated item).

### 4.2 The media engine is a preview decoder, not an editing engine

**What exists:** `kael-media` does software FFmpeg decode + CPU swscale to BGRA, decoding whole files into a RAM `Vec` capped at 256 frames / 128 MB (`lib.rs:42-43`), forward-sequential with `restart()`-to-frame-0 on backward seek (`lib.rs:364`). Frame decode runs **synchronously on the paint thread** (`media_playback.rs`). There is **no hardware decode** anywhere, **no encoder/muxer at all** (the `VTCompressionSession` bindgen in `crates/media`, `bindings.h:5`, is dead code only consuming capture buffers), and the NLE `Timeline`/`TimelineTrack`/`TimelineClip` types plus `ThumbnailCache` in `kael_engines/src/media.rs:45-160` are inert structs depended on by nothing.

**What must be built (P0–P2):**
- **Hardware decode → GPU texture** per platform (VideoToolbox / D3D11VA+MF / VAAPI+NVDEC) via FFmpeg `hw_device_ctx` + `get_format`, output staying GPU-side and feeding §4.4's surface path zero-copy.
- **Frame-accurate streaming seek** — `av_seek_frame` to nearest keyframe + decode-forward to target PTS; per-clip keyframe/PTS index; bounded read-ahead cache; decode on worker threads, paint samples only ready frames (removes the paint-thread decode).
- **Media-ingest robustness** (early P2 workstream, where CapCut clones actually fail in the field) — variable frame rate (VFR), rotation/display-matrix metadata, multiple audio tracks, embedded timecode, image sequences, alpha/ProRes 4444, still images, and broken/partial files. The current whole-file→256-frame path handles none of these.
- **Encode + mux + export pipeline** — HW encoders (VideoToolbox/NVENC/QSV/VAAPI) + software fallback; rate control, B-frames/GOP, color-tag signaling; mux to MP4/MOV with A/V sync; a timeline-frame iterator → compositor (offscreen render at arbitrary resolution, decoupled from window) → encoder → muxer; job/progress/cancel; audio mixdown.
- **Multi-track GPU compositor** (on §4.1) — per-track textures + transforms + blend modes + effect stacks → program frame; proxy/optimized-media workflow; **filmstrip (sparse-keyframe thumbnail) and audio-peak/waveform precompute pipelines** for timeline-scale display; tiered RAM/VRAM/disk frame cache with byte-budget eviction driven by §4.1's GPU-memory budget API.

### 4.3 The audio engine is a single-stream player

**What exists:** one `rodio` `Sink` per `AudioHandle` playing one fully-decoded `Arc<DecodedAudio>` (`kael-media/src/lib.rs`, `kael_audio/src/player.rs`); position is wall-clock `Instant`, not the device sample clock → unbounded A/V drift; `effects.rs` is an 11-line volume clamp; platform session/spatial backends are `-planned` strings.

**What must be built (P0/P1):** Replace `rodio` with a **`cpal`-driven real-time mixing graph**: persistent output device, N voices summed in a callback, the **device sample-counter as the timeline master clock** (video scheduled against it), per-track gain/pan/automation + DSP inserts, `rubato` resampling to project rate, fade/crossfade, waveform/peak generation, metering, and an **offline (faster-than-real-time) mixdown** mode for export. Decode audio via FFmpeg (single demuxer for A/V-sync unity) into the mixer. **The clock contract between this engine (audio master) and the video present path (§4.4) is co-designed in P0**, not discovered at the P1 integration point — A/V-sync correctness cannot be validated until the two are designed against the same clock model.

### 4.4 Cross-platform zero-copy video surface — the single most underestimated item

Broken out from §4.1 because it is the highest-uncertainty, highest-effort item and gates **all** video display. **What exists:** ONLY macOS has a real path — `CVMetalTextureCache` import (`metal_renderer.rs:147,314,1549+`) — and even it `assert_eq!`s a single hardcoded NV12 *full-range* format (`metal_renderer.rs:1575`) with one fixed YCbCr matrix (`shaders.metal:1057-1060`). Windows `draw_surfaces` is an empty `Ok(())` after the empty-check (`directx_renderer.rs:803-809`). Linux Blade has no real surface path. **What must be built:** a backend-neutral `PaintSurface` GPU-texture handle; real D3D11VA NV12/P010 SRV sampling + keyed-mutex / VideoProcessor interop on DX11; VAAPI/DMABUF→`VkImage` (`EXT_external_memory`/DRM-PRIME) import on Blade; and a colorspace-driven matrix dispatch (601/709/2020, full/limited, 8/10-bit) replacing the single hardcoded NV12-full-range matrix. **Effort:** realistically *multiple quarters per backend*, not one XL total. This is the item to spike first (§9 action 1's sibling, §8 R1) before committing the render-graph design.

### 4.5 The general-framework production gates are facade

Three gates block shipping *any* serious app and must be built for real, in parallel with the media work:
- **Accessibility (legal gate):** rip out the stub bridges (macOS `accessibility.rs` is ~184 lines of role-conversion stubs with no NSAccessibility wiring; Linux is `dbus-send` theater; AccessKit is **not even a dependency**). **Integrate AccessKit** (it implements NSAccessibility + UIAutomation + AT-SPI) — but AccessKit only provides the protocol bridges; the framework must still emit a *stable, geometry-bearing, incremental* a11y tree from a system that today rebuilds per frame and whose `AccessibilityNode` lacks geometry. That is deep surgery into the element/layout core, not a bridge swap — multiple XLs.
- **Packaging/signing/auto-update:** near-greenfield, not hardening. Today's bundlers are stubs — `bundle_macos` writes a `.app` directory + Info.plist (no `.dmg`/`.pkg`/`hdiutil`), `bundle_windows` copies the `.exe` + writes `installer.json` metadata (no MSI/MSIX/WiX), `bundle_linux` writes a `.desktop` file + copies the binary (no AppImage/.deb/Flatpak) (`xtask/src/bundle.rs:56,151,202`) — and CI literally bundles `xtask` itself (`release.yml:55` `--binary target/release/xtask`), so it has **never packaged a real app**. Build real installers in CI for a real app binary; sign + notarize (macOS hardened runtime + entitlements + stapling, **including vendored FFmpeg/OCIO dylibs**), Authenticode/Azure Trusted Signing (Windows), Flatpak/AppImage/.deb (Linux). **The auto-updater RCE is fixed as a same-week hotfix** (§9 action 1), independent of this program.
- **Crash reporting + GPU devtools + real perf gate:** native crash capture (Breakpad/Crashpad/Sentry-native) with symbolicated minidumps and GPU adapter/driver context — prioritized given the **documented occasional CVDisplayLink-thread segfaults** (`display_link.rs:92`) that the panic-only reporter cannot see; real per-backend GPU timestamp queries; golden-image pixel-diff CI (from P0, guarding the renderer re-architecture). **Replace the simulated perf gate now (P0, not P5):** `perf_bench.rs` calls `simulate_workload()` and `simulate_frame(Duration::from_millis(16/100))` — it never opens a window or touches the GPU — yet `platform-readiness.yml` gates merges on these fabricated numbers. A regression gate on fake data gives false confidence on a GPU framework.

---

## 5. Native-language integration plan

Deliberate, concentrated native dependencies. Everything else stays Rust. (Decision rule: native only when it touches fixed-function silicon, the Rust alternative is years behind on must-have coverage, or it is the correctness-critical reference implementation.)

| Concern | Pick | Native vs Rust | Mechanism / kael files | Build·Sign cost | Licensing/risk |
|---|---|:--:|---|:--:|---|
| **Decode/encode/demux** | **FFmpeg, hardware-first** | **Native (must)** | `ffmpeg-next` + `hw_device_ctx`/`get_format`; rebuild `crates/kael-media/src/lib.rs`; revive `crates/media` VT bindgen (`bindings.h:5`) | **High** (vendor/build/sign 3 OS × arch) | **Pin LGPL build** (no GPL x264/x265); H.264/HEVC/AAC royalties → prefer HW encoders that carry license via OS/driver |
| **HW codec SDKs** (VideoToolbox, MediaFoundation/DXVA/NVENC/NVDEC/QSV/AMF, VAAPI) | via FFmpeg hwaccel | **Native (must)** | FFmpeg wrappers; decoder surface stays GPU-side | (incl. above) | Driver/runtime presence; do not vendor NVENC/AMF runtimes |
| **Codec → GPU texture** | Zero-copy import | **Native at boundary only** | `CVMetalTextureCache` (exists, `metal_renderer.rs:147`); `ID3D11` shared/keyed-mutex; VK `EXT_external_memory`/DRM-PRIME | **High** (multi-quarter/backend) | DX/VK external-texture interop is the **highest-risk technical item** (see §8 R1) |
| **GPU backend** | **Keep Metal/DX11/Blade** | **Pure-Rust** (do NOT adopt wgpu) | Extend `scene.rs`/`*_renderer.rs`; wgpu would *worsen* codec interop + cost a multi-quarter rewrite | Low | — |
| **Skia / skia-safe** | **Reject** | n/a | Redundant 2nd C++ renderer; kael already has GPU 2D + `resvg` | — | Bloat |
| **Color management** | **OpenColorIO** | **Native (must)** | `cxx`/bindgen; OCIO bakes 3D-LUT texture sampled in-shader; lives at edges, not hot path | High (C++ deps: Imath/yaml-cpp) | **Stageable** — ship in-shader 601/709/2020 + transfer math first; add OCIO when grading lands. **Linear-16F working space is NOT stageable — decided in P0** |
| **Shader compile** | `naga` + build-time `xcrun`/`dxc` | **Pure-Rust** | Already wired (`build.rs`) | None | Done |
| **Audio engine** | **`cpal` + `rubato` + FFmpeg decode** | **Pure-Rust** (drop `rodio`) | Replace `kael_audio` stubs (`effects.rs` is 11 lines); FFmpeg PCM → mixer → `cpal` | None | Low (design risk, not FFI) |
| **Secure credential storage** | OS keychains | **Native (must)** | new `kael_secrets`: Security.framework SecItem / Credential Manager-DPAPI / libsecret | Medium | Replaces plaintext `kael_net::TokenStore` |
| **Accessibility** | **AccessKit** | Rust crate (native under the hood) | Replace stub bridges in `platform/*/accessibility.rs`; **not yet a dependency** | Medium-High (tree surgery, not bridge swap) | — |
| **Crash capture** | Breakpad/Crashpad or Sentry-native | **Native (must)** | Replace panic-only hook; needed for documented CVDisplayLink segfaults | Medium | Symbol upload (dSYM/PDB) pipeline; **needs a crash-symbol server** |
| **Embeddable webview** | `wry` (WebView2/WKWebView/WebKitGTK) | Rust crate over native | **Win + Linux already use `wry`** (`platform/{windows,linux}/webview.rs`); add **macOS** (`WKWebView`) | Low | macOS is the actual gap |

**Packaging implication of going native:** FFmpeg + OCIO dylibs must be vendored, `@rpath`-fixed (`install_name_tool`), **code-signed and included in macOS notarization** — `kael_release` does not yet handle bundled dylibs, and the bundlers don't even produce a `.dmg`. This is the heaviest recurring tax; a **minimal real signed+notarized macOS `.dmg` path is pulled forward into P0** (§6, §8 R-pkg) to de-risk dylib notarization *before* the codec work is deep, rather than discovering it in P3.

---

## 6. Phased roadmap

Effort: **S** ≈ 1–2 wk · **M** ≈ 3–6 wk · **L** ≈ 1.5–3 mo · **XL** ≈ 3–6 mo · **XXL** ≈ 6–12 mo (small team = ~4–6 engineers; calendar assumes meaningful parallelism only where dependencies allow). **P0 and P1 are mostly serial.** Calendar ranges are stated with confidence and are gated on the §9 interop/licensing spikes.

### Phase P-hotfix — Auto-updater RCE (ship this week, before/parallel to everything)
**Goal:** Close the live remote-code-execution hole. Verify SHA-256 + ed25519 signature (via existing `kael_release::update::verify_manifest`, `update.rs:144`) inside `download_update` *before* setting `ReadyToInstall`; use a non-guessable temp path; reject on mismatch. Effort **S** (hours–days). Exit: a tampered or unsigned package is refused before install; updater imports and calls the verify primitive.

### Phase P0 — Foundation: renderer GPU substrate + media spine (≈ 7–10 months, low-confidence until spikes land)
**Goal:** Turn the renderer into one that exposes render targets + passes + custom/compute shaders + a **linear ≥16F color-managed pipeline (bit depth decided up front)** + a cross-platform zero-copy video surface, and stand up the media-engine spine (HW decode, frame-accurate seek, off-thread decode, `cpal` audio clock). Co-design the audio↔present clock contract and the export-determinism architecture. Nothing user-visibly "edits" yet; this unblocks all of it.

| Workstream | Deliverables | Closes | Deps | Effort | Exit criterion |
|---|---|---|---|:--:|---|
| **P0-A Offscreen targets + pass API** | Public `RenderTarget` (incl. RGBA16F); configurable RT/swapchain format (vs hardcoded BGRA8); generic bind-inputs/output/pipeline pass; re-express UI + blur on it under golden-image guard; all 3 backends | Render "no RTT", "no multi-pass", "8-bit only" | P-hotfix | XXL (multi-XL/backend) | App allocates an offscreen RGBA16F target, renders the scene into it, samples it as a texture input — Metal/DX11/Blade — with UI pixels unchanged vs golden baseline |
| **P0-B Custom + compute shaders** | App/framework shader registration (WGSL→`naga`→MSL/HLSL or per-backend); compute pipeline path; binding contract; build-time validation | Render "no compute/custom shaders" | P0-A | L | A registered fragment effect and a compute kernel run and produce correct output on all 3 backends |
| **P0-C Render-graph crate + cache model** | `kael-render-graph`: DAG of passes, transient resource alloc, barriers; **time-varying cache-invalidation model (frame-PTS + param-hash + topology) designed and owned** before caching is implemented | Render "no render-graph"; cache-corruption risk | P0-A,B | L | Blur re-expressed as a graph; a 3-pass chain (decode→effect→present) evaluates with auto-allocated intermediates and correct re-eval when an input frame/param changes |
| **P0-D Color + determinism architecture** | Per-window swapchain format/colorspace; **linear working space + ≥16F intermediates committed (not deferred)**; 601/709/2020 + full/limited + 8/10-bit YUV matrices in-shader; EDR/P3 output where available; **export-determinism decision: canonical single-backend export OR tolerance-bounded multi-backend (designed now, not validated in P5)** | Render "no color mgmt/HDR"; 8-bit-only; **V10 determinism** | P0-A | L | A BT.709 clip renders correct colors on all 3 OS; 10-bit/EDR verified on capable display; a written, agreed export-determinism design exists and constrains P1-A |
| **P0-E GPU-memory budget API** | Device/host budget query; eviction hooks; foundation for V9 tiered cache | Render/Prod#1 "no GPU memory budgeting" | P0-A | M | App can query remaining budget and register evictable resources on all 3 backends |
| **P0-F Cross-platform video surface** | Backend-neutral `PaintSurface` GPU-handle; real `draw_surfaces` on DX11 (NV12/P010 SRV + keyed-mutex) + Blade (VAAPI/DMABUF→VkImage); format-dispatch replaces the NV12-full-range `assert_eq!` | Render/Win/Linux "video surface no-op"; Widgets Surface blocker | P0-A,D | XXL (multi-quarter/backend) | The same NV12/P010 GPU frame displays correctly on macOS, Windows, Linux |
| **P0-G HW decode → GPU + seek + clock contract** | FFmpeg `hw_device_ctx` HW decode into GPU textures; per-clip keyframe index; `av_seek_frame` frame-accurate seek; off-thread decode + bounded cache; **co-designed audio↔present clock contract** | Video "no HW decode", "no seek", "decode on UI thread"; V7 clock | P0-F, P0-H | XL | Frame-accurate scrub of a 10-min 4K HEVC clip at 60fps preview, frames staying on GPU, UI thread never blocks, scheduled against the audio clock |
| **P0-H Audio engine spine** | `cpal` output + mixer skeleton; device-sample-counter master clock; FFmpeg-decode → mixer; `rubato` resample | Audio "wall-clock drift", "single-stream" | — | L | Audio plays through a real mixer with the device clock as master; video schedules against it with no measurable drift over 10 min |
| **P0-I FFmpeg build/license + min macOS installer** | LGPL-pinned FFmpeg vendored + reproducible cross-build for 3 OS×arch; dylib `@rpath`+sign plumbing; **minimal real signed+notarized `.dmg` path pulled forward to validate bundled-dylib notarization** | Native-integration risk; packaging tax; bundled-dylib notarization | — | L | `cargo build` links LGPL-only FFmpeg on 3 OS, and a real `.dmg` containing the FFmpeg dylib signs+notarizes+staples clean on macOS |
| **P0-J Real perf gate + golden-image CI** | Replace `simulate_*` bench with ≥1 real headless render benchmark (open window, drive N frames through real draw/present, measure CPU+GPU+RSS); golden-image pixel-diff harness with per-backend tolerance | DevTools "simulated perf gate", "no golden tests"; guards P0-A re-architecture | P0-A (first slice) | M | CI gate runs a real render and fails on a seeded regression; golden-image diff catches a deliberate pixel change on all 3 backends |

**Phase exit:** a non-editor test app can, on **all three platforms**, HW-decode a 4K clip into a GPU texture, run it through a multi-pass graph with a custom GPU effect and correct linear-16F color, and present it vsync-paced against an audio-master clock — with a real perf gate and golden-image diff guarding the work, an agreed export-determinism design on file, and a real signed `.dmg` proven.

### Phase P1 — Vertical slice: single-clip edit + export (≈ 3–4 months, mostly serial after P0)
**Goal:** Prove the architecture end-to-end with the smallest real editor: load a clip, trim in/out, one GPU effect + one transform, scrub, and **export to a file**. This is the earliest credible "it's a video editor" milestone and de-risks the rest. **Honest dependency note:** export (P1-A) needs P0-A, P0-D, P0-F, P0-G *all correct*, so P1 cannot meaningfully begin until nearly all of P0 lands — treat the "overlap" as a short P0-tail seam, not parallelism.

| Workstream | Deliverables | Closes | Deps | Effort | Exit |
|---|---|---|---|:--:|---|
| **P1-A Encode + export** | HW encoder (VT/NVENC/QSV/VAAPI) + SW fallback; rate control/GOP/color-tag signaling; MP4/MOV mux; offscreen timeline-frame render via the canonical export path (P0-D) → encoder; progress/cancel | Video "no encode/export" (hard blocker) | P0-A,C,D,F,G | XL | A trimmed 4K clip with one effect exports to a playable MP4 at ≥ real-time using HW encode, pixels matching preview within the P0-D tolerance |
| **P1-B Offline audio mixdown** | Faster-than-real-time mixer render; FFmpeg AAC encode; mux A+V | Audio "no offline mixdown/export" | P0-H | M | Exported file has correctly synced AAC audio |
| **P1-C Transport + timeline data v1** | Play/pause/J-K-L/step/in-out/loop scheduled against the media clock; single-track `sum_tree`-backed clip model with trim | Video transport; V5 (subset); V7 | P0-G,H | M | Frame-accurate transport + trim that survives save/load, A/V in sync over 10 min |
| **P1-D Minimal editor widgets** | Ruler/playhead, scrub bar, preview surface element, one scrubbable number field | Widgets timeline/scrubber/number-field (subset) | P0-F | M | Usable single-track editing UI |

**Phase exit:** a developer can build, with kael, a working "trim-one-clip-add-one-effect-and-export" app on all three platforms, distributed via the P0-I signed `.dmg` for dogfooding. **This is the architecture-validated milestone.**

### Phase P2 — The NLE core: multi-track compositor, effects, color, audio mixer, ingest (≈ 5–7 months)
**Goal:** Real editing — multiple tracks, blend modes, transitions, keyframes, LUT/color, a full mixer, and robust ingest.

| Workstream | Deliverables | Closes | Deps | Effort |
|---|---|---|---|:--:|
| **P2-A Multi-track compositor** | Per-track texture + transform + opacity + blend-mode on video layers; masks/mattes; nesting/compound clips | V1 compositor; Video "no compositor" | P0-C,F | XL |
| **P2-B Effects + transitions + LUT** | Fixed GPU effect-pass library (blur/sharpen/CC/keying); two-input transitions; 1D/3D `.cube` LUT; keyframable params; **GPU-composited text/title/caption layer rendered at arbitrary export resolution (NOT the 8-bit UI atlas path)** | V1/V3; Video "no effects/transitions"; missing export-text path | P2-A | XL |
| **P2-C Keyframe system (EXTEND)** | Generic keyframe-track + bezier/hold/ease interpolation sampled by the render clock; **built on existing `Keyframes`/`StyledKeyframe`/`CubicBezier`/`Easing` (`animation.rs:126-304`)**, generalized from UI-property to media/bezier-handle keyframes; attach to transform/opacity/effect/audio | V5 keyframes | P1-C, existing animation core | M-L |
| **P2-D Full audio mixer + DSP** | Buses→master, per-track gain/pan/automation, EQ/comp/limiter inserts, crossfades, waveform precompute, metering | V4; Audio mixer/DSP/automation/waveform | P0-H | XL |
| **P2-E Timeline model: ripple/roll/slip** | Magnetic timeline edit ops over `sum_tree`; transitions; markers | V5 full | P1-C | L |
| **P2-F Proxy + frame cache + filmstrip at 4K** | Proxy transcode + auto-switch; tiered RAM/VRAM/disk byte-budget frame cache driven by P0-E; **background filmstrip (sparse-keyframe thumbnail) decoder** | Video "OOM at 4K", "no proxy"; filmstrip gap | P0-E,G | M-L |
| **P2-G Media-ingest robustness** | VFR, rotation/display-matrix, multi-track audio, embedded timecode, image sequences, alpha/ProRes 4444, still images, corrupt/partial files | Video "ingest fragility" (field-failure source) | P0-G | L |

**Phase exit:** multi-track 4K timeline with effects/transitions/color/keyframes/titles plays back at 60fps and exports correctly; preview == export within the P0-D tolerance; real-world media (VFR/rotated/multi-track/alpha) ingests without corruption or crash.

### Phase P3 — General-framework production gates (≈ 5–8 months, runs in parallel from P1 onward)
**Goal:** Everything required to *ship* any serious app — not optional, and largely independent of the NLE work so it parallelizes. **Note the cross-phase dependency:** P3-E incremental layout is a prerequisite for the P4-A virtualized timeline's perf budget; if P3-E slips, P4-A inherits a per-frame full Taffy rebuild (see §8 R-layout).

| Workstream | Deliverables | Closes | Effort |
|---|---|---|:--:|
| **P3-A Accessibility via AccessKit** | Add AccessKit dependency; replace stub bridges; **add node geometry + stable cross-frame IDs + incremental tree emission** (deep element/layout surgery); patterns/actions; change events + live regions; high-contrast/reduce-motion/text-scale | A11y (all blockers); Prod #3 | XXL |
| **P3-B Packaging + signing + notarize (real)** | Real installers all 3 OS (.dmg/.pkg+notarize+hardened-runtime+entitlements; MSI/MSIX+Authenticode/Azure Trusted Signing; AppImage/Flatpak/.deb), **bundling vendored dylibs**; **CI builds the real app, not `xtask`**, signs, notarizes | Packaging (all blockers); Prod #6 | XL |
| **P3-C Auto-update integrity + delta + rollout** | Build on P-hotfix: streamed/resumable download; delta updates; staged rollout + rollback; **update-feed hosting + rollout backend (server-side work, not just client)** | Packaging "no delta/rollout"; Prod #7 | L |
| **P3-D Native crash + symbolication + server** | Breakpad/Crashpad/Sentry-native; minidumps for signals/FFI/GPU faults incl. **the documented CVDisplayLink-thread segfault**; dSYM/PDB upload + **crash-symbol server**; consent/PII scrub | Packaging "no native crash"; Prod #7 | L |
| **P3-E Text/i18n correctness (4 hard subprojects)** | Complex-script shaping unification across the 3 divergent shapers (CoreText/DirectWrite/cosmic_text); BiDi (UAX#9); UAX#14 line-break (`icu_segmenter`); **incremental/dirty layout replacing per-frame Taffy rebuild (research-grade, no Taffy incremental mode)**; variable fonts; grapheme-correct editing | Text/Layout (BiDi, UAX#14, variable-font, per-frame layout); Prod #2 | XXL |
| **P3-F Secure storage + secrets** | `kael_secrets` over OS keychains; encryption-at-rest option for `kael_storage`; retire plaintext `kael_net` token store | Ancillary "no secure storage"; Prod #8 | L |

**Phase exit:** a non-trivial general app built on kael passes a11y review, ships as a signed/notarized installer (with bundled dylibs) on all 3 OS, auto-updates safely, and reports native crashes — i.e., the **Definition of Production Ready** (§7) general-app gates go green.

### Phase P4 — Editor UX completeness + project model + grading (≈ 4–5 months)
**Goal:** The professional editing surface and document lifecycle. **Widget items below are each their own multi-week effort, not one lumped XL.**

| Workstream | Deliverables | Closes | Deps | Effort |
|---|---|---|---|:--:|
| **P4-A 2D-virtualized timeline canvas** | Cull/zoom virtualized timeline (extend the existing 1D `uniform_list`/`recycling_list` + `VirtualDataSource` to 2D); ruler/playhead integration; dockable/floating panel manager + layout persistence | Widgets (timeline, dock); V6 | P3-E (incremental layout) | XL |
| **P4-B Grading widgets** | HDR-aware color wheels with live scopes feedback; production curve/keyframe editor; tree/outliner over `VirtualDataSource` | Widgets (color wheels, curve editor, tree); V6 | P2-B,C | L |
| **P4-C Project format + scale undo (EXTEND)** | Versioned/relinkable project file; **migration-test discipline (round-trip, forward-compat, corruption recovery)**; autosave/journal/crash recovery; **document-scale transactional undo built on existing `UndoableChange`/`UndoTransaction`** with coalescing/grouping | V8; State "document-scale undo"; Doc model | P2-E | L |
| **P4-D Scopes + grading + OCIO** | Waveform/vectorscope/parade/histogram (compute); lift/gamma/gain + qualifiers; OpenColorIO integration (LUT bake) on the linear-16F space already chosen in P0-D | V3 scopes/grade; Color "no OCIO" | P0-D, P0-B | L |
| **P4-E Input depth** | Pen/tablet pressure/tilt; raw multi-touch + rotation; cross-platform precise/momentum scroll; pointer-lock for scrub fields; event coalescing | Input (pen, touch, scroll parity, coalescing); V4 input | — | L |

**Phase exit:** the editor matches CapCut-class UX for editing, grading, and project management, with a project format safe to evolve across updates.

### Phase P5 — Hardening: parity, perf-at-load, devtools, stability, API freeze (≈ 4–5 months)
**Goal:** "Handle any load" and lock the public surface. (GPU timing, golden tests, and the real perf gate already landed in P0-J; P5 deepens them.)

| Workstream | Deliverables | Closes | Effort |
|---|---|---|:--:|
| **P5-A GPU profiling + frame-stats API** | Per-backend GPU timestamp queries; public `FrameStats`/FPS/dropped-frame API; per-pass GPU timing; deepen the P0-J headless-benchmark suite | DevTools (GPU timing, frame-stats) | L |
| **P5-B Perf at load + backpressure + state plane** | **Parallel Send data-plane outside the single `RefCell<App>` (`app.rs:67`)** for high-frequency media state, with a coalesced reactive bridge (do NOT make `App` itself multi-threaded); bounded FG queue + update coalescing; QoS tiers/core reservation; GPU/native resource-leak tracking; sustained-4K thermal stability | State (single-RefCell, unbounded queue, QoS); Perf #13; V9 | XL |
| **P5-C Device-loss + cross-platform parity** | **GPU device-loss recovery for Metal + Blade (Windows already has it — `directx_devices.rs`/`events.rs:1231`)**; software/WARP fallback; presentation-time/vblank feedback (Linux `wp_presentation`/X11 Present); ProMotion/VRR; high-perf adapter selection (Win); **CVDisplayLink→CADisplayLink migration** | Render device-loss (2 of 3); Win/Linux pacing & adapter; CVDisplayLink segfault; Prod #1 | L |
| **P5-D IPC zero-copy + sandbox** | Shared-memory/GPU-surface IPC transport (binary, demuxed, timeouts) for media workers; real OS sandboxing/resource limits; socket auth + signature verification; real WASM (`wasmtime`) or drop the claim | IPC/Security (all blockers); Prod #8 | XL |
| **P5-E API stability + docs/DX** | Published SemVer/deprecation policy; deliberate `pub` boundary + upstream/fork strategy; **examples gallery + `create-app` scaffolder + signing/packaging/update recipes (docs *coverage* already enforced by `#![deny(missing_docs)]`)**; hot reload | API #12; Docs #10 (examples, not coverage); DevTools hot-reload | L |
| **P5-F macOS webview + i18n** | **`WKWebView` element to reach parity with existing Win/Linux `wry`**; ICU-backed i18n (plurals/format) replacing toy `kael_i18n` | Ecosystem #11 (macOS gap); Ancillary i18n | M |

**Phase exit:** all Definition-of-Production-Ready gates (§7) green for both lenses.

---

## 7. Definition of "production ready"

Done = every gate below is green, verified by automated checks in CI where possible. Split by lens; the video-editor lens additionally requires all general-app gates.

### General-app gates (Lens A)
| Gate | Pass condition |
|---|---|
| **Platform parity** | Every public capability works identically on macOS, Windows, Linux (X11+Wayland); no `#[cfg]`-gated no-ops in shipped paths; parity matrix 100% green |
| **Rendering robustness** | GPU device-loss recovers without crash on **all three** backends; software fallback launches on blocklisted driver/VM/RDP; vsync-correct on mixed-refresh/VRR; idle → 0fps; GPU-memory budget honored |
| **Text/i18n** | BiDi + UAX#14 + grapheme-correct editing; system font fallback + color emoji; full IME with correct preedit geometry; RTL layout mirroring; locale format/plural correct in ≥ Slavic/Arabic/CJK |
| **Accessibility** | VoiceOver/Narrator/NVDA/Orca read and operate the full UI; roles/states/geometry/events present; WCAG 2.2 AA documented and audited |
| **Packaging/signing** | One CI command → signed+notarized installer per OS (.dmg/.pkg, MSI/MSIX, AppImage/Flatpak/.deb) **with bundled dylibs**; CI packages the real app (not `xtask`); Gatekeeper/SmartScreen clean |
| **Auto-update** | Signature + checksum verified before install (RCE closed); non-guessable staging path; atomic replace; rollback on failed launch; delta updates; staged rollout |
| **Crash reporting** | Native + Rust + GPU-fault + frame-thread (CVDisplayLink) capture; symbolicated minidumps with adapter/driver context; symbol server live; opt-in + PII-scrubbed |
| **Security** | OS sandbox/capability model honored; secrets in OS keychain (no plaintext); `cargo-deny`/`cargo-audit` clean; SBOM generated; `unsafe` surface audited |
| **Test coverage** | Golden-image pixel-diff on 3 backends; **real (not simulated) headless render perf gate**; headless UI + input integration tests per OS; a11y-tree assertions; parallel test pass (not only `--test-threads=1`) |
| **Perf budgets** | Steady 60/120fps, input-to-photon < 2 frames; bounded memory over an 8-hour session (no leaks); cold start beats Electron; 100k-row virtualized list at 60fps |
| **API stability** | Published SemVer + deprecation policy; frozen reviewed `pub` surface; `#[non_exhaustive]` discipline; migration guides per release |
| **Docs/DX** | `missing_docs` enforced (already true); example per subsystem; `create-app` scaffolder; signing/packaging/update recipes; hot-reload dev loop |

### Video-editor gates (Lens B — all of the above, plus)
| Gate | Pass condition |
|---|---|
| **GPU compositing** | Offscreen RTs + custom + compute shaders + render-graph live; correct cache invalidation for time-varying inputs; blend modes/masks/transforms on video layers |
| **Export determinism** | A chosen canonical export path; preview == export within a documented, golden-tested per-backend tolerance — *not* assumed bit-identical across drivers |
| **Media** | HW decode→GPU on 3 OS; frame-accurate streaming seek; HW encode + mux + timeline export; proxy workflow; robust ingest (VFR/rotation/multi-track/alpha/corrupt) |
| **Color** | Linear working space + ≥16F (committed in P0); per-clip input transform; 1D/3D LUT; HDR (PQ/HLG, 10-bit); scopes |
| **Audio** | Multi-track mixer with sample-accurate device-master clock; DSP/automation/crossfades; waveforms; offline mixdown; LUFS metering |
| **Timeline/undo** | Tracks/clips/trim/ripple/roll/slip/transitions/nesting/keyframes; versioned relinkable project with migration tests; autosave/recovery; document-scale transactional undo |
| **Editor surface** | 2D-virtualized timeline at 60fps (on incremental layout); HDR color wheels + curve editor + scrub fields; filmstrip + waveform at timeline scale |
| **Load** | Sustained, thermally-stable 4K multi-track at 60fps; tiered frame cache within GPU-memory budget; export ≥ real-time with HW encode; graceful proxy/quality fallback under load |
| **GPU observability** | Public `FrameStats` (cpu/gpu µs, presented/dropped); per-pass GPU timing; GPU/native-resource leak detection |

---

## 8. Risks, unknowns & sequencing notes

| # | Risk / unknown | Impact | Mitigation / what to prototype |
|---|---|:--:|---|
| R1 | **Cross-platform zero-copy decode→texture interop** (DX11 shared/keyed-mutex, VAAPI/DMABUF→Vulkan) is the highest-uncertainty item and is *multiple quarters per backend*; it gates all video display and the P0 calendar | **Critical** | **Prototype P0-F first** as a throwaway spike on each OS before committing the render-graph design; the calendar range hinges on this |
| R2 | **FFmpeg licensing** — default `ffmpeg-sys-next` can pull GPL x264/x265; shipping commercial GPL-linked binaries is a legal trap | High | Pin LGPL build + HW encoders in P0-I *before* any encode work; legal sign-off on codec royalty posture |
| R3 | **Export determinism** — cross-backend GPU output is NOT bit-identical (FP rounding, filtering, FMA differ per driver/GPU); "preview == export" cannot be assumed | High | **Decide the canonical-export architecture in P0-D** (single-backend export vs tolerance-bounded multi-backend); discovering non-determinism in P5 would retroactively invalidate the P1 export milestone |
| R4 | **Renderer re-architecture regression** — re-expressing pixel-perfect UI/blur on a new pass API across 3 backends + 3 shader languages | High | Land golden-image pixel-diff CI in **P0-J before** P0-A's UI re-expression; treat every refactor as guarded |
| R-layout | **Incremental layout** — Taffy has no incremental mode; full per-frame rebuild blows the budget on dense timelines; **P3-E is a hidden prerequisite for P4-A** | High | Sequence P3-E's incremental layout ahead of P4-A; prototype layout-result caching + virtualized-timeline fast path early; benchmark 10k+ nodes at 60fps; if P3-E slips, P4-A slips |
| R5 | **State-plane re-architecture** (P5-B) — moving high-frequency media state out of the single `RefCell<App>` (`app.rs:67`) can destabilize every subsystem at once | High | Build a *parallel* Send data-plane with a coalesced reactive bridge; do NOT make `App` itself multi-threaded; spike the frame-cache path first |
| R6 | **OCIO build complexity** (C++ CMake deps); **linear-16F space is not deferrable** even though OCIO is | Med | Stage OCIO to P4-D, but commit the linear-16F working space + bit depth in **P0-D** so the compositor/export don't need a second rewrite |
| R-pkg | **Packaging is near-greenfield + bundled-dylib notarization is unproven** — bundlers are stubs, CI packages `xtask` | High | Pull a **minimal real signed `.dmg` (with FFmpeg dylib) into P0-I** to de-risk notarization before codec work is deep |
| R7 | **Effort/calendar** — three items are each multiple XLs; P0→P1 is mostly serial | Planning | State the calendar as a **range (P0+P1 ≈ 10–14 mo, full program multi-year)** gated on R1/R2 spikes; run P3 as the parallel track a second sub-team owns from P1 |
| R8 | **Backend infrastructure** — staged-rollout server, crash-symbol server, update-feed hosting are assumed but unbuilt | Med | Scope/operate them inside P3-C/P3-D; do not treat as free |
| R9 | **macOS CVDisplayLink** has *documented occasional segfaults* (`display_link.rs:92`) the panic-only reporter can't see; under-ranked previously | Med | Migrate to CADisplayLink in P5-C (or earlier if P1 transport cadence needs it) **and** prioritize native minidump capture (P3-D) sooner |
| R10 | **Upstream GPUI divergence** — kael forks a fast-moving base | Med | Decide fork/merge strategy in P5-E; freeze the deliberate `pub` boundary |

**De-risk order:** ship the auto-updater hotfix and replace the simulated perf gate in **week 1**; spike R1 (interop) and R2 (licensing) and the R3 determinism decision in **weeks 1–4** — they gate the entire P0 plan and the calendar range.

---

## 9. Immediate next actions (first tickets to open this week)

1. **HOTFIX the auto-updater RCE — ship before any roadmap phase.** In `auto_updater.rs` `download_update` (lines 197-254), verify SHA-256 **and** ed25519 signature via the existing `kael_release::update::verify_manifest` (`update.rs:144`) *before* setting `ReadyToInstall`; stop discarding the parsed `edSignature` (line 364); write to a non-guessable temp path (not `gpui_update_{filename}`); reject on mismatch. Hours of work, closes a live exploitable hole, independent of everything else.
2. **Spike: cross-platform decode→GPU-texture interop** — three throwaway proofs: VideoToolbox `CVPixelBuffer`→`MTLTexture` (verify against `metal_renderer.rs:1549+`), D3D11VA `ID3D11Texture2D` shared/keyed-mutex import, VAAPI DMABUF→`VkImage`. Output: go/no-go on §4.4/P0-F and a tightened calendar range. *(De-risks R1 — the dominant schedule risk.)*
3. **Decide + pin LGPL FFmpeg build** — configure features to exclude GPL components, select HW encoders, document royalty posture, get legal sign-off. *(De-risks R2; unblocks P0-G/P1-A.)*
4. **Decide the export-determinism architecture** — single canonical export backend vs tolerance-bounded multi-backend; this decision shapes P0-A/P0-D/P1-A and must precede them. *(De-risks R3.)*
5. **Replace the simulated CI perf gate now** — swap `perf_bench.rs`'s `simulate_workload`/`simulate_frame` for one real headless render benchmark (open a window, drive N frames through the real draw/present path, measure CPU+GPU+RSS) so `platform-readiness.yml` stops gating merges on fabricated numbers.
6. **Design doc: public `RenderTarget` + pass API** — the offscreen-target/pipeline contract (incl. configurable RGBA16F format) threaded through `scene.rs` + all three `*_renderer.rs`; the keystone of P0-A. Land the golden-image CI harness alongside it.
7. **Design doc: `kael-render-graph` crate incl. the time-varying cache-invalidation model** — DAG/transient-resource/barrier model *and* the frame-PTS + param-hash + topology cache-key design (the hard part).
8. **Implement public offscreen `RenderTarget` (RGBA16F) on Metal** as the reference backend (P0-A first slice); land DX11/Blade behind it.
9. **Replace the NV12-full-range `assert_eq!` with colorspace-driven matrix dispatch** in the macOS surface shader (`metal_renderer.rs:1575`, `shaders.metal:1057`) — small, high-value, unblocks correct HD/limited-range color and seeds P0-D.
10. **Stand up the `cpal` audio output + device-sample-clock skeleton** (P0-H) and **co-design the audio↔present clock contract** with the surface team before P1 integration.
11. **Pull a minimal real signed+notarized macOS `.dmg` forward (P0-I first slice)** — fix the "package a real app at all" gap and prove bundled-FFmpeg-dylib notarization before codec work deepens.
12. **Add AccessKit as a dependency and spike it on macOS** replacing the NSAccessibility role-stubs — proves the §4.5/P3-A approach (and exposes the geometry/incremental-tree surgery cost) before committing all three platforms.

---

## How this draft was revised

The previous draft was directionally correct — its four central architectural claims were re-verified true in source (closed 8-primitive 2D batcher with zero compute/custom-shader/render-target capability and hardcoded 8-bit BGRA; macOS-only video surface with a hardcoded NV12-full-range matrix while DX11 `draw_surfaces` is an empty `Ok(())` and Blade has no real path; software whole-file-to-RAM FFmpeg with no HW decode/encode and `restart()`-to-zero seek; single-stream `rodio` audio with an 11-line "effects" file). The phase shape (P0 substrate → P1 vertical slice → P2 NLE → P3 ship-gates in parallel) and the native-integration discipline (FFmpeg/OCIO at the silicon boundary, keep the three backends, reject wgpu/Skia) were kept. The following changes were made in response to the adversarial critique, each re-checked against the source:

- **Auto-updater RCE promoted to a same-week hotfix (P-hotfix + action 1), not a buried P3-C slice.** Verified the hole is live: `download_update` writes to an attacker-guessable temp path and sets `ReadyToInstall` with no hash/signature check (`auto_updater.rs:243-253`), the parsed `edSignature` is discarded (line 364), and the existing `verify_manifest` (`update.rs:144`) is never imported. The Definition-of-Production-Ready and §1 now reflect this.
- **Calendar made honest:** P0→P1 stated as mostly serial; the three dominant items (decode→texture interop, the cross-backend renderer re-architecture, AccessKit a11y) re-scaled as *multiple XLs each* (new XXL band; P0-A, P0-F, P3-A, P3-E upgraded); P0+P1 re-baselined to ~10–14 months with explicit low confidence gated on the interop/licensing spikes.
- **Two missing subsystems added:** **export determinism** (new pillar V10, P0-D workstream, R3 — cross-backend GPU output is not bit-identical) and **GPU-memory budgeting** (new P0-E workstream owning general-bar item #1). Also added: **media-ingest robustness** (P2-G), **video-export text/title path** (P2-B), **filmstrip + waveform precompute** (P2-F/P2-D), **render-graph time-varying cache invalidation** (P0-C design owner), and **backend infrastructure** (rollout/symbol/feed servers, R8).
- **Underestimates corrected:** cross-platform video surface broken out as its own gap (§4.4) and the most underestimated item; packaging re-framed as near-greenfield (bundlers are stubs; CI bundles `xtask` itself, `release.yml:55`) with a minimal real `.dmg` pulled forward into P0-I to de-risk dylib notarization; the simulated perf gate (`perf_bench.rs` `simulate_*`) replaced in P0-J + action 5 rather than P5.
- **Overstated/pessimistic matrix entries fixed so the team builds on existing assets:** keyframe system **exists** (`animation.rs` `Keyframes`/`StyledKeyframe`/`CubicBezier`/`Easing`) → P2-C is an *extend*; transactional undo **exists** (`app_runtime.rs` `UndoableChange`/`UndoTransaction`) → P4-C builds on it; **1D virtualization + tree `VirtualDataSource` exist** → timeline/tree extend them; **`wry` webview exists on Win+Linux** → macOS is the gap (P5-F); **Windows device-loss recovery exists** → only Metal/Blade are greenfield (P5-C); **`#![deny(missing_docs)]` already enforced** → docs gap is examples, not coverage. The rendering row now shows a split 2/0 (general/editor) rating since the editor substrate is effectively absent.
- **Sequencing repaired:** linear-16F working space + bit depth committed in P0-D (not deferred to P4 with OCIO); audio↔present clock contract and export-determinism decision moved to P0; the P3-E-incremental-layout → P4-A-timeline cross-phase dependency made explicit (R-layout); CVDisplayLink segfault (`display_link.rs:92`) cited and R9 re-ranked up.
- **Minor factual fixes:** the surface assert is NV12 **full-range** specifically; the renderer has **8 primitive variants** (kept as "8"); the dead encode path is the `crates/media` `VTCompressionSession` bindgen.
