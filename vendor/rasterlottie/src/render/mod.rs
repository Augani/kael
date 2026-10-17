//! Rendering module declarations and public exports.

mod assets;
mod composition;
mod drawing;
mod drawing_paths;
#[cfg(feature = "gif")]
mod gif_encode;
mod layer_effects;
mod renderer;
mod renderer_cache;
mod renderer_types;
mod sample_cache;
mod scene;
mod text;
mod values;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_advanced;
#[cfg(test)]
mod tests_advanced_effects;
#[cfg(test)]
mod tests_layers;

#[cfg(feature = "gif")]
pub use renderer::GifRenderConfig;
pub use renderer::{
    ImageAssetResolver, PreparedAnimation, RasterFrame, RenderConfig, Renderer, Rgba8,
};
pub use tiny_skia::Pixmap;
