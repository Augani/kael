//! Headless off-screen rendering for benchmarks and golden-image tests.
//!
//! Drives the real rasterization pipeline without creating a window, so CI and
//! local tooling can measure and pixel-verify genuine rendering work instead of
//! simulated sleeps. On macOS the scene is rasterized on the GPU (Metal) and
//! read back; on platforms whose off-screen GPU path is not yet implemented the
//! renderer still builds and batches the real scene on the CPU.

use anyhow::Result;

use crate::{
    Background, Bounds, ContentMask, DevicePixels, ScaledPixels, Scene, TransformationMatrix, hsla,
    point, size,
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

/// A windowless renderer used by benchmarks and golden-image tests.
///
/// Construct once, then call [`HeadlessRenderer::render_frame`] repeatedly to
/// drive frames through the real draw path.
pub struct HeadlessRenderer {
    width: u32,
    height: u32,
    backend: HeadlessBackend,
    #[cfg(target_os = "macos")]
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

        #[cfg(target_os = "macos")]
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
            #[cfg(target_os = "macos")]
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

        #[cfg(target_os = "macos")]
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
}
