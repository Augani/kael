# Custom GPU rendering completion evidence — 2026-10-02

This review distinguishes implemented execution from translation, compile, and
hardware-runtime evidence. The public GPU slice is opt-in through
`custom-shaders`. Fragment rendering, native compute, exact texture/buffer
uploads and mixed GPU graph execution now
have real output tests; platform hardware scopes are recorded separately.

## Delivered fragment contract

- CPU registration validates WGSL syntax, semantics, one fragment entry,
  full-target UV/position interface, every resource and uniform layout, source
  retention limits, and all four backend translations before admitting a handle.
- Reusable opaque targets cover RGBA8 linear, RGBA/BGRA sRGB, RGBA16F HDR and R8.
  Fragment output is straight linear RGBA; target storage is associated linear
  color (sRGB formats encode storage), preserving correct bilinear filtering.
- Sparse group-zero uniform, texture and sampler bindings are mapped explicitly.
  Uniform bytes must be reflected WGSL/std140-compatible bytes. Texture inputs
  are GPU target handles; no target-to-UI CPU readback or re-upload is used.
- The styled target element composes through each scene's surface batches,
  carrying opacity, rect/rounded clipping, transform, color filter, and a captured
  resource revision for damage/frame-skip decisions.
- Per-window ownership, 64 combined target/buffer allocations, 64 fragment
  pipelines and 64 native compute pipelines, one default 256 MiB shared data
  budget, checked dimensions, input/output feedback rejection, and explicit
  packed readback admission are enforced before GPU work. Strong handles pin
  allocations during pressure; renderer/context loss invalidates them.
- Metal retains at most two custom submissions, reuses up to eight completed
  uniform buffers, and bounds explicit wait/reuse to ten seconds. Blade retains
  uniforms and raw texture/pipeline allocations until submission/UI fences finish
  and uses bounded waits. A timed-out Blade teardown preserves its device and raw
  allocations rather than freeing resources still referenced by the GPU.
- DX11 uses cached dynamic uniform buffers and deferred driver-owned resource
  lifetimes. WebGL uses independent sampler objects and reusable uniform buffers.

## Actual native pixel proof

On this macOS host, the Metal custom suite passed **7 tests, 0 failures, 0 ignored**:

```sh
cargo test --locked -p kael --lib --no-default-features \
  --features font-kit,test-support,runtime_shaders,custom-shaders \
  platform::mac::metal_renderer::custom_shaders::tests -- --nocapture
```

Evidence: `.artifacts/kael-compute-metal-final-tests.log`, which runs the seven
custom cases alongside the six GPU graph and eighteen existing offscreen cases.
The fresh mandatory CI helper passes **47 Metal renderer regressions** after adding
telemetry, blur clipping, packed filtering and queue-ordered uploads: seven custom,
six graph, twenty-two offscreen and twelve atlas cases,
with zero failures, ignored tests or explicit GPU skips. Evidence is
`.artifacts/kael-programmable-metal-ci.log` and its four suite logs in
`.artifacts/kael-programmable-metal-ci/`. The same helper now passes **20 actual
Blade-on-Metal cases** in `.artifacts/kael-programmable-blade-ci.log` and
`.artifacts/kael-programmable-blade-ci/`, including the required physical-device
allocation probe on macOS. Linux's corresponding eight offscreen cases omit only
that Metal-specific physical allocation counter.
The required-device
pixel tests cover cleared target initialization, associated storage
`[32,64,128,128]` for straight `[.25,.5,1,.5]`, one compilation across repeated
execution, uniform/texture/sampler binding numbers 7/9/12, forbidden feedback,
foreign ownership, pressure/drop lifetime, every target format, native direct UI
source-over `[128,64,159,255]` in BGRA over red, outside-region preservation,
revision/damage behavior, opacity, both own/ancestor rounding, translation and
grayscale. sRGB storage `[137,188,255,255]` and HDR binary16 values are checked.

Blade's actual Metal-device custom suite passed **5 tests, 0 failures, 0 ignored**:

```sh
cargo test --locked -p kael --lib --no-default-features \
  --features font-kit,test-support,macos-blade,custom-shaders \
  platform::blade::blade_renderer::custom_shaders::tests -- --nocapture
```

Evidence: `.artifacts/kael-compute-blade-final-tests.log`. It checks associated
pixels, initialization, pipeline reuse, target lifetime, all five formats,
GPU texture chaining with sparse uniforms/samplers, feedback rejection and
rounded/half-opacity direct composition. Runtime testing caught two integration
errors that translation-only checks could not: Blade requires unbound global
resources, and Naga's WGSL writer changes numeric-suffixed identifiers. The
compiler now clears bindings after validation and uses a checked static alphabetic
resource namespace with readonly storage transport for runtime uniform bytes.
This is Blade-on-Metal runtime evidence, not Vulkan hardware execution.

The complete Blade renderer suite passed **20 tests, zero failures and zero
ignored**: five custom program cases, the same six graph cases as Metal, and
nine scene/memory/filtering cases. `.artifacts/kael-programmable-blade-ci.log` records
the actual run. Optional path, MSAA, cached-snapshot and two blur textures now
allocate independently on first use, reuse the same viewport, and release after
the tracked submission completes on resize, pressure and teardown. Teardown
previously omitted both blur textures. The actual 3840×2160, four-sample probe
measured **271,581,184 bytes (259 MiB)** on Apple M2 Pro that ordinary scenes now
avoid allocating. Its source is
`crates/kael/src/platform/blade/blade_renderer/offscreen_tests.rs`.

The Blade scene regressions were red before the change: a quad/resize retained
five scratch textures, and half-blue path over half-red quad produced alpha 255
instead of 191. Path composition now uses source-over alpha. Actual pixels
`[128,0,64,191]` pass under both surface alpha conventions. Critical pressure
releases scratch, preserves caller-owned texture/buffer bytes, and reproduces
identical path/blur/cached-snapshot pixels after recovery, zero-size and resize.
`.artifacts/kael-blade-scratch-red.log` retains the original failures.

Blade scene submission waits now attempt completion once, for at most ten
seconds, instead of retrying indefinitely. Timeout and device/memory errors
invalidate the scene's shared owner and stop new scene encoding. Resize and
pressure preserve pending scratch; timed-out readback preserves its staging
and fence. Teardown keeps the pending device/raw allocations alive when safe
completion cannot be established. Simulated unsignaled/OOM/device-loss fences
verify one attempt, preserved resources and rejected stale handles. Clearing the
simulation retires the actual completed GPU work; the same CPU program then
produces green pixels on recreated resources. These are deterministic lifecycle
tests backed by real submissions, not evidence of deliberately hanging hardware.

## Browser and Windows evidence

The browser library check passed with `browser,custom-shaders`:

```sh
cargo check --locked -p kael --lib --target wasm32-unknown-unknown \
  --no-default-features --features browser,custom-shaders
```

Evidence: `.artifacts/kael-custom-shaders-web-check.log`. **Six mandatory
browser GPU tests passed, zero failures and zero ignored**, in actual headless
Chrome 154.0.8037.95, ChromeDriver 154.0.8037.92. Driver logs identify
**ANGLE Metal Renderer: Apple M2 Pro**, rather than assuming hardware from a
successful WebGL API call. `.artifacts/browser-tools/driver.json` records the
official matching driver URL and SHA-256. Runtime log:
`.artifacts/kael-custom-shaders-web-runtime.log`.

The browser tests check target orientation/associated pixels, resource/cache/
lifetime checks, sparse uniform updates and texture chaining, every format
including HDR, direct rounded/opacity composition, real context loss/restoration
with new GPU ownership, exact uploads for all five formats and the shared
nested-loop execution bound. Native-only test scaffolding/dev dependencies were
made portable so the private renderer tests actually run through Chrome.

DX11 execution, **eight mandatory WARP custom regression sources and six shared
graph cases** compile on the
Windows GNU cross target. `.artifacts/kael-compute-dx11-tests-check.log` records
`cargo check --tests --target x86_64-pc-windows-gnu` with `custom-shaders`. Cases
cover fragment formats/styles/cache, native compute/runtime arrays/storage images,
exact uploads/buffer updates, guarded fragment loops, ownership/lifetimes,
feature-level handling, packed sprite filtering, lazy scratch/pressure recovery and identical graph
pixel/buffer/cache/alias/foreign-owner contracts. This macOS host cannot
execute WARP. Windows CI must produce the runtime result; Vulkan/Linux device
parity, multiple driver vendors and additional browser engines remain unverified.

`./scripts/ci/verify-programmable-renderer.py` enforces explicit positive test
counts, required backend compute/context-loss test names and zero skipped tests
in the existing native/browser CI jobs. A missing GPU or compiler must fail;
compile-only checks cannot satisfy these runtime gates.

## Delivered native compute and upload contract

`ComputeDescriptor`/`ComputeHandle` register one native WGSL entry. CPU syntax,
semantics, resource reflection and Metal/HLSL/Blade translation occur once;
registration shares the fragment program count/retention caps. Workgroups use
the portable guarantee of 128 total invocations, axes x/y <=128, z <=64, and
16 KiB shared memory. DX11 requires feature level 11.0. WebGL2 returns typed
unsupported errors for compute/storage buffers, while retaining fragment and
upload execution.

`GpuBuffer` is a window-owned opaque zero-initialized resource. Targets and
buffers share owner identity, 64 live-allocation admission and the same byte
ledger; Metal charges actual driver allocation. Storage buffers are limited to
128 MiB on the portable native contract; CPU reflection rejects fixed storage
layouts that cannot fit that bound. Uniform, sampled texture, sampler,
readonly/read-write storage-buffer and write-only RGBA8/HDR image
bindings are checked before work. Reflected runtime arrays enforce minimum
storage and final offset/stride. Sampled/storage image feedback and any writable
buffer SRV/UAV aliases are rejected. Public buffer writes check aligned bounded
ranges; target uploads require exact packed RGBA/sRGB/binary16/R8 encoding.

Actual Metal and Blade compute cases produced associated image pixels
`[32,64,128,128]`, runtime-array buffer words 100..115, correct partial buffer
updates and exact uploads across all formats. Metal displays the computed texture
directly into the retained UI scene. Shared budget/count/foreign-owner/device-loss
and reflected array/alias regressions exercise admission without GPU allocation.
The common compute suite passed five tests, zero failures and zero ignored;
`.artifacts/kael-compute-common-tests.log` records its result. Native compute
also writes HDR storage-image values `[2.0,0.5,0.25,1.0]` and checks their exact
binary16 representation.
The shared native graph fixture passed all six cases on both Metal and Blade.
It exercises GPU fragment DAGs, mixed buffer
compute → storage-image compute → fragment output, aliases/export pinning,
revision/uniform invalidation, cache skips and pressure.

Fragments and non-synchronized compute kernels use one per-invocation 65,536
loop-body execution counter across nested loops/helper functions. Exhaustion
exits remaining loops and continues normal control flow. Actual Metal, Blade
and WebGL tests execute authored infinite nested fragment loops and check the
resulting red pixel. Compute modules with both loops and barriers preserve their
authored synchronization semantics, expose `loop_body_limit() == None`, and can
be rejected explicitly with `ComputeDescriptor::require_loop_bound()`. Removing
only some invocations from a barrier loop would risk deadlock. Synchronized GPU
loop participation is covered by the native compute regression extension.
This policy bounds authored loop work where safe; it makes no GPU wall-clock
execution guarantee.

The synchronized native regression exposed a Metal integration defect:
translated kernels take dynamic threadgroup-memory arguments, but the backend
had not assigned their allocation lengths. The red output was `[65536,0]`
instead of `[65555,19]`. Reflection now carries each used workgroup global's
16-byte-aligned length, checks the device's shared-memory limit and binds every
argument before dispatch. The actual Metal test checks `[65555,19]` after a
divergent 65,536-iteration loop followed by uniform barrier loops. The emitted
kernel is retained in `.artifacts/kael-compute-sync-probe.metal`; the earlier
failure is transcribed in `.artifacts/kael-compute-workgroup-memory-red.log`.

## Opt-in actual GPU frame telemetry

`Window::set_gpu_frame_timing_enabled(true)` enables actual Metal command-buffer
start/end timestamps and drawable presentation callbacks. Its return value
reports backend support; unsupported backends return `false`. The default path
allocates no collector or telemetry blocks, queries no timestamp, and adds no
timer, wake-up or readback. `take_gpu_frame_timings()` drains ready records.
Each record has a session-local frame ID and submitted, GPU-start, GPU-end and
optional displayed host-clock timestamps. Custom program submissions are not
counted as window scene frames.

The collector bounds pending and completed records together to 64, discarding
the oldest under pressure. An onscreen record waits for completion and the
presented callback in either order; an offscreen record requires only completion.
A dropped drawable reports no displayed time. Disabling telemetry discards its
session. Both native callbacks capture only a weak collector, with no retained
window, renderer, command buffer or drawable. Failed command buffers with absent
GPU timestamps are discarded.

Apple specifies that [GPU start/end times](https://developer.apple.com/documentation/metal/mtlcommandbuffer/gpustarttime?language=objc)
are host seconds relative to system Mach time, published after completion.
[`CACurrentMediaTime`](https://developer.apple.com/documentation/quartzcore/cacurrentmediatime%28%29)
converts `mach_absolute_time()` to seconds, and
[`presentedTime`](https://developer.apple.com/documentation/metal/mtldrawable/presentedtime)
reports host seconds when the drawable was displayed, or zero for an unpresented
or dropped frame. These are monotonic host-clock values, not wall-clock time or
Rust `Instant` values. Actual native validation checks GPU timestamps against
the surrounding host-clock interval and checks rendered pixels.

The native regression and three collector cases passed **4 tests, zero failures
and zero ignored** in `.artifacts/kael-gpu-frame-timing-tests.log`. The actual
device reported submission `651024.2192212918`, GPU start `651024.2193986251`,
and GPU end `651024.219466125` seconds, within the surrounding host interval
`[651024.2190365834,651024.2200860418]`. It verifies an opaque red BGRA pixel,
64 retained records after 65 actual submissions, a weak callback after disabling,
and a fresh frame-ID session after re-enabling. The first run also caught
host-only FFI leaking into the generated Metal shader header; that red runtime
failure is retained in `.artifacts/kael-gpu-frame-timing-shader-header-red.log`.
Drawable displayed timestamps require the separate real-window run; the
offscreen regression does not assert presentation.

## Backdrop Gaussian parity and fractional capture

Browser WebGL2 now runs actual horizontal and vertical Gaussian passes, then
applies tint, saturation, rectangular content clipping and own/ancestor rounded
coverage. Both scratch textures retain associated RGBA, so filtering and tint
preserve translucent color. The compact RGBA8 pair has a separate 64 MiB payload
ceiling, checked against device dimensions before allocation. It allocates lazily,
reuses capacity, releases on resize/pressure and discards references on context
loss without issuing deletion against a restored generation. Changed blur scenes
redraw their dependent backdrop; unchanged scenes retain the frame.

The shared capture helper rounds both absolute endpoints outward. With bounds
`(12.8,4.2,20,10)`, the correct zero-radius capture is `(12,4,21,11)`, while the
old size-only rounding omitted the final column and row. A one-pixel blue column
inside a fractional panel produced red BGRA `[0,0,255,255]` on Metal and RGBA
`[153,0,102,255]` on WebGL2 before the fix; both regressions require the original
blue texel. The actual native failure is retained in
`.artifacts/kael-blur-fractional-native-red.log`, the browser failure in
`.artifacts/kael-browser-blur-fractional-red.log`, and the CPU endpoint failure in
`.artifacts/kael-blur-fractional-capture-red.log`.

Metal/HLSL/WGSL blur uniforms now carry the innermost rounded ancestor mask.
The final pass multiplies its coverage with own-corner coverage and preserves
straight output on Metal/DX11 or Blade's selected alpha convention. The native
rounded-clip regression first drew red outside its ancestor corner instead of
retaining blue; its red evidence is in
`.artifacts/kael-blur-geometry-clipping-red.log`. Six focused native blur/style
cases and the CPU endpoint regression passed with zero ignored cases after the
fix (`.artifacts/kael-blur-geometry-clipping-final.log` and
`.artifacts/kael-blur-fractional-capture-green.log`). The Blade scene suite
passed all eight cases with zero ignored in
`.artifacts/kael-blur-blade-scenes-green.log`, including both premultiplied and
postmultiplied alpha modes for fractional capture and rounded clipping.

The mandatory WebGL2 helper passed **15 tests, zero failures and zero ignored**:
six custom-fragment/format/context-loss cases, eight actual Gaussian blur cases,
and one atlas GPU mirror/replay/pressure/retirement/reupload case. Chrome reports
`ANGLE (Apple, ANGLE Metal Renderer: Apple M2 Pro, Unspecified Version)`; the
three raw logs are in `.artifacts/kael-programmable-webgl2-ci/`, and its aggregate
result is `.artifacts/kael-programmable-webgl2-ci.log`. Gaussian capture at x18
on a red/blue step produced RGBA `[198,0,57,255]`; translucent red plus blue tint
produced `[144,0,64,208]`. Edge sampling, fractional capture, own/ancestor masks,
scissor restoration, lazy bounded reuse/rejection, dependent damage redraw and
actual context loss/restoration all executed on that driver. Strict native and
browser library Clippy passed with `-D warnings`.

## Packed atlas filtering and ordered Metal uploads

The native sprite fixture places independently owned monochrome and BGRA tiles
immediately adjacent on real atlas pages, then magnifies and fractionally
translates the sprites. The original Metal shader produced a red edge channel
of 222 rather than 255 by filtering a neighboring zero-coverage tile.
`.artifacts/kael-packed-sprite-sampling-red.log` retains the actual GPU failure.
Metal, HLSL and Blade now clamp each fragment sample to the owned tile's first
and last texel centers. Vertex UV interpolation is preserved: the fixture's
interior two-color pixel remains BGRA `[83,0,172,255]`. Both isolated edge pixels
are `[0,0,255,255]`. The fixture passes on actual Metal with queued uploads in
`.artifacts/kael-packed-sprite-ordered-upload-green.log`, and under both Blade
alpha modes in `.artifacts/kael-packed-sprite-sampling-blade-green.log`.
Windows has the identical required WARP fixture; its fresh cross-compile passes
in `.artifacts/kael-packed-sprite-windows-check.log`, but runtime proof still
requires Windows CI.

Clamping a tile does not synchronize a CPU texture write. Apple's
[texture replacement contract](https://developer.apple.com/documentation/metal/mtltexture/replace(region:mipmaplevel:withbytes:bytesperrow:)?language=objc)
requires previous GPU accesses to finish before the immediate CPU copy.
Metal atlas uploads now write reusable, mapped staging chunks and submit blits
on the same queue as scene rendering. Each chunk is at most 8 MiB, at most two
upload batches may await completion, and destination pages, staging, the transient
raster payload and orphaned in-flight texture storage share atlas admission. Declared dimensions
are checked before rasterization; invalid payloads, errors and raster unwinding
roll back reservations. Completed staging may be reused or released under
pressure; pending buffers and destination textures remain owned.

The queued-read regression first reproduced the CPU-write race: an earlier GPU
read saw `[29,29,29,29]` instead of its previous `[17,17,17,17]` texture version
(`.artifacts/kael-metal-atlas-ordered-red.log`). Queue-ordered uploads preserve
the old read and publish the new bytes afterward. Upload completion is polled
without blocking; a ten-second deadline freezes admission and retirement,
retains resources, and stops automatic progress wakes. Drawable acquisition
also keeps Metal's finite timeout enabled so a saturated presentation pool
cannot prevent those deadlines from being observed. A failed acquisition does
not advance the atlas submission clock. The current device proof is Apple M2
Pro; Intel/discrete Metal drivers remain CI coverage requirements.

Transient admission rejection requests a bounded progress refresh. A one-shot
retry survives GPU completion racing the end-of-paint query, so missing glyphs
and images are rerasterized even when the final upload has completed already.
The existing window refresh bypasses retained subtree replay. A permanent
configured admission failure with no pending uploads requests no retry, and
terminal upload failure ends progress wakes.

All twelve actual Metal atlas tests passed with zero failures or ignored cases in
`.artifacts/kael-programmable-metal-ci/ordered-atlas-uploads.log`, including a
64 MiB image split into eight chunks with exact pixels on both sides of a chunk
boundary. The deadline test blocks a real queued submission with a shared event
and advances only the test clock; it proves retained resources and stopped wakes
without deliberately hanging hardware. The same mandatory helper's twenty-two
offscreen cases pass, including packed filtering and the configured finite
drawable timeout. Final Metal/Blade library and test Clippy checks and browser
library Clippy passed with `-D warnings`. Fresh Windows and X11 Linux library/test
cross-checks pass in `.artifacts/kael-renderer-checkpoint-windows-check.log` and
`.artifacts/kael-renderer-checkpoint-linux-check.log`; those checks do not satisfy
the required native device runtime gates.

## Remaining work and limits

- Native Windows/Vulkan hardware results remain external CI requirements; local
  Metal/Blade-on-Metal/browser-on-Metal output cannot prove those driver paths.
- Native video texture import requires a separate format/lifetime contract.
  Exact texture uploads support decoded image/scalar fields; callers supply the
  selected storage encoding and premultiplied color.
- GTK4/GSK and mock/headless PlatformWindow hosts report typed unsupported for
  public GPU execution. Native X11/Wayland use the Blade execution path. The
  WebGL2 backend has no native compute stage and browser HDR is capability-gated.
- Rounded ancestor clipping retains the existing innermost screen-space contract;
  arbitrary mask shapes and nested transformed clip intersections are not newly
  claimed. Backdrop blur uses actual Gaussian passes on WebGL2 and the native
  GPU renderer backends.
- Validation is not a GPU deadline guarantee for arbitrary author code. WebGL
  synchronous readback cannot be interrupted by a framework CPU timer.

The non-media `custom_shader` example creates one procedural WGSL target and
composes it directly into a styled native window. Its actual native Metal
window capture passed and is retained at `.artifacts/kael-custom-shader-ui.png`
(1,678,850 bytes), with `.artifacts/kael-custom-shader-ui-runtime.log`. Reproduce it with
`KAEL_SHADER_CAPTURE_PATH`; this is application-window GPU composition evidence.


## Native atlas logical identity follow-on

A real Blade pressure/replay regression exposed page identity reuse after the four-frame retirement guard: the freed physical slot received the same `AtlasTextureId`, so a retained old scene could identify the newly populated page. The unchanged assertion failed in `.artifacts/kael-blade-atlas-retained-pressure.log`; it now passes with exact packed-edge BGRA `[0,0,255,255]` and interpolated interior `[83,0,172,255]` in `.artifacts/kael-blade-atlas-retained-pressure-green.log`.

Metal, Blade and Direct3D now share a checked process-wide native logical ID allocator. IDs never recycle across atlas instances, windows, device resets, or failed post-reservation GPU allocations. The existing 8-byte tile identity/shader ABI is unchanged. Each atlas kind retains a bounded active-ID-to-physical-slot map; freed physical slots remain reusable, and the maps contain only resident pages. A local injected counter verifies exhaustion without poisoning the process allocator, and a 10,000-cycle model verifies one reused physical slot, stale lookup rejection and harmless late release. The logical ID limit fails admission instead of wrapping.

Texture mappings remain live through the successful-frame retirement guard. Metal upload batches retain their concrete destination textures and driver allocation charges independently of IDs until completion or terminal teardown; this change does not alter upload ordering, peak accounting, staging bounds or timeout semantics. Every native scene is validated before GPU submission: all referenced page identities, logical tile identities and exact bounds must remain resident in that atlas. Offscreen Metal, Blade and Direct3D return a checked error for a stale or foreign scene; onscreen Metal rejects it before submission. Defensive missing-resource paths also avoid instance-buffer growth or binding replacement memory. GPU-target/buffer handles retain their separate window/device ownership checks. Native sprite identities themselves prevent cross-atlas aliasing even when a scene is presented to another window.

The native CI helper additionally requires actual pressure/replay/reupload pixels on each backend, Blade upload admission before raster work, Direct3D simulated-device-reset reupload pixels, and logical identity lifetime/exhaustion models. The following regressions and new helper gates require fresh native execution after this source checkpoint; Windows runtime acceptance must come from the actual WARP job.

The page fix also exposed a second identity layer. Etagere bucket allocation IDs
reuse an eight-bit generation after 256 cycles, including while another tile
keeps the same page alive. Each native page now owns a checked monotonic logical
tile allocator and a bounded map to the private Etagere allocation plus exact
rectangle. Retirement removes that map entry once; old, foreign-bound and duplicate
releases cannot affect a replacement. The model exercises 512 actual allocator
cycles, confirms raw Etagere ID reuse, and rejects the old logical tile throughout.
All three models pass in `.artifacts/kael-atlas-tile-identity-tests.log`.

The actual Metal surviving-page regression reproduced stale-scene aliasing:
an old red sprite sampled replacement blue `[255,0,0,255]` after its exact region
was reused (`.artifacts/kael-metal-surviving-page-tile-red.log`). The unchanged
regression now rejects that scene before GPU submission while current pixels
remain correct (`.artifacts/kael-metal-surviving-page-tile-green.log`). The CI
helper requires the same real-pixel contract on Metal, Blade and WARP, plus
foreign-atlas identity, pressure/reupload, device-reset, exhaustion and late-release
contracts. Logical tile identity is per page; globally unique page identity makes
the combined native tile identity unique across windows and devices.

## Fractional native glyph admission follow-on

Fresh visible native windows exposed truncated labels and blank data cells despite
complete accessibility text. The first fractional-position macOS glyph reported
an unpadded rectangle before admission, then enlarged its raster by a padding pixel.
Strict atlas reservation rejected that mismatch, and line painting stopped at
that glyph. The native red regression records a real CoreText glyph with declared
18×26 pixels and returned 18×27 pixels in
`.artifacts/kael-metal-native-fractional-glyph-red.log`. macOS now declares the
complete antialias footprint before allocation and rasterizes exactly that size.
The CoreGraphics baseline adjustment preserves its existing coverage positions.
DirectWrite, Linux Swash and both browser raster paths already use identical
declared and returned sizes; the strict atlas checks remain in place.

A separate actual Metal test uploads 64 distinct small masks into one packed
page and renders all 64 in one batch; every sampled mask is present and the
monochrome instance stride is 160 bytes
(`.artifacts/kael-metal-many-glyph-masks.log`). The mandatory native glyph
regression additionally renders more than 30 real CoreText masks with fractional
origins and compares every GPU coverage pixel against the native CPU raster.
Fresh visible application captures are required after the fix; synthetic batches
or accessibility text do not establish that populated table cells paint correctly.
