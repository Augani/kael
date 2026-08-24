use super::atlas::{WebAtlas, WebAtlasUpload};
use crate::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, Background, BackgroundTag, Bounds, ColorFilter,
    Corners, DevicePixels, Edges, GpuSpecs, Hsla, Image, PolychromeSprite, Quad, ScaledPixels,
    Scene, Shadow, Size, TransformationMatrix,
    assets::checked_image_frame_len,
    platform::encode_rgba_png,
    platform::web_scene_math::{
        analyze_rgba_sample, damage_scissor, differing_sample_bytes, draw_ranges,
        is_software_renderer, rgba_components,
    },
    scene::{FrameDamage, MonochromeSprite, Path, PrimitiveBatch, Underline},
};
use anyhow::{Context as _, Result, anyhow};
use collections::FxHashMap;
use js_sys::Float32Array;
use std::sync::Arc;
use wasm_bindgen::JsCast as _;
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlFramebuffer, WebGlProgram,
    WebGlShader, WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

const UNIT_QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
const UNMASKED_VENDOR_WEBGL: u32 = 0x9245;
const UNMASKED_RENDERER_WEBGL: u32 = 0x9246;
const SOLID_QUAD_INSTANCE_FLOATS: usize = 46;
const SPRITE_INSTANCE_FLOATS: usize = 42;
const SHAPE_UNIFORM_NAMES: &[&str] = &[
    "u_viewport",
    "u_vertex_bounds",
    "u_shape_bounds",
    "u_content_mask",
    "u_rounded_clip_bounds",
    "u_rounded_clip_radii",
    "u_transform",
    "u_translation",
    "u_mode",
    "u_corner_radii",
    "u_border_widths",
    "u_border_dashed",
    "u_fill_kind",
    "u_fill_count",
    "u_fill_colors[0]",
    "u_fill_stops[0]",
    "u_fill_angle",
    "u_fill_center",
    "u_fill_radius",
    "u_border_color",
    "u_color_filter",
    "u_shadow_blur",
    "u_shadow_inset",
    "u_shadow_color",
];
const TEXTURE_UNIFORM_NAMES: &[&str] = &["u_viewport", "u_texture"];
const SOLID_QUAD_UNIFORM_NAMES: &[&str] = &["u_viewport"];
const PATH_UNIFORM_NAMES: &[&str] = &["u_viewport", "u_content_mask", "u_color"];

struct CachedTexture {
    texture: WebGlTexture,
    revision: u64,
    size: Size<DevicePixels>,
}

struct UniformCache {
    locations: FxHashMap<&'static str, Option<WebGlUniformLocation>>,
}

impl UniformCache {
    fn new(gl: &Gl, program: &WebGlProgram, names: &'static [&'static str]) -> Self {
        Self {
            locations: names
                .iter()
                .copied()
                .map(|name| (name, gl.get_uniform_location(program, name)))
                .collect(),
        }
    }

    fn get(&self, name: &'static str) -> Option<&WebGlUniformLocation> {
        self.locations.get(name).and_then(Option::as_ref)
    }
}

pub(super) struct WebGlSceneRenderer {
    canvas: HtmlCanvasElement,
    gl: Gl,
    shape_program: WebGlProgram,
    shape_uniforms: UniformCache,
    solid_quad_program: WebGlProgram,
    solid_quad_uniforms: UniformCache,
    texture_program: WebGlProgram,
    texture_uniforms: UniformCache,
    path_program: WebGlProgram,
    path_uniforms: UniformCache,
    quad_buffer: WebGlBuffer,
    quad_vao: WebGlVertexArrayObject,
    solid_quad_instance_buffer: WebGlBuffer,
    solid_quad_vao: WebGlVertexArrayObject,
    texture_instance_buffer: WebGlBuffer,
    texture_vao: WebGlVertexArrayObject,
    path_buffer: WebGlBuffer,
    path_vao: WebGlVertexArrayObject,
    atlas: Arc<WebAtlas>,
    textures: FxHashMap<AtlasTextureId, CachedTexture>,
    solid_quad_instances: Vec<f32>,
    sprite_instances: Vec<f32>,
    path_vertices: Vec<f32>,
    frame_count: u64,
    has_presented_frame: bool,
    verification_pixels: Option<Vec<u8>>,
    recovery_reference: Option<Vec<u8>>,
    previous_scene: Option<Scene>,
}

impl WebGlSceneRenderer {
    pub(super) fn new(canvas: &HtmlCanvasElement, size: Size<DevicePixels>) -> Result<Self> {
        Self::new_with_atlas(canvas, size, Arc::new(WebAtlas::default()), 0, None)
    }

    /// Recreate all context-owned WebGL resources while preserving the retained
    /// CPU atlas. A restored context has no textures, shaders, buffers, or VAOs,
    /// but the next draw can upload complete atlas pages from the same data.
    pub(super) fn restored(
        canvas: &HtmlCanvasElement,
        size: Size<DevicePixels>,
        atlas: Arc<WebAtlas>,
        frame_count: u64,
        recovery_reference: Option<Vec<u8>>,
    ) -> Result<Self> {
        let renderer = Self::new_with_atlas(canvas, size, atlas, frame_count, recovery_reference)?;
        renderer.atlas.mark_all_pages_dirty();
        Ok(renderer)
    }

    fn new_with_atlas(
        canvas: &HtmlCanvasElement,
        size: Size<DevicePixels>,
        atlas: Arc<WebAtlas>,
        frame_count: u64,
        recovery_reference: Option<Vec<u8>>,
    ) -> Result<Self> {
        let context_options = js_sys::Object::new();
        js_sys::Reflect::set(
            &context_options,
            &wasm_bindgen::JsValue::from_str("preserveDrawingBuffer"),
            &wasm_bindgen::JsValue::TRUE,
        )
        .map_err(js_error)?;
        let gl = canvas
            .get_context_with_context_options("webgl2", &context_options)
            .map_err(js_error)?
            .context("this browser does not provide WebGL2")?
            .dyn_into::<Gl>()
            .map_err(|_| anyhow!("#blade returned a non-WebGL2 rendering context"))?;
        let shape_program = link_program(&gl, QUAD_VERTEX_SHADER, SHAPE_FRAGMENT_SHADER)?;
        let solid_quad_program =
            link_program(&gl, SOLID_QUAD_VERTEX_SHADER, SOLID_QUAD_FRAGMENT_SHADER)?;
        let texture_program = link_program(&gl, TEXTURE_VERTEX_SHADER, TEXTURE_FRAGMENT_SHADER)?;
        let path_program = link_program(&gl, PATH_VERTEX_SHADER, PATH_FRAGMENT_SHADER)?;
        let shape_uniforms = UniformCache::new(&gl, &shape_program, SHAPE_UNIFORM_NAMES);
        let solid_quad_uniforms =
            UniformCache::new(&gl, &solid_quad_program, SOLID_QUAD_UNIFORM_NAMES);
        let texture_uniforms = UniformCache::new(&gl, &texture_program, TEXTURE_UNIFORM_NAMES);
        let path_uniforms = UniformCache::new(&gl, &path_program, PATH_UNIFORM_NAMES);

        let quad_buffer = gl
            .create_buffer()
            .context("failed to create browser quad buffer")?;
        let quad_vao = gl
            .create_vertex_array()
            .context("failed to create browser quad vertex array")?;
        gl.bind_vertex_array(Some(&quad_vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&quad_buffer));
        unsafe {
            let vertices = Float32Array::view(&UNIT_QUAD);
            gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &vertices, Gl::STATIC_DRAW);
        }
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);

        let solid_quad_instance_buffer = gl
            .create_buffer()
            .context("failed to create browser solid-quad instance buffer")?;
        let solid_quad_vao = gl
            .create_vertex_array()
            .context("failed to create browser solid-quad vertex array")?;
        gl.bind_vertex_array(Some(&solid_quad_vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&quad_buffer));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&solid_quad_instance_buffer));
        let solid_quad_stride = i32::try_from(SOLID_QUAD_INSTANCE_FLOATS * size_of::<f32>())
            .context("browser solid-quad instance stride overflow")?;
        for (location, components, offset) in [
            (1, 4, 0),
            (2, 4, 4),
            (3, 4, 8),
            (4, 4, 12),
            (5, 4, 16),
            (6, 4, 20),
            (7, 2, 24),
            (8, 4, 26),
            (9, 4, 30),
            (10, 4, 34),
            (11, 4, 38),
            (12, 4, 42),
        ] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(
                location,
                components,
                Gl::FLOAT,
                false,
                solid_quad_stride,
                i32::try_from(offset * size_of::<f32>())
                    .context("browser solid-quad attribute offset overflow")?,
            );
            gl.vertex_attrib_divisor(location, 1);
        }

        let texture_instance_buffer = gl
            .create_buffer()
            .context("failed to create browser sprite instance buffer")?;
        let texture_vao = gl
            .create_vertex_array()
            .context("failed to create browser sprite vertex array")?;
        gl.bind_vertex_array(Some(&texture_vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&quad_buffer));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 0, 0);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&texture_instance_buffer));
        let stride = i32::try_from(SPRITE_INSTANCE_FLOATS * size_of::<f32>())
            .context("browser sprite instance stride overflow")?;
        for (location, components, offset) in [
            (1, 4, 0),
            (2, 4, 4),
            (3, 4, 8),
            (4, 4, 12),
            (5, 4, 16),
            (6, 4, 20),
            (7, 2, 24),
            (8, 4, 26),
            (9, 4, 30),
            (10, 4, 34),
            (11, 4, 38),
        ] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(
                location,
                components,
                Gl::FLOAT,
                false,
                stride,
                i32::try_from(offset * size_of::<f32>())
                    .context("browser sprite attribute offset overflow")?,
            );
            gl.vertex_attrib_divisor(location, 1);
        }

        let path_buffer = gl
            .create_buffer()
            .context("failed to create browser path buffer")?;
        let path_vao = gl
            .create_vertex_array()
            .context("failed to create browser path vertex array")?;
        gl.bind_vertex_array(Some(&path_vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&path_buffer));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, Gl::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_with_i32(1, 2, Gl::FLOAT, false, 16, 8);
        gl.bind_vertex_array(None);

        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);
        gl.disable(Gl::DEPTH_TEST);
        gl.disable(Gl::CULL_FACE);
        gl.pixel_storei(Gl::UNPACK_ALIGNMENT, 1);
        gl.viewport(0, 0, size.width.0, size.height.0);

        Ok(Self {
            canvas: canvas.clone(),
            gl,
            shape_program,
            shape_uniforms,
            solid_quad_program,
            solid_quad_uniforms,
            texture_program,
            texture_uniforms,
            path_program,
            path_uniforms,
            quad_buffer,
            quad_vao,
            solid_quad_instance_buffer,
            solid_quad_vao,
            texture_instance_buffer,
            texture_vao,
            path_buffer,
            path_vao,
            atlas,
            textures: FxHashMap::default(),
            solid_quad_instances: Vec::new(),
            sprite_instances: Vec::new(),
            path_vertices: Vec::new(),
            frame_count,
            has_presented_frame: false,
            verification_pixels: None,
            recovery_reference,
            previous_scene: None,
        })
    }

    pub(super) fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub(super) fn take_verification_pixels(&mut self) -> Option<Vec<u8>> {
        self.verification_pixels.take()
    }

    pub(super) fn atlas(&self) -> Arc<WebAtlas> {
        self.atlas.clone()
    }

    pub(super) fn resize(&mut self, size: Size<DevicePixels>) {
        self.gl.viewport(0, 0, size.width.0, size.height.0);
        // Assigning a canvas backing size clears its default framebuffer. The
        // next scheduled draw must present before capture can be truthful.
        self.has_presented_frame = false;
        self.previous_scene = None;
    }

    pub(super) fn draw(&mut self, scene: &Scene) -> Result<()> {
        // Firefox can report the WebGL context as lost one animation-frame
        // callback before it dispatches `webglcontextlost`. Do not let that
        // queued callback advance the retained frame fence or replace the last
        // valid recovery sample with the all-zero readback returned by a lost
        // context. The window's restoration listener recreates every GPU-owned
        // object and then schedules a forced frame.
        if self.gl.is_context_lost() {
            return Ok(());
        }

        let atlas = &self.atlas;
        let gl = &self.gl;
        self.textures.retain(|id, cached| {
            let retained = atlas.page_revision(*id).is_some();
            if !retained {
                gl.delete_texture(Some(&cached.texture));
            }
            retained
        });

        let damage = self
            .previous_scene
            .as_ref()
            .map_or(FrameDamage::Full, |previous| scene.damage_since(previous));
        if matches!(damage, FrameDamage::None) {
            self.canvas
                .set_attribute("data-kael-frame-damage", "none")
                .map_err(js_error)?;
            self.canvas
                .set_attribute("data-kael-frame-damage-ratio", "0.000000")
                .map_err(js_error)?;
            // A retained frame can satisfy a presentation request without GPU
            // work. Still advance the public frame fence and service opt-in
            // readback: WebView IPC, capture tests, and callers waiting for a
            // requested presentation must not stall merely because the scene is
            // byte-for-byte identical.
            self.verify_pixels_if_requested()?;
            self.frame_count = self.frame_count.saturating_add(1);
            self.canvas
                .set_attribute("data-kael-frame-count", &self.frame_count.to_string())
                .map_err(js_error)?;
            self.canvas
                .set_attribute("data-kael-frame", "presented")
                .map_err(js_error)?;
            self.has_presented_frame = true;
            return Ok(());
        }

        let viewport = [self.canvas.width() as f32, self.canvas.height() as f32];
        self.gl
            .viewport(0, 0, viewport[0] as i32, viewport[1] as i32);
        let scissor = match damage {
            FrameDamage::Region(region) => damage_scissor(bounds(region), viewport),
            FrameDamage::Full | FrameDamage::None => None,
        };
        if let Some([x, y, width, height]) = scissor {
            self.gl.enable(Gl::SCISSOR_TEST);
            self.gl.scissor(x, y, width, height);
        } else {
            self.gl.disable(Gl::SCISSOR_TEST);
        }
        self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
        self.gl.clear(Gl::COLOR_BUFFER_BIT);
        self.gl.enable(Gl::BLEND);
        self.gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);

        let render_result = (|| -> Result<()> {
            for batch in scene.batches() {
                match batch {
                    PrimitiveBatch::Shadows(shadows) => {
                        self.begin_shape_batch(viewport);
                        for shadow in shadows {
                            self.draw_shadow(shadow);
                        }
                    }
                    PrimitiveBatch::BlurRects(rects) => {
                        self.begin_shape_batch(viewport);
                        for rect in rects {
                            self.draw_simple_shape(
                                rect.bounds,
                                rect.content_mask.bounds,
                                rect.corner_radii,
                                rect.tint,
                                rect.rounded_clip_bounds,
                                rect.rounded_clip_radii,
                            );
                        }
                    }
                    PrimitiveBatch::Quads(quads) => {
                        self.draw_quads(quads, viewport)?;
                    }
                    PrimitiveBatch::Paths(paths) => {
                        self.begin_path_batch(viewport);
                        for path in paths {
                            self.draw_path(path);
                        }
                    }
                    PrimitiveBatch::Underlines(underlines) => {
                        self.begin_shape_batch(viewport);
                        for underline in underlines {
                            self.draw_underline(underline);
                        }
                    }
                    PrimitiveBatch::MonochromeSprites {
                        texture_id,
                        sprites,
                    } => self.draw_monochrome_sprites(texture_id, sprites, viewport)?,
                    PrimitiveBatch::PolychromeSprites {
                        texture_id,
                        sprites,
                    } => self.draw_polychrome_sprites(texture_id, sprites, viewport)?,
                    PrimitiveBatch::Surfaces(surfaces) => {
                        let _ = surfaces;
                        // Browser video/external surfaces are not part of the first WebGL2 backend.
                    }
                }
            }
            Ok(())
        })();
        self.gl.disable(Gl::SCISSOR_TEST);
        render_result?;
        self.previous_scene = Some(scene.clone_for_damage());

        self.canvas
            .set_attribute("data-kael-renderer", "webgl2-scene")
            .map_err(js_error)?;
        self.canvas
            .set_attribute("data-kael-frame", "presented")
            .map_err(js_error)?;
        self.canvas
            .set_attribute(
                "data-kael-frame-damage",
                if scissor.is_some() { "region" } else { "full" },
            )
            .map_err(js_error)?;
        let damage_ratio = scissor.map_or(1.0, |[_, _, width, height]| {
            let damaged = f64::from(width) * f64::from(height);
            let total = f64::from(viewport[0]) * f64::from(viewport[1]);
            if total > 0.0 { damaged / total } else { 1.0 }
        });
        self.canvas
            .set_attribute(
                "data-kael-frame-damage-ratio",
                &format!("{damage_ratio:.6}"),
            )
            .map_err(js_error)?;
        self.verify_pixels_if_requested()?;
        self.frame_count = self.frame_count.saturating_add(1);
        self.canvas
            .set_attribute("data-kael-frame-count", &self.frame_count.to_string())
            .map_err(js_error)?;
        self.has_presented_frame = true;
        Ok(())
    }

    pub(super) fn export_png(&mut self, _scene: &Scene) -> Result<Image> {
        // `Window::export_frame_png` promises the current presented frame. A
        // redraw here is both unnecessary and incorrect: glyph/image cache
        // entries may be released after presentation while their pixels remain
        // valid in the default framebuffer and GPU atlas. Read that framebuffer
        // directly, matching what the user can actually see.
        anyhow::ensure!(
            self.has_presented_frame,
            "browser window has no presented frame to capture"
        );
        let width = self.canvas.width();
        let height = self.canvas.height();
        let byte_len = checked_image_frame_len(width, height)?;
        let width_i32 = i32::try_from(width).context("browser capture width overflow")?;
        let height_i32 = i32::try_from(height).context("browser capture height overflow")?;
        let mut rgba = vec![0; byte_len];
        self.gl
            .read_pixels_with_opt_u8_array(
                0,
                0,
                width_i32,
                height_i32,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut rgba),
            )
            .map_err(js_error)?;
        anyhow::ensure!(
            self.gl.get_error() == Gl::NO_ERROR,
            "browser WebGL2 scene readback failed"
        );

        let row_bytes = usize::try_from(width)? * 4;
        for top in 0..usize::try_from(height)? / 2 {
            let bottom = usize::try_from(height)? - top - 1;
            let top_start = top * row_bytes;
            let bottom_start = bottom * row_bytes;
            let (upper, lower) = rgba.split_at_mut(bottom_start);
            upper[top_start..top_start + row_bytes].swap_with_slice(&mut lower[..row_bytes]);
        }
        encode_rgba_png(width, height, rgba)
    }

    fn verify_pixels_if_requested(&mut self) -> Result<()> {
        let verification_mode = self.canvas.get_attribute("data-kael-verify-pixels");
        let continuous = verification_mode.as_deref() == Some("continuous");
        let requested = continuous || verification_mode.as_deref() == Some("true");
        if self.frame_count < 1
            || !requested
            || (!continuous
                && self
                    .canvas
                    .get_attribute("data-kael-pixel-readback")
                    .as_deref()
                    == Some("verified"))
        {
            return Ok(());
        }

        const MAX_SAMPLE_EDGE: i32 = 512;
        let canvas_width = i32::try_from(self.canvas.width()).context("canvas width overflow")?;
        let canvas_height =
            i32::try_from(self.canvas.height()).context("canvas height overflow")?;
        let width = canvas_width.clamp(1, MAX_SAMPLE_EDGE);
        let height = canvas_height.clamp(1, MAX_SAMPLE_EDGE);
        let x = (canvas_width - width) / 2;
        let y = (canvas_height - height) / 2;
        let byte_len = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .context("browser pixel verification sample is too large")?;
        let mut pixels = vec![0; byte_len];
        self.gl
            .read_pixels_with_opt_u8_array(
                x,
                y,
                width,
                height,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut pixels),
            )
            .map_err(js_error)?;
        // Context loss may race the browser/GPU boundary during readback even
        // though JavaScript execution itself is single-threaded. Preserve the
        // previous verified pixels if the context became lost in that window.
        if self.gl.is_context_lost() {
            return Ok(());
        }
        let stats = analyze_rgba_sample(&pixels).context("browser pixel sample was malformed")?;
        let pixel_count = byte_len / 4;
        let status = if stats.is_visually_varied(pixel_count) {
            "verified"
        } else {
            "insufficient"
        };
        let mut attributes = vec![
            ("data-kael-pixel-readback", status.to_string()),
            ("data-kael-pixel-changed", stats.changed_pixels.to_string()),
            ("data-kael-pixel-luma-range", stats.luma_range().to_string()),
            ("data-kael-pixel-hash", format!("{:016x}", stats.hash)),
        ];
        if let Some(reference) = self.recovery_reference.take() {
            attributes.push((
                "data-kael-context-differing-bytes",
                differing_sample_bytes(&reference, &pixels).to_string(),
            ));
            if let Some(reference_stats) = analyze_rgba_sample(&reference) {
                attributes.push((
                    "data-kael-context-reference-hash",
                    format!("{:016x}", reference_stats.hash),
                ));
            }
        }
        for (name, value) in attributes {
            self.canvas.set_attribute(name, &value).map_err(js_error)?;
        }
        self.verification_pixels = Some(pixels);
        Ok(())
    }

    pub(super) fn gpu_specs(&self) -> GpuSpecs {
        let string_parameter = |parameter| {
            self.gl
                .get_parameter(parameter)
                .ok()
                .and_then(|value| value.as_string())
        };
        // Chromium and Firefox deliberately mask RENDERER/VENDOR behind the
        // debug-renderer extension. Prefer those values when the browser makes
        // them available so a SwiftShader/llvmpipe session cannot masquerade as
        // a hardware-backed performance run. Privacy-restricted browsers still
        // fall back to their standard, non-identifying WebGL strings.
        let exposes_debug_renderer = self
            .gl
            .get_extension("WEBGL_debug_renderer_info")
            .ok()
            .flatten()
            .is_some();
        let device_name = exposes_debug_renderer
            .then(|| string_parameter(UNMASKED_RENDERER_WEBGL))
            .flatten()
            .or_else(|| string_parameter(Gl::RENDERER))
            .unwrap_or_else(|| "WebGL2".into());
        let driver_name = exposes_debug_renderer
            .then(|| string_parameter(UNMASKED_VENDOR_WEBGL))
            .flatten()
            .or_else(|| string_parameter(Gl::VENDOR))
            .unwrap_or_else(|| "WebGL2".into());
        let driver_info = string_parameter(Gl::VERSION).unwrap_or_else(|| "WebGL2".into());
        GpuSpecs {
            is_software_emulated: is_software_renderer(&device_name, &driver_name, &driver_info),
            device_name,
            driver_name,
            driver_info,
        }
    }

    pub(super) fn destroy(&mut self) {
        for (_, cached) in self.textures.drain() {
            self.gl.delete_texture(Some(&cached.texture));
        }
        self.gl.delete_buffer(Some(&self.quad_buffer));
        self.gl
            .delete_buffer(Some(&self.solid_quad_instance_buffer));
        self.gl.delete_buffer(Some(&self.texture_instance_buffer));
        self.gl.delete_buffer(Some(&self.path_buffer));
        self.gl.delete_vertex_array(Some(&self.quad_vao));
        self.gl.delete_vertex_array(Some(&self.solid_quad_vao));
        self.gl.delete_vertex_array(Some(&self.texture_vao));
        self.gl.delete_vertex_array(Some(&self.path_vao));
        self.gl.delete_program(Some(&self.shape_program));
        self.gl.delete_program(Some(&self.solid_quad_program));
        self.gl.delete_program(Some(&self.texture_program));
        self.gl.delete_program(Some(&self.path_program));
    }

    fn begin_shape_batch(&self, viewport: [f32; 2]) {
        self.gl.use_program(Some(&self.shape_program));
        self.gl.bind_vertex_array(Some(&self.quad_vao));
        uniform2f(
            &self.gl,
            &self.shape_uniforms,
            "u_viewport",
            viewport[0],
            viewport[1],
        );
    }

    fn draw_quads(&mut self, quads: &[Quad], viewport: [f32; 2]) -> Result<()> {
        self.solid_quad_instances.clear();
        let mut solid_count = 0usize;
        for quad in quads {
            if matches!(quad.background.tag, BackgroundTag::Solid)
                && matches!(quad.border_color.tag, BackgroundTag::Solid)
            {
                append_solid_quad_instance(&mut self.solid_quad_instances, quad);
                solid_count += 1;
            } else {
                self.flush_solid_quads(solid_count, viewport)?;
                solid_count = 0;
                self.begin_shape_batch(viewport);
                self.draw_quad(quad);
            }
        }
        self.flush_solid_quads(solid_count, viewport)
    }

    fn flush_solid_quads(&mut self, count: usize, viewport: [f32; 2]) -> Result<()> {
        if count == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            self.solid_quad_instances.len() == count * SOLID_QUAD_INSTANCE_FLOATS,
            "invalid browser solid-quad instance payload"
        );
        let count = i32::try_from(count).context("browser solid-quad batch is too large")?;
        self.gl.use_program(Some(&self.solid_quad_program));
        self.gl.bind_vertex_array(Some(&self.solid_quad_vao));
        uniform2f(
            &self.gl,
            &self.solid_quad_uniforms,
            "u_viewport",
            viewport[0],
            viewport[1],
        );
        self.gl
            .bind_buffer(Gl::ARRAY_BUFFER, Some(&self.solid_quad_instance_buffer));
        unsafe {
            let array = Float32Array::view(&self.solid_quad_instances);
            self.gl
                .buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &array, Gl::STREAM_DRAW);
        }
        self.gl
            .draw_arrays_instanced(Gl::TRIANGLE_STRIP, 0, 4, count);
        self.solid_quad_instances.clear();
        Ok(())
    }

    fn draw_quad(&self, quad: &Quad) {
        self.set_shape_geometry(
            quad.bounds,
            quad.bounds,
            quad.content_mask.bounds,
            quad.rounded_clip_bounds,
            quad.rounded_clip_radii,
            quad.transform,
        );
        uniform1i(&self.gl, &self.shape_uniforms, "u_mode", 0);
        uniform4f_array(
            &self.gl,
            &self.shape_uniforms,
            "u_corner_radii",
            corners(quad.corner_radii),
        );
        uniform4f_array(
            &self.gl,
            &self.shape_uniforms,
            "u_border_widths",
            edges(quad.border_widths),
        );
        uniform1i(
            &self.gl,
            &self.shape_uniforms,
            "u_border_dashed",
            i32::from(matches!(quad.border_style, crate::BorderStyle::Dashed)),
        );
        set_background(&self.gl, &self.shape_uniforms, &quad.background);
        let border = representative_color(&quad.border_color);
        uniform4f_array(&self.gl, &self.shape_uniforms, "u_border_color", border);
        set_color_filter(&self.gl, &self.shape_uniforms, quad.color_filter);
        self.gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
    }

    fn draw_shadow(&self, shadow: &Shadow) {
        let margin = (shadow.blur_radius.0 * 3.0).max(1.0);
        let vertex_bounds = Bounds {
            origin: crate::point(
                ScaledPixels(shadow.bounds.origin.x.0 - margin),
                ScaledPixels(shadow.bounds.origin.y.0 - margin),
            ),
            size: crate::size(
                ScaledPixels(shadow.bounds.size.width.0 + margin * 2.0),
                ScaledPixels(shadow.bounds.size.height.0 + margin * 2.0),
            ),
        };
        self.set_shape_geometry(
            vertex_bounds,
            shadow.bounds,
            shadow.content_mask.bounds,
            shadow.rounded_clip_bounds,
            shadow.rounded_clip_radii,
            TransformationMatrix::unit(),
        );
        uniform1i(&self.gl, &self.shape_uniforms, "u_mode", 1);
        uniform4f_array(
            &self.gl,
            &self.shape_uniforms,
            "u_corner_radii",
            corners(shadow.corner_radii),
        );
        uniform1f(
            &self.gl,
            &self.shape_uniforms,
            "u_shadow_blur",
            shadow.blur_radius.0.max(0.5),
        );
        uniform1i(
            &self.gl,
            &self.shape_uniforms,
            "u_shadow_inset",
            shadow.inset as i32,
        );
        uniform4f_array(
            &self.gl,
            &self.shape_uniforms,
            "u_shadow_color",
            rgba_components(shadow.color),
        );
        set_color_filter(&self.gl, &self.shape_uniforms, shadow.color_filter);
        self.gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_simple_shape(
        &self,
        bounds: Bounds<ScaledPixels>,
        content_mask: Bounds<ScaledPixels>,
        corner_radii: Corners<ScaledPixels>,
        color: Hsla,
        rounded_clip_bounds: Bounds<ScaledPixels>,
        rounded_clip_radii: Corners<ScaledPixels>,
    ) {
        self.set_shape_geometry(
            bounds,
            bounds,
            content_mask,
            rounded_clip_bounds,
            rounded_clip_radii,
            TransformationMatrix::unit(),
        );
        uniform1i(&self.gl, &self.shape_uniforms, "u_mode", 0);
        uniform4f_array(
            &self.gl,
            &self.shape_uniforms,
            "u_corner_radii",
            corners(corner_radii),
        );
        uniform4f_array(&self.gl, &self.shape_uniforms, "u_border_widths", [0.0; 4]);
        set_solid_background(&self.gl, &self.shape_uniforms, color);
        uniform4f_array(&self.gl, &self.shape_uniforms, "u_border_color", [0.0; 4]);
        set_color_filter(&self.gl, &self.shape_uniforms, ColorFilter::identity());
        self.gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
    }

    fn draw_underline(&self, underline: &Underline) {
        self.draw_simple_shape(
            underline.bounds,
            underline.content_mask.bounds,
            Corners::default(),
            underline.color,
            underline.rounded_clip_bounds,
            underline.rounded_clip_radii,
        );
    }

    fn set_shape_geometry(
        &self,
        vertex_bounds: Bounds<ScaledPixels>,
        shape_bounds: Bounds<ScaledPixels>,
        content_mask: Bounds<ScaledPixels>,
        rounded_clip_bounds: Bounds<ScaledPixels>,
        rounded_clip_radii: Corners<ScaledPixels>,
        transform: TransformationMatrix,
    ) {
        let uniforms = &self.shape_uniforms;
        uniform4f_array(&self.gl, uniforms, "u_vertex_bounds", bounds(vertex_bounds));
        uniform4f_array(&self.gl, uniforms, "u_shape_bounds", bounds(shape_bounds));
        uniform4f_array(&self.gl, uniforms, "u_content_mask", bounds(content_mask));
        uniform4f_array(
            &self.gl,
            uniforms,
            "u_rounded_clip_bounds",
            bounds(rounded_clip_bounds),
        );
        uniform4f_array(
            &self.gl,
            uniforms,
            "u_rounded_clip_radii",
            corners(rounded_clip_radii),
        );
        set_transform(&self.gl, uniforms, transform);
    }

    fn draw_monochrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        sprites: &[MonochromeSprite],
        viewport: [f32; 2],
    ) -> Result<()> {
        let Some(first) = sprites.first() else {
            return Ok(());
        };
        anyhow::ensure!(
            first.tile.texture_id == texture_id,
            "monochrome Scene batch texture id does not match its first tile"
        );
        let (texture, page_size) = self.texture(&first.tile)?;
        let required_floats = sprites.len().saturating_mul(SPRITE_INSTANCE_FLOATS);
        self.sprite_instances.clear();
        self.sprite_instances.reserve(required_floats);
        for sprite in sprites {
            anyhow::ensure!(
                sprite.tile.texture_id == texture_id,
                "monochrome Scene batch spans multiple atlas pages"
            );
            append_sprite_instance(
                &mut self.sprite_instances,
                sprite.bounds,
                sprite.content_mask.bounds,
                Corners::default(),
                sprite.rounded_clip_bounds,
                sprite.rounded_clip_radii,
                sprite.transformation,
                tile_uv_bounds(&sprite.tile, page_size),
                rgba_components(sprite.color),
                sprite.color_filter,
                [1.0, 0.0, 0.0, 1.0],
            );
        }
        self.begin_texture(&texture, viewport);
        self.draw_sprite_instances(&self.sprite_instances, sprites.len())
    }

    fn draw_polychrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        sprites: &[PolychromeSprite],
        viewport: [f32; 2],
    ) -> Result<()> {
        let Some(first) = sprites.first() else {
            return Ok(());
        };
        anyhow::ensure!(
            first.tile.texture_id == texture_id,
            "polychrome Scene batch texture id does not match its first tile"
        );
        let (texture, page_size) = self.texture(&first.tile)?;
        let required_floats = sprites.len().saturating_mul(SPRITE_INSTANCE_FLOATS);
        self.sprite_instances.clear();
        self.sprite_instances.reserve(required_floats);
        for sprite in sprites {
            anyhow::ensure!(
                sprite.tile.texture_id == texture_id,
                "polychrome Scene batch spans multiple atlas pages"
            );
            append_sprite_instance(
                &mut self.sprite_instances,
                sprite.bounds,
                sprite.content_mask.bounds,
                sprite.corner_radii,
                sprite.rounded_clip_bounds,
                sprite.rounded_clip_radii,
                sprite.transformation,
                tile_uv_bounds(&sprite.tile, page_size),
                rgba_components(sprite.color),
                sprite.color_filter,
                [
                    0.0,
                    f32::from(sprite.sprite_kind == crate::POLYCHROME_SPRITE_KIND_PREMULTIPLIED),
                    f32::from(sprite.grayscale),
                    sprite.opacity,
                ],
            );
        }
        self.begin_texture(&texture, viewport);
        self.draw_sprite_instances(&self.sprite_instances, sprites.len())
    }

    fn begin_texture(&self, texture: &WebGlTexture, viewport: [f32; 2]) {
        self.gl.use_program(Some(&self.texture_program));
        self.gl.bind_vertex_array(Some(&self.texture_vao));
        uniform2f(
            &self.gl,
            &self.texture_uniforms,
            "u_viewport",
            viewport[0],
            viewport[1],
        );
        self.gl.active_texture(Gl::TEXTURE0);
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(texture));
        uniform1i(&self.gl, &self.texture_uniforms, "u_texture", 0);
    }

    fn draw_sprite_instances(&self, instances: &[f32], count: usize) -> Result<()> {
        anyhow::ensure!(
            instances.len() == count * SPRITE_INSTANCE_FLOATS,
            "invalid browser sprite instance payload"
        );
        let count = i32::try_from(count).context("browser sprite batch is too large")?;
        self.gl
            .bind_buffer(Gl::ARRAY_BUFFER, Some(&self.texture_instance_buffer));
        unsafe {
            let array = Float32Array::view(instances);
            self.gl
                .buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &array, Gl::STREAM_DRAW);
        }
        self.gl
            .draw_arrays_instanced(Gl::TRIANGLE_STRIP, 0, 4, count);
        Ok(())
    }

    fn texture(&mut self, tile: &AtlasTile) -> Result<(WebGlTexture, Size<DevicePixels>)> {
        let id = tile.texture_id;
        let known_revision = self.textures.get(&id).map(|cached| cached.revision);
        let page_revision = self
            .atlas
            .page_revision(id)
            .with_context(|| format!("browser atlas texture {id:?} is unavailable"))?;
        if known_revision != Some(page_revision) {
            let upload = self
                .atlas
                .upload(id, known_revision)?
                .context("browser atlas revision changed without upload data")?;
            if let Some(cached) = self.textures.get_mut(&id) {
                update_texture(&self.gl, cached, &upload)?;
            } else {
                let texture = upload_texture(&self.gl, &upload)?;
                self.textures.insert(
                    id,
                    CachedTexture {
                        texture,
                        revision: upload.revision,
                        size: upload.page_size,
                    },
                );
            }
            self.atlas.acknowledge_upload(id, upload.revision);
        }
        let cached = self
            .textures
            .get(&id)
            .context("browser atlas texture was not cached after upload")?;
        Ok((cached.texture.clone(), cached.size))
    }

    fn begin_path_batch(&self, viewport: [f32; 2]) {
        self.gl.use_program(Some(&self.path_program));
        self.gl.bind_vertex_array(Some(&self.path_vao));
        uniform2f(
            &self.gl,
            &self.path_uniforms,
            "u_viewport",
            viewport[0],
            viewport[1],
        );
    }

    fn draw_path(&mut self, path: &Path<ScaledPixels>) {
        if path.vertices.is_empty() {
            return;
        }
        uniform4f_array(
            &self.gl,
            &self.path_uniforms,
            "u_content_mask",
            bounds(path.content_mask.bounds),
        );
        uniform4f_array(
            &self.gl,
            &self.path_uniforms,
            "u_color",
            representative_color(&path.color),
        );

        for range in draw_ranges(path.vertices.len(), 4_095) {
            let vertex_count = range.len();
            let required_floats = vertex_count.saturating_mul(4);
            self.path_vertices.clear();
            self.path_vertices.reserve(required_floats);
            for vertex in &path.vertices[range] {
                self.path_vertices.extend_from_slice(&[
                    vertex.xy_position.x.0,
                    vertex.xy_position.y.0,
                    vertex.st_position.x,
                    vertex.st_position.y,
                ]);
            }
            self.gl
                .bind_buffer(Gl::ARRAY_BUFFER, Some(&self.path_buffer));
            unsafe {
                let array = Float32Array::view(&self.path_vertices);
                self.gl.buffer_data_with_array_buffer_view(
                    Gl::ARRAY_BUFFER,
                    &array,
                    Gl::STREAM_DRAW,
                );
            }
            self.gl.draw_arrays(Gl::TRIANGLES, 0, vertex_count as i32);
        }
    }
}

fn upload_texture(gl: &Gl, upload: &WebAtlasUpload) -> Result<WebGlTexture> {
    let texture = gl
        .create_texture()
        .context("failed to create browser atlas texture")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
    let (internal_format, format) = texture_format(upload.kind);
    anyhow::ensure!(
        upload.bounds.origin.x.0 >= 0
            && upload.bounds.origin.y.0 >= 0
            && upload.bounds.size.width.0 > 0
            && upload.bounds.size.height.0 > 0
            && upload.bounds.origin.x.0 + upload.bounds.size.width.0 <= upload.page_size.width.0
            && upload.bounds.origin.y.0 + upload.bounds.size.height.0 <= upload.page_size.height.0,
        "new browser atlas upload is outside its page"
    );
    // Allocate the complete page without copying a zero-filled 512–1024px CPU buffer through
    // wasm-bindgen. When the first live region does not cover the page, initialize the rest with
    // one GPU clear. Firefox otherwise performs the same clear lazily on the first partial upload,
    // producing a warning and a less predictable render-thread stall.
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        internal_format,
        upload.page_size.width.0,
        upload.page_size.height.0,
        0,
        format,
        Gl::UNSIGNED_BYTE,
        None,
    )
    .map_err(js_error)?;
    if upload.bounds.origin.x.0 != 0
        || upload.bounds.origin.y.0 != 0
        || upload.bounds.size != upload.page_size
    {
        clear_new_texture_storage(gl, &texture)?;
    }
    gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        upload.bounds.origin.x.0,
        upload.bounds.origin.y.0,
        upload.bounds.size.width.0,
        upload.bounds.size.height.0,
        format,
        Gl::UNSIGNED_BYTE,
        Some(&upload.bytes),
    )
    .map_err(js_error)?;
    Ok(texture)
}

fn clear_new_texture_storage(gl: &Gl, texture: &WebGlTexture) -> Result<()> {
    let framebuffer: WebGlFramebuffer = gl
        .create_framebuffer()
        .context("failed to create browser atlas initialization framebuffer")?;
    gl.bind_framebuffer(Gl::FRAMEBUFFER, Some(&framebuffer));
    gl.framebuffer_texture_2d(
        Gl::FRAMEBUFFER,
        Gl::COLOR_ATTACHMENT0,
        Gl::TEXTURE_2D,
        Some(texture),
        0,
    );
    let framebuffer_status = gl.check_framebuffer_status(Gl::FRAMEBUFFER);
    if framebuffer_status == Gl::FRAMEBUFFER_COMPLETE {
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear(Gl::COLOR_BUFFER_BIT);
    }
    gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
    gl.delete_framebuffer(Some(&framebuffer));
    // Full-scene draws use opaque black as their clear color. Restore that
    // owned WebGL state so atlas initialization cannot influence later frames.
    gl.clear_color(0.0, 0.0, 0.0, 1.0);
    anyhow::ensure!(
        framebuffer_status == Gl::FRAMEBUFFER_COMPLETE,
        "browser atlas initialization framebuffer is incomplete (0x{framebuffer_status:04x})"
    );
    Ok(())
}

fn update_texture(gl: &Gl, cached: &mut CachedTexture, upload: &WebAtlasUpload) -> Result<()> {
    anyhow::ensure!(
        cached.size == upload.page_size,
        "browser atlas page changed size without changing texture id"
    );
    gl.bind_texture(Gl::TEXTURE_2D, Some(&cached.texture));
    let (_, format) = texture_format(upload.kind);
    gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        upload.bounds.origin.x.0,
        upload.bounds.origin.y.0,
        upload.bounds.size.width.0,
        upload.bounds.size.height.0,
        format,
        Gl::UNSIGNED_BYTE,
        Some(&upload.bytes),
    )
    .map_err(js_error)?;
    cached.revision = upload.revision;
    Ok(())
}

fn texture_format(kind: AtlasTextureKind) -> (i32, u32) {
    match kind {
        AtlasTextureKind::Monochrome => (Gl::R8 as i32, Gl::RED),
        AtlasTextureKind::Polychrome => (Gl::RGBA8 as i32, Gl::RGBA),
    }
}

fn set_transform(gl: &Gl, uniforms: &UniformCache, transform: TransformationMatrix) {
    if let Some(location) = uniforms.get("u_transform") {
        gl.uniform_matrix2fv_with_f32_array(
            Some(location),
            false,
            &[
                transform.rotation_scale[0][0],
                transform.rotation_scale[1][0],
                transform.rotation_scale[0][1],
                transform.rotation_scale[1][1],
            ],
        );
    }
    uniform2f(
        gl,
        uniforms,
        "u_translation",
        transform.translation[0],
        transform.translation[1],
    );
}

fn set_background(gl: &Gl, uniforms: &UniformCache, background: &Background) {
    uniform1i(gl, uniforms, "u_fill_kind", background.tag as i32);
    uniform1f(
        gl,
        uniforms,
        "u_fill_angle",
        background.gradient_angle_or_pattern_height,
    );
    uniform2f(
        gl,
        uniforms,
        "u_fill_center",
        background.center[0],
        background.center[1],
    );
    uniform2f(
        gl,
        uniforms,
        "u_fill_radius",
        background.radius[0].max(0.0001),
        background.radius[1].max(0.0001),
    );
    let mut colors = [0.0f32; 32];
    let mut stops = [0.0f32; 8];
    let count = if matches!(background.tag, BackgroundTag::Solid) {
        colors[..4].copy_from_slice(&rgba_components(background.solid));
        1
    } else {
        let count = (background.stop_count as usize).clamp(1, 8);
        for (index, stop) in background.colors[..count].iter().enumerate() {
            colors[index * 4..index * 4 + 4].copy_from_slice(&rgba_components(stop.color));
            stops[index] = stop.percentage;
        }
        count
    };
    uniform1i(gl, uniforms, "u_fill_count", count as i32);
    if let Some(location) = uniforms.get("u_fill_colors[0]") {
        gl.uniform4fv_with_f32_array(Some(location), &colors);
    }
    if let Some(location) = uniforms.get("u_fill_stops[0]") {
        gl.uniform1fv_with_f32_array(Some(location), &stops);
    }
}

fn set_solid_background(gl: &Gl, uniforms: &UniformCache, color: Hsla) {
    uniform1i(gl, uniforms, "u_fill_kind", BackgroundTag::Solid as i32);
    uniform1i(gl, uniforms, "u_fill_count", 1);
    let mut colors = [0.0f32; 32];
    colors[..4].copy_from_slice(&rgba_components(color));
    if let Some(location) = uniforms.get("u_fill_colors[0]") {
        gl.uniform4fv_with_f32_array(Some(location), &colors);
    }
}

fn representative_color(background: &Background) -> [f32; 4] {
    if matches!(background.tag, BackgroundTag::Solid) || background.stop_count == 0 {
        rgba_components(background.solid)
    } else {
        rgba_components(background.colors[0].color)
    }
}

fn set_color_filter(gl: &Gl, uniforms: &UniformCache, filter: ColorFilter) {
    uniform4f_array(
        gl,
        uniforms,
        "u_color_filter",
        [
            filter.grayscale,
            filter.saturate,
            filter.brightness,
            filter.contrast,
        ],
    );
}

fn bounds(bounds: Bounds<ScaledPixels>) -> [f32; 4] {
    [
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
    ]
}

fn tile_uv_bounds(tile: &AtlasTile, page_size: Size<DevicePixels>) -> [f32; 4] {
    let page_width = page_size.width.0 as f32;
    let page_height = page_size.height.0 as f32;
    [
        tile.bounds.origin.x.0 as f32 / page_width,
        tile.bounds.origin.y.0 as f32 / page_height,
        tile.bounds.size.width.0 as f32 / page_width,
        tile.bounds.size.height.0 as f32 / page_height,
    ]
}

fn append_solid_quad_instance(instances: &mut Vec<f32>, quad: &Quad) {
    let start = instances.len();
    instances.extend_from_slice(&bounds(quad.bounds));
    instances.extend_from_slice(&bounds(quad.content_mask.bounds));
    instances.extend_from_slice(&corners(quad.corner_radii));
    instances.extend_from_slice(&bounds(quad.rounded_clip_bounds));
    instances.extend_from_slice(&corners(quad.rounded_clip_radii));
    instances.extend_from_slice(&[
        quad.transform.rotation_scale[0][0],
        quad.transform.rotation_scale[1][0],
        quad.transform.rotation_scale[0][1],
        quad.transform.rotation_scale[1][1],
    ]);
    instances.extend_from_slice(&quad.transform.translation);
    instances.extend_from_slice(&edges(quad.border_widths));
    instances.extend_from_slice(&rgba_components(quad.background.solid));
    instances.extend_from_slice(&rgba_components(quad.border_color.solid));
    instances.extend_from_slice(&[
        quad.color_filter.grayscale,
        quad.color_filter.saturate,
        quad.color_filter.brightness,
        quad.color_filter.contrast,
    ]);
    instances.extend_from_slice(&[
        f32::from(matches!(quad.border_style, crate::BorderStyle::Dashed)),
        f32::from(corners_have_radius(quad.corner_radii)),
        f32::from(bounds_are_active(quad.rounded_clip_bounds)),
        f32::from(edges_have_width(quad.border_widths)),
    ]);
    debug_assert_eq!(instances.len() - start, SOLID_QUAD_INSTANCE_FLOATS);
}

#[allow(clippy::too_many_arguments)]
fn append_sprite_instance(
    instances: &mut Vec<f32>,
    sprite_bounds: Bounds<ScaledPixels>,
    content_mask: Bounds<ScaledPixels>,
    corner_radii: Corners<ScaledPixels>,
    rounded_clip_bounds: Bounds<ScaledPixels>,
    rounded_clip_radii: Corners<ScaledPixels>,
    transform: TransformationMatrix,
    uv_bounds: [f32; 4],
    tint: [f32; 4],
    filter: ColorFilter,
    options: [f32; 4],
) {
    let start = instances.len();
    instances.extend_from_slice(&bounds(sprite_bounds));
    instances.extend_from_slice(&bounds(content_mask));
    instances.extend_from_slice(&corners(corner_radii));
    instances.extend_from_slice(&bounds(rounded_clip_bounds));
    instances.extend_from_slice(&corners(rounded_clip_radii));
    instances.extend_from_slice(&[
        transform.rotation_scale[0][0],
        transform.rotation_scale[1][0],
        transform.rotation_scale[0][1],
        transform.rotation_scale[1][1],
    ]);
    instances.extend_from_slice(&transform.translation);
    instances.extend_from_slice(&uv_bounds);
    instances.extend_from_slice(&tint);
    instances.extend_from_slice(&[
        filter.grayscale,
        filter.saturate,
        filter.brightness,
        filter.contrast,
    ]);
    instances.extend_from_slice(&options);
    debug_assert_eq!(instances.len() - start, SPRITE_INSTANCE_FLOATS);
}

fn corners(corners: Corners<ScaledPixels>) -> [f32; 4] {
    [
        corners.top_left.0,
        corners.top_right.0,
        corners.bottom_right.0,
        corners.bottom_left.0,
    ]
}

fn edges(edges: Edges<ScaledPixels>) -> [f32; 4] {
    [edges.top.0, edges.right.0, edges.bottom.0, edges.left.0]
}

fn corners_have_radius(value: Corners<ScaledPixels>) -> bool {
    corners(value).into_iter().any(|radius| radius > 0.0)
}

fn edges_have_width(value: Edges<ScaledPixels>) -> bool {
    edges(value).into_iter().any(|width| width > 0.0)
}

fn bounds_are_active(bounds: Bounds<ScaledPixels>) -> bool {
    bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0
}

fn uniform1i(gl: &Gl, uniforms: &UniformCache, name: &'static str, value: i32) {
    gl.uniform1i(uniforms.get(name), value);
}

fn uniform1f(gl: &Gl, uniforms: &UniformCache, name: &'static str, value: f32) {
    gl.uniform1f(uniforms.get(name), value);
}

fn uniform2f(gl: &Gl, uniforms: &UniformCache, name: &'static str, x: f32, y: f32) {
    gl.uniform2f(uniforms.get(name), x, y);
}

fn uniform4f_array(gl: &Gl, uniforms: &UniformCache, name: &'static str, value: [f32; 4]) {
    gl.uniform4f(uniforms.get(name), value[0], value[1], value[2], value[3]);
}

fn link_program(gl: &Gl, vertex_source: &str, fragment_source: &str) -> Result<WebGlProgram> {
    let vertex = compile_shader(gl, Gl::VERTEX_SHADER, vertex_source)?;
    let fragment = compile_shader(gl, Gl::FRAGMENT_SHADER, fragment_source)?;
    let program = gl
        .create_program()
        .context("failed to create WebGL2 program")?;
    gl.attach_shader(&program, &vertex);
    gl.attach_shader(&program, &fragment);
    gl.link_program(&program);
    let linked = gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        .unwrap_or(false);
    let log = gl.get_program_info_log(&program).unwrap_or_default();
    gl.delete_shader(Some(&vertex));
    gl.delete_shader(Some(&fragment));
    anyhow::ensure!(linked, "WebGL2 program link failed: {log}");
    Ok(program)
}

fn compile_shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader> {
    let shader = gl
        .create_shader(kind)
        .context("failed to create WebGL2 shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    let compiled = gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false);
    let log = gl.get_shader_info_log(&shader).unwrap_or_default();
    anyhow::ensure!(compiled, "WebGL2 shader compile failed: {log}");
    Ok(shader)
}

fn js_error(value: wasm_bindgen::JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

const QUAD_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_unit;
uniform vec2 u_viewport;
uniform vec4 u_vertex_bounds;
uniform mat2 u_transform;
uniform vec2 u_translation;
out vec2 v_unit;
out vec2 v_local;
out vec2 v_world;
void main() {
    v_unit = a_unit;
    v_local = u_vertex_bounds.xy + a_unit * u_vertex_bounds.zw;
    v_world = u_transform * v_local + u_translation;
    vec2 clip = v_world / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    gl_Position = vec4(clip, 0.0, 1.0);
}
"#;

const SOLID_QUAD_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_unit;
layout(location = 1) in vec4 a_vertex_bounds;
layout(location = 2) in vec4 a_content_mask;
layout(location = 3) in vec4 a_corner_radii;
layout(location = 4) in vec4 a_rounded_clip_bounds;
layout(location = 5) in vec4 a_rounded_clip_radii;
layout(location = 6) in vec4 a_transform;
layout(location = 7) in vec2 a_translation;
layout(location = 8) in vec4 a_border_widths;
layout(location = 9) in vec4 a_fill_color;
layout(location = 10) in vec4 a_border_color;
layout(location = 11) in vec4 a_color_filter;
layout(location = 12) in vec4 a_options;
uniform vec2 u_viewport;
out vec2 v_local;
out vec2 v_world;
flat out vec4 v_shape_bounds;
flat out vec4 v_content_mask;
flat out vec4 v_corner_radii;
flat out vec4 v_rounded_clip_bounds;
flat out vec4 v_rounded_clip_radii;
flat out vec4 v_border_widths;
flat out vec4 v_fill_color;
flat out vec4 v_border_color;
flat out vec4 v_color_filter;
flat out vec4 v_options;
void main() {
    v_local = a_vertex_bounds.xy + a_unit * a_vertex_bounds.zw;
    mat2 transform = mat2(a_transform);
    v_world = transform * v_local + a_translation;
    v_shape_bounds = a_vertex_bounds;
    v_content_mask = a_content_mask;
    v_corner_radii = a_corner_radii;
    v_rounded_clip_bounds = a_rounded_clip_bounds;
    v_rounded_clip_radii = a_rounded_clip_radii;
    v_border_widths = a_border_widths;
    v_fill_color = a_fill_color;
    v_border_color = a_border_color;
    v_color_filter = a_color_filter;
    v_options = a_options;
    vec2 clip = v_world / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    gl_Position = vec4(clip, 0.0, 1.0);
}
"#;

const SOLID_QUAD_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
in vec2 v_local;
in vec2 v_world;
flat in vec4 v_shape_bounds;
flat in vec4 v_content_mask;
flat in vec4 v_corner_radii;
flat in vec4 v_rounded_clip_bounds;
flat in vec4 v_rounded_clip_radii;
flat in vec4 v_border_widths;
flat in vec4 v_fill_color;
flat in vec4 v_border_color;
flat in vec4 v_color_filter;
flat in vec4 v_options;
out vec4 out_color;

float rounded_sdf(vec2 p, vec4 bounds, vec4 radii) {
    vec2 center = bounds.xy + bounds.zw * 0.5;
    vec2 local = p - center;
    float radius = local.y < 0.0
        ? (local.x < 0.0 ? radii.x : radii.y)
        : (local.x < 0.0 ? radii.w : radii.z);
    radius = clamp(radius, 0.0, min(bounds.z, bounds.w) * 0.5);
    vec2 q = abs(local) - bounds.zw * 0.5 + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}

float rect_mask(vec2 p, vec4 bounds) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    return step(bounds.x, p.x) * step(bounds.y, p.y)
        * step(p.x, bounds.x + bounds.z) * step(p.y, bounds.y + bounds.w);
}

float rounded_mask(vec2 p, vec4 bounds, vec4 radii) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    float distance = rounded_sdf(p, bounds, radii);
    float aa = max(fwidth(distance), 0.75);
    return 1.0 - smoothstep(-aa, aa, distance);
}

vec4 filtered(vec4 color) {
    color.rgb = (color.rgb - 0.5) * v_color_filter.w + 0.5;
    color.rgb *= v_color_filter.z;
    float luminance = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = mix(vec3(luminance), color.rgb, v_color_filter.y);
    color.rgb = mix(color.rgb, vec3(luminance), v_color_filter.x);
    return clamp(color, 0.0, 1.0);
}

void main() {
    float clip = rect_mask(v_world, v_content_mask);
    if (v_options.z > 0.5) {
        bool rounded_clip = any(greaterThan(v_rounded_clip_radii, vec4(0.0)));
        clip *= rounded_clip
            ? rounded_mask(v_world, v_rounded_clip_bounds, v_rounded_clip_radii)
            : rect_mask(v_world, v_rounded_clip_bounds);
    }
    if (clip <= 0.0) discard;

    float outer = 1.0;
    float aa = 0.75;
    if (v_options.y > 0.5) {
        float distance = rounded_sdf(v_local, v_shape_bounds, v_corner_radii);
        aa = max(fwidth(distance), 0.75);
        outer = 1.0 - smoothstep(-aa, aa, distance);
        if (outer <= 0.0) discard;
    }

    float inner = outer;
    float border_factor = 0.0;
    if (v_options.w > 0.5) {
        vec4 inner_bounds = vec4(
            v_shape_bounds.x + v_border_widths.w,
            v_shape_bounds.y + v_border_widths.x,
            max(0.0, v_shape_bounds.z - v_border_widths.w - v_border_widths.y),
            max(0.0, v_shape_bounds.w - v_border_widths.x - v_border_widths.z)
        );
        float border_max = max(max(v_border_widths.x, v_border_widths.y),
            max(v_border_widths.z, v_border_widths.w));
        if (v_options.y > 0.5) {
            vec4 inner_radii = max(v_corner_radii - vec4(border_max), vec4(0.0));
            float inner_distance = rounded_sdf(v_local, inner_bounds, inner_radii);
            inner = 1.0 - smoothstep(-aa, aa, inner_distance);
        } else {
            inner = rect_mask(v_local, inner_bounds);
        }
        border_factor = max(outer - inner, 0.0);
        if (v_options.x > 0.5) {
            float perimeter_axis = abs(v_local.x - v_shape_bounds.x) < border_max
                || abs(v_local.x - (v_shape_bounds.x + v_shape_bounds.z)) < border_max
                ? v_local.y : v_local.x;
            border_factor *= step(0.5, fract(perimeter_axis / max(border_max * 3.0, 3.0)));
        }
    }

    vec4 color = v_fill_color * inner + v_border_color * border_factor;
    if (!all(equal(v_color_filter, vec4(0.0, 1.0, 1.0, 1.0)))) {
        color = filtered(color);
    }
    color.a *= clip;
    out_color = vec4(color.rgb * color.a, color.a);
}
"#;

const TEXTURE_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_unit;
layout(location = 1) in vec4 a_vertex_bounds;
layout(location = 2) in vec4 a_content_mask;
layout(location = 3) in vec4 a_corner_radii;
layout(location = 4) in vec4 a_rounded_clip_bounds;
layout(location = 5) in vec4 a_rounded_clip_radii;
layout(location = 6) in vec4 a_transform;
layout(location = 7) in vec2 a_translation;
layout(location = 8) in vec4 a_uv_bounds;
layout(location = 9) in vec4 a_tint;
layout(location = 10) in vec4 a_color_filter;
layout(location = 11) in vec4 a_options;
uniform vec2 u_viewport;
out vec2 v_unit;
out vec2 v_local;
out vec2 v_world;
flat out vec4 v_shape_bounds;
flat out vec4 v_content_mask;
flat out vec4 v_corner_radii;
flat out vec4 v_rounded_clip_bounds;
flat out vec4 v_rounded_clip_radii;
flat out vec4 v_uv_bounds;
flat out vec4 v_tint;
flat out vec4 v_color_filter;
flat out vec4 v_options;
void main() {
    v_unit = a_unit;
    v_local = a_vertex_bounds.xy + a_unit * a_vertex_bounds.zw;
    mat2 transform = mat2(a_transform);
    v_world = transform * v_local + a_translation;
    v_shape_bounds = a_vertex_bounds;
    v_content_mask = a_content_mask;
    v_corner_radii = a_corner_radii;
    v_rounded_clip_bounds = a_rounded_clip_bounds;
    v_rounded_clip_radii = a_rounded_clip_radii;
    v_uv_bounds = a_uv_bounds;
    v_tint = a_tint;
    v_color_filter = a_color_filter;
    v_options = a_options;
    vec2 clip = v_world / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    gl_Position = vec4(clip, 0.0, 1.0);
}
"#;

const SHAPE_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
in vec2 v_unit;
in vec2 v_local;
in vec2 v_world;
out vec4 out_color;
uniform int u_mode;
uniform vec4 u_shape_bounds;
uniform vec4 u_content_mask;
uniform vec4 u_corner_radii;
uniform vec4 u_border_widths;
uniform int u_border_dashed;
uniform vec4 u_rounded_clip_bounds;
uniform vec4 u_rounded_clip_radii;
uniform int u_fill_kind;
uniform int u_fill_count;
uniform vec4 u_fill_colors[8];
uniform float u_fill_stops[8];
uniform float u_fill_angle;
uniform vec2 u_fill_center;
uniform vec2 u_fill_radius;
uniform vec4 u_border_color;
uniform float u_shadow_blur;
uniform int u_shadow_inset;
uniform vec4 u_shadow_color;
uniform vec4 u_color_filter;
const float PI = 3.14159265359;

float rounded_sdf(vec2 p, vec4 bounds, vec4 radii) {
    vec2 center = bounds.xy + bounds.zw * 0.5;
    vec2 local = p - center;
    float radius = local.y < 0.0
        ? (local.x < 0.0 ? radii.x : radii.y)
        : (local.x < 0.0 ? radii.w : radii.z);
    radius = clamp(radius, 0.0, min(bounds.z, bounds.w) * 0.5);
    vec2 q = abs(local) - bounds.zw * 0.5 + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}

float rect_mask(vec2 p, vec4 bounds) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    return step(bounds.x, p.x) * step(bounds.y, p.y)
        * step(p.x, bounds.x + bounds.z) * step(p.y, bounds.y + bounds.w);
}

float rounded_mask(vec2 p, vec4 bounds, vec4 radii) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    float distance = rounded_sdf(p, bounds, radii);
    float aa = max(fwidth(distance), 0.75);
    return 1.0 - smoothstep(-aa, aa, distance);
}

vec4 filtered(vec4 color) {
    color.rgb = (color.rgb - 0.5) * u_color_filter.w + 0.5;
    color.rgb *= u_color_filter.z;
    float luminance = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = mix(vec3(luminance), color.rgb, u_color_filter.y);
    color.rgb = mix(color.rgb, vec3(luminance), u_color_filter.x);
    return clamp(color, 0.0, 1.0);
}

vec4 gradient(float t) {
    t = clamp(t, 0.0, 1.0);
    vec4 color = u_fill_colors[0];
    for (int index = 0; index < 7; index++) {
        if (index + 1 >= u_fill_count) break;
        float start = u_fill_stops[index];
        float end = u_fill_stops[index + 1];
        if (t >= start) {
            float local_t = clamp((t - start) / max(end - start, 0.00001), 0.0, 1.0);
            color = mix(u_fill_colors[index], u_fill_colors[index + 1], local_t);
        }
    }
    return color;
}

vec4 fill_color() {
    if (u_fill_kind == 0) return u_fill_colors[0];
    vec2 relative = (v_local - u_shape_bounds.xy) / max(u_shape_bounds.zw, vec2(0.0001));
    if (u_fill_kind == 1) {
        float radians = (mod(u_fill_angle, 360.0) - 90.0) * PI / 180.0;
        vec2 direction = vec2(cos(radians), sin(radians));
        float t = dot(relative - 0.5, direction) + 0.5;
        return gradient(t);
    }
    if (u_fill_kind == 2) {
        float packed = u_fill_angle;
        float width = floor(packed / 65535.0) / 255.0;
        float interval = mod(packed, 65535.0) / 255.0;
        float period = max((width + interval) * 0.70710678, 0.5);
        float stripe = mod((v_local.x + v_local.y) * 0.70710678, period);
        vec4 color = u_fill_colors[0];
        color.a *= 1.0 - smoothstep(width * 0.5, width * 0.5 + 1.0, stripe);
        return color;
    }
    if (u_fill_kind == 3) {
        vec2 delta = (relative - u_fill_center) / max(u_fill_radius, vec2(0.0001));
        return gradient(length(delta));
    }
    vec2 delta = relative - u_fill_center;
    float t = fract((atan(delta.y, delta.x) + PI + u_fill_angle * PI / 180.0) / (2.0 * PI));
    return gradient(t);
}

void main() {
    float clip = rect_mask(v_world, u_content_mask)
        * rounded_mask(v_world, u_rounded_clip_bounds, u_rounded_clip_radii);
    if (clip <= 0.0) discard;
    float distance = rounded_sdf(v_local, u_shape_bounds, u_corner_radii);
    if (u_mode == 1) {
        float sigma = max(u_shadow_blur, 0.5);
        float alpha;
        if (u_shadow_inset != 0) {
            alpha = (1.0 - exp(-0.5 * pow(max(-distance, 0.0) / sigma, 2.0)))
                * step(distance, 0.0);
        } else {
            alpha = exp(-0.5 * pow(max(distance, 0.0) / sigma, 2.0));
        }
        vec4 color = filtered(u_shadow_color);
        color.a *= alpha * clip;
        out_color = vec4(color.rgb * color.a, color.a);
        return;
    }

    float aa = max(fwidth(distance), 0.75);
    float outer = 1.0 - smoothstep(-aa, aa, distance);
    if (outer <= 0.0) discard;
    vec4 inner_bounds = vec4(
        u_shape_bounds.x + u_border_widths.w,
        u_shape_bounds.y + u_border_widths.x,
        max(0.0, u_shape_bounds.z - u_border_widths.w - u_border_widths.y),
        max(0.0, u_shape_bounds.w - u_border_widths.x - u_border_widths.z)
    );
    float border_max = max(max(u_border_widths.x, u_border_widths.y),
        max(u_border_widths.z, u_border_widths.w));
    vec4 inner_radii = max(u_corner_radii - vec4(border_max), vec4(0.0));
    float inner_distance = rounded_sdf(v_local, inner_bounds, inner_radii);
    float inner = border_max > 0.0 ? 1.0 - smoothstep(-aa, aa, inner_distance) : outer;
    float border_factor = max(outer - inner, 0.0);
    if (u_border_dashed != 0) {
        float perimeter_axis = abs(v_local.x - u_shape_bounds.x) < border_max
            || abs(v_local.x - (u_shape_bounds.x + u_shape_bounds.z)) < border_max
            ? v_local.y : v_local.x;
        border_factor *= step(0.5, fract(perimeter_axis / max(border_max * 3.0, 3.0)));
    }
    vec4 color = fill_color() * inner + u_border_color * border_factor;
    color = filtered(color);
    color.a *= clip;
    out_color = vec4(color.rgb * color.a, color.a);
}
"#;

const TEXTURE_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
in vec2 v_unit;
in vec2 v_local;
in vec2 v_world;
flat in vec4 v_shape_bounds;
flat in vec4 v_content_mask;
flat in vec4 v_corner_radii;
flat in vec4 v_rounded_clip_bounds;
flat in vec4 v_rounded_clip_radii;
flat in vec4 v_uv_bounds;
flat in vec4 v_tint;
flat in vec4 v_color_filter;
flat in vec4 v_options;
out vec4 out_color;
uniform sampler2D u_texture;

float rounded_sdf(vec2 p, vec4 bounds, vec4 radii) {
    vec2 center = bounds.xy + bounds.zw * 0.5;
    vec2 local = p - center;
    float radius = local.y < 0.0
        ? (local.x < 0.0 ? radii.x : radii.y)
        : (local.x < 0.0 ? radii.w : radii.z);
    radius = clamp(radius, 0.0, min(bounds.z, bounds.w) * 0.5);
    vec2 q = abs(local) - bounds.zw * 0.5 + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}
float rect_mask(vec2 p, vec4 bounds) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    return step(bounds.x, p.x) * step(bounds.y, p.y)
        * step(p.x, bounds.x + bounds.z) * step(p.y, bounds.y + bounds.w);
}
float rounded_mask(vec2 p, vec4 bounds, vec4 radii) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    float distance = rounded_sdf(p, bounds, radii);
    float aa = max(fwidth(distance), 0.75);
    return 1.0 - smoothstep(-aa, aa, distance);
}
vec4 filtered(vec4 color) {
    color.rgb = (color.rgb - 0.5) * v_color_filter.w + 0.5;
    color.rgb *= v_color_filter.z;
    float luminance = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    color.rgb = mix(vec3(luminance), color.rgb, v_color_filter.y);
    color.rgb = mix(color.rgb, vec3(luminance), v_color_filter.x);
    return clamp(color, 0.0, 1.0);
}
void main() {
    float clip = rect_mask(v_world, v_content_mask)
        * rounded_mask(v_world, v_rounded_clip_bounds, v_rounded_clip_radii)
        * rounded_mask(v_local, v_shape_bounds, v_corner_radii);
    if (clip <= 0.0) discard;
    vec4 sample_color = texture(u_texture, v_uv_bounds.xy + v_unit * v_uv_bounds.zw);
    vec4 color;
    if (v_options.x > 0.5) {
        float alpha = sample_color.r * v_tint.a;
        color = vec4(v_tint.rgb * alpha, alpha);
    } else {
        // Kael's cross-platform polychrome atlas contract stores BGRA bytes.
        sample_color = sample_color.bgra;
        if (v_options.z > 0.5) {
            float luminance = dot(sample_color.rgb, vec3(0.2126, 0.7152, 0.0722));
            sample_color.rgb = vec3(luminance);
        }
        color = sample_color;
        if (v_options.y < 0.5) color.rgb *= color.a;
    }
    color = filtered(color);
    color *= v_options.w * clip;
    out_color = color;
}
"#;

const PATH_VERTEX_SHADER: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_st;
uniform vec2 u_viewport;
out vec2 v_position;
out vec2 v_st;
void main() {
    v_position = a_position;
    v_st = a_st;
    vec2 clip = a_position / u_viewport * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    gl_Position = vec4(clip, 0.0, 1.0);
}
"#;

const PATH_FRAGMENT_SHADER: &str = r#"#version 300 es
precision highp float;
in vec2 v_position;
in vec2 v_st;
out vec4 out_color;
uniform vec4 u_content_mask;
uniform vec4 u_color;
void main() {
    if (v_position.x < u_content_mask.x || v_position.y < u_content_mask.y
        || v_position.x > u_content_mask.x + u_content_mask.z
        || v_position.y > u_content_mask.y + u_content_mask.w) discard;
    vec2 dx = dFdx(v_st);
    vec2 dy = dFdy(v_st);
    float alpha = 1.0;
    if (length(vec2(dx.x, dy.x)) >= 0.001) {
        vec2 gradient = 2.0 * v_st.xx * vec2(dx.x, dy.x) - vec2(dx.y, dy.y);
        float distance = (v_st.x * v_st.x - v_st.y) / max(length(gradient), 0.0001);
        alpha = clamp(0.5 - distance, 0.0, 1.0);
    }
    float out_alpha = u_color.a * alpha;
    out_color = vec4(u_color.rgb * out_alpha, out_alpha);
}
"#;
