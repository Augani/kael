// Doing `if let` gives you nice scoping with passes/encoders
#![allow(irrefutable_let_patterns)]

#[cfg(feature = "custom-shaders")]
mod custom_shaders;
#[cfg(all(test, feature = "custom-shaders"))]
mod graph_tests;
#[cfg(test)]
mod offscreen_tests;

use super::{BladeAtlas, BladeContext};
use crate::{
    Background, BlurRect, Bounds, Corners, DevicePixels, GpuSpecs, Hsla, MonochromeSprite, Path,
    Point, PolychromeSprite, PrimitiveBatch, Quad, ScaledPixels, Scene, Shadow, Size, Underline,
};
use blade_graphics as gpu;
use blade_util::{BufferBelt, BufferBeltDescriptor};
use bytemuck::{Pod, Zeroable};
#[cfg(target_os = "macos")]
use media::core_video::CVMetalTextureCache;
use std::sync::Arc;

const MAX_FRAME_TIME_MS: u32 = 10000;
const MAX_SCENE_READBACK_BYTES: usize = 256 * 1024 * 1024;
const SCENE_READBACK_ROW_ALIGNMENT: usize = 256;

pub struct BladeSceneReadback {
    pub width: u32,
    pub height: u32,
    /// Pixels normalized to premultiplied BGRA byte order.
    pub bgra: Vec<u8>,
    pub premultiplied_alpha: bool,
}

#[derive(Clone, Copy)]
struct BladeReadbackTarget {
    texture: gpu::Texture,
    view: gpu::TextureView,
}

#[derive(Clone, Copy)]
struct BladeReadbackLayout {
    width: u32,
    height: u32,
    padded_row_bytes: u32,
    row_bytes: usize,
    allocation_bytes: u64,
    format: gpu::TextureFormat,
}

struct PendingBladeReadback {
    buffer: gpu::Buffer,
    target: BladeReadbackTarget,
    width: u32,
    height: u32,
    padded_row_bytes: usize,
    row_bytes: usize,
    format: gpu::TextureFormat,
    premultiplied_alpha: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalParams {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    pad: u32,
}

//Note: we can't use `Bounds` directly here because
// it doesn't implement Pod + Zeroable
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PodBounds {
    origin: [f32; 2],
    size: [f32; 2],
}

impl From<Bounds<ScaledPixels>> for PodBounds {
    fn from(bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceParams {
    bounds: PodBounds,
    content_mask: PodBounds,
}

#[derive(blade_macros::ShaderData)]
struct ShaderQuadsData {
    globals: GlobalParams,
    b_quads: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderBlurData {
    globals: GlobalParams,
    t_sprite: gpu::TextureView,
    s_sprite: gpu::Sampler,
    b_blurs: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderShadowsData {
    globals: GlobalParams,
    b_shadows: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderPathRasterizationData {
    globals: GlobalParams,
    b_path_vertices: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderPathsData {
    globals: GlobalParams,
    t_sprite: gpu::TextureView,
    s_sprite: gpu::Sampler,
    b_path_sprites: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderUnderlinesData {
    globals: GlobalParams,
    b_underlines: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderMonoSpritesData {
    globals: GlobalParams,
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    t_sprite: gpu::TextureView,
    s_sprite: gpu::Sampler,
    b_mono_sprites: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderPolySpritesData {
    globals: GlobalParams,
    t_sprite: gpu::TextureView,
    s_sprite: gpu::Sampler,
    b_poly_sprites: gpu::BufferPiece,
}

#[derive(blade_macros::ShaderData)]
struct ShaderSurfacesData {
    globals: GlobalParams,
    surface_locals: SurfaceParams,
    t_y: gpu::TextureView,
    t_cb_cr: gpu::TextureView,
    s_surface: gpu::Sampler,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct PathRasterizationVertex {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct BlurPass {
    target_bounds: Bounds<ScaledPixels>,
    sample_bounds: Bounds<ScaledPixels>,
    clip_bounds: Bounds<ScaledPixels>,
    corner_radii: Corners<ScaledPixels>,
    tint: Hsla,
    blur_radius: ScaledPixels,
    saturation: f32,
    rounded_clip_bounds: Bounds<ScaledPixels>,
    rounded_clip_radii: Corners<ScaledPixels>,
}

impl BlurPass {
    fn horizontal(blur_rect: &BlurRect, capture_bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            target_bounds: capture_bounds,
            sample_bounds: capture_bounds,
            clip_bounds: capture_bounds,
            corner_radii: Corners::default(),
            tint: Hsla::transparent_black(),
            blur_radius: blur_rect.blur_radius,
            saturation: 1.0,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
        }
    }

    fn composite(blur_rect: &BlurRect, capture_bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            target_bounds: blur_rect.bounds,
            sample_bounds: capture_bounds,
            clip_bounds: blur_rect.content_mask.bounds,
            corner_radii: blur_rect.corner_radii,
            tint: blur_rect.tint,
            blur_radius: blur_rect.blur_radius,
            saturation: blur_rect.saturation,
            rounded_clip_bounds: blur_rect.rounded_clip_bounds,
            rounded_clip_radii: blur_rect.rounded_clip_radii,
        }
    }
}

struct BladePipelines {
    quads: gpu::RenderPipeline,
    blur_horizontal: gpu::RenderPipeline,
    blur_composite: gpu::RenderPipeline,
    shadows: gpu::RenderPipeline,
    path_rasterization: gpu::RenderPipeline,
    paths: gpu::RenderPipeline,
    underlines: gpu::RenderPipeline,
    mono_sprites: gpu::RenderPipeline,
    poly_sprites: gpu::RenderPipeline,
    surfaces: gpu::RenderPipeline,
}

impl BladePipelines {
    fn new(gpu: &gpu::Context, surface_info: gpu::SurfaceInfo, path_sample_count: u32) -> Self {
        use gpu::ShaderData as _;

        log::info!(
            "Initializing Blade pipelines for surface {:?}",
            surface_info
        );
        let shader = gpu.create_shader(gpu::ShaderDesc {
            source: include_str!("shaders.wgsl"),
            naga_module: None,
        });
        shader.check_struct_size::<GlobalParams>();
        shader.check_struct_size::<SurfaceParams>();
        shader.check_struct_size::<Quad>();
        shader.check_struct_size::<BlurPass>();
        shader.check_struct_size::<Shadow>();
        shader.check_struct_size::<PathRasterizationVertex>();
        shader.check_struct_size::<PathSprite>();
        shader.check_struct_size::<Underline>();
        shader.check_struct_size::<MonochromeSprite>();
        shader.check_struct_size::<PolychromeSprite>();

        // See https://apoorvaj.io/alpha-compositing-opengl-blending-and-premultiplied-alpha/
        let blend_mode = match surface_info.alpha {
            gpu::AlphaMode::Ignored => gpu::BlendState::ALPHA_BLENDING,
            gpu::AlphaMode::PreMultiplied => gpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            gpu::AlphaMode::PostMultiplied => gpu::BlendState::ALPHA_BLENDING,
        };
        let color_targets = &[gpu::ColorTargetState {
            format: surface_info.format,
            blend: Some(blend_mode),
            write_mask: gpu::ColorWrites::default(),
        }];

        Self {
            quads: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "quads",
                data_layouts: &[&ShaderQuadsData::layout()],
                vertex: shader.at("vs_quad"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_quad")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            blur_horizontal: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "blur-horizontal",
                data_layouts: &[&ShaderBlurData::layout()],
                vertex: shader.at("vs_blur"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_blur_horizontal")),
                // Captured framebuffer samples are premultiplied regardless of
                // the surface's shader-output alpha convention.
                color_targets: &[gpu::ColorTargetState {
                    format: surface_info.format,
                    blend: Some(gpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: gpu::ColorWrites::default(),
                }],
                multisample_state: gpu::MultisampleState::default(),
            }),
            blur_composite: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "blur-composite",
                data_layouts: &[&ShaderBlurData::layout()],
                vertex: shader.at("vs_blur"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_blur_composite")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            shadows: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "shadows",
                data_layouts: &[&ShaderShadowsData::layout()],
                vertex: shader.at("vs_shadow"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_shadow")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            path_rasterization: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "path_rasterization",
                data_layouts: &[&ShaderPathRasterizationData::layout()],
                vertex: shader.at("vs_path_rasterization"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_path_rasterization")),
                // The original implementation was using ADDITIVE blende mode,
                // I don't know why
                // color_targets: &[gpu::ColorTargetState {
                //     format: PATH_TEXTURE_FORMAT,
                //     blend: Some(gpu::BlendState::ADDITIVE),
                //     write_mask: gpu::ColorWrites::default(),
                // }],
                color_targets: &[gpu::ColorTargetState {
                    format: surface_info.format,
                    blend: Some(gpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: gpu::ColorWrites::default(),
                }],
                multisample_state: gpu::MultisampleState {
                    sample_count: path_sample_count,
                    ..Default::default()
                },
            }),
            paths: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "paths",
                data_layouts: &[&ShaderPathsData::layout()],
                vertex: shader.at("vs_path"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_path")),
                color_targets: &[gpu::ColorTargetState {
                    format: surface_info.format,
                    blend: Some(gpu::BlendState {
                        color: gpu::BlendComponent::OVER,
                        alpha: gpu::BlendComponent::OVER,
                    }),
                    write_mask: gpu::ColorWrites::default(),
                }],
                multisample_state: gpu::MultisampleState::default(),
            }),
            underlines: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "underlines",
                data_layouts: &[&ShaderUnderlinesData::layout()],
                vertex: shader.at("vs_underline"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_underline")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            mono_sprites: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "mono-sprites",
                data_layouts: &[&ShaderMonoSpritesData::layout()],
                vertex: shader.at("vs_mono_sprite"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_mono_sprite")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            poly_sprites: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "poly-sprites",
                data_layouts: &[&ShaderPolySpritesData::layout()],
                vertex: shader.at("vs_poly_sprite"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_poly_sprite")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
            surfaces: gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "surfaces",
                data_layouts: &[&ShaderSurfacesData::layout()],
                vertex: shader.at("vs_surface"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(shader.at("fs_surface")),
                color_targets,
                multisample_state: gpu::MultisampleState::default(),
            }),
        }
    }

    fn destroy(&mut self, gpu: &gpu::Context) {
        gpu.destroy_render_pipeline(&mut self.quads);
        gpu.destroy_render_pipeline(&mut self.blur_horizontal);
        gpu.destroy_render_pipeline(&mut self.blur_composite);
        gpu.destroy_render_pipeline(&mut self.shadows);
        gpu.destroy_render_pipeline(&mut self.path_rasterization);
        gpu.destroy_render_pipeline(&mut self.paths);
        gpu.destroy_render_pipeline(&mut self.underlines);
        gpu.destroy_render_pipeline(&mut self.mono_sprites);
        gpu.destroy_render_pipeline(&mut self.poly_sprites);
        gpu.destroy_render_pipeline(&mut self.surfaces);
    }
}

pub struct BladeSurfaceConfig {
    pub size: gpu::Extent,
    pub transparent: bool,
}

//Note: we could see some of these fields moved into `BladeContext`
// so that they are shared between windows. E.g. `pipelines`.
// But that is complicated by the fact that pipelines depend on
// the format and alpha mode.
pub struct BladeRenderer {
    gpu: Arc<gpu::Context>,
    surface: Option<gpu::Surface>,
    surface_info: gpu::SurfaceInfo,
    surface_config: gpu::SurfaceConfig,
    command_encoder: gpu::CommandEncoder,
    last_sync_point: Option<gpu::SyncPoint>,
    device_failed: bool,
    #[cfg(test)]
    wait_override: Option<Result<bool, gpu::DeviceError>>,
    #[cfg(test)]
    wait_calls: usize,
    /// Readback buffers whose copy submission did not complete within the
    /// synchronous export deadline. They remain alive until the queue's
    /// tracked sync point completes (or the device reports a terminal error).
    deferred_readbacks: Vec<PendingBladeReadback>,
    pipelines: BladePipelines,
    instance_belt: BufferBelt,
    atlas: Arc<BladeAtlas>,
    atlas_byte_budget: Option<u64>,
    atlas_sampler: gpu::Sampler,
    #[cfg(target_os = "macos")]
    core_video_texture_cache: CVMetalTextureCache,
    path_intermediate_texture: Option<gpu::Texture>,
    path_intermediate_texture_view: Option<gpu::TextureView>,
    path_intermediate_msaa_texture: Option<gpu::Texture>,
    path_intermediate_msaa_texture_view: Option<gpu::TextureView>,
    cached_surface_texture: Option<gpu::Texture>,
    cached_surface_texture_view: Option<gpu::TextureView>,
    blur_source_texture: Option<gpu::Texture>,
    blur_source_texture_view: Option<gpu::TextureView>,
    blur_horizontal_texture: Option<gpu::Texture>,
    blur_horizontal_texture_view: Option<gpu::TextureView>,
    rendering_parameters: RenderingParameters,
    #[cfg(feature = "custom-shaders")]
    custom: custom_shaders::BladeCustomRenderer,
}

impl BladeRenderer {
    #[cfg(test)]
    fn scratch_texture_count(&self) -> usize {
        [
            self.path_intermediate_texture,
            self.path_intermediate_msaa_texture,
            self.cached_surface_texture,
            self.blur_source_texture,
            self.blur_horizontal_texture,
        ]
        .iter()
        .filter(|texture| texture.is_some())
        .count()
    }
    pub fn new<I: raw_window_handle::HasWindowHandle + raw_window_handle::HasDisplayHandle>(
        context: &BladeContext,
        window: &I,
        config: BladeSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let surface_config = gpu::SurfaceConfig {
            size: config.size,
            usage: gpu::TextureUsage::TARGET,
            display_sync: gpu::DisplaySync::Recent,
            color_space: gpu::ColorSpace::Srgb,
            allow_exclusive_full_screen: false,
            transparent: config.transparent,
        };
        let surface = context
            .gpu
            .create_surface_configured(window, surface_config)
            .map_err(|err| anyhow::anyhow!("Failed to create surface: {err:?}"))?;

        let surface_info = surface.info();
        Self::with_surface(context, Some(surface), surface_info, surface_config)
    }

    fn with_surface(
        context: &BladeContext,
        surface: Option<gpu::Surface>,
        surface_info: gpu::SurfaceInfo,
        surface_config: gpu::SurfaceConfig,
    ) -> anyhow::Result<Self> {
        let command_encoder = context.gpu.create_command_encoder(gpu::CommandEncoderDesc {
            name: "main",
            buffer_count: 2,
        });
        let rendering_parameters = RenderingParameters::from_env(context);
        let pipelines = BladePipelines::new(
            &context.gpu,
            surface_info,
            rendering_parameters.path_sample_count,
        );
        let instance_belt = BufferBelt::new(BufferBeltDescriptor {
            memory: gpu::Memory::Shared,
            min_chunk_size: 0x1000,
            alignment: 0x40, // Vulkan `minStorageBufferOffsetAlignment` on Intel Xe
        });
        let atlas = Arc::new(BladeAtlas::new(&context.gpu));
        let atlas_sampler = context.gpu.create_sampler(gpu::SamplerDesc {
            name: "path rasterization sampler",
            mag_filter: gpu::FilterMode::Linear,
            min_filter: gpu::FilterMode::Linear,
            ..Default::default()
        });

        #[cfg(target_os = "macos")]
        let core_video_texture_cache = {
            let metal_device = context.gpu.metal_device();
            let metal_device = objc2::rc::Retained::as_ptr(&metal_device) as *const _;
            // SAFETY: Blade returned a live, retained object conforming to
            // MTLDevice. It remains alive through this call, and CoreVideo
            // retains the device for the cache lifetime.
            unsafe { CVMetalTextureCache::new(metal_device) }
        }
        .map_err(|error| anyhow::anyhow!("failed to create CoreVideo texture cache: {error:#}"))?;

        Ok(Self {
            gpu: Arc::clone(&context.gpu),
            surface,
            surface_info,
            surface_config,
            command_encoder,
            last_sync_point: None,
            device_failed: false,
            #[cfg(test)]
            wait_override: None,
            #[cfg(test)]
            wait_calls: 0,
            deferred_readbacks: Vec::new(),
            pipelines,
            instance_belt,
            atlas,
            atlas_byte_budget: None,
            atlas_sampler,
            #[cfg(target_os = "macos")]
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_texture_view: None,
            path_intermediate_msaa_texture: None,
            path_intermediate_msaa_texture_view: None,
            cached_surface_texture: None,
            cached_surface_texture_view: None,
            blur_source_texture: None,
            blur_source_texture_view: None,
            blur_horizontal_texture: None,
            blur_horizontal_texture_view: None,
            rendering_parameters,
            #[cfg(feature = "custom-shaders")]
            custom: custom_shaders::BladeCustomRenderer::new(Arc::clone(&context.gpu)),
        })
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn gpu_allocated_bytes(&self) -> u64 {
        use objc2_metal::MTLDevice as _;
        self.gpu.metal_device().currentAllocatedSize() as u64
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn create_gpu_buffer(
        &mut self,
        descriptor: crate::GpuBufferDescriptor,
    ) -> std::result::Result<crate::GpuBuffer, crate::RenderTargetError> {
        self.custom.create_buffer(descriptor)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn validate_gpu_buffer(
        &mut self,
        buffer: &crate::GpuBuffer,
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.validate_buffer(buffer)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn write_gpu_buffer(
        &mut self,
        buffer: &crate::GpuBuffer,
        offset: u64,
        bytes: &[u8],
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.write_buffer(buffer, offset, bytes)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn read_gpu_buffer(
        &mut self,
        buffer: &crate::GpuBuffer,
    ) -> std::result::Result<Vec<u8>, crate::RenderTargetError> {
        self.custom.read_buffer(buffer)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn dispatch_compute(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.dispatch(shader, bindings, groups)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn write_render_target(
        &mut self,
        target: &crate::RenderTarget,
        pixels: &[u8],
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.write_target(target, pixels)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn create_render_target(
        &mut self,
        descriptor: crate::RenderTargetDescriptor,
    ) -> Result<crate::RenderTarget, crate::RenderTargetError> {
        self.custom.create(descriptor)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn render_shader(
        &mut self,
        target: &crate::RenderTarget,
        shader: &crate::ShaderHandle,
        bindings: &crate::ShaderBindings,
    ) -> Result<(), crate::RenderTargetError> {
        self.custom.render(target, shader, bindings)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn read_render_target(
        &mut self,
        target: &crate::RenderTarget,
    ) -> Result<crate::RenderTargetReadback, crate::RenderTargetError> {
        self.custom.read(target)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn validate_render_target(
        &self,
        target: &crate::RenderTarget,
    ) -> Result<(), crate::RenderTargetError> {
        self.custom.validate(target)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn set_render_target_byte_budget(&mut self, bytes: u64) {
        self.custom.set_budget(bytes);
    }

    fn wait_for_submission(&mut self, point: &gpu::SyncPoint) -> Result<bool, gpu::DeviceError> {
        #[cfg(test)]
        {
            self.wait_calls += 1;
            if let Some(result) = self.wait_override.clone() {
                return result;
            }
        }
        self.gpu.wait_for(point, MAX_FRAME_TIME_MS)
    }

    fn invalidate_device(&mut self) {
        self.device_failed = true;
        #[cfg(feature = "custom-shaders")]
        self.custom.invalidate_device();
    }

    /// Return false while encoded resources may still be in flight. A timeout
    /// or memory error preserves the tracked fence for later retirement; only
    /// a completed fence permits destroying queued resources.
    fn wait_for_gpu(&mut self) -> bool {
        let Some(point) = self.last_sync_point.clone() else {
            debug_assert!(self.deferred_readbacks.is_empty());
            return true;
        };
        match self.wait_for_submission(&point) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                self.invalidate_device();
                log::error!(
                    "Blade scene submission did not complete within its bounded wait; retaining in-flight resources"
                );
                return false;
            }
        }
        self.last_sync_point = None;
        for pending in std::mem::take(&mut self.deferred_readbacks) {
            self.destroy_scene_readback_resources(pending);
        }
        true
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        self.update_drawable_size_impl(size, false);
    }

    /// Like `update_drawable_size` but skips the check that the size has changed. This is useful in
    /// cases like restoring a window from minimization where the size is the same but the
    /// renderer's swap chain needs to be recreated.
    #[cfg_attr(
        any(target_os = "macos", target_os = "linux", target_os = "freebsd"),
        allow(dead_code)
    )]
    pub fn update_drawable_size_even_if_unchanged(&mut self, size: Size<DevicePixels>) {
        self.update_drawable_size_impl(size, true);
    }

    fn update_drawable_size_impl(&mut self, size: Size<DevicePixels>, always_resize: bool) {
        let gpu_size = gpu::Extent {
            width: u32::try_from(size.width.0).unwrap_or(0),
            height: u32::try_from(size.height.0).unwrap_or(0),
            depth: 1,
        };

        if always_resize || gpu_size != self.surface_config.size {
            if !self.wait_for_gpu() {
                return;
            }
            self.surface_config.size = gpu_size;
            if let Some(surface) = &mut self.surface {
                if gpu_size.width == 0 || gpu_size.height == 0 {
                    self.release_scratch();
                    return;
                }
                self.gpu.reconfigure_surface(surface, self.surface_config);
                self.surface_info = surface.info();
            }
            self.release_scratch();
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        if transparent != self.surface_config.transparent {
            if !self.wait_for_gpu() {
                return;
            }
            self.surface_config.transparent = transparent;
            if let Some(surface) = &mut self.surface {
                self.gpu.reconfigure_surface(surface, self.surface_config);
                self.surface_info = surface.info();
            }
            self.release_scratch();
            self.pipelines.destroy(&self.gpu);
            self.pipelines = BladePipelines::new(
                &self.gpu,
                self.surface_info,
                self.rendering_parameters.path_sample_count,
            );
        }
    }

    #[cfg_attr(
        any(target_os = "macos", feature = "wayland", target_os = "windows"),
        allow(dead_code)
    )]
    pub fn viewport_size(&self) -> gpu::Extent {
        self.surface_config.size
    }

    pub fn sprite_atlas(&self) -> &Arc<BladeAtlas> {
        &self.atlas
    }

    /// Set a soft byte budget for the glyph/sprite atlas. When set, the renderer evicts the
    /// least-recently-used atlas tiles down to this budget at the end of each frame
    /// (protecting tiles still in flight). `None` (the default) disables eviction.
    #[allow(dead_code)]
    pub fn set_atlas_byte_budget(&mut self, budget: Option<u64>) {
        self.atlas_byte_budget = budget;
        self.atlas.set_admission_limits(budget);
    }

    pub(crate) fn shed_memory(&mut self, level: crate::MemoryPressureLevel) {
        if level != crate::MemoryPressureLevel::Normal {
            if !self.wait_for_gpu() {
                return;
            }
            self.release_scratch();
            #[cfg(feature = "custom-shaders")]
            self.custom.shed_memory();
            self.atlas
                .evict_to_budget_keeping(self.atlas_byte_budget.unwrap_or(0), 4);
        }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn gpu_specs(&self) -> GpuSpecs {
        let info = self.gpu.device_information();

        GpuSpecs {
            is_software_emulated: info.is_software_emulated,
            device_name: info.device_name.clone(),
            driver_name: info.driver_name.clone(),
            driver_info: info.driver_info.clone(),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn layer_ptr(&self) -> *mut metal::CAMetalLayer {
        objc2::rc::Retained::as_ptr(&self.surface.as_ref().expect("native surface").metal_layer())
            as *mut _
    }

    // Scratch images match the current viewport exactly because path UVs use
    // viewport dimensions. Resize and pressure callers wait for the queue
    // before releasing the old images; ordinary scenes allocate none.
    fn ensure_path_intermediate(&mut self) {
        if self.path_intermediate_texture.is_none() {
            let size = self.surface_config.size;
            let (texture, view) = create_path_intermediate_texture(
                &self.gpu,
                self.surface_info.format,
                size.width,
                size.height,
            );
            self.path_intermediate_texture = Some(texture);
            self.path_intermediate_texture_view = Some(view);
            (
                self.path_intermediate_msaa_texture,
                self.path_intermediate_msaa_texture_view,
            ) = create_msaa_texture_if_needed(
                &self.gpu,
                self.surface_info.format,
                size.width,
                size.height,
                self.rendering_parameters.path_sample_count,
            )
            .unzip();
        }
    }

    fn ensure_cached_surface(&mut self) {
        if self.cached_surface_texture.is_none() {
            let size = self.surface_config.size;
            let (texture, view) = create_path_intermediate_texture(
                &self.gpu,
                self.surface_info.format,
                size.width,
                size.height,
            );
            self.cached_surface_texture = Some(texture);
            self.cached_surface_texture_view = Some(view);
        }
    }

    fn ensure_blur_intermediates(&mut self) {
        let size = self.surface_config.size;
        for (texture, view) in [
            (
                &mut self.blur_source_texture,
                &mut self.blur_source_texture_view,
            ),
            (
                &mut self.blur_horizontal_texture,
                &mut self.blur_horizontal_texture_view,
            ),
        ] {
            if texture.is_none() {
                let (new_texture, new_view) = create_path_intermediate_texture(
                    &self.gpu,
                    self.surface_info.format,
                    size.width,
                    size.height,
                );
                *texture = Some(new_texture);
                *view = Some(new_view);
            }
        }
    }

    /// The tracked scene submission must have completed before calling this.
    fn release_scratch(&mut self) {
        for (texture, view) in [
            (
                &mut self.path_intermediate_texture,
                &mut self.path_intermediate_texture_view,
            ),
            (
                &mut self.path_intermediate_msaa_texture,
                &mut self.path_intermediate_msaa_texture_view,
            ),
            (
                &mut self.cached_surface_texture,
                &mut self.cached_surface_texture_view,
            ),
            (
                &mut self.blur_source_texture,
                &mut self.blur_source_texture_view,
            ),
            (
                &mut self.blur_horizontal_texture,
                &mut self.blur_horizontal_texture_view,
            ),
        ] {
            if let Some(view) = view.take() {
                self.gpu.destroy_texture_view(view);
            }
            if let Some(texture) = texture.take() {
                self.gpu.destroy_texture(texture);
            }
        }
    }

    #[profiling::function]
    fn draw_paths_to_intermediate(
        &mut self,
        paths: &[Path<ScaledPixels>],
        width: f32,
        height: f32,
    ) {
        self.ensure_path_intermediate();
        self.command_encoder
            .init_texture(self.path_intermediate_texture.unwrap());
        if let Some(msaa_texture) = self.path_intermediate_msaa_texture {
            self.command_encoder.init_texture(msaa_texture);
        }

        let target = if let Some(msaa_view) = self.path_intermediate_msaa_texture_view {
            gpu::RenderTarget {
                view: msaa_view,
                init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
                finish_op: gpu::FinishOp::ResolveTo(self.path_intermediate_texture_view.unwrap()),
            }
        } else {
            gpu::RenderTarget {
                view: self.path_intermediate_texture_view.unwrap(),
                init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
                finish_op: gpu::FinishOp::Store,
            }
        };
        if let mut pass = self.command_encoder.render(
            "rasterize paths",
            gpu::RenderTargetSet {
                colors: &[target],
                depth_stencil: None,
            },
        ) {
            let globals = GlobalParams {
                viewport_size: [width, height],
                premultiplied_alpha: 0,
                pad: 0,
            };
            let mut encoder = pass.with(&self.pipelines.path_rasterization);

            let mut vertices = Vec::new();
            for path in paths {
                vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                    xy_position: v.xy_position,
                    st_position: v.st_position,
                    color: path.color,
                    bounds: path.clipped_bounds(),
                }));
            }
            let vertex_buf = unsafe { self.instance_belt.alloc_typed(&vertices, &self.gpu) };
            encoder.bind(
                0,
                &ShaderPathRasterizationData {
                    globals,
                    b_path_vertices: vertex_buf,
                },
            );
            encoder.draw(0, vertices.len() as u32, 0, 1);
        }
    }

    pub fn destroy(&mut self) {
        if !self.wait_for_gpu() {
            // Vulkan resources are freed immediately by destroy_*. Preserve
            // this device and its raw allocations if the queue remains hung.
            // Metal command buffers retain encoded objects until completion.
            std::mem::forget(Arc::clone(&self.gpu));
            log::error!(
                "Blade scene teardown retained a pending device instead of freeing in-flight resources"
            );
            return;
        }
        self.atlas.destroy();
        self.gpu.destroy_sampler(self.atlas_sampler);
        self.instance_belt.destroy(&self.gpu);
        self.gpu.destroy_command_encoder(&mut self.command_encoder);
        self.pipelines.destroy(&self.gpu);
        if let Some(surface) = &mut self.surface {
            self.gpu.destroy_surface(surface);
        }
        self.release_scratch();
    }

    pub fn draw(&mut self, scene: &Scene) {
        if let Err(error) = self.draw_internal(scene, false) {
            log::error!("Blade scene rendering failed: {error:#}");
        }
    }

    pub fn render_scene_to_bgra(&mut self, scene: &Scene) -> anyhow::Result<BladeSceneReadback> {
        self.draw_internal(scene, true)?
            .ok_or_else(|| anyhow::anyhow!("Blade scene readback was not produced"))
    }

    fn draw_internal(
        &mut self,
        scene: &Scene,
        capture: bool,
    ) -> anyhow::Result<Option<BladeSceneReadback>> {
        anyhow::ensure!(
            !self.device_failed,
            "Blade scene device requires recreation after submission failure"
        );
        if !capture && (self.surface_config.size.width == 0 || self.surface_config.size.height == 0)
        {
            return Ok(None);
        }
        if !self.deferred_readbacks.is_empty() {
            anyhow::ensure!(
                self.wait_for_gpu(),
                "Blade scene readback retirement is still pending"
            );
        }
        let readback_layout = capture.then(|| self.scene_readback_layout()).transpose()?;
        self.atlas.mark_scene_used(scene);
        self.command_encoder.start();
        self.atlas.before_frame(&mut self.command_encoder);

        let frame = if capture {
            None
        } else {
            profiling::scope!("acquire frame");
            Some(
                self.surface
                    .as_mut()
                    .ok_or_else(|| {
                        anyhow::anyhow!("offscreen Blade renderer has no presentation surface")
                    })?
                    .acquire_frame(),
            )
        };
        let readback_target =
            readback_layout.map(|layout| self.create_scene_readback_target(layout.format));
        let (target_texture, target_view) = if let Some(target) = readback_target {
            (target.texture, target.view)
        } else {
            let frame = frame
                .as_ref()
                .expect("a non-capture Blade draw must acquire a surface frame");
            (frame.texture(), frame.texture_view())
        };
        self.command_encoder.init_texture(target_texture);

        let globals = GlobalParams {
            viewport_size: [
                self.surface_config.size.width as f32,
                self.surface_config.size.height as f32,
            ],
            // Reuse the surface pipeline's matching shader/blend contract.
            // Both contracts leave premultiplied pixels in the render target:
            // straight shader output is multiplied by ALPHA_BLENDING, while
            // premultiplied shader output uses PREMULTIPLIED_ALPHA_BLENDING.
            premultiplied_alpha: match self.surface_info.alpha {
                gpu::AlphaMode::Ignored | gpu::AlphaMode::PostMultiplied => 0,
                gpu::AlphaMode::PreMultiplied => 1,
            },
            pad: 0,
        };

        let mut pass = self.command_encoder.render(
            "main",
            gpu::RenderTargetSet {
                colors: &[gpu::RenderTarget {
                    view: target_view,
                    init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
                    finish_op: gpu::FinishOp::Store,
                }],
                depth_stencil: None,
            },
        );

        profiling::scope!("render pass");
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::BlurRects(blur_rects) => {
                    drop(pass);
                    self.draw_blur_rects(blur_rects, target_texture, target_view, globals);
                    pass = self.command_encoder.render(
                        "main",
                        gpu::RenderTargetSet {
                            colors: &[gpu::RenderTarget {
                                view: target_view,
                                init_op: gpu::InitOp::Load,
                                finish_op: gpu::FinishOp::Store,
                            }],
                            depth_stencil: None,
                        },
                    );
                }
                PrimitiveBatch::Quads(quads) => {
                    let instance_buf = unsafe { self.instance_belt.alloc_typed(quads, &self.gpu) };
                    let mut encoder = pass.with(&self.pipelines.quads);
                    encoder.bind(
                        0,
                        &ShaderQuadsData {
                            globals,
                            b_quads: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, quads.len() as u32);
                }
                PrimitiveBatch::Shadows(shadows) => {
                    let instance_buf =
                        unsafe { self.instance_belt.alloc_typed(shadows, &self.gpu) };
                    let mut encoder = pass.with(&self.pipelines.shadows);
                    encoder.bind(
                        0,
                        &ShaderShadowsData {
                            globals,
                            b_shadows: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, shadows.len() as u32);
                }
                PrimitiveBatch::Paths(paths) => {
                    let Some(first_path) = paths.first() else {
                        continue;
                    };
                    drop(pass);
                    self.draw_paths_to_intermediate(
                        paths,
                        self.surface_config.size.width as f32,
                        self.surface_config.size.height as f32,
                    );
                    pass = self.command_encoder.render(
                        "main",
                        gpu::RenderTargetSet {
                            colors: &[gpu::RenderTarget {
                                view: target_view,
                                init_op: gpu::InitOp::Load,
                                finish_op: gpu::FinishOp::Store,
                            }],
                            depth_stencil: None,
                        },
                    );
                    let mut encoder = pass.with(&self.pipelines.paths);
                    // When copying paths from the intermediate texture to the drawable,
                    // each pixel must only be copied once, in case of transparent paths.
                    //
                    // If all paths have the same draw order, then their bounds are all
                    // disjoint, so we can copy each path's bounds individually. If this
                    // batch combines different draw orders, we perform a single copy
                    // for a minimal spanning rect.
                    let sprites = if paths.last().unwrap().order == first_path.order {
                        paths
                            .iter()
                            .map(|path| PathSprite {
                                bounds: path.clipped_bounds(),
                            })
                            .collect()
                    } else {
                        let mut bounds = first_path.clipped_bounds();
                        for path in paths.iter().skip(1) {
                            bounds = bounds.union(&path.clipped_bounds());
                        }
                        vec![PathSprite { bounds }]
                    };
                    let instance_buf =
                        unsafe { self.instance_belt.alloc_typed(&sprites, &self.gpu) };
                    encoder.bind(
                        0,
                        &ShaderPathsData {
                            globals,
                            t_sprite: self.path_intermediate_texture_view.unwrap(),
                            s_sprite: self.atlas_sampler,
                            b_path_sprites: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, sprites.len() as u32);
                }
                PrimitiveBatch::Underlines(underlines) => {
                    let instance_buf =
                        unsafe { self.instance_belt.alloc_typed(underlines, &self.gpu) };
                    let mut encoder = pass.with(&self.pipelines.underlines);
                    encoder.bind(
                        0,
                        &ShaderUnderlinesData {
                            globals,
                            b_underlines: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, underlines.len() as u32);
                }
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => {
                    let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
                        log::warn!("skipping monochrome sprites with a stale Blade atlas texture");
                        continue;
                    };
                    let instance_buf =
                        unsafe { self.instance_belt.alloc_typed(sprites, &self.gpu) };
                    let mut encoder = pass.with(&self.pipelines.mono_sprites);
                    encoder.bind(
                        0,
                        &ShaderMonoSpritesData {
                            globals,
                            gamma_ratios: self.rendering_parameters.gamma_ratios,
                            grayscale_enhanced_contrast: self
                                .rendering_parameters
                                .grayscale_enhanced_contrast,
                            t_sprite: tex_info.raw_view,
                            s_sprite: self.atlas_sampler,
                            b_mono_sprites: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, sprites.len() as u32);
                }
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => {
                    let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
                        log::warn!("skipping polychrome sprites with a stale Blade atlas texture");
                        continue;
                    };
                    let instance_buf =
                        unsafe { self.instance_belt.alloc_typed(sprites, &self.gpu) };
                    let mut encoder = pass.with(&self.pipelines.poly_sprites);
                    encoder.bind(
                        0,
                        &ShaderPolySpritesData {
                            globals,
                            t_sprite: tex_info.raw_view,
                            s_sprite: self.atlas_sampler,
                            b_poly_sprites: instance_buf,
                        },
                    );
                    encoder.draw(0, 4, 0, sprites.len() as u32);
                }
                PrimitiveBatch::Surfaces(surfaces) => {
                    for surface in surfaces {
                        #[cfg(feature = "custom-shaders")]
                        if let crate::PaintSurfaceSource::RenderTarget { target, .. } =
                            &surface.source
                        {
                            let viewport = crate::size(
                                DevicePixels(globals.viewport_size[0] as i32),
                                DevicePixels(globals.viewport_size[1] as i32),
                            );
                            if let Err(error) = self.custom.draw(
                                surface,
                                target,
                                viewport,
                                self.surface_info.format,
                                &mut pass,
                            ) {
                                log::error!("custom target display failed: {error}");
                            }
                            continue;
                        }

                        #[cfg(not(target_os = "macos"))]
                        {
                            let _ = surface;
                            continue;
                        };

                        #[cfg(target_os = "macos")]
                        {
                            let crate::PaintSurfaceSource::CoreVideo(image_buffer) =
                                &surface.source
                            else {
                                continue;
                            };
                            let (t_y, t_cb_cr) = {
                                if image_buffer.get_pixel_format()
                                    != core_video::pixel_buffer::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
                                {
                                    log::warn!("skipping Blade surface with unsupported pixel format");
                                    continue;
                                }

                                let Ok(y_texture) =
                                    self.core_video_texture_cache.create_texture_from_image(
                                        &image_buffer,
                                        None,
                                        metal::MTLPixelFormat::R8Unorm,
                                        image_buffer.get_width_of_plane(0),
                                        image_buffer.get_height_of_plane(0),
                                        0,
                                    )
                                else {
                                    log::warn!("failed to create Blade Y-plane Metal texture");
                                    continue;
                                };
                                let Ok(cb_cr_texture) =
                                    self.core_video_texture_cache.create_texture_from_image(
                                        &image_buffer,
                                        None,
                                        metal::MTLPixelFormat::RG8Unorm,
                                        image_buffer.get_width_of_plane(1),
                                        image_buffer.get_height_of_plane(1),
                                        1,
                                    )
                                else {
                                    log::warn!("failed to create Blade chroma-plane Metal texture");
                                    continue;
                                };
                                let Some(y_texture_ref) = y_texture.as_texture_ref() else {
                                    log::warn!("CoreVideo Y-plane texture has no Metal texture");
                                    continue;
                                };
                                let Some(cb_cr_texture_ref) = cb_cr_texture.as_texture_ref() else {
                                    log::warn!(
                                        "CoreVideo chroma-plane texture has no Metal texture"
                                    );
                                    continue;
                                };
                                let Some(y_texture) = retain_core_video_texture(y_texture_ref)
                                else {
                                    log::warn!("failed to retain Blade Y-plane Metal texture");
                                    continue;
                                };
                                let Some(cb_cr_texture) =
                                    retain_core_video_texture(cb_cr_texture_ref)
                                else {
                                    log::warn!("failed to retain Blade chroma-plane Metal texture");
                                    continue;
                                };
                                (
                                    gpu::TextureView::from_metal_texture(
                                        &y_texture,
                                        gpu::TexelAspects::COLOR,
                                    ),
                                    gpu::TextureView::from_metal_texture(
                                        &cb_cr_texture,
                                        gpu::TexelAspects::COLOR,
                                    ),
                                )
                            };

                            let mut _encoder = pass.with(&self.pipelines.surfaces);
                            _encoder.bind(
                                0,
                                &ShaderSurfacesData {
                                    globals,
                                    surface_locals: SurfaceParams {
                                        bounds: surface.bounds.into(),
                                        content_mask: surface.content_mask.bounds.into(),
                                    },
                                    t_y,
                                    t_cb_cr,
                                    s_surface: self.atlas_sampler,
                                },
                            );

                            _encoder.draw(0, 4, 0, 1);
                        }
                    }
                }
            }
        }
        drop(pass);

        self.draw_cached_surface_snapshots(scene);

        let pending_readback = readback_target
            .zip(readback_layout)
            .map(|(target, layout)| self.enqueue_scene_readback(target, layout));

        if let Some(frame) = frame {
            self.command_encoder.present(frame);
        }
        let sync_point = self.gpu.submit(&mut self.command_encoder);
        #[cfg(feature = "custom-shaders")]
        self.custom.after_frame(&sync_point);

        profiling::scope!("finish");
        self.instance_belt.flush(&sync_point);
        self.atlas.after_frame(&sync_point);

        let readback = if let Some(pending) = pending_readback {
            let wait_result = self.wait_for_submission(&sync_point);
            match wait_result {
                Ok(true) => Some(self.finish_scene_readback(pending)?),
                Ok(false) => {
                    // Keep the in-flight resource alive. The current queue
                    // sync point covers this copy and all earlier work.
                    self.invalidate_device();
                    self.deferred_readbacks.push(pending);
                    self.last_sync_point = Some(sync_point);
                    anyhow::bail!("Blade scene readback timed out waiting for the GPU");
                }
                Err(error) => {
                    self.invalidate_device();
                    self.deferred_readbacks.push(pending);
                    self.last_sync_point = Some(sync_point);
                    return Err(anyhow::anyhow!(
                        "Blade scene readback GPU wait failed: {error:?}"
                    ));
                }
            }
        } else {
            None
        };

        let previous_completed = self.wait_for_gpu();
        self.last_sync_point = Some(sync_point);
        anyhow::ensure!(
            previous_completed && !self.device_failed,
            "Blade scene submission failure requires device recreation"
        );
        // Only a successful fence gate advances retirement. Failed submissions
        // must not make pending atlas regions eligible for overwrite or release.
        if let Some(budget) = self.atlas_byte_budget {
            const IN_FLIGHT_FRAMES: u64 = 4;
            self.atlas.evict_to_budget_keeping(budget, IN_FLIGHT_FRAMES);
        }
        self.atlas.advance_frame();
        Ok(readback)
    }

    fn create_scene_readback_target(&self, format: gpu::TextureFormat) -> BladeReadbackTarget {
        let texture = self.gpu.create_texture(gpu::TextureDesc {
            name: "Kael scene readback target",
            format,
            size: self.surface_config.size,
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage: gpu::TextureUsage::TARGET | gpu::TextureUsage::COPY,
            external: None,
        });
        let view = self.gpu.create_texture_view(
            texture,
            gpu::TextureViewDesc {
                name: "Kael scene readback target view",
                format,
                dimension: gpu::ViewDimension::D2,
                subresources: &Default::default(),
            },
        );
        BladeReadbackTarget { texture, view }
    }

    fn scene_readback_layout(&self) -> anyhow::Result<BladeReadbackLayout> {
        let width = self.surface_config.size.width;
        let height = self.surface_config.size.height;
        anyhow::ensure!(
            width > 0 && height > 0,
            "Blade scene readback target is empty"
        );
        let format = self.surface_info.format;
        anyhow::ensure!(
            matches!(
                format,
                gpu::TextureFormat::Bgra8Unorm
                    | gpu::TextureFormat::Bgra8UnormSrgb
                    | gpu::TextureFormat::Rgba8Unorm
                    | gpu::TextureFormat::Rgba8UnormSrgb
            ),
            "Blade scene readback does not support surface format {format:?}"
        );
        let row_bytes = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("Blade scene readback row byte count overflowed"))?;
        let padded_row_bytes = row_bytes
            .checked_add(SCENE_READBACK_ROW_ALIGNMENT - 1)
            .map(|value| value & !(SCENE_READBACK_ROW_ALIGNMENT - 1))
            .ok_or_else(|| anyhow::anyhow!("Blade scene readback row alignment overflowed"))?;
        let allocation_bytes = usize::try_from(height)
            .ok()
            .and_then(|height| height.checked_mul(padded_row_bytes))
            .ok_or_else(|| anyhow::anyhow!("Blade scene readback byte count overflowed"))?;
        anyhow::ensure!(
            allocation_bytes <= MAX_SCENE_READBACK_BYTES,
            "Blade scene readback exceeds the {MAX_SCENE_READBACK_BYTES}-byte safety limit"
        );
        let padded_row_bytes = u32::try_from(padded_row_bytes)
            .map_err(|_| anyhow::anyhow!("Blade scene readback row pitch exceeds u32"))?;
        Ok(BladeReadbackLayout {
            width,
            height,
            padded_row_bytes,
            row_bytes,
            allocation_bytes: u64::try_from(allocation_bytes)
                .map_err(|_| anyhow::anyhow!("Blade scene readback size does not fit u64"))?,
            format,
        })
    }

    fn enqueue_scene_readback(
        &mut self,
        target: BladeReadbackTarget,
        layout: BladeReadbackLayout,
    ) -> PendingBladeReadback {
        let buffer = self.gpu.create_buffer(gpu::BufferDesc {
            name: "Kael scene readback",
            size: layout.allocation_bytes,
            memory: gpu::Memory::Shared,
        });
        let mut transfer = self.command_encoder.transfer("scene readback");
        transfer.copy_texture_to_buffer(
            target.texture.into(),
            buffer.into(),
            layout.padded_row_bytes,
            self.surface_config.size,
        );
        drop(transfer);
        PendingBladeReadback {
            buffer,
            target,
            width: layout.width,
            height: layout.height,
            padded_row_bytes: layout.padded_row_bytes as usize,
            row_bytes: layout.row_bytes,
            format: layout.format,
            // Every current pipeline stores a premultiplied composited target:
            // PreMultiplied emits premultiplied RGB with OVER blending, while
            // Ignored/PostMultiplied emits straight RGB that ALPHA_BLENDING
            // multiplies by source alpha as it writes the transparent target.
            premultiplied_alpha: true,
        }
    }

    fn finish_scene_readback(
        &self,
        pending: PendingBladeReadback,
    ) -> anyhow::Result<BladeSceneReadback> {
        // Keep cleanup outside the fallible copy closure so every allocation,
        // pointer, and arithmetic error releases the GPU buffer exactly once.
        let copy_result = (|| -> anyhow::Result<Vec<u8>> {
            let row_count = usize::try_from(pending.height)
                .map_err(|_| anyhow::anyhow!("Blade scene readback height does not fit usize"))?;
            let output_len = row_count
                .checked_mul(pending.row_bytes)
                .ok_or_else(|| anyhow::anyhow!("Blade scene readback output size overflowed"))?;
            let source = pending.buffer.data();
            anyhow::ensure!(
                !source.is_null(),
                "Blade scene readback buffer is not host-visible"
            );
            let mut bgra = Vec::new();
            bgra.try_reserve_exact(output_len).map_err(|error| {
                anyhow::anyhow!("allocating Blade scene readback buffer: {error}")
            })?;
            bgra.resize(output_len, 0);
            for row in 0..row_count {
                let source_offset = row
                    .checked_mul(pending.padded_row_bytes)
                    .ok_or_else(|| anyhow::anyhow!("Blade source row offset overflowed"))?;
                let destination_offset = row
                    .checked_mul(pending.row_bytes)
                    .ok_or_else(|| anyhow::anyhow!("Blade destination row offset overflowed"))?;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.add(source_offset),
                        bgra.as_mut_ptr().add(destination_offset),
                        pending.row_bytes,
                    );
                }
            }
            Ok(bgra)
        })();
        let format = pending.format;
        let width = pending.width;
        let height = pending.height;
        let premultiplied_alpha = pending.premultiplied_alpha;
        self.destroy_scene_readback_resources(pending);
        let mut bgra = copy_result?;

        if matches!(
            format,
            gpu::TextureFormat::Rgba8Unorm | gpu::TextureFormat::Rgba8UnormSrgb
        ) {
            for pixel in bgra.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok(BladeSceneReadback {
            width,
            height,
            bgra,
            premultiplied_alpha,
        })
    }

    fn destroy_scene_readback_resources(&self, pending: PendingBladeReadback) {
        self.gpu.destroy_buffer(pending.buffer);
        self.gpu.destroy_texture_view(pending.target.view);
        self.gpu.destroy_texture(pending.target.texture);
    }

    fn draw_cached_surface_snapshots(&mut self, scene: &Scene) {
        if scene.cached_surface_snapshots.is_empty() {
            return;
        }
        self.ensure_cached_surface();
        for snapshot in &scene.cached_surface_snapshots {
            let snapshot_scene = scene.snapshot_subscene(snapshot.paint_operations.clone());
            self.command_encoder
                .init_texture(self.cached_surface_texture.unwrap());

            let globals = GlobalParams {
                viewport_size: [
                    self.surface_config.size.width as f32,
                    self.surface_config.size.height as f32,
                ],
                premultiplied_alpha: match self.surface_info.alpha {
                    gpu::AlphaMode::Ignored | gpu::AlphaMode::PostMultiplied => 0,
                    gpu::AlphaMode::PreMultiplied => 1,
                },
                pad: 0,
            };

            let mut pass = self.command_encoder.render(
                "cached surface snapshot",
                gpu::RenderTargetSet {
                    colors: &[gpu::RenderTarget {
                        view: self.cached_surface_texture_view.unwrap(),
                        init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
                        finish_op: gpu::FinishOp::Store,
                    }],
                    depth_stencil: None,
                },
            );

            for batch in snapshot_scene.batches() {
                match batch {
                    PrimitiveBatch::BlurRects(blur_rects) => {
                        drop(pass);
                        self.draw_blur_rects(
                            blur_rects,
                            self.cached_surface_texture.unwrap(),
                            self.cached_surface_texture_view.unwrap(),
                            globals,
                        );
                        pass = self.command_encoder.render(
                            "cached surface snapshot",
                            gpu::RenderTargetSet {
                                colors: &[gpu::RenderTarget {
                                    view: self.cached_surface_texture_view.unwrap(),
                                    init_op: gpu::InitOp::Load,
                                    finish_op: gpu::FinishOp::Store,
                                }],
                                depth_stencil: None,
                            },
                        );
                    }
                    PrimitiveBatch::Quads(quads) => {
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(quads, &self.gpu) };
                        let mut encoder = pass.with(&self.pipelines.quads);
                        encoder.bind(
                            0,
                            &ShaderQuadsData {
                                globals,
                                b_quads: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, quads.len() as u32);
                    }
                    PrimitiveBatch::Shadows(shadows) => {
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(shadows, &self.gpu) };
                        let mut encoder = pass.with(&self.pipelines.shadows);
                        encoder.bind(
                            0,
                            &ShaderShadowsData {
                                globals,
                                b_shadows: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, shadows.len() as u32);
                    }
                    PrimitiveBatch::Paths(paths) => {
                        let Some(first_path) = paths.first() else {
                            continue;
                        };
                        drop(pass);
                        self.draw_paths_to_intermediate(
                            paths,
                            self.surface_config.size.width as f32,
                            self.surface_config.size.height as f32,
                        );
                        pass = self.command_encoder.render(
                            "cached surface snapshot",
                            gpu::RenderTargetSet {
                                colors: &[gpu::RenderTarget {
                                    view: self.cached_surface_texture_view.unwrap(),
                                    init_op: gpu::InitOp::Load,
                                    finish_op: gpu::FinishOp::Store,
                                }],
                                depth_stencil: None,
                            },
                        );
                        let mut encoder = pass.with(&self.pipelines.paths);
                        let sprites = if paths.last().unwrap().order == first_path.order {
                            paths
                                .iter()
                                .map(|path| PathSprite {
                                    bounds: path.clipped_bounds(),
                                })
                                .collect()
                        } else {
                            let mut bounds = first_path.clipped_bounds();
                            for path in paths.iter().skip(1) {
                                bounds = bounds.union(&path.clipped_bounds());
                            }
                            vec![PathSprite { bounds }]
                        };
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(&sprites, &self.gpu) };
                        encoder.bind(
                            0,
                            &ShaderPathsData {
                                globals,
                                t_sprite: self.path_intermediate_texture_view.unwrap(),
                                s_sprite: self.atlas_sampler,
                                b_path_sprites: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, sprites.len() as u32);
                    }
                    PrimitiveBatch::Underlines(underlines) => {
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(underlines, &self.gpu) };
                        let mut encoder = pass.with(&self.pipelines.underlines);
                        encoder.bind(
                            0,
                            &ShaderUnderlinesData {
                                globals,
                                b_underlines: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, underlines.len() as u32);
                    }
                    PrimitiveBatch::MonochromeSprites {
                        texture_id,
                        sprites,
                    } => {
                        let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
                            log::warn!(
                                "skipping cached monochrome sprites with a stale Blade atlas texture"
                            );
                            continue;
                        };
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(sprites, &self.gpu) };
                        let mut encoder = pass.with(&self.pipelines.mono_sprites);
                        encoder.bind(
                            0,
                            &ShaderMonoSpritesData {
                                globals,
                                gamma_ratios: self.rendering_parameters.gamma_ratios,
                                grayscale_enhanced_contrast: self
                                    .rendering_parameters
                                    .grayscale_enhanced_contrast,
                                t_sprite: tex_info.raw_view,
                                s_sprite: self.atlas_sampler,
                                b_mono_sprites: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, sprites.len() as u32);
                    }
                    PrimitiveBatch::PolychromeSprites {
                        texture_id,
                        sprites,
                    } => {
                        let Some(tex_info) = self.atlas.get_texture_info(texture_id) else {
                            log::warn!(
                                "skipping cached polychrome sprites with a stale Blade atlas texture"
                            );
                            continue;
                        };
                        let instance_buf =
                            unsafe { self.instance_belt.alloc_typed(sprites, &self.gpu) };
                        let mut encoder = pass.with(&self.pipelines.poly_sprites);
                        encoder.bind(
                            0,
                            &ShaderPolySpritesData {
                                globals,
                                t_sprite: tex_info.raw_view,
                                s_sprite: self.atlas_sampler,
                                b_poly_sprites: instance_buf,
                            },
                        );
                        encoder.draw(0, 4, 0, sprites.len() as u32);
                    }
                    PrimitiveBatch::Surfaces(surfaces) => {
                        for surface in surfaces {
                            #[cfg(feature = "custom-shaders")]
                            if let crate::PaintSurfaceSource::RenderTarget { target, .. } =
                                &surface.source
                            {
                                let viewport = crate::size(
                                    DevicePixels(globals.viewport_size[0] as i32),
                                    DevicePixels(globals.viewport_size[1] as i32),
                                );
                                if let Err(error) = self.custom.draw(
                                    surface,
                                    target,
                                    viewport,
                                    self.surface_info.format,
                                    &mut pass,
                                ) {
                                    log::error!("custom target display failed: {error}");
                                }
                                continue;
                            }

                            #[cfg(not(target_os = "macos"))]
                            let _ = surface;
                            #[cfg(target_os = "macos")]
                            {
                                let crate::PaintSurfaceSource::CoreVideo(image_buffer) =
                                    &surface.source
                                else {
                                    continue;
                                };
                                let (t_y, t_cb_cr) = {
                                    if image_buffer.get_pixel_format()
                                        != core_video::pixel_buffer::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
                                    {
                                        log::warn!("skipping Blade surface with unsupported pixel format");
                                        continue;
                                    }

                                    let Ok(y_texture) =
                                        self.core_video_texture_cache.create_texture_from_image(
                                            &image_buffer,
                                            None,
                                            metal::MTLPixelFormat::R8Unorm,
                                            image_buffer.get_width_of_plane(0),
                                            image_buffer.get_height_of_plane(0),
                                            0,
                                        )
                                    else {
                                        log::warn!("failed to create Blade Y-plane Metal texture");
                                        continue;
                                    };
                                    let Ok(cb_cr_texture) =
                                        self.core_video_texture_cache.create_texture_from_image(
                                            &image_buffer,
                                            None,
                                            metal::MTLPixelFormat::RG8Unorm,
                                            image_buffer.get_width_of_plane(1),
                                            image_buffer.get_height_of_plane(1),
                                            1,
                                        )
                                    else {
                                        log::warn!(
                                            "failed to create Blade chroma-plane Metal texture"
                                        );
                                        continue;
                                    };
                                    let Some(y_texture_ref) = y_texture.as_texture_ref() else {
                                        log::warn!(
                                            "CoreVideo Y-plane texture has no Metal texture"
                                        );
                                        continue;
                                    };
                                    let Some(cb_cr_texture_ref) = cb_cr_texture.as_texture_ref()
                                    else {
                                        log::warn!(
                                            "CoreVideo chroma-plane texture has no Metal texture"
                                        );
                                        continue;
                                    };
                                    let Some(y_texture) = retain_core_video_texture(y_texture_ref)
                                    else {
                                        log::warn!("failed to retain Blade Y-plane Metal texture");
                                        continue;
                                    };
                                    let Some(cb_cr_texture) =
                                        retain_core_video_texture(cb_cr_texture_ref)
                                    else {
                                        log::warn!(
                                            "failed to retain Blade chroma-plane Metal texture"
                                        );
                                        continue;
                                    };

                                    (
                                        gpu::TextureView::from_metal_texture(
                                            &y_texture,
                                            gpu::TexelAspects::COLOR,
                                        ),
                                        gpu::TextureView::from_metal_texture(
                                            &cb_cr_texture,
                                            gpu::TexelAspects::COLOR,
                                        ),
                                    )
                                };

                                let mut _encoder = pass.with(&self.pipelines.surfaces);
                                _encoder.bind(
                                    0,
                                    &ShaderSurfacesData {
                                        globals,
                                        surface_locals: SurfaceParams {
                                            bounds: surface.bounds.into(),
                                            content_mask: surface.content_mask.bounds.into(),
                                        },
                                        t_y,
                                        t_cb_cr,
                                        s_surface: self.atlas_sampler,
                                    },
                                );

                                _encoder.draw(0, 4, 0, 1);
                            }
                        }
                    }
                }
            }
            drop(pass);

            let Some(texture_info) = self.atlas.get_texture_info(snapshot.target.texture_id) else {
                log::warn!("skipping cached-surface copy to a stale Blade atlas texture");
                continue;
            };
            let mut transfers = self.command_encoder.transfer("cached surface blit");
            transfers.copy_texture_to_texture(
                gpu::TexturePiece {
                    texture: self.cached_surface_texture.unwrap(),
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        snapshot.source_bounds.origin.x.0 as u32,
                        snapshot.source_bounds.origin.y.0 as u32,
                        0,
                    ],
                },
                gpu::TexturePiece {
                    texture: texture_info.raw_texture,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        snapshot.target.bounds.origin.x.0 as u32,
                        snapshot.target.bounds.origin.y.0 as u32,
                        0,
                    ],
                },
                gpu::Extent {
                    width: snapshot.source_bounds.size.width.0 as u32,
                    height: snapshot.source_bounds.size.height.0 as u32,
                    depth: 1,
                },
            );
        }
    }

    fn draw_blur_rects(
        &mut self,
        blur_rects: &[BlurRect],
        target_texture: gpu::Texture,
        target_view: gpu::TextureView,
        globals: GlobalParams,
    ) {
        if blur_rects.is_empty() {
            return;
        }

        let viewport_size = Size {
            width: DevicePixels(self.surface_config.size.width as i32),
            height: DevicePixels(self.surface_config.size.height as i32),
        };
        if !blur_rects
            .iter()
            .any(|blur| !blur.capture_bounds(viewport_size).is_empty())
        {
            return;
        }
        self.ensure_blur_intermediates();
        self.command_encoder
            .init_texture(self.blur_source_texture.unwrap());
        self.command_encoder
            .init_texture(self.blur_horizontal_texture.unwrap());

        for blur_rect in blur_rects {
            let capture_bounds = blur_rect.capture_bounds(viewport_size);
            if capture_bounds.is_empty() {
                continue;
            }

            let horizontal_pass = BlurPass::horizontal(blur_rect, capture_bounds);
            let composite_pass = BlurPass::composite(blur_rect, capture_bounds);

            let mut transfers = self.command_encoder.transfer("blur copy");
            transfers.copy_texture_to_texture(
                gpu::TexturePiece {
                    texture: target_texture,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        capture_bounds.origin.x.0 as u32,
                        capture_bounds.origin.y.0 as u32,
                        0,
                    ],
                },
                gpu::TexturePiece {
                    texture: self.blur_source_texture.unwrap(),
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        capture_bounds.origin.x.0 as u32,
                        capture_bounds.origin.y.0 as u32,
                        0,
                    ],
                },
                gpu::Extent {
                    width: capture_bounds.size.width.0 as u32,
                    height: capture_bounds.size.height.0 as u32,
                    depth: 1,
                },
            );
            drop(transfers);

            let mut pass = self.command_encoder.render(
                "blur horizontal",
                gpu::RenderTargetSet {
                    colors: &[gpu::RenderTarget {
                        view: self.blur_horizontal_texture_view.unwrap(),
                        init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
                        finish_op: gpu::FinishOp::Store,
                    }],
                    depth_stencil: None,
                },
            );
            let instance_buf = unsafe {
                self.instance_belt
                    .alloc_typed(&[horizontal_pass], &self.gpu)
            };
            {
                let mut encoder = pass.with(&self.pipelines.blur_horizontal);
                encoder.bind(
                    0,
                    &ShaderBlurData {
                        globals,
                        t_sprite: self.blur_source_texture_view.unwrap(),
                        s_sprite: self.atlas_sampler,
                        b_blurs: instance_buf,
                    },
                );
                encoder.draw(0, 4, 0, 1);
            }
            drop(pass);

            let mut pass = self.command_encoder.render(
                "blur composite",
                gpu::RenderTargetSet {
                    colors: &[gpu::RenderTarget {
                        view: target_view,
                        init_op: gpu::InitOp::Load,
                        finish_op: gpu::FinishOp::Store,
                    }],
                    depth_stencil: None,
                },
            );
            let instance_buf =
                unsafe { self.instance_belt.alloc_typed(&[composite_pass], &self.gpu) };
            {
                let mut encoder = pass.with(&self.pipelines.blur_composite);
                encoder.bind(
                    0,
                    &ShaderBlurData {
                        globals,
                        t_sprite: self.blur_horizontal_texture_view.unwrap(),
                        s_sprite: self.atlas_sampler,
                        b_blurs: instance_buf,
                    },
                );
                encoder.draw(0, 4, 0, 1);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn retain_core_video_texture(
    texture: &metal::TextureRef,
) -> Option<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>>> {
    let texture = foreign_types::ForeignTypeRef::as_ptr(texture)
        as *mut objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>;
    // SAFETY: `texture` came from `CVMetalTextureGetTexture` and remains valid
    // for this call because the owning CVMetalTexture wrapper is still live.
    // `retain` creates the independent ownership required by Blade's view.
    unsafe { objc2::rc::Retained::retain(texture) }
}

fn create_path_intermediate_texture(
    gpu: &gpu::Context,
    format: gpu::TextureFormat,
    width: u32,
    height: u32,
) -> (gpu::Texture, gpu::TextureView) {
    let texture = gpu.create_texture(gpu::TextureDesc {
        name: "path intermediate",
        format,
        size: gpu::Extent {
            width,
            height,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count: 1,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE | gpu::TextureUsage::TARGET,
        external: None,
    });
    let texture_view = gpu.create_texture_view(
        texture,
        gpu::TextureViewDesc {
            name: "path intermediate view",
            format,
            dimension: gpu::ViewDimension::D2,
            subresources: &Default::default(),
        },
    );
    (texture, texture_view)
}

fn create_msaa_texture_if_needed(
    gpu: &gpu::Context,
    format: gpu::TextureFormat,
    width: u32,
    height: u32,
    sample_count: u32,
) -> Option<(gpu::Texture, gpu::TextureView)> {
    if sample_count <= 1 {
        return None;
    }
    let texture_msaa = gpu.create_texture(gpu::TextureDesc {
        name: "path intermediate msaa",
        format,
        size: gpu::Extent {
            width,
            height,
            depth: 1,
        },
        array_layer_count: 1,
        mip_level_count: 1,
        sample_count,
        dimension: gpu::TextureDimension::D2,
        usage: gpu::TextureUsage::TARGET,
        external: None,
    });
    let texture_view_msaa = gpu.create_texture_view(
        texture_msaa,
        gpu::TextureViewDesc {
            name: "path intermediate msaa view",
            format,
            dimension: gpu::ViewDimension::D2,
            subresources: &Default::default(),
        },
    );

    Some((texture_msaa, texture_view_msaa))
}

/// A set of parameters that can be set using a corresponding environment variable.
struct RenderingParameters {
    // Env var: KAEL_PATH_SAMPLE_COUNT
    // workaround for known amdgpu/radv path rendering issue
    path_sample_count: u32,

    // Env var: KAEL_FONTS_GAMMA
    // Allowed range [1.0, 2.2], other values are clipped
    // Default: 1.8
    gamma_ratios: [f32; 4],
    // Env var: KAEL_FONTS_GRAYSCALE_ENHANCED_CONTRAST
    // Allowed range: [0.0, ..), other values are clipped
    // Default: 1.0
    grayscale_enhanced_contrast: f32,
}

impl RenderingParameters {
    fn from_env(context: &BladeContext) -> Self {
        use std::env;

        let path_sample_count = env::var("KAEL_PATH_SAMPLE_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .or_else(|| {
                [4, 2, 1]
                    .into_iter()
                    .find(|&n| (context.gpu.capabilities().sample_count_mask & n) != 0)
            })
            .unwrap_or(1);
        let gamma = env::var("KAEL_FONTS_GAMMA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.8_f32)
            .clamp(1.0, 2.2);
        let gamma_ratios = Self::get_gamma_ratios(gamma);
        let grayscale_enhanced_contrast = env::var("KAEL_FONTS_GRAYSCALE_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0_f32)
            .max(0.0);

        Self {
            path_sample_count,
            gamma_ratios,
            grayscale_enhanced_contrast,
        }
    }

    // Gamma ratios for brightening/darkening edges for better contrast
    // https://github.com/microsoft/terminal/blob/1283c0f5b99a2961673249fa77c6b986efb5086c/src/renderer/atlas/dwrite.cpp#L50
    fn get_gamma_ratios(gamma: f32) -> [f32; 4] {
        const GAMMA_INCORRECT_TARGET_RATIOS: [[f32; 4]; 13] = [
            [0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0, 0.0000 / 4.0], // gamma = 1.0
            [0.0166 / 4.0, -0.0807 / 4.0, 0.2227 / 4.0, -0.0751 / 4.0], // gamma = 1.1
            [0.0350 / 4.0, -0.1760 / 4.0, 0.4325 / 4.0, -0.1370 / 4.0], // gamma = 1.2
            [0.0543 / 4.0, -0.2821 / 4.0, 0.6302 / 4.0, -0.1876 / 4.0], // gamma = 1.3
            [0.0739 / 4.0, -0.3963 / 4.0, 0.8167 / 4.0, -0.2287 / 4.0], // gamma = 1.4
            [0.0933 / 4.0, -0.5161 / 4.0, 0.9926 / 4.0, -0.2616 / 4.0], // gamma = 1.5
            [0.1121 / 4.0, -0.6395 / 4.0, 1.1588 / 4.0, -0.2877 / 4.0], // gamma = 1.6
            [0.1300 / 4.0, -0.7649 / 4.0, 1.3159 / 4.0, -0.3080 / 4.0], // gamma = 1.7
            [0.1469 / 4.0, -0.8911 / 4.0, 1.4644 / 4.0, -0.3234 / 4.0], // gamma = 1.8
            [0.1627 / 4.0, -1.0170 / 4.0, 1.6051 / 4.0, -0.3347 / 4.0], // gamma = 1.9
            [0.1773 / 4.0, -1.1420 / 4.0, 1.7385 / 4.0, -0.3426 / 4.0], // gamma = 2.0
            [0.1908 / 4.0, -1.2652 / 4.0, 1.8650 / 4.0, -0.3476 / 4.0], // gamma = 2.1
            [0.2031 / 4.0, -1.3864 / 4.0, 1.9851 / 4.0, -0.3501 / 4.0], // gamma = 2.2
        ];

        const NORM13: f32 = ((0x10000 as f64) / (255.0 * 255.0) * 4.0) as f32;
        const NORM24: f32 = ((0x100 as f64) / (255.0) * 4.0) as f32;

        let index = ((gamma * 10.0).round() as usize).clamp(10, 22) - 10;
        let ratios = GAMMA_INCORRECT_TARGET_RATIOS[index];

        [
            ratios[0] * NORM13,
            ratios[1] * NORM24,
            ratios[2] * NORM13,
            ratios[3] * NORM24,
        ]
    }
}
