# rasterlottie

[![Crates.io](https://img.shields.io/crates/v/rasterlottie.svg)](https://crates.io/crates/rasterlottie)
[![Docs.rs](https://docs.rs/rasterlottie/badge.svg)](https://docs.rs/rasterlottie)
[![CI](https://github.com/neodyland/rasterlottie/actions/workflows/ci.yml/badge.svg)](https://github.com/neodyland/rasterlottie/actions/workflows/ci.yml)

`rasterlottie` is a pure Rust, headless Lottie rasterizer focused on a validated target corpus.

The immediate target is not full Lottie parity. The target is a deterministic,
server-side renderer that can parse a pragmatic subset of Lottie JSON, explain
why unsupported animations fail, and rasterize supported animations into RGBA
frames. GIF export is available through the default `gif` feature.

## Installation

```toml
[dependencies]
rasterlottie = "<ver>"
```

The default feature set enables `gif`, `images`, and `text`.

Enable the optional `dotlottie` feature when you need to load `.lottie` archives:

```toml
[dependencies]
rasterlottie = { version = "<ver>", features = ["dotlottie"] }
```

If you only need frame rendering without GIF export, image assets, or text layers:

```toml
[dependencies]
rasterlottie = { version = "<ver>", default-features = false }
```

## Basic usage

```rust
use rasterlottie::{Animation, RenderConfig, Renderer};

let animation = Animation::from_json_str(
    r#"{
        "v":"5.7.6",
        "fr":30,
        "ip":0,
        "op":30,
        "w":64,
        "h":64,
        "layers":[]
    }"#,
)?;

let frame = Renderer::default().render_frame(&animation, 0.0, RenderConfig::default())?;
assert_eq!((frame.width, frame.height), (64, 64));
# Ok::<(), rasterlottie::RasterlottieError>(())
```

With `dotlottie` enabled, you can also load a packaged archive:

```rust,no_run
# #[cfg(feature = "dotlottie")]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use rasterlottie::Animation;

let dotlottie = std::fs::read("example.lottie")?;
let animation = Animation::from_dotlottie_bytes(&dotlottie)?;
# let _animation = animation;
# Ok(())
# }
# #[cfg(not(feature = "dotlottie"))]
# fn main() {}
```

## Current status

- Parses a small but useful Lottie subset
- Reports unsupported features through a deterministic support analyzer
- Exposes a rendering API backed by `tiny-skia`
- Can render static rectangles, rounded rectangles, and ellipses into RGBA frames
- Can render static and basic animated custom bezier shape paths into RGBA frames
- Honors animation and layer frame windows, plus precomp start time and stretch
- Supports scalar time remapping on precomp layers
- Can evaluate basic animated scalar and vector properties over time, including spatial and split position transforms
- Supports linear and radial gradient fills and strokes
- Supports layer masks for `add`, `subtract`, `intersect`, and `none`
- Supports alpha and luminance track mattes, including inverted modes
- Supports embedded data URL image assets and image layers when the default `images` feature is enabled
- Supports external image assets through an explicit resolver API when the default `images` feature is enabled
- Supports loading `.lottie` archives when the optional `dotlottie` feature is enabled
- Supports glyph-backed text layers with stepped document keyframes when the default `text` feature is enabled
- Ignores controller-style effects on null layers
- Accepts single-source merge paths that collapse to a no-op
- Includes JSON fixture regressions for gradient and matte semantics under `tests/fixtures`
- Includes PNG golden image-diff regressions for representative shape fixtures under `tests/image_diff.rs`
- Validates a small public target corpus under `tests/corpus`
- Supports layer parenting, stroke dash patterns, trim paths, polystars, and repeaters
- Can encode supported animations into GIF bytes when the default `gif` feature is enabled
- Supports solid fills and strokes for the current target corpus

## Target corpus support profile

The default profile is intentionally strict. It allows:

- shape layers
- precomp layers
- null layers
- common shape item containers and primitives

It currently rejects:

- text animators, text paths, and boxed text
- external image assets without a resolver
- layer effects on rendered layers
- expressions
- non-precomp or non-scalar time remapping
- unsupported gradient encodings or gradient types
- unsupported mask and track matte modes
- unknown shape item kinds

## Why start this way

The hardest part of a pure Rust renderer is not drawing a rectangle. The hard
part is drawing the right rectangle for the right frame and failing
predictably when the animation needs a feature that is not implemented yet.

This project starts by making that contract explicit first.

## Validation

- `tests/image_diff.rs` compares representative fixture renders against checked-in PNG goldens
- `tests/corpus.rs` validates the current public target corpus listed in `tests/corpus/manifest.json`
- `tests/reference/render_fixture.html` is the browser-side reference harness used to regenerate PNG goldens
- `cargo run --example rasterlottie_cli -- analyze <input.json>` prints the current support report
- `cargo run --example rasterlottie_cli -- render-gif <input.json> <output.gif>` renders a GIF with optional `--fps`, `--duration`, and `--quantizer-speed` when the default `gif` feature is enabled
- `cargo run --example rasterlottie_cli -- render-png <input.json> <output.png>` renders a PNG with optional `--frame`, `--background`, and `--scale`
- `cargo run --example rasterlottie_cli -- render-mp4 <input.json> <output.mp4>` renders an MP4 with optional `--fps`, `--duration`, `--background`, `--scale`, `--crf`, `--preset`, `--codec`, `--pix-fmt`, and `--lossless`
- `cargo run --example rasterlottie_cli --features dotlottie -- analyze <input.lottie>` loads a packaged archive and prints the selected animation's support report
- `cargo run --example rasterlottie_cli --features dotlottie -- render-gif <input.lottie> <output.gif>` renders a GIF from a packaged archive
- `cargo run --example rasterlottie_cli --features dotlottie -- render-png <input.lottie> <output.png>` renders a PNG from a packaged archive
- `cargo run --example rasterlottie_cli --features dotlottie -- render-mp4 <input.lottie> <output.mp4>` renders an MP4 from a packaged archive
- `cargo run --example benchmark_render -- <input.json> --mode gif --json` benchmarks raw frame rendering with the same frame sampling used by `render-gif`
- `cargo run --example benchmark_render --features dotlottie -- <input.lottie> --mode gif --json` benchmarks packaged archives with the same frame sampling used by `render-gif`
- `tools/bench/compare-with-rlottie-docker.ps1 -InputJson work\input.json -Mode both` compares `rasterlottie` raw render timing against `rlottie` inside Docker

## Tracing

`rasterlottie` can be built with an optional `tracing` feature for runtime diagnostics.

- `cargo run --example rasterlottie_cli --features tracing -- analyze <input.json>`
- `cargo run --example rasterlottie_cli --features tracing -- render-gif <input.json> <output.gif> --fps 60`
- `cargo run --example rasterlottie_cli --features tracing -- render-mp4 <input.json> <output.mp4> --fps 60`

Set `RASTERLOTTIE_TRACE=1` to enable subscriber initialization in the example CLI.

- `RUST_LOG=rasterlottie_cli=debug` shows top-level CLI spans such as `prepare_animation`, `render_animation`, `encode_frames`, `write_output`, and `wait_ffmpeg`
- `RUST_LOG=rasterlottie=trace` adds renderer internals such as `render_layer_stack`, `composite_pixmap`, `apply_track_matte`, `draw_path`, `trim_path`, `fill_path`, `stroke_path`, and GIF-specific spans like `encode_gif_frame`

Example:

```powershell
$env:RASTERLOTTIE_TRACE='1'
$env:RUST_LOG='rasterlottie_cli=debug,rasterlottie=trace'
cargo run --example rasterlottie_cli --features tracing -- render-mp4 work\input.json work\output.mp4 --fps 60
```

The tracing feature is disabled by default, so normal library consumers do not pay for the subscriber or emit renderer spans unless they opt in.

## Next milestones

1. Expand the public corpus only when `miq-gen-rs` traffic shows a real need
2. Add broader text support only if the target corpus needs text animators, text paths, or boxed text
3. Reassess rendered layer effects, expressions, and richer time-remap cases against real-world inputs
4. Add richer frame export helpers for APNG and fixture pipelines
5. Expand reference image-diff coverage for gradients, mattes, and any newly supported public corpus samples

## License

Licensed under either of the following, at your option:

- MIT
- Apache-2.0
