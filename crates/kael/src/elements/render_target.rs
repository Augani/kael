use crate::{RenderTarget, Surface, SurfaceSource, surface};

/// Display a custom GPU target directly in the window that created it.
///
/// The returned element supports the same sizing and object-fit behavior as a
/// surface. Sampling stays on the GPU; [`crate::Window::read_render_target`]
/// is only needed when the application explicitly requests CPU pixels.
pub fn render_target(target: RenderTarget) -> Surface {
    surface(SurfaceSource::RenderTarget(target))
}
