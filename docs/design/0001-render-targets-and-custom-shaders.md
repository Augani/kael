# Design 0001: Public render targets, passes, and custom shaders

- Status: **Draft — for discussion**
- Tracks: roadmap workstreams P0-A (offscreen targets + pass API), P0-B (custom
  + compute shaders), P0-C (render-graph executor)
- Audience: anyone building on Kael, and other GPUI forks interested in
  aligning on an extensibility API

## Motivation

The single most-requested capability in the GPUI ecosystem is the one upstream
has declined to add: a way for applications to run their own GPU work. Without
it, anything beyond the built-in primitive set — performant custom gradients,
generative backgrounds, shader-driven effects, offscreen composition — is
either impossible or requires forking the renderer.

Kael's renderer today is a fixed-function 2D batcher. `Primitive`
(`crates/kael/src/scene.rs:311`) is a closed, crate-private enum of 8 variants
(shadow, blur rect, quad, path, underline, mono/poly sprite, surface), each
drawn by a fixed pipeline compiled from built-in shader source per backend
(MSL / HLSL / WGSL). Adding one visual effect means editing the enum, all
three backends, and all three shader files in lockstep. The existing
`runtime_shaders` cargo feature (`platform/mac/metal_renderer.rs:41-43`) only
recompiles the *built-in* source at runtime — it is a development convenience,
not an extensibility API.

This document proposes the public API that fixes that — for every Kael
application, not just the media stack. It is deliberately a *framework*
feature: the media compositor becomes one consumer among many.

## Goals

1. Applications can allocate **typed offscreen render targets** (including
   `Rgba16Float` for linear/HDR work) and render into them.
2. Applications can register **custom fragment shaders** and **compute
   kernels**, validated at registration, and run them as passes with declared
   bindings.
3. Passes compose into the existing **`kael_render_graph`** DAG (scheduling,
   transient-resource lifetimes, and time-varying cache invalidation already
   exist and are GPU-agnostic) — this proposal adds the missing **executor**.
4. Targets are **sampleable by the UI**: the output of a pass chain can be
   painted into the element tree like any image/surface.
5. Resource usage participates in **`kael_gpu_budget`** (budget query +
   evictable registration already exist).
6. Identical observable behavior on Metal, DirectX 11, and Vulkan/Blade,
   within a documented per-backend pixel tolerance.

## Non-goals

- **Arbitrary untrusted shaders.** Shaders are application code, compiled at
  build/registration time — not end-user/plugin input. (A sandboxed plugin
  story can layer on later; it is out of scope here.)
- **Replacing the built-in primitive path.** The fixed pipelines stay; they
  are re-expressed *on top of* the pass abstraction only when golden-image
  tests guard the migration (roadmap P0-J).
- **A new shading language.** We standardize on WGSL as the portable source
  of truth.

## Proposed API

### Shading language: WGSL in, `naga` out

Authors write WGSL. At registration, `naga` validates it and translates to
MSL (Metal), HLSL (DX11), or passes it through (Blade/Vulkan). `naga` is
already wired into the build (`crates/kael/build.rs` validates the built-in
WGSL today), so this adds no new toolchain. Per-backend hand-written
overrides are possible (`ShaderSource::per_backend`) but discouraged.

### Render targets

```rust
pub enum RenderTargetFormat {
    Bgra8UnormSrgb,   // matches today's swapchain
    Rgba8UnormSrgb,
    Rgba16Float,      // linear working space / HDR
    R8Unorm,          // masks
}

pub struct RenderTargetDesc {
    pub size: Size<DevicePixels>,
    pub format: RenderTargetFormat,
    pub label: &'static str,
}

impl Window {
    /// Allocate an offscreen target. Counts against the GPU memory budget;
    /// fails (does not abort) when the budget is exhausted.
    pub fn create_render_target(&mut self, desc: RenderTargetDesc)
        -> Result<RenderTarget>;
}
```

`RenderTarget` is a handle: cheap to clone, backend resources owned by the
renderer, freed when the last handle drops or when evicted under budget
pressure (evicted targets are observably invalid and must be re-rendered —
the same contract `kael_gpu_budget` defines for its evictable resources).

### Shader registration

```rust
pub struct ShaderBindings {
    /// Sampled texture inputs, bound in declaration order.
    pub textures: u32,
    /// Size of the uniform block in bytes (std140-compatible layout).
    pub uniform_bytes: u32,
}

impl App {
    /// Validate + translate once; reuse the handle every frame.
    pub fn register_fragment_shader(
        &mut self, source: ShaderSource, bindings: ShaderBindings,
    ) -> Result<ShaderHandle>;

    pub fn register_compute_shader(
        &mut self, source: ShaderSource, bindings: ShaderBindings,
    ) -> Result<ComputeHandle>;
}
```

Registration is the validation boundary: malformed WGSL, binding mismatches,
or unsupported features fail here with a source-mapped error, never at draw
time. Fragment shaders receive a full-target triangle with UV; the binding
contract (declared textures + one uniform block) is fixed, which is what
makes three-backend portability tractable.

### Passes and the graph

`kael_render_graph` already models everything needed — `ResourceDesc`
(transient/imported textures), `PassDesc` with `read`/`write`/`param_hash`/
`frame_pts`, compilation to an execution order with barriers, transient-memory
aliasing, and cache keys for time-varying inputs. The new piece is the
executor that maps a compiled graph onto the renderer:

```rust
impl Window {
    /// Execute a compiled graph. `imports` binds imported resources to real
    /// targets/textures; passes whose cache_key is unchanged since the last
    /// execution are skipped.
    pub fn execute_render_graph(
        &mut self,
        graph: &CompiledGraph,
        imports: &[(ResourceId, RenderTargetOrTexture)],
        shaders: &[(PassId, PassProgram)],   // fragment or compute + uniforms
    ) -> Result<()>;
}
```

Single-pass convenience (`window.run_pass(shader, &inputs, &target, uniforms)`)
wraps a one-node graph for the common case.

### Painting the result

A new element teaches the existing sprite/surface path to sample a target:

```rust
div().child(render_target_image(target.clone()).size_full())
```

This reuses the `PolychromeSprite`/`Surface` machinery — no new primitive
variant is needed for display.

## Backend mapping

| Concept | Metal | DirectX 11 | Blade/Vulkan |
|---|---|---|---|
| RenderTarget | `MTLTexture` (render target usage) | `ID3D11Texture2D` + RTV/SRV | `blade` texture w/ `RENDER_TARGET` |
| Fragment pass | `MTLRenderPipelineState` per shader | PS + draw | render pipeline |
| Compute pass | `MTLComputePipelineState` | CS dispatch | compute pipeline |
| Uniforms | `setFragmentBytes` | constant buffer | uniform buffer |
| Barriers | implicit / fences | implicit | from graph `barriers()` |

Pipeline states are cached per `(shader, target format)`; registration
pre-warms the current swapchain format.

## Staging

1. **Slice 1 (Metal reference):** `RenderTarget` + `register_fragment_shader`
   + `run_pass` + `render_target_image`, golden-image tests from day one.
   Exit: an app renders a custom WGSL effect into an offscreen target and
   shows it in the element tree.
2. **Slice 2:** DX11 + Blade parity for slice 1; cross-backend golden tests
   with documented tolerance.
3. **Slice 3:** compute shaders; `execute_render_graph` over `CompiledGraph`
   with cache-key skipping; budget-pressure eviction.
4. **Slice 4 (separate effort):** re-express built-in blur on the pass API
   under golden-image guard — the first step of unifying the fixed-function
   path, only after the public API is stable.

A non-media example ships with slice 1 (e.g. a shader-driven animated
gradient background) per the workspace layering rule: public framework
features are exercised by at least one non-media consumer.

## Open questions

1. **MSAA on offscreen targets** — needed for path rendering into targets;
   defer or include in slice 1?
2. **Sampling the swapchain** — backdrop-style effects want "what's behind
   me" as an input; the internal blur path does this today. Expose, or keep
   internal until slice 4?
3. **`uniform_bytes` layout validation** — accept a `#[repr(C)]` struct via a
   derive (`#[derive(ShaderUniforms)]`) instead of raw bytes?
4. **Reuse across windows** — shader handles are app-level, targets are
   window-level (device-owned). Is per-window allocation acceptable for v1?

Feedback welcome — especially from other GPUI forks; if this contract works
for your tree, we would rather converge on one API than ship four.
