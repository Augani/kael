use super::metal_atlas::MetalAtlas;
use crate::{
    AtlasTextureId, Background, BlurRect, Bounds, ContentMask, Corners, DevicePixels, Hsla,
    MonochromeSprite, PaintSurface, Path, Point, PolychromeSprite, PrimitiveBatch, Quad,
    ScaledPixels, Scene, Shadow, Size, Surface, Underline, point, size,
};
use anyhow::Result;
use block::ConcreteBlock;
use objc2_foundation::NSSize;

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

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: crate::Size<f32>,
    _transparent: bool,
) -> Renderer {
    MetalRenderer::new(context)
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
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        let mut devices = metal::Device::all();
        devices.sort_by_key(|device| (device.is_removable(), device.is_low_power()));
        let Some(device) = devices.pop() else {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        };

        let layer = metal::MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        layer.set_opaque(false);
        layer.set_maximum_drawable_count(3);
        unsafe {
            let cg_color_space = core_graphics::color_space::CGColorSpace::create_with_name(
                core_graphics::color_space::kCGColorSpaceSRGB,
            )
            .expect("failed to create sRGB color space");
            // CALayer autoresizing mask bits: kCALayerWidthSizable=2, kCALayerHeightSizable=16
            const CA_AUTORESIZING_MASK: u32 = 2 | 16;
            let layer_obj = (&*layer as *const _) as *mut AnyObject;
            let cs_ptr = CGColorSpacePtr(cg_color_space.as_ptr() as *mut c_void);
            let _: () = msg_send![layer_obj, setColorspace: cs_ptr];
            let _: () = msg_send![layer_obj, setAllowsNextDrawableTimeout: false];
            let _: () = msg_send![layer_obj, setNeedsDisplayOnBoundsChange: true];
            let _: () = msg_send![layer_obj, setAutoresizingMask: CA_AUTORESIZING_MASK];
        }
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

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
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
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
        let blur_horizontal_pipeline_state = build_pipeline_state(
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
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone()));
        let core_video_texture_cache =
            CVMetalTextureCache::new(None, device.clone(), None).unwrap();

        Self {
            device,
            layer,
            presents_with_transaction: false,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            quads_multiply_pipeline_state,
            quads_screen_pipeline_state,
            quads_blend_fetch_pipeline_state,
            quads_pipeline_state_rgba16f,
            shadows_pipeline_state_rgba16f,
            underlines_pipeline_state_rgba16f,
            blur_horizontal_pipeline_state,
            blur_composite_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
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
        }
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

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        self.layer
            .set_presents_with_transaction(presents_with_transaction);
    }

    pub fn update_drawable_size(&mut self, new_size: Size<DevicePixels>) {
        if self.drawable_size == new_size {
            return;
        }

        self.counters.resize_events += 1;
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
        self.counters.capacity_growths += 1;
        self.update_path_intermediate_textures(self.drawable_capacity);
        self.update_cached_surface_texture(self.drawable_capacity);
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

    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        if texture_covers(self.path_intermediate_texture.as_ref(), size)
            && (!self.uses_msaa()
                || texture_covers(self.path_intermediate_msaa_texture.as_ref(), size))
        {
            return;
        }

        self.counters.path_texture_allocations += 1;

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
    }

    fn update_cached_surface_texture(&mut self, size: Size<DevicePixels>) {
        if texture_covers(self.cached_surface_texture.as_ref(), size) {
            return;
        }

        self.counters.cached_surface_texture_allocations += 1;

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.cached_surface_texture = Some(self.device.new_texture(&texture_descriptor));
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

        self.counters.blur_texture_allocations += 1;

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
        self.counters.draw_calls += 1;
        self.counters.frames_requested += 1;
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
            self.counters.next_drawable_failures += 1;
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

        self.ensure_buffer_size(scene);

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
                        command_buffer.commit();
                        command_buffer.wait_until_scheduled();
                        drawable.present();
                    } else {
                        command_buffer.present_drawable(drawable);
                        command_buffer.commit();
                    }

                    self.counters.frames_presented += 1;
                    let present_instant = Instant::now();
                    if let Some(last_present_instant) = self.last_present_instant {
                        let interval =
                            present_instant.saturating_duration_since(last_present_instant);
                        let interval_micros = interval.as_micros() as u64;
                        self.counters.max_frame_interval_micros =
                            self.counters.max_frame_interval_micros.max(interval_micros);
                        if interval > Duration::from_micros(16_667) {
                            self.counters.missed_display_intervals += 1;
                        }
                    }
                    self.last_present_instant = Some(present_instant);
                    self.counters.last_present_timestamp_micros = present_instant
                        .duration_since(drawable_started_at)
                        .as_micros()
                        as u64;

                    if next_drawable_wait_micros > 2_000 {
                        self.counters.drawable_stall_count += 1;
                    }

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
                    instance_buffer_pool.reset(buffer_size * 2);
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

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(RENDER_TARGET_PIXEL_FORMAT);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);
        let target_ref: &metal::TextureRef = &target;

        self.ensure_buffer_size(scene);
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

        let bytes_per_row = align_up_256(width * 4);
        let buffer_len = bytes_per_row * height;
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

        command_buffer.commit();
        command_buffer.wait_until_completed();

        self.instance_buffer_pool.lock().release(instance_buffer);

        if !snapshots_ok {
            anyhow::bail!("scene exceeded instance buffer capacity during offscreen render");
        }

        let row_bytes = (width * 4) as usize;
        let src_stride = bytes_per_row as usize;
        let mut bgra = vec![0u8; row_bytes * height as usize];
        unsafe {
            let contents = staging.contents() as *const u8;
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
        align_offset(instance_offset);
        let bytes_len = mem::size_of_val(instances);
        if *instance_offset + bytes_len > instance_buffer.size {
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
        *instance_offset += bytes_len;
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

        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_width(width);
        descriptor.set_height(height);
        descriptor.set_pixel_format(metal::MTLPixelFormat::RGBA16Float);
        descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        let target = self.device.new_texture(&descriptor);

        self.ensure_buffer_size(scene);
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

        let bytes_per_row = align_up_256(width * 8);
        let buffer_len = bytes_per_row * height;
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
        command_buffer.commit();
        command_buffer.wait_until_completed();
        self.instance_buffer_pool.lock().release(instance_buffer);

        if let Some(message) = error {
            anyhow::bail!("{message}");
        }

        let row_stride = bytes_per_row as usize;
        let mut rgba = vec![0.0f32; (width * 4 * height) as usize];
        unsafe {
            let contents = staging.contents() as *const u8;
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
            let slice = std::slice::from_raw_parts(contents, data.len());
            data.copy_from_slice(slice);
        }
        Ok(())
    }

    fn ensure_buffer_size(&mut self, scene: &Scene) {
        const ALIGN: usize = 256;
        let align_up = |size: usize| size.div_ceil(ALIGN) * ALIGN;

        let total_path_vertices: usize = scene.paths.iter().map(|p| p.vertices.len()).sum();

        let estimated_bytes = align_up(mem::size_of::<Shadow>() * scene.shadows.len())
            + align_up(mem::size_of::<Quad>() * scene.quads.len())
            + align_up(mem::size_of::<PathRasterizationVertex>() * total_path_vertices)
            + align_up(mem::size_of::<PathSprite>() * scene.paths.len())
            + align_up(mem::size_of::<Underline>() * scene.underlines.len())
            + align_up(mem::size_of::<MonochromeSprite>() * scene.monochrome_sprites.len())
            + align_up(mem::size_of::<PolychromeSprite>() * scene.polychrome_sprites.len())
            + align_up(mem::size_of::<SurfaceBounds>()) * scene.surfaces.len();

        let required = estimated_bytes + estimated_bytes / 5;

        let mut pool = self.instance_buffer_pool.lock();
        if pool.buffer_size < required {
            let mut new_size = pool.buffer_size;
            while new_size < required {
                new_size *= 2;
            }
            new_size = new_size.min(256 * 1024 * 1024);
            pool.reset(new_size);
            self.counters.instance_buffer_growths += 1;
        }
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

                    let did_draw = self.draw_paths_to_intermediate(
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
        let Some(cached_surface_texture) = self.cached_surface_texture.clone() else {
            return true;
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

            let atlas_texture = self.sprite_atlas.metal_texture(snapshot.target.texture_id);
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
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
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

        align_offset(instance_offset);
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
        let next_offset = *instance_offset + vertices_bytes_len;
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
        align_offset(instance_offset);

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
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + shadow_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

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
        align_offset(instance_offset);

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
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + quad_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

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

        align_offset(instance_offset);
        let sprite_bytes_len = mem::size_of_val(sprites.as_slice());
        let next_offset = *instance_offset + sprite_bytes_len;
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
        align_offset(instance_offset);

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
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + underline_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

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
        align_offset(instance_offset);

        let sprite_bytes_len = mem::size_of_val(sprites);
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

        let texture = self.sprite_atlas.metal_texture(texture_id);
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
        align_offset(instance_offset);

        let texture = self.sprite_atlas.metal_texture(texture_id);
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
        let buffer_contents =
            unsafe { (instance_buffer.metal_buffer.contents() as *mut u8).add(*instance_offset) };

        let next_offset = *instance_offset + sprite_bytes_len;
        if next_offset > instance_buffer.size {
            return false;
        }

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
        command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for surface in surfaces {
            let texture_size = size(
                DevicePixels::from(surface.image_buffer.get_width() as i32),
                DevicePixels::from(surface.image_buffer.get_height() as i32),
            );

            assert_eq!(
                surface.image_buffer.get_pixel_format(),
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            );

            let y_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::R8Unorm,
                    surface.image_buffer.get_width_of_plane(0),
                    surface.image_buffer.get_height_of_plane(0),
                    0,
                )
                .unwrap();
            let cb_cr_texture = self
                .core_video_texture_cache
                .create_texture_from_image(
                    surface.image_buffer.as_concrete_TypeRef(),
                    None,
                    MTLPixelFormat::RG8Unorm,
                    surface.image_buffer.get_width_of_plane(1),
                    surface.image_buffer.get_height_of_plane(1),
                    1,
                )
                .unwrap();

            align_offset(instance_offset);
            let next_offset = *instance_offset + mem::size_of::<Surface>();
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
            // let y_texture = y_texture.get_texture().unwrap().
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(y_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(cb_cr_texture.as_concrete_TypeRef());
                Some(metal::TextureRef::from_ptr(texture as *mut _))
            });

            let ycbcr_matrix = surface_ycbcr_matrix(&surface.image_buffer);
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

fn align_up_256(value: u64) -> u64 {
    (value + 255) & !255
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
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
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
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function("quad_vertex", None)
        .expect("error locating quad_vertex");
    let fragment_fn = library
        .get_function("quad_fragment", None)
        .expect("error locating quad_fragment");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
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
        .expect("could not create render pipeline state")
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
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(false);

    device.new_render_pipeline_state(&descriptor).ok()
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
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
        .expect("could not create render pipeline state")
}

// Align to multiples of 256 make Metal happy.
fn align_offset(offset: &mut usize) {
    *offset = (*offset).div_ceil(256) * 256;
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
        Some(MetalRenderer::new(Arc::new(Mutex::new(
            InstanceBufferPool::default(),
        ))))
    }

    fn full_viewport_quad(side: f32, color: Hsla) -> Quad {
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
            image_buffer,
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
