use super::metal_atlas::MetalAtlas;
#[cfg(feature = "custom-shaders")]
mod custom_shaders;
#[cfg(all(test, feature = "custom-shaders"))]
mod graph_tests;
use crate::{
    AtlasTextureId, Background, BlurRect, Bounds, ContentMask, Corners, DevicePixels, Hsla,
    MonochromeSprite, PaintSurface, Path, Point, PolychromeSprite, PrimitiveBatch, Quad,
    ScaledPixels, Scene, Shadow, Size, Surface, Underline, point, size,
};
use anyhow::Result;
use block::ConcreteBlock;
use objc2_foundation::NSSize;

use crate::frame_timing::collector::GpuFrameTimingCollector;
use core_foundation::base::TCFType;
use core_video::{
    metal_texture::CVMetalTextureGetTexture, metal_texture_cache::CVMetalTextureCache,
    pixel_buffer::kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{CAMetalLayer, CommandQueue, MTLPixelFormat, MTLResourceOptions, NSRange};
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::runtime::AnyObject;
use parking_lot::Mutex;

fn current_host_time() -> f64 {
    // Keep host-only FFI inside this function: cbindgen also parses this file to
    // generate the Metal shader header, whose language does not support double.
    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {
        fn CACurrentMediaTime() -> f64;
    }
    unsafe { CACurrentMediaTime() }
}

#[repr(transparent)]
struct CGColorSpacePtr(*mut c_void);

unsafe impl Encode for CGColorSpacePtr {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("CGColorSpace", &[]));
}

use std::{
    cell::Cell,
    ffi::c_void,
    mem, ptr,
    sync::Arc,
    time::{Duration, Instant},
};

// Exported to metal
pub(crate) type PointF = crate::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
const RENDER_TARGET_PIXEL_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm;

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

#[allow(dead_code)]
pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: crate::Size<f32>,
    _transparent: bool,
) -> Renderer {
    MetalRenderer::new(context)
}

pub unsafe fn try_new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: crate::Size<f32>,
    _transparent: bool,
) -> Result<Renderer> {
    MetalRenderer::try_new(context)
}

pub(crate) struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: 2 * 1024 * 1024,
            buffers: Vec::new(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RendererCounters {
    resize_events: u64,
    capacity_growths: u64,
    path_texture_allocations: u64,
    cached_surface_texture_allocations: u64,
    blur_texture_allocations: u64,
    draw_calls: u64,
    next_drawable_failures: u64,
    next_drawable_wait_micros: u64,
    max_next_drawable_wait_micros: u64,
    instance_buffer_growths: u64,
    frames_requested: u64,
    frames_presented: u64,
    missed_display_intervals: u64,
    drawable_stall_count: u64,
    max_frame_interval_micros: u64,
    last_present_timestamp_micros: u64,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(&mut self, device: &metal::Device) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            device.new_buffer(
                self.buffer_size as u64,
                MTLResourceOptions::StorageModeManaged,
            )
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

pub(crate) struct MetalRenderer {
    device: metal::Device,
    layer: metal::MetalLayer,
    presents_with_transaction: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    quads_multiply_pipeline_state: metal::RenderPipelineState,
    quads_screen_pipeline_state: metal::RenderPipelineState,
    quads_blend_fetch_pipeline_state: Option<metal::RenderPipelineState>,
    quads_pipeline_state_rgba16f: metal::RenderPipelineState,
    shadows_pipeline_state_rgba16f: metal::RenderPipelineState,
    underlines_pipeline_state_rgba16f: metal::RenderPipelineState,
    blur_horizontal_pipeline_state: metal::RenderPipelineState,
    blur_composite_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    atlas_byte_budget: Option<u64>,
    core_video_texture_cache: core_video::metal_texture_cache::CVMetalTextureCache,
    drawable_size: Size<DevicePixels>,
    drawable_capacity: Size<DevicePixels>,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    cached_surface_texture: Option<metal::Texture>,
    blur_source_texture: Option<metal::Texture>,
    blur_horizontal_texture: Option<metal::Texture>,
    path_sample_count: u32,
    counters: RendererCounters,
    last_present_instant: Option<Instant>,
    gpu_frame_timings: Option<Arc<Mutex<GpuFrameTimingCollector>>>,
    #[cfg(feature = "custom-shaders")]
    custom: custom_shaders::MetalCustomRenderer,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    #[allow(dead_code)]
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        Self::try_new(instance_buffer_pool).expect("failed to initialize Metal renderer")
    }

    pub fn try_new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Result<Self> {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        let mut devices = metal::Device::all();
        devices.sort_by_key(|device| (device.is_removable(), device.is_low_power()));
        let device = devices
            .pop()
            .ok_or_else(|| anyhow::anyhow!("unable to access a compatible graphics device"))?;

        let layer = metal::MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        layer.set_opaque(false);
        layer.set_maximum_drawable_count(3);
        unsafe {
            let cg_color_space = core_graphics::color_space::CGColorSpace::create_with_name(
                core_graphics::color_space::kCGColorSpaceSRGB,
            )
            .ok_or_else(|| anyhow::anyhow!("failed to create sRGB color space"))?;
            // CALayer autoresizing mask bits: kCALayerWidthSizable=2, kCALayerHeightSizable=16
            const CA_AUTORESIZING_MASK: u32 = 2 | 16;
            let layer_obj = (&*layer as *const _) as *mut AnyObject;
            let cs_ptr = CGColorSpacePtr(cg_color_space.as_ptr() as *mut c_void);
            let _: () = msg_send![layer_obj, setColorspace: cs_ptr];
            // A saturated drawable pool must yield so asynchronous upload and
            // device-failure deadlines can be observed on subsequent frames.
            let _: () = msg_send![layer_obj, setAllowsNextDrawableTimeout: true];
            let _: () = msg_send![layer_obj, setNeedsDisplayOnBoundsChange: true];
            let _: () = msg_send![layer_obj, setAutoresizingMask: CA_AUTORESIZING_MASK];
        }
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .map_err(|error| anyhow::anyhow!("building Metal shader library: {error}"))?;
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .map_err(|error| anyhow::anyhow!("loading Metal shader library: {error}"))?;

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            MTLResourceOptions::StorageModeManaged,
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_premultiplied_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let quads_multiply_pipeline_state = build_quad_blend_pipeline_state(
            &device,
            &library,
            "quads_multiply",
            RENDER_TARGET_PIXEL_FORMAT,
            metal::MTLBlendFactor::DestinationColor,
            metal::MTLBlendFactor::Zero,
        );
        let quads_screen_pipeline_state = build_quad_blend_pipeline_state(
            &device,
            &library,
            "quads_screen",
            RENDER_TARGET_PIXEL_FORMAT,
            metal::MTLBlendFactor::One,
            metal::MTLBlendFactor::OneMinusSourceColor,
        );
        let quads_blend_fetch_pipeline_state =
            build_quad_blend_fetch_pipeline_state(&device, &library, RENDER_TARGET_PIXEL_FORMAT);
        let quads_pipeline_state_rgba16f = build_pipeline_state(
            &device,
            &library,
            "quads_rgba16f",
            "quad_vertex",
            "quad_fragment",
            metal::MTLPixelFormat::RGBA16Float,
        );
        let shadows_pipeline_state_rgba16f = build_pipeline_state(
            &device,
            &library,
            "shadows_rgba16f",
            "shadow_vertex",
            "shadow_fragment",
            metal::MTLPixelFormat::RGBA16Float,
        );
        let underlines_pipeline_state_rgba16f = build_pipeline_state(
            &device,
            &library,
            "underlines_rgba16f",
            "underline_vertex",
            "underline_fragment",
            metal::MTLPixelFormat::RGBA16Float,
        );
        let blur_horizontal_pipeline_state = build_premultiplied_pipeline_state(
            &device,
            &library,
            "blur_horizontal",
            "blur_vertex",
            "blur_horizontal_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let blur_composite_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "blur_composite",
            "blur_vertex",
            "blur_composite_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            RENDER_TARGET_PIXEL_FORMAT,
        );

        let command_queue = device.new_command_queue();
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone(), command_queue.clone()));
        let core_video_texture_cache = CVMetalTextureCache::new(None, device.clone(), None)
            .map_err(|status| {
                anyhow::anyhow!("creating CoreVideo Metal texture cache failed with {status}")
            })?;

        Ok(Self {
            device,
            layer,
            presents_with_transaction: false,
            command_queue,
            paths_rasterization_pipeline_state: paths_rasterization_pipeline_state?,
            path_sprites_pipeline_state: path_sprites_pipeline_state?,
            shadows_pipeline_state: shadows_pipeline_state?,
            quads_pipeline_state: quads_pipeline_state?,
            quads_multiply_pipeline_state: quads_multiply_pipeline_state?,
            quads_screen_pipeline_state: quads_screen_pipeline_state?,
            quads_blend_fetch_pipeline_state,
            quads_pipeline_state_rgba16f: quads_pipeline_state_rgba16f?,
            shadows_pipeline_state_rgba16f: shadows_pipeline_state_rgba16f?,
            underlines_pipeline_state_rgba16f: underlines_pipeline_state_rgba16f?,
            blur_horizontal_pipeline_state: blur_horizontal_pipeline_state?,
            blur_composite_pipeline_state: blur_composite_pipeline_state?,
            underlines_pipeline_state: underlines_pipeline_state?,
            monochrome_sprites_pipeline_state: monochrome_sprites_pipeline_state?,
            polychrome_sprites_pipeline_state: polychrome_sprites_pipeline_state?,
            surfaces_pipeline_state: surfaces_pipeline_state?,
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            atlas_byte_budget: None,
            core_video_texture_cache,
            drawable_size: size(DevicePixels(0), DevicePixels(0)),
            drawable_capacity: size(DevicePixels(0), DevicePixels(0)),
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            cached_surface_texture: None,
            blur_source_texture: None,
            blur_horizontal_texture: None,
            path_sample_count: PATH_SAMPLE_COUNT,
            counters: RendererCounters::default(),
            last_present_instant: None,
            gpu_frame_timings: None,
            #[cfg(feature = "custom-shaders")]
            custom: custom_shaders::MetalCustomRenderer::default(),
        })
    }

    pub fn layer(&self) -> &metal::MetalLayerRef {
        &self.layer
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer.as_ptr()
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    /// Set a soft byte budget for the glyph/sprite atlas. When set, the renderer evicts the
    /// least-recently-used atlas tiles down to this budget at the end of each presented frame
    /// (protecting tiles used within the swapchain's in-flight depth). `None` (the default)
    /// disables eviction, leaving atlas behavior unchanged.
    #[allow(dead_code)]
    pub fn set_atlas_byte_budget(&mut self, budget: Option<u64>) {
        self.atlas_byte_budget = budget;
        self.sprite_atlas.set_admission_limits(budget);
    }

    pub(crate) fn gpu_allocated_bytes(&self) -> u64 {
        self.device.current_allocated_size() as u64
    }

    pub(crate) fn set_gpu_frame_timing_enabled(&mut self, enabled: bool) -> bool {
        if enabled {
            self.gpu_frame_timings
                .get_or_insert_with(|| Arc::new(Mutex::new(GpuFrameTimingCollector::new())));
        } else {
            self.gpu_frame_timings = None;
        }
        true
    }

    pub(crate) fn take_gpu_frame_timings(&self) -> Vec<crate::GpuFrameTiming> {
        self.gpu_frame_timings
            .as_ref()
            .map(|collector| collector.lock().take())
            .unwrap_or_default()
    }

    /// The disabled path performs only an option check before the existing commit.
    /// Neither block retains this renderer, its window, or any drawable/buffer.
    fn commit_with_gpu_timing(
        &self,
        command_buffer: &metal::CommandBufferRef,
        drawable: Option<&metal::MetalDrawableRef>,
    ) {
        if let Some(collector) = self.gpu_frame_timings.as_ref() {
            let frame_id = collector.lock().begin(0.0, drawable.is_some());
            if let Some(frame_id) = frame_id {
                let completed_collector = Arc::downgrade(collector);
                let completed = ConcreteBlock::new(move |buffer: &metal::CommandBufferRef| {
                    if let Some(collector) = completed_collector.upgrade() {
                        // Metal publishes both timestamps only after completion.
                        let buffer = buffer.as_ptr().cast::<AnyObject>();
                        let (start, end): (f64, f64) = unsafe {
                            (
                                msg_send![buffer, GPUStartTime],
                                msg_send![buffer, GPUEndTime],
                            )
                        };
                        collector.lock().complete(frame_id, start, end);
                    }
                })
                .copy();
                command_buffer.add_completed_handler(&completed);
                if let Some(drawable) = drawable {
                    let presented_collector = Arc::downgrade(collector);
                    let presented = ConcreteBlock::new(move |drawable: &metal::DrawableRef| {
                        if let Some(collector) = presented_collector.upgrade() {
                            collector
                                .lock()
                                .presented(frame_id, drawable.presented_time());
                        }
                    })
                    .copy();
                    drawable.add_presented_handler(&presented);
                }
                collector.lock().submitted(frame_id, current_host_time());
            }
        }
        command_buffer.commit();
    }

    pub(crate) fn shed_memory(&mut self, level: crate::MemoryPressureLevel) {
        if level == crate::MemoryPressureLevel::Normal {
            return;
        }
        self.path_intermediate_texture = None;
        self.path_intermediate_msaa_texture = None;
        self.cached_surface_texture = None;
        self.blur_source_texture = None;
        self.blur_horizontal_texture = None;
        self.instance_buffer_pool.lock().buffers.clear();
        #[cfg(feature = "custom-shaders")]
        self.custom.shed();
        // Retained command buffers own the objects they sample. Tile regions
        // still in recent submitted frames remain protected from reuse.
        self.sprite_atlas
            .evict_to_budget_keeping(self.atlas_byte_budget.unwrap_or(0), 4);
    }

    #[cfg(feature = "custom-shaders")]
    pub(crate) fn create_gpu_buffer(
        &mut self,
        descriptor: crate::GpuBufferDescriptor,
    ) -> std::result::Result<crate::GpuBuffer, crate::RenderTargetError> {
        self.custom
            .create_buffer(&self.device, &self.command_queue, descriptor)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn validate_gpu_buffer(
        &self,
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
        self.custom
            .write_buffer(&self.device, &self.command_queue, buffer, offset, bytes)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn read_gpu_buffer(
        &mut self,
        buffer: &crate::GpuBuffer,
    ) -> std::result::Result<Vec<u8>, crate::RenderTargetError> {
        self.custom
            .read_buffer(&self.device, &self.command_queue, buffer)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn dispatch_compute(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom
            .dispatch(&self.device, &self.command_queue, shader, bindings, groups)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn write_render_target(
        &mut self,
        target: &crate::RenderTarget,
        pixels: &[u8],
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom
            .write_target(&self.device, &self.command_queue, target, pixels)
    }
    #[cfg(feature = "custom-shaders")]
    pub(crate) fn create_render_target(
        &mut self,
        descriptor: crate::RenderTargetDescriptor,
    ) -> std::result::Result<crate::RenderTarget, crate::RenderTargetError> {
        self.custom
            .create(&self.device, &self.command_queue, descriptor)
    }

    #[cfg(feature = "custom-shaders")]
    pub(crate) fn render_shader(
        &mut self,
        target: &crate::RenderTarget,
        shader: &crate::ShaderHandle,
        bindings: &crate::ShaderBindings,
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom
            .render(&self.device, &self.command_queue, target, shader, bindings)
    }

    #[cfg(feature = "custom-shaders")]
    pub(crate) fn read_render_target(
        &mut self,
        target: &crate::RenderTarget,
    ) -> std::result::Result<crate::RenderTargetReadback, crate::RenderTargetError> {
        self.custom.read(&self.device, &self.command_queue, target)
    }

    #[cfg(feature = "custom-shaders")]
    pub(crate) fn validate_render_target(
        &self,
        target: &crate::RenderTarget,
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.validate(target)
    }

    #[cfg(feature = "custom-shaders")]
    pub(crate) fn set_render_target_byte_budget(&mut self, bytes: u64) {
        self.custom.set_budget(bytes);
    }

    #[cfg(feature = "custom-shaders")]
    fn draw_render_target_surface(
        &mut self,
        surface: &PaintSurface,
        target: &crate::RenderTarget,
        viewport_size: Size<DevicePixels>,
        encoder: &metal::RenderCommandEncoderRef,
    ) -> std::result::Result<(), crate::RenderTargetError> {
        self.custom.draw(
            &self.device,
            surface,
            target,
            viewport_size,
            encoder,
            RENDER_TARGET_PIXEL_FORMAT,
        )
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        self.layer
            .set_presents_with_transaction(presents_with_transaction);
    }

    pub fn update_drawable_size(&mut self, new_size: Size<DevicePixels>) {
        let safe_size = size(
            DevicePixels(
                new_size
                    .width
                    .0
                    .clamp(0, crate::MAX_ATLAS_TEXTURE_DIMENSION),
            ),
            DevicePixels(
                new_size
                    .height
                    .0
                    .clamp(0, crate::MAX_ATLAS_TEXTURE_DIMENSION),
            ),
        );
        if safe_size != new_size {
            log::warn!(
                "clamping unsafe Metal drawable size {:?} to {:?}",
                new_size,
                safe_size
            );
        }
        let new_size = safe_size;
        if self.drawable_size == new_size {
            return;
        }

        self.counters.resize_events = self.counters.resize_events.saturating_add(1);
        self.drawable_size = new_size;
        let drawable_size = NSSize {
            width: new_size.width.0 as f64,
            height: new_size.height.0 as f64,
        };
        unsafe {
            let layer_obj = (self.layer() as *const _) as *mut AnyObject;
            let _: () = msg_send![layer_obj, setDrawableSize: drawable_size];
        }
        let device_pixels_size = Size {
            width: DevicePixels(drawable_size.width as i32),
            height: DevicePixels(drawable_size.height as i32),
        };

        if device_pixels_size.width.0 <= 0 || device_pixels_size.height.0 <= 0 {
            self.drawable_capacity = size(DevicePixels(0), DevicePixels(0));
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            self.cached_surface_texture = None;
            self.blur_source_texture = None;
            self.blur_horizontal_texture = None;
            return;
        }

        if self.drawable_capacity.width.0 >= device_pixels_size.width.0
            && self.drawable_capacity.height.0 >= device_pixels_size.height.0
        {
            return;
        }

        self.drawable_capacity = size(
            DevicePixels(
                self.drawable_capacity
                    .width
                    .0
                    .max(device_pixels_size.width.0),
            ),
            DevicePixels(
                self.drawable_capacity
                    .height
                    .0
                    .max(device_pixels_size.height.0),
            ),
        );
        self.counters.capacity_growths = self.counters.capacity_growths.saturating_add(1);
        // Scratch targets belong to the primitive paths that use them. Growing
        // a window must not allocate MSAA/path and subtree-cache textures for a
        // scene that only draws quads, text, or images.
        log::trace!(
            "metal renderer drawable capacity grew to {:?}; resize_events={} capacity_growths={} path_allocations={} cached_surface_allocations={} blur_allocations={}",
            self.drawable_capacity,
            self.counters.resize_events,
            self.counters.capacity_growths,
            self.counters.path_texture_allocations,
            self.counters.cached_surface_texture_allocations,
            self.counters.blur_texture_allocations,
        );
    }

    fn ensure_path_intermediate_textures(&mut self, size: Size<DevicePixels>) -> bool {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return false;
        }

        if texture_covers(self.path_intermediate_texture.as_ref(), size)
            && (!self.uses_msaa()
                || texture_covers(self.path_intermediate_msaa_texture.as_ref(), size))
        {
            return true;
        }

        let size = scratch_texture_capacity(self.path_intermediate_texture.as_ref(), size);

        self.counters.path_texture_allocations =
            self.counters.path_texture_allocations.saturating_add(1);

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            let mut msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
        true
    }

    fn ensure_cached_surface_texture(&mut self, size: Size<DevicePixels>) -> bool {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.cached_surface_texture = None;
            return false;
        }

        if texture_covers(self.cached_surface_texture.as_ref(), size) {
            return true;
        }

        let size = scratch_texture_capacity(self.cached_surface_texture.as_ref(), size);

        self.counters.cached_surface_texture_allocations = self
            .counters
            .cached_surface_texture_allocations
            .saturating_add(1);

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.cached_surface_texture = Some(self.device.new_texture(&texture_descriptor));
        true
    }

    fn ensure_blur_textures(&mut self, size: Size<DevicePixels>) -> bool {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.blur_source_texture = None;
            self.blur_horizontal_texture = None;
            return false;
        }

        if texture_covers(self.blur_source_texture.as_ref(), size)
            && texture_covers(self.blur_horizontal_texture.as_ref(), size)
        {
            return true;
        }

        self.counters.blur_texture_allocations =
            self.counters.blur_texture_allocations.saturating_add(1);

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.blur_source_texture = Some(self.device.new_texture(&texture_descriptor));
        self.blur_horizontal_texture = Some(self.device.new_texture(&texture_descriptor));
        true
    }

    fn uses_msaa(&self) -> bool {
        self.path_sample_count > 1
    }

    pub fn update_transparency(&self, _transparent: bool) {
        // todo(mac)?
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        self.counters.draw_calls = self.counters.draw_calls.saturating_add(1);
        self.counters.frames_requested = self.counters.frames_requested.saturating_add(1);
        let layer = self.layer.clone();
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable_started_at = Instant::now();
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            self.counters.next_drawable_failures =
                self.counters.next_drawable_failures.saturating_add(1);
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };
        let next_drawable_wait_micros = drawable_started_at.elapsed().as_micros() as u64;
        self.counters.next_drawable_wait_micros = self
            .counters
            .next_drawable_wait_micros
            .saturating_add(next_drawable_wait_micros);
        self.counters.max_next_drawable_wait_micros = self
            .counters
            .max_next_drawable_wait_micros
            .max(next_drawable_wait_micros);

        if let Err(error) = self.ensure_buffer_size(scene) {
            log::error!("scene exceeds safe Metal instance-buffer limits: {error:#}");
            return;
        }

        if let Err(error) = self.sprite_atlas.mark_scene_used(scene) {
            log::warn!("Metal scene rejected before submission: {error:#}");
            return;
        }

        if let Err(error) = self.sprite_atlas.flush_uploads() {
            log::error!("failed to flush Metal atlas uploads: {error:#}");
            return;
        }

        loop {
            let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);

            let command_buffer =
                self.draw_primitives(scene, &mut instance_buffer, drawable, viewport_size);

            match command_buffer {
                Ok(command_buffer) => {
                    let instance_buffer_pool = self.instance_buffer_pool.clone();
                    let instance_buffer = Cell::new(Some(instance_buffer));
                    let block = ConcreteBlock::new(move |_| {
                        if let Some(instance_buffer) = instance_buffer.take() {
                            instance_buffer_pool.lock().release(instance_buffer);
                        }
                    });
                    let block = block.copy();
                    command_buffer.add_completed_handler(&block);

                    if self.presents_with_transaction {
                        self.commit_with_gpu_timing(&command_buffer, Some(drawable));
                        command_buffer.wait_until_scheduled();
                        drawable.present();
                    } else {
                        command_buffer.present_drawable(drawable);
                        self.commit_with_gpu_timing(&command_buffer, Some(drawable));
                    }

                    self.counters.frames_presented =
                        self.counters.frames_presented.saturating_add(1);
                    let present_instant = Instant::now();
                    if let Some(last_present_instant) = self.last_present_instant {
                        let interval =
                            present_instant.saturating_duration_since(last_present_instant);
                        let interval_micros = interval.as_micros() as u64;
                        self.counters.max_frame_interval_micros =
                            self.counters.max_frame_interval_micros.max(interval_micros);
                        if interval > Duration::from_micros(16_667) {
                            self.counters.missed_display_intervals =
                                self.counters.missed_display_intervals.saturating_add(1);
                        }
                    }
                    self.last_present_instant = Some(present_instant);
                    self.counters.last_present_timestamp_micros = present_instant
                        .duration_since(drawable_started_at)
                        .as_micros()
                        as u64;

                    if next_drawable_wait_micros > 2_000 {
                        self.counters.drawable_stall_count =
                            self.counters.drawable_stall_count.saturating_add(1);
                    }

                    // End of frame: shed least-recently-used atlas tiles to the budget (if
                    // configured), protecting the frames still in flight, then advance the
                    // atlas clock so the next frame's glyphs are stamped fresh and protected.
                    if let Some(budget) = self.atlas_byte_budget {
                        const IN_FLIGHT_FRAMES: u64 = 4;
                        self.sprite_atlas
                            .evict_to_budget_keeping(budget, IN_FLIGHT_FRAMES);
                    }
                    self.sprite_atlas.advance_frame();

                    return;
                }
                Err(err) => {
                    log::error!(
                        "failed to render: {}. retrying with larger instance buffer size",
                        err
                    );
                    let mut instance_buffer_pool = self.instance_buffer_pool.lock();
                    let buffer_size = instance_buffer_pool.buffer_size;
                    if buffer_size >= 256 * 1024 * 1024 {
                        log::error!("instance buffer size grew too large: {}", buffer_size);
                        break;
                    }
                    let next_size = buffer_size
                        .checked_mul(2)
                        .unwrap_or(256 * 1024 * 1024)
                        .min(256 * 1024 * 1024);
                    instance_buffer_pool.reset(next_size);
                    log::info!(
                        "increased instance buffer size to {}",
                        instance_buffer_pool.buffer_size
                    );
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn debug_counters(&self) -> RendererCounters {
        self.counters
    }

    #[allow(dead_code)]
    pub fn reset_counters(&mut self) {
        let last_ts = self.counters.last_present_timestamp_micros;
        self.counters = RendererCounters::default();
        self.counters.last_present_timestamp_micros = last_ts;
    }

    pub(crate) fn render_scene_to_bytes(
        &mut self,
        scene: &Scene,
        viewport_size: Size<DevicePixels>,
    ) -> Result<OffscreenReadback> {
        let width = viewport_size.width.0.max(0) as u64;
        let height = viewport_size.height.0.max(0) as u64;
        if width == 0 || height == 0 {
            anyhow::bail!("offscreen render requires a non-zero viewport");
        }
        anyhow::ensure!(
            width <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64
                && height <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64,
            "offscreen viewport exceeds safe Metal texture dimensions"
        );
        // Reject oversized captures before allocating the target, scratch
        // textures, or instance buffers for a frame that cannot be returned.
        let (bytes_per_row, buffer_len, packed_len) = checked_readback_layout(width, height, 4)?;

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);
        let target_ref: &metal::TextureRef = &target;

        self.ensure_buffer_size(scene)?;
        self.sprite_atlas.mark_scene_used(scene)?;
        self.sprite_atlas.flush_uploads()?;
        let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);

        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1.0 } else { 0.0 };
        let mut instance_offset = 0;

        let command_encoder = new_texture_command_encoder(
            command_buffer,
            target_ref,
            viewport_size,
            metal::MTLLoadAction::Clear,
            alpha,
        );

        let scene_ok = self.draw_scene_with_encoder(
            scene,
            &mut instance_buffer,
            &mut instance_offset,
            viewport_size,
            command_buffer,
            target_ref,
            command_encoder,
            |command_buffer, load_action| {
                new_texture_command_encoder(
                    command_buffer,
                    target_ref,
                    viewport_size,
                    load_action,
                    alpha,
                )
            },
        );

        let snapshots_ok = scene_ok
            && self.draw_cached_surface_snapshots(
                scene,
                &mut instance_buffer,
                &mut instance_offset,
                viewport_size,
                command_buffer,
            );

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as u64,
        });

        let staging = self
            .device
            .new_buffer(buffer_len, MTLResourceOptions::StorageModeShared);

        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture_to_buffer(
            target_ref,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
            metal::MTLSize {
                width,
                height,
                depth: 1,
            },
            &staging,
            0,
            bytes_per_row,
            buffer_len,
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();

        self.commit_with_gpu_timing(command_buffer, None);
        command_buffer.wait_until_completed();

        self.instance_buffer_pool.lock().release(instance_buffer);

        if !snapshots_ok {
            anyhow::bail!("scene exceeded instance buffer capacity during offscreen render");
        }

        let row_bytes = usize::try_from(width * 4)?;
        let src_stride = bytes_per_row as usize;
        let mut bgra = vec![0u8; packed_len];
        unsafe {
            let contents = staging.contents() as *const u8;
            anyhow::ensure!(!contents.is_null(), "Metal readback buffer is not mapped");
            let src = std::slice::from_raw_parts(contents, buffer_len as usize);
            for y in 0..height as usize {
                let src_start = y * src_stride;
                let dst_start = y * row_bytes;
                bgra[dst_start..dst_start + row_bytes]
                    .copy_from_slice(&src[src_start..src_start + row_bytes]);
            }
        }

        Ok(OffscreenReadback {
            width: width as u32,
            height: height as u32,
            bgra,
        })
    }

    #[cfg(test)]
    fn encode_scene_into(
        &mut self,
        scene: &Scene,
        target_ref: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        load_action: metal::MTLLoadAction,
        scissor: Option<metal::MTLScissorRect>,
    ) -> Result<()> {
        self.ensure_buffer_size(scene)?;
        self.sprite_atlas.mark_scene_used(scene)?;
        self.sprite_atlas.flush_uploads()?;
        let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);

        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1.0 } else { 0.0 };
        let mut instance_offset = 0;

        let apply_scissor = |encoder: &metal::RenderCommandEncoderRef| {
            if let Some(rect) = scissor {
                encoder.set_scissor_rect(rect);
            }
        };

        let command_encoder = new_texture_command_encoder(
            command_buffer,
            target_ref,
            viewport_size,
            load_action,
            alpha,
        );
        apply_scissor(command_encoder);

        let scene_ok = self.draw_scene_with_encoder(
            scene,
            &mut instance_buffer,
            &mut instance_offset,
            viewport_size,
            command_buffer,
            target_ref,
            command_encoder,
            |command_buffer, load_action| {
                let encoder = new_texture_command_encoder(
                    command_buffer,
                    target_ref,
                    viewport_size,
                    load_action,
                    alpha,
                );
                apply_scissor(encoder);
                encoder
            },
        );

        let snapshots_ok = scene_ok
            && self.draw_cached_surface_snapshots(
                scene,
                &mut instance_buffer,
                &mut instance_offset,
                viewport_size,
                command_buffer,
            );

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as u64,
        });

        self.commit_with_gpu_timing(command_buffer, None);
        command_buffer.wait_until_completed();

        self.instance_buffer_pool.lock().release(instance_buffer);

        if !snapshots_ok {
            anyhow::bail!("scene exceeded instance buffer capacity during damage render");
        }
        Ok(())
    }

    /// Render `base` fully, then re-rasterize only the `damage` rectangle from `next` on
    /// top (the compositor's "load previous contents and repaint just the dirty region"
    /// path), and read back the composited result. This exercises the scissor + load
    /// mechanism the fine-grained dirty-region path relies on, so its output can be
    /// pixel-compared against a full render of `next`.
    #[cfg(test)]
    pub(crate) fn render_damage_to_bytes(
        &mut self,
        base: &Scene,
        next: &Scene,
        damage: Bounds<ScaledPixels>,
        viewport_size: Size<DevicePixels>,
    ) -> Result<OffscreenReadback> {
        let width = viewport_size.width.0.max(0) as u64;
        let height = viewport_size.height.0.max(0) as u64;
        if width == 0 || height == 0 {
            anyhow::bail!("offscreen render requires a non-zero viewport");
        }
        anyhow::ensure!(
            width <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64
                && height <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64,
            "offscreen viewport exceeds safe Metal texture dimensions"
        );
        let (bytes_per_row, buffer_len, packed_len) = checked_readback_layout(width, height, 4)?;

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);
        let target_ref: &metal::TextureRef = &target;

        let left = damage.origin.x.0.max(0.0).floor() as u64;
        let top = damage.origin.y.0.max(0.0).floor() as u64;
        let right = (damage.origin.x.0 + damage.size.width.0).max(0.0).ceil() as u64;
        let bottom = (damage.origin.y.0 + damage.size.height.0).max(0.0).ceil() as u64;
        let x = left.min(width);
        let y = top.min(height);
        let scissor = metal::MTLScissorRect {
            x,
            y,
            width: right.min(width).saturating_sub(x),
            height: bottom.min(height).saturating_sub(y),
        };

        self.encode_scene_into(
            base,
            target_ref,
            viewport_size,
            metal::MTLLoadAction::Clear,
            None,
        )?;
        self.encode_scene_into(
            next,
            target_ref,
            viewport_size,
            metal::MTLLoadAction::Load,
            Some(scissor),
        )?;

        let staging = self
            .device
            .new_buffer(buffer_len, MTLResourceOptions::StorageModeShared);

        let command_buffer = self.command_queue.new_command_buffer();
        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture_to_buffer(
            target_ref,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
            metal::MTLSize {
                width,
                height,
                depth: 1,
            },
            &staging,
            0,
            bytes_per_row,
            buffer_len,
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        let row_bytes = usize::try_from(width * 4)?;
        let src_stride = bytes_per_row as usize;
        let mut bgra = vec![0u8; packed_len];
        unsafe {
            let contents = staging.contents() as *const u8;
            anyhow::ensure!(!contents.is_null(), "Metal readback buffer is not mapped");
            let src = std::slice::from_raw_parts(contents, buffer_len as usize);
            for y in 0..height as usize {
                let src_start = y * src_stride;
                let dst_start = y * row_bytes;
                bgra[dst_start..dst_start + row_bytes]
                    .copy_from_slice(&src[src_start..src_start + row_bytes]);
            }
        }

        Ok(OffscreenReadback {
            width: width as u32,
            height: height as u32,
            bgra,
        })
    }

    fn encode_instanced<T>(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        pipeline: &metal::RenderPipelineStateRef,
        instances: &[T],
        viewport_size: Size<DevicePixels>,
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
    ) -> bool {
        if instances.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }
        let bytes_len = mem::size_of_val(instances);
        let Some(next_offset) = (*instance_offset).checked_add(bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        encoder.set_render_pipeline_state(pipeline);
        encoder.set_vertex_buffer(0, Some(&self.unit_vertices), 0);
        encoder.set_vertex_buffer(
            1,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        encoder.set_fragment_buffer(
            1,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        encoder.set_vertex_bytes(
            2,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        unsafe {
            let dst = (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset);
            ptr::copy_nonoverlapping(instances.as_ptr() as *const u8, dst, bytes_len);
        }
        encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            instances.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    pub(crate) fn render_scene_to_f16(
        &mut self,
        scene: &Scene,
        viewport_size: Size<DevicePixels>,
    ) -> Result<OffscreenReadbackF16> {
        let width = viewport_size.width.0.max(0) as u64;
        let height = viewport_size.height.0.max(0) as u64;
        if width == 0 || height == 0 {
            anyhow::bail!("offscreen render requires a non-zero viewport");
        }
        anyhow::ensure!(
            width <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64
                && height <= crate::MAX_ATLAS_TEXTURE_DIMENSION as u64,
            "offscreen viewport exceeds safe Metal texture dimensions"
        );
        let (bytes_per_row, buffer_len, _) = checked_readback_layout(width, height, 8)?;
        let decoded_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|values| usize::try_from(values).ok())
            .filter(|values| {
                values
                    .checked_mul(mem::size_of::<f32>())
                    .is_some_and(|bytes| bytes <= MAX_METAL_READBACK_BYTES)
            })
            .ok_or_else(|| anyhow::anyhow!("decoded Metal readback exceeds its memory budget"))?;

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(metal::MTLPixelFormat::RGBA16Float);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);

        self.ensure_buffer_size(scene)?;
        self.sprite_atlas.mark_scene_used(scene)?;
        self.sprite_atlas.flush_uploads()?;
        let mut instance_buffer = self.instance_buffer_pool.lock().acquire(&self.device);
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1.0 } else { 0.0 };

        let command_encoder = new_texture_command_encoder(
            command_buffer,
            &target,
            viewport_size,
            metal::MTLLoadAction::Clear,
            alpha,
        );

        let mut instance_offset = 0usize;
        let mut error: Option<&'static str> = None;
        for batch in scene.batches() {
            let ok = match batch {
                PrimitiveBatch::Quads(quads) => self.encode_instanced(
                    command_encoder,
                    &self.quads_pipeline_state_rgba16f,
                    quads,
                    viewport_size,
                    &mut instance_buffer,
                    &mut instance_offset,
                ),
                PrimitiveBatch::Shadows(shadows) => self.encode_instanced(
                    command_encoder,
                    &self.shadows_pipeline_state_rgba16f,
                    shadows,
                    viewport_size,
                    &mut instance_buffer,
                    &mut instance_offset,
                ),
                PrimitiveBatch::Underlines(underlines) => self.encode_instanced(
                    command_encoder,
                    &self.underlines_pipeline_state_rgba16f,
                    underlines,
                    viewport_size,
                    &mut instance_buffer,
                    &mut instance_offset,
                ),
                #[cfg(feature = "custom-shaders")]
                PrimitiveBatch::Surfaces(surfaces) => {
                    let mut rendered = true;
                    for surface in surfaces {
                        let crate::PaintSurfaceSource::RenderTarget { target, .. } =
                            &surface.source
                        else {
                            error =
                                Some("external video is not supported in the RGBA16F render path");
                            rendered = false;
                            break;
                        };
                        if self
                            .custom
                            .draw(
                                &self.device,
                                surface,
                                target,
                                viewport_size,
                                command_encoder,
                                MTLPixelFormat::RGBA16Float,
                            )
                            .is_err()
                        {
                            error =
                                Some("custom GPU target display failed in the RGBA16F render path");
                            rendered = false;
                            break;
                        }
                    }
                    rendered
                }
                _ => {
                    error = Some("primitive type not yet supported in the RGBA16F render path");
                    break;
                }
            };
            if !ok {
                error = Some("instance buffer capacity exceeded during RGBA16F render");
                break;
            }
        }
        command_encoder.end_encoding();

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as u64,
        });

        let staging = self
            .device
            .new_buffer(buffer_len, MTLResourceOptions::StorageModeShared);

        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture_to_buffer(
            &target,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
            metal::MTLSize {
                width,
                height,
                depth: 1,
            },
            &staging,
            0,
            bytes_per_row,
            buffer_len,
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();
        self.commit_with_gpu_timing(command_buffer, None);
        command_buffer.wait_until_completed();
        self.instance_buffer_pool.lock().release(instance_buffer);

        if let Some(message) = error {
            anyhow::bail!("{message}");
        }

        let row_stride = bytes_per_row as usize;
        let mut rgba = vec![0.0f32; decoded_len];
        unsafe {
            let contents = staging.contents() as *const u8;
            anyhow::ensure!(!contents.is_null(), "Metal readback buffer is not mapped");
            let src = std::slice::from_raw_parts(contents, buffer_len as usize);
            for y in 0..height as usize {
                let row = y * row_stride;
                for x in 0..width as usize {
                    for channel in 0..4 {
                        let byte_index = row + (x * 8) + (channel * 2);
                        let bits = u16::from_le_bytes([src[byte_index], src[byte_index + 1]]);
                        rgba[(y * width as usize + x) * 4 + channel] = f16_to_f32(bits);
                    }
                }
            }
        }

        Ok(OffscreenReadbackF16 {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }

    pub(crate) fn run_compute_kernel(
        &self,
        source: &str,
        entry: &str,
        data: &mut [f32],
    ) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        anyhow::ensure!(
            source.len() <= 4 * 1024 * 1024,
            "Metal compute source exceeds 4 MiB"
        );
        anyhow::ensure!(
            !entry.is_empty() && entry.len() <= 256,
            "Metal compute entry name is invalid"
        );
        anyhow::ensure!(
            mem::size_of_val(data) <= MAX_METAL_READBACK_BYTES,
            "Metal compute buffer exceeds its memory budget"
        );

        let library = self
            .device
            .new_library_with_source(source, &metal::CompileOptions::new())
            .map_err(|err| anyhow::anyhow!("failed to compile compute kernel: {err}"))?;
        let function = library
            .get_function(entry, None)
            .map_err(|err| anyhow::anyhow!("compute entry '{entry}' not found: {err}"))?;
        let pipeline = self
            .device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|err| anyhow::anyhow!("failed to create compute pipeline: {err}"))?;

        let byte_len = mem::size_of_val(data) as u64;
        let buffer = self.device.new_buffer_with_data(
            data.as_ptr() as *const c_void,
            byte_len,
            MTLResourceOptions::StorageModeShared,
        );

        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&pipeline);
        encoder.set_buffer(0, Some(&buffer), 0);

        let count = data.len() as u64;
        let threads_per_group = pipeline
            .max_total_threads_per_threadgroup()
            .min(count)
            .max(1);
        encoder.dispatch_threads(
            metal::MTLSize {
                width: count,
                height: 1,
                depth: 1,
            },
            metal::MTLSize {
                width: threads_per_group,
                height: 1,
                depth: 1,
            },
        );
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        unsafe {
            let contents = buffer.contents() as *const f32;
            anyhow::ensure!(!contents.is_null(), "Metal compute buffer is not mapped");
            let slice = std::slice::from_raw_parts(contents, data.len());
            data.copy_from_slice(slice);
        }
        Ok(())
    }

    /// Apply a per-pixel coverage `mask` to a tightly-packed BGRA8 `pixels` buffer on the
    /// GPU, scaling each pixel's alpha by its mask value. This is the GPU equivalent of the
    /// CPU `apply_clip_mask_bgra` reference — the path a polygon clip uses to offload the
    /// mask multiply from the CPU.
    #[cfg(test)]
    pub(crate) fn apply_clip_mask(&self, pixels: &mut [u8], mask: &[f32]) -> Result<()> {
        let count = (pixels.len() / 4).min(mask.len());
        if count == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            pixels.len() <= MAX_METAL_READBACK_BYTES
                && mem::size_of_val(mask) <= MAX_METAL_READBACK_BYTES,
            "Metal clip-mask buffers exceed their memory budget"
        );
        anyhow::ensure!(count <= u32::MAX as usize, "Metal clip-mask is too large");

        const KERNEL: &str = concat!(
            "#include <metal_stdlib>\n",
            "using namespace metal;\n",
            "kernel void apply_clip_mask(device uchar4* pixels [[buffer(0)]],\n",
            "                            device const float* mask [[buffer(1)]],\n",
            "                            constant uint& count [[buffer(2)]],\n",
            "                            uint id [[thread_position_in_grid]]) {\n",
            "    if (id >= count) { return; }\n",
            "    uchar4 p = pixels[id];\n",
            "    float coverage = clamp(mask[id], 0.0, 1.0);\n",
            "    p.w = uchar(round(float(p.w) * coverage));\n",
            "    pixels[id] = p;\n",
            "}\n",
        );

        let library = self
            .device
            .new_library_with_source(KERNEL, &metal::CompileOptions::new())
            .map_err(|err| anyhow::anyhow!("failed to compile clip-mask kernel: {err}"))?;
        let function = library
            .get_function("apply_clip_mask", None)
            .map_err(|err| anyhow::anyhow!("clip-mask entry not found: {err}"))?;
        let pipeline = self
            .device
            .new_compute_pipeline_state_with_function(&function)
            .map_err(|err| anyhow::anyhow!("failed to create clip-mask pipeline: {err}"))?;

        let pixel_bytes = (count * 4) as u64;
        let pixel_buffer = self.device.new_buffer_with_data(
            pixels.as_ptr() as *const c_void,
            pixel_bytes,
            MTLResourceOptions::StorageModeShared,
        );
        let mask_buffer = self.device.new_buffer_with_data(
            mask.as_ptr() as *const c_void,
            (count * mem::size_of::<f32>()) as u64,
            MTLResourceOptions::StorageModeShared,
        );
        let count_u32 = count as u32;
        let count_buffer = self.device.new_buffer_with_data(
            &count_u32 as *const u32 as *const c_void,
            mem::size_of::<u32>() as u64,
            MTLResourceOptions::StorageModeShared,
        );

        let command_buffer = self.command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&pipeline);
        encoder.set_buffer(0, Some(&pixel_buffer), 0);
        encoder.set_buffer(1, Some(&mask_buffer), 0);
        encoder.set_buffer(2, Some(&count_buffer), 0);

        let threads = count as u64;
        let threads_per_group = pipeline
            .max_total_threads_per_threadgroup()
            .min(threads)
            .max(1);
        encoder.dispatch_threads(
            metal::MTLSize {
                width: threads,
                height: 1,
                depth: 1,
            },
            metal::MTLSize {
                width: threads_per_group,
                height: 1,
                depth: 1,
            },
        );
        encoder.end_encoding();
        command_buffer.commit();
        command_buffer.wait_until_completed();

        unsafe {
            let contents = pixel_buffer.contents() as *const u8;
            anyhow::ensure!(!contents.is_null(), "Metal clip-mask buffer is not mapped");
            let slice = std::slice::from_raw_parts(contents, pixel_bytes as usize);
            pixels[..pixel_bytes as usize].copy_from_slice(slice);
        }
        Ok(())
    }

    fn ensure_buffer_size(&mut self, scene: &Scene) -> Result<()> {
        const ALIGN: usize = 256;
        const MAX_INSTANCE_BUFFER_BYTES: usize = 256 * 1024 * 1024;
        let align_up = |size: usize| {
            size.checked_add(ALIGN - 1)
                .map(|size| size / ALIGN * ALIGN)
                .ok_or_else(|| anyhow::anyhow!("instance-buffer alignment overflow"))
        };

        let total_path_vertices = scene.paths.iter().try_fold(0usize, |total, path| {
            total
                .checked_add(path.vertices.len())
                .ok_or_else(|| anyhow::anyhow!("path vertex count overflow"))
        })?;

        let counts = [
            (mem::size_of::<Shadow>(), scene.shadows.len()),
            (mem::size_of::<Quad>(), scene.quads.len()),
            (
                mem::size_of::<PathRasterizationVertex>(),
                total_path_vertices,
            ),
            (mem::size_of::<PathSprite>(), scene.paths.len()),
            (mem::size_of::<Underline>(), scene.underlines.len()),
            (
                mem::size_of::<MonochromeSprite>(),
                scene.monochrome_sprites.len(),
            ),
            (
                mem::size_of::<PolychromeSprite>(),
                scene.polychrome_sprites.len(),
            ),
            (mem::size_of::<SurfaceBounds>(), scene.surfaces.len()),
        ];
        let estimated_bytes = counts
            .into_iter()
            .try_fold(0usize, |total, (stride, count)| {
                let bytes = stride
                    .checked_mul(count)
                    .ok_or_else(|| anyhow::anyhow!("instance-buffer size overflow"))?;
                total
                    .checked_add(align_up(bytes)?)
                    .ok_or_else(|| anyhow::anyhow!("instance-buffer size overflow"))
            })?;

        let required = estimated_bytes
            .checked_add(estimated_bytes / 5)
            .ok_or_else(|| anyhow::anyhow!("instance-buffer headroom overflow"))?;
        anyhow::ensure!(
            required <= MAX_INSTANCE_BUFFER_BYTES,
            "scene requires {required} instance bytes; maximum is {MAX_INSTANCE_BUFFER_BYTES}"
        );

        let mut pool = self.instance_buffer_pool.lock();
        if pool.buffer_size < required {
            let mut new_size = pool.buffer_size;
            while new_size < required {
                new_size = new_size
                    .checked_mul(2)
                    .unwrap_or(MAX_INSTANCE_BUFFER_BYTES)
                    .min(MAX_INSTANCE_BUFFER_BYTES);
            }
            pool.reset(new_size);
            self.counters.instance_buffer_growths =
                self.counters.instance_buffer_growths.saturating_add(1);
        }
        Ok(())
    }

    fn draw_primitives(
        &mut self,
        scene: &Scene,
        instance_buffer: &mut InstanceBuffer,
        drawable: &metal::MetalDrawableRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.layer.is_opaque() { 1. } else { 0. };
        let mut instance_offset = 0;

        let command_encoder = new_drawable_command_encoder(
            command_buffer,
            drawable,
            viewport_size,
            metal::MTLLoadAction::Clear,
            alpha,
        );

        if !self.draw_scene_with_encoder(
            scene,
            instance_buffer,
            &mut instance_offset,
            viewport_size,
            command_buffer,
            drawable.texture(),
            command_encoder,
            |command_buffer, load_action| {
                new_drawable_command_encoder(
                    command_buffer,
                    drawable,
                    viewport_size,
                    load_action,
                    alpha,
                )
            },
        ) {
            anyhow::bail!(
                "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            );
        }

        if !self.draw_cached_surface_snapshots(
            scene,
            instance_buffer,
            &mut instance_offset,
            viewport_size,
            command_buffer,
        ) {
            anyhow::bail!(
                "cached surface snapshots exceeded instance buffer capacity: {}",
                scene.cached_surface_snapshots.len(),
            );
        }

        instance_buffer.metal_buffer.did_modify_range(NSRange {
            location: 0,
            length: instance_offset as u64,
        });
        Ok(command_buffer.to_owned())
    }

    fn draw_scene_with_encoder<'a, F>(
        &mut self,
        scene: &Scene,
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_buffer: &'a metal::CommandBufferRef,
        target_texture: &'a metal::TextureRef,
        mut command_encoder: &'a metal::RenderCommandEncoderRef,
        reopen_encoder: F,
    ) -> bool
    where
        F: Fn(
            &'a metal::CommandBufferRef,
            metal::MTLLoadAction,
        ) -> &'a metal::RenderCommandEncoderRef,
    {
        for batch in scene.batches() {
            let ok = match batch {
                PrimitiveBatch::Shadows(shadows) => self.draw_shadows(
                    shadows,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::BlurRects(blur_rects) => {
                    command_encoder.end_encoding();
                    let did_draw = self.draw_blur_rects(
                        blur_rects,
                        viewport_size,
                        command_buffer,
                        target_texture,
                    );
                    command_encoder = reopen_encoder(command_buffer, metal::MTLLoadAction::Load);
                    did_draw
                }
                PrimitiveBatch::Quads(quads) => self.draw_quads(
                    quads,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Paths(paths) => {
                    command_encoder.end_encoding();

                    let did_draw = self.ensure_path_intermediate_textures(viewport_size)
                        && self.draw_paths_to_intermediate(
                            paths,
                            instance_buffer,
                            instance_offset,
                            viewport_size,
                            command_buffer,
                        );

                    command_encoder = reopen_encoder(command_buffer, metal::MTLLoadAction::Load);

                    if did_draw {
                        self.draw_paths_from_intermediate(
                            paths,
                            instance_buffer,
                            instance_offset,
                            viewport_size,
                            command_encoder,
                        )
                    } else {
                        false
                    }
                }
                PrimitiveBatch::Underlines(underlines) => self.draw_underlines(
                    underlines,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_monochrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => self.draw_polychrome_sprites(
                    texture_id,
                    sprites,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::Surfaces(surfaces) => self.draw_surfaces(
                    surfaces,
                    instance_buffer,
                    instance_offset,
                    viewport_size,
                    command_encoder,
                ),
            };

            if !ok {
                command_encoder.end_encoding();
                return false;
            }
        }

        command_encoder.end_encoding();
        true
    }

    fn draw_blur_rects(
        &mut self,
        blur_rects: &[BlurRect],
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
        target_texture: &metal::TextureRef,
    ) -> bool {
        if blur_rects.is_empty() {
            return true;
        }

        if !self.ensure_blur_textures(viewport_size) {
            return false;
        }

        let Some(blur_source_texture) = self.blur_source_texture.as_ref() else {
            return false;
        };
        let Some(blur_horizontal_texture) = self.blur_horizontal_texture.as_ref() else {
            return false;
        };

        for blur_rect in blur_rects {
            let capture_bounds = blur_rect.capture_bounds(viewport_size);
            if capture_bounds.is_empty() {
                continue;
            }

            let horizontal_pass = BlurPass::horizontal(blur_rect, capture_bounds);
            let composite_pass = BlurPass::composite(blur_rect, capture_bounds);

            let blit_encoder = command_buffer.new_blit_command_encoder();
            blit_encoder.copy_from_texture(
                target_texture,
                0,
                0,
                metal::MTLOrigin {
                    x: capture_bounds.origin.x.0 as u64,
                    y: capture_bounds.origin.y.0 as u64,
                    z: 0,
                },
                metal::MTLSize {
                    width: capture_bounds.size.width.0 as u64,
                    height: capture_bounds.size.height.0 as u64,
                    depth: 1,
                },
                blur_source_texture,
                0,
                0,
                metal::MTLOrigin {
                    x: capture_bounds.origin.x.0 as u64,
                    y: capture_bounds.origin.y.0 as u64,
                    z: 0,
                },
            );
            blit_encoder.end_encoding();

            let horizontal_encoder = new_texture_command_encoder(
                command_buffer,
                blur_horizontal_texture,
                viewport_size,
                metal::MTLLoadAction::Clear,
                0.0,
            );
            horizontal_encoder.set_render_pipeline_state(&self.blur_horizontal_pipeline_state);
            horizontal_encoder.set_vertex_buffer(
                BlurInputIndex::Vertices as u64,
                Some(&self.unit_vertices),
                0,
            );
            horizontal_encoder.set_vertex_bytes(
                BlurInputIndex::BlurPass as u64,
                mem::size_of::<BlurPass>() as u64,
                &horizontal_pass as *const BlurPass as *const _,
            );
            horizontal_encoder.set_fragment_bytes(
                BlurInputIndex::BlurPass as u64,
                mem::size_of::<BlurPass>() as u64,
                &horizontal_pass as *const BlurPass as *const _,
            );
            horizontal_encoder.set_vertex_bytes(
                BlurInputIndex::ViewportSize as u64,
                mem::size_of_val(&viewport_size) as u64,
                &viewport_size as *const Size<DevicePixels> as *const _,
            );
            horizontal_encoder.set_fragment_texture(
                BlurInputIndex::SourceTexture as u64,
                Some(blur_source_texture),
            );
            horizontal_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            horizontal_encoder.end_encoding();

            let composite_encoder = new_texture_command_encoder(
                command_buffer,
                target_texture,
                viewport_size,
                metal::MTLLoadAction::Load,
                0.0,
            );
            composite_encoder.set_render_pipeline_state(&self.blur_composite_pipeline_state);
            composite_encoder.set_vertex_buffer(
                BlurInputIndex::Vertices as u64,
                Some(&self.unit_vertices),
                0,
            );
            composite_encoder.set_vertex_bytes(
                BlurInputIndex::BlurPass as u64,
                mem::size_of::<BlurPass>() as u64,
                &composite_pass as *const BlurPass as *const _,
            );
            composite_encoder.set_fragment_bytes(
                BlurInputIndex::BlurPass as u64,
                mem::size_of::<BlurPass>() as u64,
                &composite_pass as *const BlurPass as *const _,
            );
            composite_encoder.set_vertex_bytes(
                BlurInputIndex::ViewportSize as u64,
                mem::size_of_val(&viewport_size) as u64,
                &viewport_size as *const Size<DevicePixels> as *const _,
            );
            composite_encoder.set_fragment_texture(
                BlurInputIndex::SourceTexture as u64,
                Some(blur_horizontal_texture),
            );
            composite_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            composite_encoder.end_encoding();
        }

        true
    }

    fn draw_cached_surface_snapshots(
        &mut self,
        scene: &Scene,
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> bool {
        if scene.cached_surface_snapshots.is_empty() {
            return true;
        }
        if !self.ensure_cached_surface_texture(viewport_size) {
            return false;
        }
        let Some(cached_surface_texture) = self.cached_surface_texture.clone() else {
            return false;
        };

        for snapshot in &scene.cached_surface_snapshots {
            let snapshot_scene = scene.snapshot_subscene(snapshot.paint_operations.clone());
            let command_encoder = new_texture_command_encoder(
                command_buffer,
                cached_surface_texture.as_ref(),
                viewport_size,
                metal::MTLLoadAction::Clear,
                0.0,
            );

            if !self.draw_scene_with_encoder(
                &snapshot_scene,
                instance_buffer,
                instance_offset,
                viewport_size,
                command_buffer,
                cached_surface_texture.as_ref(),
                command_encoder,
                |command_buffer, load_action| {
                    new_texture_command_encoder(
                        command_buffer,
                        cached_surface_texture.as_ref(),
                        viewport_size,
                        load_action,
                        0.0,
                    )
                },
            ) {
                return false;
            }

            let Some(atlas_texture) = self.sprite_atlas.metal_texture(snapshot.target.texture_id)
            else {
                log::warn!("skipping cached-surface copy from a stale Metal atlas texture");
                continue;
            };
            let blit_encoder = command_buffer.new_blit_command_encoder();
            blit_encoder.copy_from_texture(
                cached_surface_texture.as_ref(),
                0,
                0,
                metal::MTLOrigin {
                    x: snapshot.source_bounds.origin.x.0 as u64,
                    y: snapshot.source_bounds.origin.y.0 as u64,
                    z: 0,
                },
                metal::MTLSize {
                    width: snapshot.source_bounds.size.width.0 as u64,
                    height: snapshot.source_bounds.size.height.0 as u64,
                    depth: 1,
                },
                atlas_texture.as_ref(),
                0,
                0,
                metal::MTLOrigin {
                    x: snapshot.target.bounds.origin.x.0 as u64,
                    y: snapshot.target.bounds.origin.y.0 as u64,
                    z: 0,
                },
            );
            blit_encoder.end_encoding();
        }

        true
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> bool {
        if paths.is_empty() {
            return true;
        }
        let Some(intermediate_texture) = &self.path_intermediate_texture else {
            return false;
        };

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let Some(color_attachment) = render_pass_descriptor.color_attachments().object_at(0) else {
            log::error!("Metal render pass has no color attachment");
            return false;
        };
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);

        if !align_offset(instance_offset) {
            command_encoder.end_encoding();
            return false;
        }
        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertices_bytes_len = mem::size_of_val(vertices.as_slice());
        let Some(next_offset) = (*instance_offset).checked_add(vertices_bytes_len) else {
            command_encoder.end_encoding();
            return false;
        };
        if next_offset > instance_buffer.size {
            command_encoder.end_encoding();
            return false;
        }
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                vertices.as_ptr() as *const u8,
                buffer_contents,
                vertices_bytes_len,
            );
        }
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );
        *instance_offset = next_offset;

        command_encoder.end_encoding();
        true
    }

    fn draw_shadows(
        &self,
        shadows: &[Shadow],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if shadows.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let shadow_bytes_len = mem::size_of_val(shadows);
        let Some(next_offset) = (*instance_offset).checked_add(shadow_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        unsafe {
            ptr::copy_nonoverlapping(
                shadows.as_ptr() as *const u8,
                buffer_contents,
                shadow_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_quads(
        &self,
        quads: &[Quad],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if quads.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }

        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let quad_bytes_len = mem::size_of_val(quads);
        let Some(next_offset) = (*instance_offset).checked_add(quad_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        unsafe {
            ptr::copy_nonoverlapping(quads.as_ptr() as *const u8, buffer_contents, quad_bytes_len);
        }

        // Group consecutive quads by their blend pipeline so destination-reading modes
        // get real blending: Multiply/Screen via fixed-function blend factors, and
        // Overlay/SoftLight/Difference via the framebuffer-fetch pipeline (simple quads
        // only — bordered/rounded ones keep the in-shader approximation). The common
        // all-normal run draws in one call exactly as before.
        let blend_key = |q: &Quad| -> u32 {
            match q.blend_mode {
                1 => 1,
                2 => 2,
                3..=5 => {
                    let simple = q.corner_radii.top_left.0 == 0.0
                        && q.corner_radii.top_right.0 == 0.0
                        && q.corner_radii.bottom_left.0 == 0.0
                        && q.corner_radii.bottom_right.0 == 0.0
                        && q.border_widths.top.0 == 0.0
                        && q.border_widths.right.0 == 0.0
                        && q.border_widths.bottom.0 == 0.0
                        && q.border_widths.left.0 == 0.0;
                    if simple { 3 } else { 0 }
                }
                _ => 0,
            }
        };
        let stride = mem::size_of::<Quad>();
        let mut run_start = 0usize;
        while run_start < quads.len() {
            let key = blend_key(&quads[run_start]);
            let mut run_end = run_start + 1;
            while run_end < quads.len() && blend_key(&quads[run_end]) == key {
                run_end += 1;
            }
            let pipeline = match key {
                1 => &self.quads_multiply_pipeline_state,
                2 => &self.quads_screen_pipeline_state,
                3 => self
                    .quads_blend_fetch_pipeline_state
                    .as_ref()
                    .unwrap_or(&self.quads_pipeline_state),
                _ => &self.quads_pipeline_state,
            };
            let run_offset = (*instance_offset + run_start * stride) as u64;
            command_encoder.set_render_pipeline_state(pipeline);
            command_encoder.set_vertex_buffer(
                QuadInputIndex::Quads as u64,
                Some(&instance_buffer.metal_buffer),
                run_offset,
            );
            command_encoder.set_fragment_buffer(
                QuadInputIndex::Quads as u64,
                Some(&instance_buffer.metal_buffer),
                run_offset,
            );
            command_encoder.draw_primitives_instanced(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                (run_end - run_start) as u64,
            );
            run_start = run_end;
        }

        *instance_offset = next_offset;
        true
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        let Some(first_path) = paths.first() else {
            return true;
        };

        let Some(ref intermediate_texture) = self.path_intermediate_texture else {
            return false;
        };

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        if !align_offset(instance_offset) {
            return false;
        }
        let sprite_bytes_len = mem::size_of_val(sprites.as_slice());
        let Some(next_offset) = (*instance_offset).checked_add(sprite_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }

        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };
        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;

        true
    }

    fn draw_underlines(
        &self,
        underlines: &[Underline],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if underlines.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );

        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        let underline_bytes_len = mem::size_of_val(underlines);
        let Some(next_offset) = (*instance_offset).checked_add(underline_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        unsafe {
            ptr::copy_nonoverlapping(
                underlines.as_ptr() as *const u8,
                buffer_contents,
                underline_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[MonochromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }

        let sprite_bytes_len = mem::size_of_val(sprites);
        let Some(next_offset) = (*instance_offset).checked_add(sprite_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            log::warn!("skipping monochrome sprites with a stale Metal atlas texture");
            return true;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: &[PolychromeSprite],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        if sprites.is_empty() {
            return true;
        }
        if !align_offset(instance_offset) {
            return false;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            log::warn!("skipping polychrome sprites with a stale Metal atlas texture");
            return true;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_buffer.metal_buffer),
            *instance_offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        let sprite_bytes_len = mem::size_of_val(sprites);
        let Some(next_offset) = (*instance_offset).checked_add(sprite_bytes_len) else {
            return false;
        };
        if next_offset > instance_buffer.size {
            return false;
        }
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        unsafe {
            ptr::copy_nonoverlapping(
                sprites.as_ptr() as *const u8,
                buffer_contents,
                sprite_bytes_len,
            );
        }

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        *instance_offset = next_offset;
        true
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        instance_buffer: &mut InstanceBuffer,
        instance_offset: &mut usize,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> bool {
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for surface in surfaces {
            let image_buffer = match &surface.source {
                crate::PaintSurfaceSource::CoreVideo(image_buffer) => image_buffer,
                #[cfg(feature = "custom-shaders")]
                crate::PaintSurfaceSource::RenderTarget { target, .. } => {
                    if let Err(error) = self.draw_render_target_surface(
                        surface,
                        target,
                        viewport_size,
                        command_encoder,
                    ) {
                        log::warn!("Metal GPU target display rejected: {error}");
                    }
                    continue;
                }
            };
            command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
            command_encoder.set_vertex_buffer(
                SurfaceInputIndex::Vertices as u64,
                Some(&self.unit_vertices),
                0,
            );
            let texture_size = size(
                DevicePixels::from(image_buffer.get_width() as i32),
                DevicePixels::from(image_buffer.get_height() as i32),
            );

            if image_buffer.get_pixel_format() != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
                log::warn!("skipping Metal surface with unsupported pixel format");
                continue;
            }

            let Ok(y_texture) = self.core_video_texture_cache.create_texture_from_image(
                image_buffer.as_concrete_TypeRef(),
                None,
                MTLPixelFormat::R8Unorm,
                image_buffer.get_width_of_plane(0),
                image_buffer.get_height_of_plane(0),
                0,
            ) else {
                log::warn!("failed to create Metal Y-plane texture");
                continue;
            };
            let Ok(cb_cr_texture) = self.core_video_texture_cache.create_texture_from_image(
                image_buffer.as_concrete_TypeRef(),
                None,
                MTLPixelFormat::RG8Unorm,
                image_buffer.get_width_of_plane(1),
                image_buffer.get_height_of_plane(1),
                1,
            ) else {
                log::warn!("failed to create Metal chroma-plane texture");
                continue;
            };
            let y_texture_ptr =
                unsafe { CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef()) };
            if y_texture_ptr.is_null() {
                log::warn!("CoreVideo Y-plane texture has no Metal texture");
                continue;
            }
            let cb_cr_texture_ptr =
                unsafe { CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef()) };
            if cb_cr_texture_ptr.is_null() {
                log::warn!("CoreVideo chroma-plane texture has no Metal texture");
                continue;
            }
            let y_texture_ref = unsafe { metal::TextureRef::from_ptr(y_texture_ptr as *mut _) };
            let cb_cr_texture_ref =
                unsafe { metal::TextureRef::from_ptr(cb_cr_texture_ptr as *mut _) };

            if !align_offset(instance_offset) {
                return false;
            }
            let Some(next_offset) = (*instance_offset).checked_add(mem::size_of::<Surface>())
            else {
                return false;
            };
            if next_offset > instance_buffer.size {
                return false;
            }

            command_encoder.set_vertex_buffer(
                SurfaceInputIndex::Surfaces as u64,
                Some(&instance_buffer.metal_buffer),
                *instance_offset as u64,
            );
            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            command_encoder
                .set_fragment_texture(SurfaceInputIndex::YTexture as u64, Some(y_texture_ref));
            command_encoder.set_fragment_texture(
                SurfaceInputIndex::CbCrTexture as u64,
                Some(cb_cr_texture_ref),
            );

            let ycbcr_matrix = surface_ycbcr_matrix(image_buffer);
            command_encoder.set_fragment_bytes(
                SurfaceInputIndex::YCbCrMatrix as u64,
                mem::size_of_val(&ycbcr_matrix) as u64,
                ycbcr_matrix.as_ptr() as *const _,
            );

            unsafe {
                let buffer_contents = (instance_buffer.metal_buffer.contents() as *mut u8)
                    .add(*instance_offset)
                    as *mut SurfaceBounds;
                ptr::write(
                    buffer_contents,
                    SurfaceBounds {
                        bounds: surface.bounds,
                        content_mask: surface.content_mask.clone(),
                    },
                );
            }

            command_encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
            *instance_offset = next_offset;
        }
        true
    }
}

fn texture_covers(texture: Option<&metal::Texture>, size: Size<DevicePixels>) -> bool {
    let Some(texture) = texture else {
        return false;
    };

    texture.width() as i32 >= size.width.0 && texture.height() as i32 >= size.height.0
}

fn scratch_texture_capacity(
    texture: Option<&metal::Texture>,
    requested: Size<DevicePixels>,
) -> Size<DevicePixels> {
    let Some(texture) = texture else {
        return requested;
    };
    // Keep each used scratch target's own high-water mark. An earlier resize
    // without this feature must not inflate its first allocation, and moving
    // between portrait and landscape windows must not repeatedly reallocate.
    size(
        DevicePixels(requested.width.0.max(texture.width() as i32)),
        DevicePixels(requested.height.0.max(texture.height() as i32)),
    )
}

/// Select the YCbCr→RGB matrix for a video surface from its frame's tagged
/// colorspace, defaulting to BT.601 when no matrix attachment is present.
///
/// The biplanar surface format is full-range 8-bit, so range/bit-depth are fixed;
/// only the matrix coefficients (BT.601 / BT.709 / BT.2020) vary by frame.
fn surface_ycbcr_matrix(image_buffer: &core_video::pixel_buffer::CVPixelBuffer) -> [[f32; 4]; 4] {
    use crate::video_color::{VideoColorRange, ycbcr_to_rgb_matrix};
    let coefficients = surface_matrix_coefficients(image_buffer);
    ycbcr_to_rgb_matrix(coefficients, VideoColorRange::Full, 8)
}

fn surface_matrix_coefficients(
    image_buffer: &core_video::pixel_buffer::CVPixelBuffer,
) -> crate::video_color::VideoMatrixCoefficients {
    use crate::video_color::VideoMatrixCoefficients;
    use core_video::buffer::CVBufferGetAttachment;
    use core_video::image_buffer::{
        CVYCbCrMatrixGetIntegerCodePointForString, kCVImageBufferYCbCrMatrixKey,
    };

    let value = unsafe {
        CVBufferGetAttachment(
            image_buffer.as_concrete_TypeRef(),
            kCVImageBufferYCbCrMatrixKey,
            std::ptr::null_mut(),
        )
    };
    if value.is_null() {
        return VideoMatrixCoefficients::Bt601;
    }
    match unsafe { CVYCbCrMatrixGetIntegerCodePointForString(value as _) } {
        1 => VideoMatrixCoefficients::Bt709,
        9 | 10 => VideoMatrixCoefficients::Bt2020Ncl,
        _ => VideoMatrixCoefficients::Bt601,
    }
}

/// Pixels read back from an off-screen render, in the renderer's native
/// `BGRA8Unorm` layout, tightly packed at `width * 4` bytes per row.
pub(crate) struct OffscreenReadback {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

const MAX_METAL_READBACK_BYTES: usize = 256 * 1024 * 1024;

fn checked_readback_layout(
    width: u64,
    height: u64,
    bytes_per_pixel: u64,
) -> Result<(u64, u64, usize)> {
    let packed_row = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| anyhow::anyhow!("Metal readback row size overflowed"))?;
    let bytes_per_row = packed_row
        .checked_add(255)
        .map(|value| value & !255)
        .ok_or_else(|| anyhow::anyhow!("Metal readback alignment overflowed"))?;
    let buffer_len = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow::anyhow!("Metal readback buffer size overflowed"))?;
    let packed_len = packed_row
        .checked_mul(height)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= MAX_METAL_READBACK_BYTES)
        .ok_or_else(|| anyhow::anyhow!("Metal readback exceeds its memory budget"))?;
    anyhow::ensure!(
        buffer_len <= MAX_METAL_READBACK_BYTES as u64,
        "aligned Metal readback exceeds its memory budget"
    );
    Ok((bytes_per_row, buffer_len, packed_len))
}

/// Pixels read back from an off-screen `RGBA16Float` render, decoded to `f32`,
/// tightly packed `[r, g, b, a]` per pixel. Values may exceed `1.0` (HDR).
pub(crate) struct OffscreenReadbackF16 {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<f32>,
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = if (bits >> 15) & 1 == 1 { -1.0 } else { 1.0 };
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;
    match exponent {
        0 => sign * (mantissa as f32) * 2.0f32.powi(-24),
        0x1f => {
            if mantissa == 0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + mantissa as f32 / 1024.0) * 2.0f32.powi(exponent as i32 - 15),
    }
}

pub(crate) fn metal_is_available() -> bool {
    !metal::Device::all().is_empty()
}

fn new_drawable_command_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    drawable: &'a metal::MetalDrawableRef,
    viewport_size: Size<DevicePixels>,
    load_action: metal::MTLLoadAction,
    clear_alpha: f64,
) -> &'a metal::RenderCommandEncoderRef {
    new_texture_command_encoder(
        command_buffer,
        drawable.texture(),
        viewport_size,
        load_action,
        clear_alpha,
    )
}

fn new_texture_command_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    load_action: metal::MTLLoadAction,
    clear_alpha: f64,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    color_attachment.set_load_action(load_action);
    if matches!(load_action, metal::MTLLoadAction::Clear) {
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., clear_alpha));
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> Result<metal::RenderPipelineState> {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal vertex function {vertex_fn_name}: {error}")
        })?;
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal fragment function {fragment_fn_name}: {error}")
        })?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| anyhow::anyhow!("Metal pipeline {label} has no color attachment"))?;
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!("creating Metal render pipeline {label}: {error}"))
}

/// Build a quad pipeline whose color attachment uses custom RGB blend factors, so
/// destination-reading blend modes (multiply, screen) are evaluated by fixed-function
/// blending. Alpha accumulates with standard over compositing.
fn build_quad_blend_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    pixel_format: metal::MTLPixelFormat,
    source_rgb: metal::MTLBlendFactor,
    destination_rgb: metal::MTLBlendFactor,
) -> Result<metal::RenderPipelineState> {
    let vertex_fn = library
        .get_function("quad_vertex", None)
        .map_err(|error| anyhow::anyhow!("locating Metal quad_vertex: {error}"))?;
    let fragment_fn = library
        .get_function("quad_fragment", None)
        .map_err(|error| anyhow::anyhow!("locating Metal quad_fragment: {error}"))?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| anyhow::anyhow!("Metal pipeline {label} has no color attachment"))?;
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(source_rgb);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(destination_rgb);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!("creating Metal render pipeline {label}: {error}"))
}

/// Build the destination-reading blend pipeline (`quad_fragment_blend`, which reads the
/// framebuffer via `[[color(0)]]`). Blending is disabled because the shader composites
/// over the backdrop itself. Returns `None` when the device/driver does not support
/// programmable blending (e.g. Intel Macs), so callers fall back to the standard pipeline.
fn build_quad_blend_fetch_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    pixel_format: metal::MTLPixelFormat,
) -> Option<metal::RenderPipelineState> {
    let vertex_fn = library.get_function("quad_vertex", None).ok()?;
    let fragment_fn = library.get_function("quad_fragment_blend", None).ok()?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label("quads_blend_fetch");
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0)?;
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(false);

    device.new_render_pipeline_state(&descriptor).ok()
}

// Paths and blur scratch passes already emit premultiplied RGB. Applying
// SourceAlpha again would darken translucent samples at each pass.
fn build_premultiplied_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> Result<metal::RenderPipelineState> {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal vertex function {vertex_fn_name}: {error}")
        })?;
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal fragment function {fragment_fn_name}: {error}")
        })?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| anyhow::anyhow!("Metal pipeline {label} has no color attachment"))?;
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!("creating Metal render pipeline {label}: {error}"))
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> Result<metal::RenderPipelineState> {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal vertex function {vertex_fn_name}: {error}")
        })?;
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .map_err(|error| {
            anyhow::anyhow!("locating Metal fragment function {fragment_fn_name}: {error}")
        })?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| anyhow::anyhow!("Metal pipeline {label} has no color attachment"))?;
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!("creating Metal render pipeline {label}: {error}"))
}

// Align to multiples of 256 make Metal happy.
fn align_offset(offset: &mut usize) -> bool {
    let Some(aligned) = (*offset).checked_add(255).map(|offset| offset / 256 * 256) else {
        return false;
    };
    *offset = aligned;
    true
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum BlurInputIndex {
    Vertices = 0,
    BlurPass = 1,
    ViewportSize = 2,
    SourceTexture = 3,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
    YCbCrMatrix = 6,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct BlurPass {
    pub target_bounds: Bounds<ScaledPixels>,
    pub sample_bounds: Bounds<ScaledPixels>,
    pub clip_bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tint: Hsla,
    pub blur_radius: ScaledPixels,
    pub saturation: f32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
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

#[cfg(test)]
mod offscreen_tests {
    use super::*;
    use crate::{ColorFilter, TransformationMatrix, hsla};

    fn headless() -> Option<MetalRenderer> {
        if !metal_is_available() {
            eprintln!("skipping offscreen test: no Metal device available");
            return None;
        }
        MetalRenderer::try_new(Arc::new(Mutex::new(InstanceBufferPool::default())))
            .map_err(|error| eprintln!("skipping offscreen test: {error:#}"))
            .ok()
    }

    #[test]
    fn instance_offsets_align_without_overflow() {
        let mut offset = 0;
        assert!(align_offset(&mut offset));
        assert_eq!(offset, 0);

        let mut offset = 257;
        assert!(align_offset(&mut offset));
        assert_eq!(offset, 512);

        let mut offset = usize::MAX;
        assert!(!align_offset(&mut offset));
        assert_eq!(offset, usize::MAX);
    }

    #[test]
    fn readback_layout_is_aligned_bounded_and_checked() {
        let (stride, buffer, packed) = checked_readback_layout(65, 2, 4).unwrap();
        assert_eq!(stride, 512);
        assert_eq!(buffer, 1_024);
        assert_eq!(packed, 520);

        assert!(checked_readback_layout(u64::MAX, 2, 4).is_err());
        assert!(checked_readback_layout(16_384, 16_384, 4).is_err());
    }

    pub(super) fn full_viewport_quad(side: f32, color: Hsla) -> Quad {
        let bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(side), ScaledPixels(side)),
        };
        Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: Background::from(color),
            transform: TransformationMatrix::unit(),
            ..Default::default()
        }
    }

    fn full_viewport_path(side: f32, color: Hsla) -> Path<ScaledPixels> {
        let mut builder = crate::PathBuilder::fill();
        builder.move_to(point(crate::px(0.0), crate::px(0.0)));
        builder.line_to(point(crate::px(side), crate::px(0.0)));
        builder.line_to(point(crate::px(side), crate::px(side)));
        builder.line_to(point(crate::px(0.0), crate::px(side)));
        builder.close();
        let mut path = builder.build().unwrap();
        path.color = Background::from(color);
        path.content_mask = ContentMask {
            bounds: path.bounds,
        };
        path.scale(1.0)
    }

    fn blur_rect(
        bounds: Bounds<ScaledPixels>,
        sigma: f32,
        tint: Hsla,
        saturation: f32,
    ) -> BlurRect {
        BlurRect {
            order: 0,
            blur_radius: ScaledPixels(sigma),
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            tint,
            saturation,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
        }
    }

    #[test]
    fn resize_and_quad_render_leave_optional_scratch_unallocated() {
        let Some(mut renderer) = headless() else {
            return;
        };
        renderer.update_drawable_size(size(DevicePixels(3840), DevicePixels(2160)));
        renderer.update_drawable_size(size(DevicePixels(1920), DevicePixels(1080)));

        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();

        assert!(renderer.path_intermediate_texture.is_none());
        assert!(renderer.path_intermediate_msaa_texture.is_none());
        assert!(renderer.cached_surface_texture.is_none());
        assert!(renderer.blur_source_texture.is_none());
        assert!(renderer.blur_horizontal_texture.is_none());
        assert_eq!(renderer.counters.path_texture_allocations, 0);
        assert_eq!(renderer.counters.cached_surface_texture_allocations, 0);
        assert_eq!(renderer.counters.blur_texture_allocations, 0);
    }

    #[test]
    fn offscreen_paths_allocate_on_use_reuse_capacity_and_survive_zero_size() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let drawable_acquisition_is_bounded: bool = unsafe {
            let layer = (&*renderer.layer as *const _) as *mut AnyObject;
            msg_send![layer, allowsNextDrawableTimeout]
        };
        assert!(
            drawable_acquisition_is_bounded,
            "a blocked drawable pool must yield for GPU failure deadlines"
        );
        renderer.update_drawable_size(size(DevicePixels(128), DevicePixels(128)));
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_path(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();

        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        let center = ((8 * 16) + 8) * 4;
        assert!(frame.bgra[center + 2] > 200 && frame.bgra[center + 3] > 200);
        let texture = renderer.path_intermediate_texture.as_ref().unwrap();
        assert_eq!((texture.width(), texture.height()), (16, 16));
        assert!(renderer.path_intermediate_msaa_texture.is_some());
        assert!(renderer.cached_surface_texture.is_none());
        assert_eq!(renderer.counters.path_texture_allocations, 1);

        for (width, height) in [(32, 16), (16, 32), (32, 16), (8, 8)] {
            renderer
                .render_scene_to_bytes(&scene, size(DevicePixels(width), DevicePixels(height)))
                .unwrap();
        }
        let texture = renderer.path_intermediate_texture.as_ref().unwrap();
        assert_eq!((texture.width(), texture.height()), (32, 32));
        assert_eq!(renderer.counters.path_texture_allocations, 3);

        renderer.update_drawable_size(size(DevicePixels(0), DevicePixels(0)));
        assert!(renderer.path_intermediate_texture.is_none());
        assert!(renderer.path_intermediate_msaa_texture.is_none());
        renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        assert_eq!(renderer.counters.path_texture_allocations, 4);
    }

    #[test]
    fn cached_subtree_scratch_allocates_only_for_snapshots() {
        use crate::{CachedSurfaceParams, CachedSurfaceSnapshot, PlatformAtlas};
        use std::borrow::Cow;

        let Some(mut renderer) = headless() else {
            return;
        };
        renderer.update_drawable_size(size(DevicePixels(128), DevicePixels(128)));
        let viewport = size(DevicePixels(16), DevicePixels(16));
        let tile = renderer
            .sprite_atlas
            .get_or_insert_with(
                &CachedSurfaceParams {
                    cache_id: 1,
                    size: viewport,
                }
                .into(),
                &mut || Ok(Some((viewport, Cow::Owned(vec![0; 16 * 16 * 4])))),
            )
            .unwrap()
            .unwrap();
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.request_cached_surface_snapshot(CachedSurfaceSnapshot {
            paint_operations: 0..scene.paint_operations.len(),
            source_bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), viewport),
            target: tile,
        });
        scene.finish();

        renderer.render_scene_to_bytes(&scene, viewport).unwrap();
        let texture = renderer.cached_surface_texture.as_ref().unwrap();
        assert_eq!((texture.width(), texture.height()), (16, 16));
        assert_eq!(renderer.counters.cached_surface_texture_allocations, 1);
        assert!(renderer.path_intermediate_texture.is_none());
        renderer.render_scene_to_bytes(&scene, viewport).unwrap();
        assert_eq!(renderer.counters.cached_surface_texture_allocations, 1);

        renderer.update_drawable_size(size(DevicePixels(0), DevicePixels(0)));
        assert!(renderer.cached_surface_texture.is_none());
        renderer.render_scene_to_bytes(&scene, viewport).unwrap();
        assert_eq!(renderer.counters.cached_surface_texture_allocations, 2);
    }

    #[test]
    fn translucent_quads_use_source_over_alpha() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 0.5)));
        scene.insert_primitive(full_viewport_quad(16.0, hsla(2.0 / 3.0, 1.0, 0.5, 0.5)));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        let center = ((8 * 16) + 8) * 4;
        let pixel = &frame.bgra[center..center + 4];
        // A half-alpha blue layer over half-alpha red yields premultiplied
        // BGRA (0.5, 0, 0.25, 0.75), not additive alpha 1.0.
        for (&actual, expected) in pixel.iter().zip([128u8, 0, 64, 191]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "source-over pixel: {pixel:?}"
            );
        }
    }

    #[test]
    fn translucent_paths_use_source_over_alpha() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 0.5)));
        scene.insert_primitive(full_viewport_path(16.0, hsla(2.0 / 3.0, 1.0, 0.5, 0.5)));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        let center = ((8 * 16) + 8) * 4;
        let pixel = &frame.bgra[center..center + 4];
        for (&actual, expected) in pixel.iter().zip([128u8, 0, 64, 191]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "source-over path pixel: {pixel:?}"
            );
        }
    }

    #[test]
    fn backdrop_blur_preserves_premultiplied_color_with_translucent_tint() {
        let Some(mut renderer) = headless() else {
            return;
        };
        for (tint, saturation, expected) in [
            (Hsla::transparent_black(), 1.0, [0u8, 0, 191, 191]),
            (hsla(2.0 / 3.0, 1.0, 0.5, 0.25), 1.0, [64, 0, 143, 207]),
            (Hsla::transparent_black(), 0.0, [27, 27, 91, 191]),
        ] {
            let mut scene = Scene::default();
            let backdrop = full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 0.5));
            let bounds = backdrop.bounds;
            scene.insert_primitive(backdrop);
            scene.insert_primitive(blur_rect(bounds, 2.0, tint, saturation));
            scene.finish();
            let frame = renderer
                .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
                .unwrap();
            let center = ((8 * 16) + 8) * 4;
            let pixel = &frame.bgra[center..center + 4];
            // Retain the existing final pass's source-over semantics. For a
            // uniform backdrop, blur changes no samples: composite the tint
            // over its premultiplied sample, then that result over the target.
            for (&actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 2,
                    "blur pixel: {pixel:?}, expected channel {expected}"
                );
            }
        }
    }

    #[test]
    fn backdrop_blur_keeps_capture_coordinates() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(64.0, hsla(0.0, 1.0, 0.5, 1.0)));
        let mut blue = full_viewport_quad(64.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0));
        blue.bounds.origin.x = ScaledPixels(20.0);
        blue.bounds.size.width = ScaledPixels(44.0);
        scene.insert_primitive(blue);
        let bounds = Bounds::new(
            point(ScaledPixels(12.0), ScaledPixels(4.0)),
            size(ScaledPixels(20.0), ScaledPixels(24.0)),
        );
        scene.insert_primitive(blur_rect(bounds, 2.0, Hsla::transparent_black(), 1.0));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(64), DevicePixels(32)))
            .unwrap();

        // Gaussian weights for a red/blue step at x=20, sigma=2, radius=6.
        // Samples remain in absolute viewport coordinates; the wider capture
        // must not be rescaled into the panel's narrower 20-pixel rectangle.
        for (x, expected) in [(18, [57u8, 0, 198, 255]), (22, [229, 0, 26, 255])] {
            let offset = ((16 * 64) + x) * 4;
            let pixel = &frame.bgra[offset..offset + 4];
            for (&actual, expected) in pixel.iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 2,
                    "blur at x={x}: {pixel:?}, expected channel {expected}"
                );
            }
        }
    }

    #[test]
    fn backdrop_blur_clamps_capture_at_texel_centers() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        let backdrop = full_viewport_quad(32.0, hsla(0.0, 1.0, 0.5, 1.0));
        let bounds = backdrop.bounds;
        scene.insert_primitive(backdrop);
        let mut blue = full_viewport_quad(32.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0));
        blue.bounds.origin.x = ScaledPixels(31.0);
        blue.bounds.size.width = ScaledPixels(1.0);
        scene.insert_primitive(blue);
        scene.insert_primitive(blur_rect(bounds, 1.0, Hsla::transparent_black(), 1.0));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(32), DevicePixels(32)))
            .unwrap();
        let offset = ((16 * 32) + 31) * 4;
        let pixel = &frame.bgra[offset..offset + 4];
        // Replicate the last blue texel at the viewport boundary. For sigma=1
        // and radius=3, offsets >= 0 contribute 0.699525 of the Gaussian weight.
        // Clamping to the texel's left edge would halve that blue contribution.
        for (&actual, expected) in pixel.iter().zip([178u8, 0, 77, 255]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "edge blur: {pixel:?}, expected channel {expected}"
            );
        }
    }

    #[test]
    fn backdrop_blur_fractional_capture_includes_last_visible_texel() {
        let mut renderer = headless().expect("fractional blur regression requires Metal");
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(64.0, hsla(0.0, 1.0, 0.5, 1.0)));
        let mut blue = full_viewport_quad(32.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0));
        blue.bounds.origin.x = ScaledPixels(32.0);
        blue.bounds.size.width = ScaledPixels(1.0);
        blue.content_mask.bounds = blue.bounds;
        scene.insert_primitive(blue);
        scene.insert_primitive(blur_rect(
            Bounds::new(
                point(ScaledPixels(12.8), ScaledPixels(4.2)),
                size(ScaledPixels(20.0), ScaledPixels(20.0)),
            ),
            0.0,
            Hsla::transparent_black(),
            1.0,
        ));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(64), DevicePixels(32)))
            .unwrap();
        let pixel = &frame.bgra[((16 * 64) + 32) * 4..][..4];
        assert_eq!(pixel, &[255, 0, 0, 255]);
    }

    #[test]
    fn backdrop_blur_respects_own_and_ancestor_rounded_clips() {
        let mut renderer = headless().expect("rounded blur regression requires Metal");
        for ancestor in [false, true] {
            let mut scene = Scene::default();
            scene.insert_primitive(full_viewport_quad(32.0, hsla(2.0 / 3.0, 1.0, 0.5, 1.0)));
            let bounds = Bounds::new(
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size(ScaledPixels(32.0), ScaledPixels(32.0)),
            );
            let mut panel = blur_rect(bounds, 1.0, hsla(0.0, 1.0, 0.5, 1.0), 1.0);
            panel.corner_radii = Corners::all(ScaledPixels(8.0));
            if ancestor {
                panel.rounded_clip_bounds = Bounds::new(
                    point(ScaledPixels(8.0), ScaledPixels(8.0)),
                    size(ScaledPixels(16.0), ScaledPixels(16.0)),
                );
                panel.rounded_clip_radii = Corners::all(ScaledPixels(8.0));
                panel.content_mask.bounds = Bounds::new(
                    point(ScaledPixels(9.0), ScaledPixels(8.0)),
                    size(ScaledPixels(15.0), ScaledPixels(16.0)),
                );
            }
            scene.insert_primitive(panel);
            scene.finish();
            let frame = renderer
                .render_scene_to_bytes(&scene, size(DevicePixels(32), DevicePixels(32)))
                .unwrap();
            assert_eq!(&frame.bgra[((16 * 32) + 16) * 4..][..4], &[0, 0, 255, 255]);
            let outside = if ancestor { (9, 8) } else { (0, 0) };
            assert_eq!(
                &frame.bgra[((outside.1 * 32) + outside.0) * 4..][..4],
                &[255, 0, 0, 255],
                "ancestor={ancestor}"
            );
        }
    }

    #[test]
    fn packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation() {
        let mut renderer = headless().expect("atlas sampling regression requires Metal");
        let scene =
            crate::scene::sprite_sampling_tests::packed_sprite_scene(&*renderer.sprite_atlas);
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(32), DevicePixels(40)))
            .unwrap();
        crate::scene::sprite_sampling_tests::assert_packed_sprite_pixels(&frame.bgra);
    }

    #[test]
    fn native_fractional_glyph_rasters_match_reserved_bounds_and_render_the_complete_line() {
        use crate::{
            FontRun, GlyphRasterMode, MacTextSystem, PlatformAtlas, PlatformTextSystem,
            RenderGlyphParams, font, px,
        };
        use std::borrow::Cow;
        let mut renderer = headless().expect("native glyph regression requires Metal");
        let fonts = MacTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();
        let text = "Launch plan source Bold Table Overview Status";
        let layout = fonts.layout_line(
            text,
            px(18.0),
            &[FontRun {
                font_id,
                len: text.len(),
            }],
        );
        let viewport = size(DevicePixels(1600), DevicePixels(64));
        let mask_bounds = Bounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(1600.0), ScaledPixels(64.0)),
        );
        let mut scene = Scene::default();
        let mut expected = Vec::new();
        let mut fractional = 0;
        for run in &layout.runs {
            for glyph in &run.glyphs {
                let device_x = glyph.position.x.0 * 2.0;
                let variant_x = ((device_x - device_x.floor()) * crate::SUBPIXEL_VARIANTS_X as f32)
                    .floor() as u8;
                fractional += usize::from(variant_x != 0);
                let params = RenderGlyphParams {
                    font_id: run.font_id,
                    glyph_id: glyph.id,
                    font_size: px(18.0),
                    subpixel_variant: point(variant_x, 1),
                    scale_factor: 2.0,
                    is_emoji: false,
                    raster_mode: GlyphRasterMode::Grayscale,
                };
                let bounds = fonts.glyph_raster_bounds(&params).unwrap();
                if bounds.size.width.0 == 0 || bounds.size.height.0 == 0 {
                    continue;
                }
                let (bitmap_size, payload) = fonts.rasterize_glyph(&params, bounds).unwrap();
                assert_eq!(
                    bitmap_size, bounds.size,
                    "glyph {} fractional variant {:?} does not honor the declared raster bounds",
                    glyph.index, params.subpixel_variant
                );
                let tile = renderer
                    .sprite_atlas
                    .get_or_insert_with_size(
                        &crate::AtlasKey::Glyph(params),
                        bounds.size,
                        &mut || Ok(Some((bitmap_size, Cow::Borrowed(&payload)))),
                    )
                    .unwrap()
                    .unwrap();
                let x = expected.len() * 40 + 4;
                let y = 8;
                expected.push((x, y, bitmap_size, payload));
                scene.insert_primitive(MonochromeSprite {
                    order: 0,
                    pad: 0,
                    bounds: Bounds::new(
                        point(ScaledPixels(x as f32), ScaledPixels(y as f32)),
                        bitmap_size.map(Into::into),
                    ),
                    content_mask: ContentMask {
                        bounds: mask_bounds,
                    },
                    color: hsla(0.0, 0.0, 1.0, 1.0),
                    tile,
                    transformation: TransformationMatrix::unit(),
                    rounded_clip_bounds: Bounds::default(),
                    rounded_clip_radii: Corners::default(),
                    color_filter: ColorFilter::identity(),
                });
            }
        }
        assert!(
            fractional > 10,
            "fixture must exercise fractional glyph origins"
        );
        assert!(
            expected.len() > 30,
            "fixture must render the complete label set"
        );
        scene.finish();
        let frame = renderer.render_scene_to_bytes(&scene, viewport).unwrap();
        for (index, (x, y, bitmap_size, payload)) in expected.iter().enumerate() {
            for row in 0..bitmap_size.height.0 as usize {
                for column in 0..bitmap_size.width.0 as usize {
                    let coverage = payload[row * bitmap_size.width.0 as usize + column];
                    let alpha = ((coverage as f32 / 255.0).powf(0.85) * 255.0).round() as u8;
                    let pixel = &frame.bgra[((y + row) * 1600 + x + column) * 4..][..4];
                    assert!(
                        pixel.iter().all(|channel| channel.abs_diff(alpha) <= 2),
                        "glyph {index} raster pixel ({column},{row}) GPU={pixel:?} CPU alpha={alpha}"
                    );
                }
            }
        }
        eprintln!(
            "Metal painted {} real CoreText glyph rasters, {fractional} fractional origins, every CPU coverage pixel matched",
            expected.len()
        );
    }

    #[test]
    fn many_glyph_masks_upload_and_render_every_instance_in_one_batch() {
        use crate::{PlatformAtlas, RenderSvgParams};
        use std::borrow::Cow;
        let mut renderer = headless().expect("many-glyph regression requires Metal");
        let viewport = size(DevicePixels(640), DevicePixels(32));
        let mask = ContentMask {
            bounds: Bounds::new(
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size(ScaledPixels(640.0), ScaledPixels(32.0)),
            ),
        };
        let mut scene = Scene::default();
        let mut page = None;
        for index in 0..64 {
            let payload = vec![255; 8 * 16];
            let tile = renderer
                .sprite_atlas
                .get_or_insert_with_size(
                    &crate::AtlasKey::Svg(RenderSvgParams {
                        path: format!("many-glyph-mask-{index}").into(),
                        size: size(DevicePixels(8), DevicePixels(16)),
                    }),
                    size(DevicePixels(8), DevicePixels(16)),
                    &mut || {
                        Ok(Some((
                            size(DevicePixels(8), DevicePixels(16)),
                            Cow::Borrowed(&payload),
                        )))
                    },
                )
                .unwrap()
                .unwrap();
            assert_eq!(*page.get_or_insert(tile.texture_id), tile.texture_id);
            scene.insert_primitive(MonochromeSprite {
                order: 0,
                pad: 0,
                bounds: Bounds::new(
                    point(ScaledPixels(index as f32 * 10.0 + 0.25), ScaledPixels(8.25)),
                    size(ScaledPixels(8.0), ScaledPixels(16.0)),
                ),
                content_mask: mask.clone(),
                color: hsla(0.0, 0.0, 1.0, 1.0),
                tile,
                transformation: TransformationMatrix::unit(),
                rounded_clip_bounds: Bounds::default(),
                rounded_clip_radii: Corners::default(),
                color_filter: ColorFilter::identity(),
            });
        }
        scene.finish();
        let frame = renderer.render_scene_to_bytes(&scene, viewport).unwrap();
        for index in 0..64 {
            let pixel = &frame.bgra[(16 * 640 + index * 10 + 4) * 4..][..4];
            assert_eq!(
                pixel,
                &[255, 255, 255, 255],
                "glyph {index} did not render: {pixel:?}"
            );
        }
        eprintln!(
            "Metal rendered all 64 packed glyph-mask instances; Rust monochrome stride={}",
            std::mem::size_of::<MonochromeSprite>()
        );
    }

    #[test]
    fn atlas_pressure_retains_replayed_pixels_and_reuploads_after_retirement() {
        use crate::PlatformAtlas;
        use crate::scene::sprite_sampling_tests::*;
        let mut renderer = headless().expect("atlas ownership regression requires Metal");
        let atlas = renderer.sprite_atlas.clone();
        let scene = packed_sprite_scene(&*atlas);
        let viewport = size(DevicePixels(32), DevicePixels(40));
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bytes(&scene, viewport)
                .unwrap()
                .bgra,
        );
        let identities: Vec<_> = scene.atlas_tiles().map(|tile| tile.texture_id).collect();
        remove_packed_sprite_keys(&*atlas);
        reject_packed_sprite_growth_before_raster(&*atlas);
        assert_eq!(atlas.evict_to_budget_keeping(0, 4), 0);
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bytes(&scene, viewport)
                .unwrap()
                .bgra,
        );
        for id in &identities {
            assert!(atlas.metal_texture(*id).is_some());
        }
        for _ in 0..4 {
            atlas.advance_frame();
        }
        for id in &identities {
            assert!(atlas.metal_texture(*id).is_none());
        }
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits::default());
        let restored = packed_sprite_scene(&*atlas);
        assert!(
            restored
                .atlas_tiles()
                .all(|tile| !identities.contains(&tile.texture_id))
        );
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bytes(&restored, viewport)
                .unwrap()
                .bgra,
        );
        // Stale and cross-window scenes are rejected before GPU submission.
        assert!(renderer.render_scene_to_bytes(&scene, viewport).is_err());
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bytes(&restored, viewport)
                .unwrap()
                .bgra,
        );
        let mut foreign = headless().expect("foreign atlas regression requires Metal");
        let foreign_scene = packed_sprite_scene(&*foreign.sprite_atlas);
        assert_packed_sprite_pixels(
            &foreign
                .render_scene_to_bytes(&foreign_scene, viewport)
                .unwrap()
                .bgra,
        );
        assert!(foreign.render_scene_to_bytes(&restored, viewport).is_err());
        assert_packed_sprite_pixels(
            &foreign
                .render_scene_to_bytes(&foreign_scene, viewport)
                .unwrap()
                .bgra,
        );
    }

    #[test]
    fn surviving_atlas_page_reuse_rejects_retired_tile_before_gpu_submission() {
        use crate::PlatformAtlas;
        use crate::scene::sprite_sampling_tests::*;
        let mut renderer = headless().expect("tile identity regression requires Metal");
        let atlas = renderer.sprite_atlas.clone();
        let first = surviving_page_tile(&*atlas, 994, [0, 0, 255, 255]);
        let survivor = surviving_page_tile(&*atlas, 995, [0, 255, 0, 255]);
        assert_eq!(first.texture_id, survivor.texture_id);
        let old_scene = surviving_page_scene(first.clone());
        let viewport = size(DevicePixels(16), DevicePixels(16));
        let red = renderer
            .render_scene_to_bytes(&old_scene, viewport)
            .unwrap();
        assert_eq!(&red.bgra[(8 * 16 + 8) * 4..][..4], &[0, 0, 255, 255]);
        atlas.remove(&surviving_page_key(994));
        for _ in 0..4 {
            atlas.advance_frame();
        }
        let replacement = surviving_page_tile(&*atlas, 996, [255, 0, 0, 255]);
        assert_eq!(
            replacement.texture_id, first.texture_id,
            "fixture must keep the same page"
        );
        assert_eq!(
            replacement.bounds, first.bounds,
            "fixture must reuse exact region"
        );
        let current = renderer
            .render_scene_to_bytes(&surviving_page_scene(replacement), viewport)
            .unwrap();
        assert_eq!(&current.bgra[(8 * 16 + 8) * 4..][..4], &[255, 0, 0, 255]);
        let stale = renderer.render_scene_to_bytes(&old_scene, viewport);
        if let Ok(frame) = &stale {
            eprintln!(
                "stale surviving-page GPU pixel BGRA={:?}",
                &frame.bgra[(8 * 16 + 8) * 4..][..4]
            );
        }
        assert!(
            stale.is_err(),
            "retired tile must be rejected before sampling replacement region"
        );
    }

    #[test]
    fn oversized_readbacks_fail_before_scratch_allocations() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_path(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        let oversized = size(DevicePixels(16_384), DevicePixels(16_384));
        let error = renderer
            .render_scene_to_bytes(&scene, oversized)
            .err()
            .unwrap();
        assert!(error.to_string().contains("memory budget"));
        let error = renderer
            .render_damage_to_bytes(&scene, &scene, scene.paths[0].bounds, oversized)
            .err()
            .unwrap();
        assert!(error.to_string().contains("memory budget"));

        // Eight-byte GPU pixels fit the staging limit here, while decoded
        // four-channel f32 pixels exceed it. Both checks must happen first.
        let error = renderer
            .render_scene_to_f16(&scene, size(DevicePixels(4097), DevicePixels(4097)))
            .err()
            .unwrap();
        assert!(error.to_string().contains("decoded Metal readback"));
        assert!(renderer.path_intermediate_texture.is_none());
        assert!(renderer.path_intermediate_msaa_texture.is_none());
        assert!(renderer.cached_surface_texture.is_none());
        assert_eq!(renderer.counters.path_texture_allocations, 0);
        assert_eq!(renderer.counters.cached_surface_texture_allocations, 0);
        assert_eq!(renderer.counters.blur_texture_allocations, 0);
        assert!(renderer.instance_buffer_pool.lock().buffers.is_empty());
    }

    #[test]
    fn offscreen_empty_scene_clears_to_transparent() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        assert_eq!((frame.width, frame.height), (16, 16));
        assert_eq!(frame.bgra.len(), 16 * 16 * 4);
        assert!(
            frame.bgra.iter().all(|&byte| byte == 0),
            "transparent clear should produce all-zero BGRA"
        );
    }

    #[test]
    fn offscreen_opaque_quad_fills_its_color() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();

        let center = ((8 * 16) + 8) * 4;
        let (b, g, r, a) = (
            frame.bgra[center],
            frame.bgra[center + 1],
            frame.bgra[center + 2],
            frame.bgra[center + 3],
        );
        assert!(a > 200, "center should be near-opaque, got a={a}");
        assert!(
            r > 150 && r > g && r > b,
            "red quad should dominate the center pixel: r={r} g={g} b={b}"
        );
    }

    #[test]
    fn offscreen_gpu_frame_timing_is_opt_in_bounded_and_uses_actual_host_clock() {
        let mut renderer = headless().expect("GPU telemetry regression requires a Metal device");
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        let dimensions = size(DevicePixels(16), DevicePixels(16));
        renderer.render_scene_to_bytes(&scene, dimensions).unwrap();
        assert!(renderer.gpu_frame_timings.is_none());
        assert!(renderer.take_gpu_frame_timings().is_empty());

        assert!(renderer.set_gpu_frame_timing_enabled(true));
        let before = current_host_time();
        let frame = renderer.render_scene_to_bytes(&scene, dimensions).unwrap();
        let after = current_host_time();
        assert_eq!(&frame.bgra[((8 * 16) + 8) * 4..][..4], &[0, 0, 255, 255]);
        let records = renderer.take_gpu_frame_timings();
        assert_eq!(records.len(), 1);
        let timing = records[0];
        eprintln!("actual Metal GPU frame timing: {timing:?}; host window [{before}, {after}]");
        assert!(timing.submitted_time_seconds >= before);
        assert!(timing.gpu_start_time_seconds >= timing.submitted_time_seconds);
        assert!(timing.gpu_end_time_seconds >= timing.gpu_start_time_seconds);
        assert!(timing.gpu_end_time_seconds <= after);
        assert_eq!(timing.presented_time_seconds, None);
        assert!(renderer.take_gpu_frame_timings().is_empty());

        for _ in 0..=crate::frame_timing::collector::MAX_GPU_FRAME_TIMINGS {
            renderer.render_scene_to_bytes(&scene, dimensions).unwrap();
        }
        let retained = renderer.take_gpu_frame_timings();
        assert_eq!(
            retained.len(),
            crate::frame_timing::collector::MAX_GPU_FRAME_TIMINGS
        );
        assert_eq!(
            retained[0].frame_id, 2,
            "the oldest undrained record must be evicted"
        );

        let collector = Arc::downgrade(renderer.gpu_frame_timings.as_ref().unwrap());
        let pending = renderer.command_queue.new_command_buffer().to_owned();
        let encoder = pending.new_blit_command_encoder();
        encoder.end_encoding();
        renderer.commit_with_gpu_timing(&pending, None);
        renderer.set_gpu_frame_timing_enabled(false);
        pending.wait_until_completed();
        assert!(
            collector.upgrade().is_none(),
            "a completion callback must not retain a disabled session"
        );
        assert!(renderer.take_gpu_frame_timings().is_empty());
        assert!(renderer.set_gpu_frame_timing_enabled(true));
        renderer.render_scene_to_bytes(&scene, dimensions).unwrap();
        assert_eq!(renderer.take_gpu_frame_timings()[0].frame_id, 0);
    }

    #[test]
    fn offscreen_rgba16f_renders_quad_to_float() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.finish();
        let frame = renderer
            .render_scene_to_f16(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        assert_eq!((frame.width, frame.height), (16, 16));
        assert_eq!(frame.rgba.len(), 16 * 16 * 4);

        let center = ((8 * 16) + 8) * 4;
        let (r, g, b, a) = (
            frame.rgba[center],
            frame.rgba[center + 1],
            frame.rgba[center + 2],
            frame.rgba[center + 3],
        );
        assert!(a > 0.78, "alpha should be near-opaque as float, got {a}");
        assert!(r > 0.6, "red channel should be high as float, got r={r}");
        assert!(r > g && r > b, "red should dominate: r={r} g={g} b={b}");
    }

    #[test]
    fn offscreen_rgba16f_empty_is_transparent() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let mut scene = Scene::default();
        scene.finish();
        let frame = renderer
            .render_scene_to_f16(&scene, size(DevicePixels(8), DevicePixels(8)))
            .unwrap();
        assert!(frame.rgba.iter().all(|&value| value == 0.0));
    }

    #[test]
    fn offscreen_rgba16f_renders_multiple_primitive_types() {
        let Some(mut renderer) = headless() else {
            return;
        };
        let full = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(full_viewport_quad(16.0, hsla(0.0, 1.0, 0.5, 1.0)));
        scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
            bounds: Bounds {
                origin: point(ScaledPixels(2.0), ScaledPixels(12.0)),
                size: size(ScaledPixels(12.0), ScaledPixels(2.0)),
            },
            content_mask: ContentMask { bounds: full },
            color: hsla(0.6, 1.0, 0.5, 1.0),
            thickness: ScaledPixels(2.0),
            wavy: 0,
            color_filter: ColorFilter::identity(),
        });
        scene.finish();

        // Renders through the batch loop (Quads + Underlines) with no
        // "unsupported primitive" error.
        let frame = renderer
            .render_scene_to_f16(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        let above_underline = ((4 * 16) + 8) * 4;
        assert!(
            frame.rgba[above_underline] > 0.6,
            "quad red should render under the multi-primitive path"
        );
    }

    fn make_solid_nv12(
        side: usize,
        y_val: u8,
        cb_val: u8,
        cr_val: u8,
        matrix: core_foundation::string::CFStringRef,
    ) -> core_video::pixel_buffer::CVPixelBuffer {
        use core_foundation::base::{CFType, TCFType};
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;
        use core_video::buffer::{CVBufferSetAttachment, kCVAttachmentMode_ShouldPropagate};
        use core_video::image_buffer::kCVImageBufferYCbCrMatrixKey;
        use core_video::pixel_buffer::{
            CVPixelBuffer, kCVPixelBufferIOSurfacePropertiesKey,
            kCVPixelBufferMetalCompatibilityKey,
        };

        let empty: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[]);
        let options = CFDictionary::from_CFType_pairs(&[
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferMetalCompatibilityKey) },
                CFBoolean::true_value().as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCVPixelBufferIOSurfacePropertiesKey) },
                empty.as_CFType(),
            ),
        ]);

        let buffer = CVPixelBuffer::new(
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            side,
            side,
            Some(&options),
        )
        .expect("create NV12 pixel buffer");

        buffer.lock_base_address(0);
        unsafe {
            let y_base = buffer.get_base_address_of_plane(0) as *mut u8;
            let y_stride = buffer.get_bytes_per_row_of_plane(0);
            for row in 0..buffer.get_height_of_plane(0) {
                for col in 0..buffer.get_width_of_plane(0) {
                    *y_base.add(row * y_stride + col) = y_val;
                }
            }
            let c_base = buffer.get_base_address_of_plane(1) as *mut u8;
            let c_stride = buffer.get_bytes_per_row_of_plane(1);
            for row in 0..buffer.get_height_of_plane(1) {
                for col in 0..buffer.get_width_of_plane(1) {
                    *c_base.add(row * c_stride + col * 2) = cb_val;
                    *c_base.add(row * c_stride + col * 2 + 1) = cr_val;
                }
            }
        }
        buffer.unlock_base_address(0);

        unsafe {
            CVBufferSetAttachment(
                buffer.as_concrete_TypeRef(),
                kCVImageBufferYCbCrMatrixKey,
                matrix as _,
                kCVAttachmentMode_ShouldPropagate,
            );
        }
        buffer
    }

    fn render_surface_center(
        renderer: &mut MetalRenderer,
        image_buffer: core_video::pixel_buffer::CVPixelBuffer,
        side: usize,
    ) -> (u8, u8, u8) {
        let bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: size(ScaledPixels(side as f32), ScaledPixels(side as f32)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            source: crate::PaintSurfaceSource::CoreVideo(image_buffer),
        });
        scene.finish();
        let frame = renderer
            .render_scene_to_bytes(
                &scene,
                size(DevicePixels(side as i32), DevicePixels(side as i32)),
            )
            .unwrap();
        let center = (((side / 2) * side) + (side / 2)) * 4;
        (
            frame.bgra[center + 2],
            frame.bgra[center + 1],
            frame.bgra[center],
        )
    }

    #[test]
    fn offscreen_surface_uses_tagged_colorspace_matrix() {
        use crate::video_color::{VideoColorRange, VideoMatrixCoefficients, convert_ycbcr};
        use core_video::image_buffer::{
            kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrix_ITU_R_709_2,
        };
        let Some(mut renderer) = headless() else {
            return;
        };

        let side = 16usize;
        let (y, cb, cr) = (150u8, 90u8, 180u8);

        let buffer_601 = make_solid_nv12(side, y, cb, cr, unsafe {
            kCVImageBufferYCbCrMatrix_ITU_R_601_4
        });
        let buffer_709 = make_solid_nv12(side, y, cb, cr, unsafe {
            kCVImageBufferYCbCrMatrix_ITU_R_709_2
        });
        let (r601, g601, b601) = render_surface_center(&mut renderer, buffer_601, side);
        let (r709, g709, b709) = render_surface_center(&mut renderer, buffer_709, side);

        let expect = |coeffs| {
            let rgb = convert_ycbcr(
                coeffs,
                VideoColorRange::Full,
                8,
                y as f32 / 255.0,
                cb as f32 / 255.0,
                cr as f32 / 255.0,
            );
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as i32;
            (q(rgb[0]), q(rgb[1]), q(rgb[2]))
        };
        let (er6, eg6, eb6) = expect(VideoMatrixCoefficients::Bt601);
        let (er7, eg7, eb7) = expect(VideoMatrixCoefficients::Bt709);

        let tol = 6i32;
        let close = |a: u8, b: i32| (a as i32 - b).abs() <= tol;
        assert!(
            close(r601, er6) && close(g601, eg6) && close(b601, eb6),
            "601 surface got ({r601},{g601},{b601}), expected ~({er6},{eg6},{eb6})"
        );
        assert!(
            close(r709, er7) && close(g709, eg7) && close(b709, eb7),
            "709 surface got ({r709},{g709},{b709}), expected ~({er7},{eg7},{eb7})"
        );
        let divergence = (r601 as i32 - r709 as i32).abs()
            + (g601 as i32 - g709 as i32).abs()
            + (b601 as i32 - b709 as i32).abs();
        assert!(
            divergence > tol,
            "colorspace dispatch must change output: 601=({r601},{g601},{b601}) 709=({r709},{g709},{b709})"
        );
    }

    #[test]
    fn golden_diff_catches_render_determinism_and_differences() {
        use crate::golden::{Tolerance, compare};
        let Some(mut renderer) = headless() else {
            return;
        };
        let dims = size(DevicePixels(16), DevicePixels(16));
        let render = |r: &mut MetalRenderer, color: Hsla| {
            let mut scene = Scene::default();
            scene.insert_primitive(full_viewport_quad(16.0, color));
            scene.finish();
            r.render_scene_to_bytes(&scene, dims).unwrap()
        };

        let red_a = render(&mut renderer, hsla(0.0, 1.0, 0.5, 1.0));
        let red_b = render(&mut renderer, hsla(0.0, 1.0, 0.5, 1.0));
        let blue = render(&mut renderer, hsla(0.66, 1.0, 0.5, 1.0));

        // Identical scenes rasterize deterministically — exact, zero-diff match.
        let same = compare(
            &red_a.bgra,
            &red_b.bgra,
            red_a.width,
            red_a.height,
            &Tolerance::exact(),
        )
        .unwrap();
        assert_eq!(same.failing_pixels, 0);
        assert!(same.passes(&Tolerance::exact()));

        // A genuinely different frame fails even the lenient GPU tolerance.
        let differ = compare(
            &red_a.bgra,
            &blue.bgra,
            red_a.width,
            red_a.height,
            &Tolerance::gpu(),
        )
        .unwrap();
        assert!(differ.failing_pixels > 0);
        assert!(!differ.passes(&Tolerance::gpu()));
    }
}
