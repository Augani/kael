//! Headless off-screen rendering for benchmarks and golden-image tests.
//!
//! Drives the real rasterization pipeline without creating a window, so CI and
//! local tooling can measure and pixel-verify genuine rendering work instead of
//! simulated sleeps. On macOS the scene is rasterized on the GPU (Metal) and
//! read back; on platforms whose off-screen GPU path is not yet implemented the
//! renderer still builds and batches the real scene on the CPU.

use anyhow::Result;

#[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
use crate::DevicePixels;
use crate::{
    Background, Bounds, ContentMask, ScaledPixels, Scene, TransformationMatrix, hsla, point, size,
};

/// Which rendering backend a [`HeadlessRenderer`] is using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlessBackend {
    /// Real GPU rasterization with read-back pixels (macOS / Metal today).
    Gpu,
    /// Real scene construction and batching on the CPU, without rasterization.
    ///
    /// Used on platforms whose off-screen GPU path is not yet implemented. The
    /// work performed is genuine (no simulated delays), but no pixels are read
    /// back, so the frame checksum is derived from scene structure instead.
    CpuOnly,
}

/// One frame produced by a [`HeadlessRenderer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedFrame {
    /// Frame width in device pixels.
    pub width: u32,
    /// Frame height in device pixels.
    pub height: u32,
    /// Stable content checksum: pixel-derived on a GPU backend, structure-derived on CPU-only.
    pub checksum: u64,
    /// Whether the frame was rasterized on the GPU.
    pub gpu: bool,
}

/// One frame rendered into an `RGBA16Float` off-screen target (linear, ≥16-bit).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HdrFrame {
    /// Frame width in device pixels.
    pub width: u32,
    /// Frame height in device pixels.
    pub height: u32,
    /// Peak channel value across the frame; may exceed `1.0` (HDR headroom).
    pub peak: f32,
    /// Stable checksum of the decoded float pixels.
    pub checksum: u64,
}

/// A windowless renderer used by benchmarks and golden-image tests.
///
/// Construct once, then call [`HeadlessRenderer::render_frame`] repeatedly to
/// drive frames through the real draw path.
pub struct HeadlessRenderer {
    width: u32,
    height: u32,
    backend: HeadlessBackend,
    #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
    metal: Option<crate::metal_renderer::MetalRenderer>,
}

impl HeadlessRenderer {
    /// Create a headless renderer for the given device-pixel dimensions.
    ///
    /// Selects the GPU backend when a compatible device is present, otherwise
    /// falls back to the CPU-only backend.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            anyhow::bail!("headless renderer requires non-zero dimensions");
        }

        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        {
            use crate::metal_renderer::{MetalRenderer, metal_is_available};
            use parking_lot::Mutex;
            use std::sync::Arc;

            if metal_is_available() {
                let renderer = MetalRenderer::new(Arc::new(Mutex::new(Default::default())));
                return Ok(Self {
                    width,
                    height,
                    backend: HeadlessBackend::Gpu,
                    metal: Some(renderer),
                });
            }
        }

        Ok(Self {
            width,
            height,
            backend: HeadlessBackend::CpuOnly,
            #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
            metal: None,
        })
    }

    /// The backend actually in use.
    pub fn backend(&self) -> HeadlessBackend {
        self.backend
    }

    /// The configured frame dimensions in device pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Build a procedural scene of `complexity` quads and process one frame.
    ///
    /// On a GPU backend the scene is rasterized off-screen and the checksum is
    /// derived from the read-back pixels; on a CPU-only backend the real scene
    /// is built and batched and the checksum is derived from its structure.
    pub fn render_frame(&mut self, complexity: usize) -> Result<RenderedFrame> {
        let scene = build_benchmark_scene(self.width, self.height, complexity);

        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_mut() {
            let viewport = size(
                DevicePixels(self.width as i32),
                DevicePixels(self.height as i32),
            );
            let readback = renderer.render_scene_to_bytes(&scene, viewport)?;
            return Ok(RenderedFrame {
                width: readback.width,
                height: readback.height,
                checksum: seahash::hash(&readback.bgra),
                gpu: true,
            });
        }

        Ok(RenderedFrame {
            width: self.width,
            height: self.height,
            checksum: scene.structural_checksum(),
            gpu: false,
        })
    }

    /// Render a procedural scene into an `RGBA16Float` off-screen target (the
    /// linear ≥16-bit working format), returning peak/checksum stats. Available
    /// only on the GPU backend.
    pub fn render_frame_rgba16f(&mut self, complexity: usize) -> Result<HdrFrame> {
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_mut() {
            let scene = build_benchmark_scene(self.width, self.height, complexity);
            let viewport = size(
                DevicePixels(self.width as i32),
                DevicePixels(self.height as i32),
            );
            let readback = renderer.render_scene_to_f16(&scene, viewport)?;
            let mut peak = 0.0f32;
            let mut checksum = 0xcbf2_9ce4_8422_2325u64;
            for &value in &readback.rgba {
                peak = peak.max(value);
                checksum ^= value.to_bits() as u64;
                checksum = checksum.wrapping_mul(0x0000_0100_0000_01b3);
            }
            return Ok(HdrFrame {
                width: readback.width,
                height: readback.height,
                peak,
                checksum,
            });
        }

        let _ = complexity;
        anyhow::bail!("RGBA16Float rendering is only available on the GPU backend")
    }

    /// Rasterize an arbitrary scene off-screen and read back its pixels as tightly
    /// packed BGRA bytes (`width * height * 4`). Returns `None` on the CPU-only
    /// backend, where no pixels are produced. Intended for golden-image and
    /// pixel-level tests that need to assert on real rendered output.
    #[cfg(test)]
    pub(crate) fn render_scene_to_bytes(&mut self, scene: &Scene) -> Result<Option<Vec<u8>>> {
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_mut() {
            let viewport = size(
                DevicePixels(self.width as i32),
                DevicePixels(self.height as i32),
            );
            let readback = renderer.render_scene_to_bytes(scene, viewport)?;
            return Ok(Some(readback.bgra));
        }
        let _ = scene;
        Ok(None)
    }

    /// Render `base` fully, then re-rasterize only the `damage` rectangle of `next` on
    /// top of it (the fine-grained dirty-region path: load the prior frame, scissor to the
    /// changed rect, repaint). Returns the composited BGRA bytes, or `None` on the CPU-only
    /// backend. Lets golden tests assert that a per-rectangle partial repaint is pixel-for-
    /// pixel identical to a full repaint of `next`.
    #[cfg(test)]
    pub(crate) fn render_damage_to_bytes(
        &mut self,
        base: &Scene,
        next: &Scene,
        damage: Bounds<ScaledPixels>,
    ) -> Result<Option<Vec<u8>>> {
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_mut() {
            let viewport = size(
                DevicePixels(self.width as i32),
                DevicePixels(self.height as i32),
            );
            let readback = renderer.render_damage_to_bytes(base, next, damage, viewport)?;
            return Ok(Some(readback.bgra));
        }
        let _ = (base, next, damage);
        Ok(None)
    }

    /// Apply a per-pixel coverage `mask` to a BGRA8 `pixels` buffer on the GPU, returning
    /// `true` if the GPU path ran (and `pixels` was modified in place) or `false` on the
    /// CPU-only backend. Lets golden tests confirm the GPU clip-mask multiply matches the
    /// CPU `apply_clip_mask_bgra` reference.
    #[cfg(test)]
    pub(crate) fn apply_clip_mask_gpu(&self, pixels: &mut [u8], mask: &[f32]) -> Result<bool> {
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_ref() {
            renderer.apply_clip_mask(pixels, mask)?;
            return Ok(true);
        }
        let _ = (pixels, mask);
        Ok(false)
    }

    /// Run a built-in GPU compute kernel that doubles each input value, proving
    /// the compute-pipeline path end-to-end. Available only on the GPU backend.
    pub fn run_compute_doubler(&self, data: &[f32]) -> Result<Vec<f32>> {
        #[cfg(all(target_os = "macos", not(feature = "macos-blade")))]
        if let Some(renderer) = self.metal.as_ref() {
            const KERNEL: &str = concat!(
                "#include <metal_stdlib>\n",
                "using namespace metal;\n",
                "kernel void double_values(device float* data [[buffer(0)]],\n",
                "                          uint id [[thread_position_in_grid]]) {\n",
                "    data[id] = data[id] * 2.0;\n",
                "}\n",
            );
            let mut buffer = data.to_vec();
            renderer.run_compute_kernel(KERNEL, "double_values", &mut buffer)?;
            return Ok(buffer);
        }
        let _ = data;
        anyhow::bail!("compute is only available on the GPU backend")
    }
}

fn build_benchmark_scene(width: u32, height: u32, complexity: usize) -> Scene {
    let mut scene = Scene::default();
    let count = complexity.max(1);
    let cols = ((count as f64).sqrt().ceil() as u32).max(1);
    let rows = (count as u32).div_ceil(cols).max(1);
    let cell_w = (width as f32 / cols as f32).max(1.0);
    let cell_h = (height as f32 / rows as f32).max(1.0);

    let viewport = Bounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
    };

    for i in 0..count {
        let col = (i as u32 % cols) as f32;
        let row = (i as u32 / cols) as f32;
        let bounds = Bounds {
            origin: point(ScaledPixels(col * cell_w), ScaledPixels(row * cell_h)),
            size: size(ScaledPixels(cell_w), ScaledPixels(cell_h)),
        };
        let hue = (i as f32 / count as f32).fract();
        scene.insert_primitive(crate::Quad {
            bounds,
            content_mask: ContentMask { bounds: viewport },
            background: Background::from(hsla(hue, 0.7, 0.5, 1.0)),
            transform: TransformationMatrix::unit(),
            ..Default::default()
        });
    }

    scene.finish();
    scene
}

#[cfg(test)]
fn build_quad_scene(width: u32, height: u32, background: Background) -> Scene {
    let mut scene = Scene::default();
    let viewport = Bounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
    };
    scene.insert_primitive(crate::Quad {
        bounds: viewport,
        content_mask: ContentMask { bounds: viewport },
        background,
        transform: TransformationMatrix::unit(),
        ..Default::default()
    });
    scene.finish();
    scene
}

#[cfg(test)]
fn build_gradient_scene(width: u32, height: u32, stops: &[crate::LinearColorStop]) -> Scene {
    build_quad_scene(
        width,
        height,
        crate::multi_stop_linear_gradient(90.0, stops),
    )
}

#[cfg(test)]
fn build_blend_scene(
    width: u32,
    height: u32,
    bg: crate::Hsla,
    fg: crate::Hsla,
    fg_blend_mode: u32,
) -> Scene {
    let mut scene = Scene::default();
    let full = Bounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
    };
    scene.insert_primitive(crate::Quad {
        bounds: full,
        content_mask: ContentMask { bounds: full },
        background: Background::from(bg),
        transform: TransformationMatrix::unit(),
        ..Default::default()
    });
    let right_half = Bounds {
        origin: point(ScaledPixels(width as f32 / 2.0), ScaledPixels(0.0)),
        size: size(
            ScaledPixels(width as f32 / 2.0),
            ScaledPixels(height as f32),
        ),
    };
    scene.insert_primitive(crate::Quad {
        bounds: right_half,
        content_mask: ContentMask { bounds: full },
        background: Background::from(fg),
        transform: TransformationMatrix::unit(),
        blend_mode: fg_blend_mode,
        ..Default::default()
    });
    scene.finish();
    scene
}

#[cfg(test)]
fn channel_range(bytes: &[u8], channel: usize) -> (u8, u8) {
    let (mut lo, mut hi) = (255u8, 0u8);
    for pixel in bytes.chunks_exact(4) {
        lo = lo.min(pixel[channel]);
        hi = hi.max(pixel[channel]);
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_frame_is_deterministic_and_does_real_work() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };

        let first = renderer.render_frame(32).unwrap();
        let second = renderer.render_frame(32).unwrap();

        assert_eq!((first.width, first.height), (64, 64));
        assert_eq!(
            first.checksum, second.checksum,
            "identical scenes must produce identical frames"
        );

        let denser = renderer.render_frame(64).unwrap();
        assert_ne!(
            first.checksum, denser.checksum,
            "a different scene must produce a different checksum"
        );

        match renderer.backend() {
            HeadlessBackend::Gpu => assert!(first.gpu),
            HeadlessBackend::CpuOnly => assert!(!first.gpu),
        }
    }

    #[test]
    fn rgba16f_frame_is_deterministic_or_unsupported() {
        let mut renderer = match HeadlessRenderer::new(32, 32) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        match renderer.render_frame_rgba16f(16) {
            Ok(frame) => {
                assert_eq!((frame.width, frame.height), (32, 32));
                assert!(frame.peak > 0.0);
                let again = renderer.render_frame_rgba16f(16).unwrap();
                assert_eq!(frame.checksum, again.checksum);
            }
            Err(_) => assert_eq!(renderer.backend(), HeadlessBackend::CpuOnly),
        }
    }

    #[test]
    fn linear_gradient_rasterizes_a_color_range_not_a_solid_fill() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let stops = [
            crate::LinearColorStop {
                color: hsla(0.0, 1.0, 0.5, 1.0),
                percentage: 0.0,
            },
            crate::LinearColorStop {
                color: hsla(0.66, 1.0, 0.5, 1.0),
                percentage: 1.0,
            },
        ];
        let scene = build_gradient_scene(64, 64, &stops);
        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");
        assert_eq!(bytes.len(), 64 * 64 * 4);

        let (min_r, max_r) = channel_range(&bytes, 2);
        let (min_b, max_b) = channel_range(&bytes, 0);
        assert!(
            max_r - min_r > 80,
            "red channel should span a gradient range, got {min_r}..{max_r}"
        );
        assert!(
            max_b - min_b > 80,
            "blue channel should span a gradient range, got {min_b}..{max_b}"
        );
    }

    #[test]
    fn solid_quad_rasterizes_a_uniform_color() {
        let mut renderer = match HeadlessRenderer::new(32, 32) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let scene = build_quad_scene(32, 32, Background::from(hsla(0.0, 1.0, 0.5, 1.0)));
        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        let (min_r, max_r) = channel_range(&bytes, 2);
        let (_, max_g) = channel_range(&bytes, 1);
        let (_, max_b) = channel_range(&bytes, 0);
        assert!(
            min_r > 180,
            "a solid red fill should keep a high red channel everywhere, got min {min_r}"
        );
        assert!(
            max_r - min_r < 16,
            "a solid fill should be uniform, red spanned {min_r}..{max_r}"
        );
        assert!(
            max_g < 90 && max_b < 90,
            "a red fill should carry little green/blue, got g{max_g} b{max_b}"
        );
    }

    #[test]
    fn radial_gradient_varies_from_center_to_edge() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let stops = [
            crate::LinearColorStop {
                color: hsla(0.0, 1.0, 0.5, 1.0),
                percentage: 0.0,
            },
            crate::LinearColorStop {
                color: hsla(0.66, 1.0, 0.5, 1.0),
                percentage: 1.0,
            },
        ];
        let scene = build_quad_scene(64, 64, crate::radial_gradient(0.5, 0.5, 0.5, &stops));
        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        let (min_r, max_r) = channel_range(&bytes, 2);
        let (min_b, max_b) = channel_range(&bytes, 0);
        assert!(
            max_r - min_r > 80,
            "radial gradient should vary in red, got {min_r}..{max_r}"
        );
        assert!(
            max_b - min_b > 80,
            "radial gradient should vary in blue, got {min_b}..{max_b}"
        );
    }

    #[test]
    fn conic_gradient_varies_around_the_sweep() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let stops = [
            crate::LinearColorStop {
                color: hsla(0.0, 1.0, 0.5, 1.0),
                percentage: 0.0,
            },
            crate::LinearColorStop {
                color: hsla(0.66, 1.0, 0.5, 1.0),
                percentage: 1.0,
            },
        ];
        let scene = build_quad_scene(64, 64, crate::conic_gradient(0.5, 0.5, 0.0, &stops));
        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        let (min_r, max_r) = channel_range(&bytes, 2);
        assert!(
            max_r - min_r > 80,
            "conic gradient should vary in red around the sweep, got {min_r}..{max_r}"
        );
    }

    #[test]
    fn eight_stop_gradient_renders_a_stop_beyond_the_old_four_cap() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let red = hsla(0.0, 1.0, 0.5, 1.0);
        let green = hsla(0.33, 1.0, 0.5, 1.0);
        let stops: Vec<crate::LinearColorStop> = (0..8u32)
            .map(|i| crate::LinearColorStop {
                color: if i == 5 { green } else { red },
                percentage: i as f32 / 7.0,
            })
            .collect();
        let scene = build_quad_scene(64, 64, crate::multi_stop_linear_gradient(90.0, &stops));
        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        let mut saw_green = false;
        for pixel in bytes.chunks_exact(4) {
            let (b, g, r) = (pixel[0], pixel[1], pixel[2]);
            if g > 120 && r < 120 && b < 120 {
                saw_green = true;
                break;
            }
        }
        assert!(
            saw_green,
            "an 8-stop gradient must render its 6th stop (green); a <=4-stop pipeline would drop it"
        );
    }

    #[test]
    fn multiply_and_screen_blend_modes_read_the_destination() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        // R channel at row 32: x=16 is backdrop-only, x=48 is the blended overlap.
        let sample = |bytes: &[u8], x: usize| bytes[(32 * 64 + x) * 4 + 2] as i32;

        // White over a gray backdrop, Multiply: real `src*dst` leaves the backdrop
        // unchanged (white is the multiply identity); the old `src*src` self-blend
        // would turn it white.
        let multiply = build_blend_scene(
            64,
            64,
            hsla(0.0, 0.0, 0.5, 1.0),
            hsla(0.0, 0.0, 1.0, 1.0),
            1,
        );
        let m = renderer
            .render_scene_to_bytes(&multiply)
            .unwrap()
            .expect("gpu backend yields pixels");
        let (m_bg, m_overlap) = (sample(&m, 16), sample(&m, 48));
        assert!(
            (m_overlap - m_bg).abs() < 28,
            "white multiply must leave the backdrop ~unchanged (real dst read): bg={m_bg} overlap={m_overlap}"
        );
        assert!(
            m_overlap < 220,
            "multiply overlap must not be white (the old self-blend result): {m_overlap}"
        );

        // Black over a gray backdrop, Screen: real screen leaves the backdrop unchanged
        // (black is the screen identity); the old self-blend would turn it black.
        let screen = build_blend_scene(
            64,
            64,
            hsla(0.0, 0.0, 0.5, 1.0),
            hsla(0.0, 0.0, 0.0, 1.0),
            2,
        );
        let s = renderer
            .render_scene_to_bytes(&screen)
            .unwrap()
            .expect("gpu backend yields pixels");
        let (s_bg, s_overlap) = (sample(&s, 16), sample(&s, 48));
        assert!(
            (s_overlap - s_bg).abs() < 28,
            "black screen must leave the backdrop ~unchanged (real dst read): bg={s_bg} overlap={s_overlap}"
        );
        assert!(
            s_overlap > 35,
            "screen overlap must not be black (the old self-blend result): {s_overlap}"
        );
    }

    #[test]
    fn framebuffer_fetch_blend_modes_read_the_destination() {
        let mut renderer = match HeadlessRenderer::new(64, 64) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let white = hsla(0.0, 0.0, 1.0, 1.0);
        let gray = hsla(0.0, 0.0, 0.5, 1.0);
        let sample = |bytes: &[u8], x: usize| bytes[(32 * 64 + x) * 4 + 2] as i32;

        // Difference of white over a WHITE backdrop = |1 - 1| = black. The old
        // self-blend approximation (|src - 0.5|) would give mid-gray instead. Skip if
        // the device lacks programmable blending (the backdrop then isn't ~white only
        // when the fetch path ran — here it always renders white either way, so the
        // overlap is the real discriminator).
        let difference = build_blend_scene(64, 64, white, white, 5);
        let d = renderer
            .render_scene_to_bytes(&difference)
            .unwrap()
            .expect("gpu backend yields pixels");
        let d_overlap = sample(&d, 48);
        assert!(
            d_overlap < 70,
            "difference(white, white) must rasterize ~black via real dst read; \
             the old approximation gives mid-gray. overlap={d_overlap}"
        );

        // Overlay of mid-gray over a WHITE backdrop = white (real); the approximation
        // gives mid-gray.
        let overlay = build_blend_scene(64, 64, white, gray, 3);
        let o = renderer
            .render_scene_to_bytes(&overlay)
            .unwrap()
            .expect("gpu backend yields pixels");
        let o_overlap = sample(&o, 48);
        assert!(
            o_overlap > 200,
            "overlay(gray, white) must rasterize ~white via real dst read; \
             the old approximation gives mid-gray. overlap={o_overlap}"
        );
    }

    #[test]
    fn compute_doubler_runs_on_gpu_or_is_unsupported() {
        let renderer = match HeadlessRenderer::new(8, 8) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        match renderer.run_compute_doubler(&[1.0, 2.0, 3.0, 4.0]) {
            Ok(output) => assert_eq!(output, vec![2.0, 4.0, 6.0, 8.0]),
            Err(_) => assert_eq!(renderer.backend(), HeadlessBackend::CpuOnly),
        }
    }

    fn corner_quad_scene(
        viewport: u32,
        corner: Bounds<ScaledPixels>,
        corner_color: crate::Hsla,
    ) -> Scene {
        let mut scene = Scene::default();
        let full = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(viewport as f32), ScaledPixels(viewport as f32)),
        };
        scene.insert_primitive(crate::Quad {
            bounds: full,
            content_mask: ContentMask { bounds: full },
            background: Background::from(hsla(0.0, 1.0, 0.5, 1.0)),
            transform: TransformationMatrix::unit(),
            ..Default::default()
        });
        scene.insert_primitive(crate::Quad {
            bounds: corner,
            content_mask: ContentMask { bounds: full },
            background: Background::from(corner_color),
            transform: TransformationMatrix::unit(),
            ..Default::default()
        });
        scene.finish();
        scene
    }

    #[test]
    fn per_rectangle_partial_repaint_matches_a_full_repaint() {
        let viewport = 64u32;
        let mut renderer = match HeadlessRenderer::new(viewport, viewport) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let corner = Bounds {
            origin: point(ScaledPixels(40.0), ScaledPixels(40.0)),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        // Only the corner quad changes color between frames; the full-viewport red
        // background is identical, so the damage must localize to the corner rectangle.
        let base = corner_quad_scene(viewport, corner, hsla(0.33, 1.0, 0.5, 1.0));
        let next = corner_quad_scene(viewport, corner, hsla(0.66, 1.0, 0.5, 1.0));

        let damage = next.damage_since(&base);
        assert_eq!(
            damage,
            crate::FrameDamage::Region(corner),
            "a color change confined to the corner quad must produce exactly that damage rect"
        );

        let partial = renderer
            .render_damage_to_bytes(&base, &next, corner)
            .unwrap()
            .expect("gpu backend yields pixels");
        let full = renderer
            .render_scene_to_bytes(&next)
            .unwrap()
            .expect("gpu backend yields pixels");
        assert_eq!(partial.len(), full.len());

        let max_diff = partial
            .iter()
            .zip(&full)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            max_diff <= 2,
            "scissor + load partial repaint must be pixel-identical to a full repaint, max channel diff {max_diff}"
        );
    }

    #[test]
    fn arbitrary_triangle_clip_produces_correct_pixels() {
        let viewport = 48u32;
        let mut renderer = match HeadlessRenderer::new(viewport, viewport) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        // Rasterize a solid red full-viewport quad on the real GPU pipeline.
        let scene = build_quad_scene(
            viewport,
            viewport,
            Background::from(hsla(0.0, 1.0, 0.5, 1.0)),
        );
        let mut bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        // Clip the rendered pixels to an upward-pointing triangle via the ClipShape mask.
        let triangle = crate::ClipShape::ConvexPolygon {
            vertices: vec![
                point(crate::px(24.0), crate::px(2.0)),
                point(crate::px(2.0), crate::px(46.0)),
                point(crate::px(46.0), crate::px(46.0)),
            ],
        };
        let mask = triangle.rasterize_mask(
            point(crate::px(0.0), crate::px(0.0)),
            viewport as usize,
            viewport as usize,
            crate::px(1.0),
        );
        crate::apply_clip_mask_bgra(&mut bytes, &mask);

        let alpha = |x: u32, y: u32| bytes[((y * viewport + x) * 4 + 3) as usize];
        // BGRA readback: channel index 2 is red.
        let red = |x: u32, y: u32| bytes[((y * viewport + x) * 4 + 2) as usize];

        // Inside the triangle (near its centroid ~(24, 31)): opaque red survives the clip.
        assert!(
            alpha(24, 30) > 250,
            "interior stays opaque, got {}",
            alpha(24, 30)
        );
        assert!(red(24, 30) > 180, "interior keeps its red fill");

        // The top corners lie above the triangle's slanted edges → cut to transparent.
        assert_eq!(alpha(2, 2), 0, "top-left corner is outside the triangle");
        assert_eq!(alpha(46, 2), 0, "top-right corner is outside the triangle");
    }

    #[test]
    fn circle_clip_shape_renders_through_the_rounded_clip_shader() {
        let viewport = 40u32;
        let mut renderer = match HeadlessRenderer::new(viewport, viewport) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        // A ClipShape::Circle maps to the rounded-clip the quad shader already honors —
        // drive a full red quad through that clip and confirm it renders as a circle.
        let circle = crate::ClipShape::Circle {
            center: point(crate::px(20.0), crate::px(20.0)),
            radius: crate::px(20.0),
        };
        let (clip_bounds, clip_radii) = circle.as_rounded_clip().expect("circle maps");

        let full = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(viewport as f32), ScaledPixels(viewport as f32)),
        };
        let to_scaled_bounds = Bounds {
            origin: point(
                ScaledPixels(clip_bounds.origin.x.0),
                ScaledPixels(clip_bounds.origin.y.0),
            ),
            size: size(
                ScaledPixels(clip_bounds.size.width.0),
                ScaledPixels(clip_bounds.size.height.0),
            ),
        };
        let scaled_radii = crate::Corners {
            top_left: ScaledPixels(clip_radii.top_left.0),
            top_right: ScaledPixels(clip_radii.top_right.0),
            bottom_right: ScaledPixels(clip_radii.bottom_right.0),
            bottom_left: ScaledPixels(clip_radii.bottom_left.0),
        };

        let mut scene = Scene::default();
        scene.insert_primitive(crate::Quad {
            bounds: full,
            content_mask: ContentMask { bounds: full },
            background: Background::from(hsla(0.0, 1.0, 0.5, 1.0)),
            rounded_clip_bounds: to_scaled_bounds,
            rounded_clip_radii: scaled_radii,
            transform: TransformationMatrix::unit(),
            ..Default::default()
        });
        scene.finish();

        let bytes = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");
        let alpha = |x: u32, y: u32| bytes[((y * viewport + x) * 4 + 3) as usize];

        // Center of the inscribed circle: opaque. The four square corners are outside the
        // circle, so the rounded clip cuts them to transparent.
        assert!(
            alpha(20, 20) > 250,
            "circle center is opaque, got {}",
            alpha(20, 20)
        );
        assert_eq!(alpha(1, 1), 0, "top-left corner is outside the circle");
        assert_eq!(alpha(38, 1), 0, "top-right corner is outside the circle");
        assert_eq!(alpha(1, 38), 0, "bottom-left corner is outside the circle");
        assert_eq!(
            alpha(38, 38),
            0,
            "bottom-right corner is outside the circle"
        );
    }

    #[test]
    fn gpu_clip_mask_matches_the_cpu_reference_and_clips_a_triangle() {
        let viewport = 48u32;
        let mut renderer = match HeadlessRenderer::new(viewport, viewport) {
            Ok(renderer) => renderer,
            Err(_) => return,
        };
        if renderer.backend() != HeadlessBackend::Gpu {
            return;
        }

        let scene = build_quad_scene(
            viewport,
            viewport,
            Background::from(hsla(0.0, 1.0, 0.5, 1.0)),
        );
        let rendered = renderer
            .render_scene_to_bytes(&scene)
            .unwrap()
            .expect("gpu backend yields pixels");

        let triangle = crate::ClipShape::ConvexPolygon {
            vertices: vec![
                point(crate::px(24.0), crate::px(2.0)),
                point(crate::px(2.0), crate::px(46.0)),
                point(crate::px(46.0), crate::px(46.0)),
            ],
        };
        let mask = triangle.rasterize_mask(
            point(crate::px(0.0), crate::px(0.0)),
            viewport as usize,
            viewport as usize,
            crate::px(1.0),
        );

        // Apply the same mask two ways: the CPU reference and the GPU compute kernel.
        let mut cpu = rendered.clone();
        crate::apply_clip_mask_bgra(&mut cpu, &mask);

        let mut gpu = rendered.clone();
        let ran = renderer.apply_clip_mask_gpu(&mut gpu, &mask).unwrap();
        assert!(ran, "gpu backend must run the clip-mask kernel");

        // The GPU multiply must match the CPU reference within rounding tolerance.
        let max_diff = cpu
            .iter()
            .zip(&gpu)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            max_diff <= 1,
            "gpu clip-mask must match the cpu reference, max byte diff {max_diff}"
        );

        // And it produces the correct clip: interior opaque, exterior cut.
        let alpha = |x: u32, y: u32| gpu[((y * viewport + x) * 4 + 3) as usize];
        assert!(alpha(24, 30) > 250, "interior stays opaque");
        assert_eq!(alpha(2, 2), 0, "exterior corner is cut");
        assert_eq!(alpha(46, 2), 0, "exterior corner is cut");
    }
}
