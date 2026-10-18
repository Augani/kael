//! WebGL2 custom fragments and associated-color GPU-to-UI composition.
use crate::render_target::{RenderTargetDisplayParams, TargetRegistry};
use crate::{
    DevicePixels, PaintSurface, RenderTarget, RenderTargetDescriptor, RenderTargetError,
    RenderTargetFormat, RenderTargetReadback, ShaderBackend, ShaderBinding, ShaderBindings,
    ShaderHandle, ShaderResourceKind, ShaderResourceSlotKind, ShaderSampler,
    ShaderTextureSamplerPair, Size,
};
use std::collections::BTreeMap;
use wasm_bindgen::JsCast as _;
use web_sys::{
    WebGl2RenderingContext as Gl, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlSampler,
    WebGlTexture, WebGlVertexArrayObject,
};

type Result<T> = std::result::Result<T, RenderTargetError>;
const MAX_PIPELINES: usize = 64;
const MAX_READBACK: u64 = 256 * 1024 * 1024;
struct Target {
    image: WebGlTexture,
    framebuffer: WebGlFramebuffer,
}
struct Uniform {
    buffer: WebGlBuffer,
    slot: u32,
}
struct Pipeline {
    program: WebGlProgram,
    uniforms: BTreeMap<u32, Uniform>,
    pairs: Vec<ShaderTextureSamplerPair>,
    used: u64,
}
struct Display {
    program: WebGlProgram,
    buffer: WebGlBuffer,
}

pub(super) struct WebCustomRenderer {
    gl: Gl,
    targets: TargetRegistry<Target>,
    pipelines: BTreeMap<(u64, RenderTargetFormat), Pipeline>,
    samplers: BTreeMap<ShaderSampler, WebGlSampler>,
    vao: WebGlVertexArrayObject,
    display: Option<Display>,
    tick: u64,
    #[cfg(test)]
    compilations: u32,
}
impl WebCustomRenderer {
    pub(super) fn new(gl: &Gl) -> Result<Self> {
        let vao = gl
            .create_vertex_array()
            .ok_or_else(|| backend("failed to create custom vertex array"))?;
        Ok(Self {
            gl: gl.clone(),
            targets: Default::default(),
            pipelines: Default::default(),
            samplers: Default::default(),
            vao,
            display: None,
            tick: 0,
            #[cfg(test)]
            compilations: 0,
        })
    }
    pub(super) fn invalidate(&mut self) {
        self.targets.invalidate_device();
        for target in self.targets.invalidate_and_drain() {
            destroy_target(&self.gl, target);
        }
    }
    fn live(&mut self) -> Result<()> {
        if self.gl.is_context_lost() {
            // Restoration constructs a fresh renderer/owner; old handles must
            // become invalid as soon as the loss is observed.
            self.invalidate();
            return Err(RenderTargetError::WrongDevice);
        }
        Ok(())
    }
    fn prune(&mut self) {
        for target in self.targets.take_unused() {
            destroy_target(&self.gl, target);
        }
    }
    pub(super) fn validate(&self, target: &RenderTarget) -> Result<()> {
        if self.gl.is_context_lost() {
            return Err(RenderTargetError::WrongDevice);
        }
        self.targets.get(target).map(|_| ())
    }
    pub(super) fn set_budget(&mut self, bytes: u64) {
        self.targets.set_budget(bytes);
        self.prune();
    }
    pub(super) fn shed_memory(&mut self) {
        self.prune();
        for (_, pipeline) in std::mem::take(&mut self.pipelines) {
            destroy_pipeline(&self.gl, pipeline);
        }
    }
    pub(super) fn create(&mut self, descriptor: RenderTargetDescriptor) -> Result<RenderTarget> {
        self.live()?;
        self.targets.check_request(descriptor)?;
        self.prune();
        let bytes = self.targets.check_allocation(descriptor)?;
        if descriptor.format == RenderTargetFormat::Rgba16Float
            && self
                .gl
                .get_extension("EXT_color_buffer_float")
                .map_err(backend)?
                .is_none()
        {
            return Err(RenderTargetError::Unsupported(
                "browser lacks EXT_color_buffer_float for HDR targets",
            ));
        }
        let image = self
            .gl
            .create_texture()
            .ok_or_else(|| backend("failed to create custom texture"))?;
        let framebuffer = match self.gl.create_framebuffer() {
            Some(v) => v,
            None => {
                self.gl.delete_texture(Some(&image));
                return Err(backend("failed to create custom framebuffer"));
            }
        };
        let target = Target { image, framebuffer };
        let previous = framebuffer_binding(&self.gl)?;
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(&target.image));
        self.gl.tex_storage_2d(
            Gl::TEXTURE_2D,
            1,
            format(descriptor.format),
            descriptor.width as i32,
            descriptor.height as i32,
        );
        self.gl
            .tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
        self.gl
            .tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
        self.gl
            .tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
        self.gl
            .tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
        self.gl
            .bind_framebuffer(Gl::FRAMEBUFFER, Some(&target.framebuffer));
        self.gl.framebuffer_texture_2d(
            Gl::FRAMEBUFFER,
            Gl::COLOR_ATTACHMENT0,
            Gl::TEXTURE_2D,
            Some(&target.image),
            0,
        );
        let complete =
            self.gl.check_framebuffer_status(Gl::FRAMEBUFFER) == Gl::FRAMEBUFFER_COMPLETE;
        let scissor = self.gl.is_enabled(Gl::SCISSOR_TEST);
        self.gl.disable(Gl::SCISSOR_TEST);
        self.gl.clear_color(0.0, 0.0, 0.0, 0.0);
        self.gl.clear(Gl::COLOR_BUFFER_BIT);
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER, previous.as_ref());
        self.gl.bind_texture(Gl::TEXTURE_2D, None);
        if scissor {
            self.gl.enable(Gl::SCISSOR_TEST);
        }
        let allocation_error = check_error(&self.gl);
        if !complete || allocation_error.is_err() {
            destroy_target(&self.gl, target);
            return Err(allocation_error
                .err()
                .unwrap_or(RenderTargetError::Unsupported(
                    "browser cannot render requested target format",
                )));
        }
        // WebGL stores BGRA-format requests in RGBA sRGB; public channels and
        // byte-ordered readback have the same contract, without a CPU swizzle.
        self.targets.insert(descriptor, target, bytes)
    }
    fn sampler(&mut self, kind: ShaderSampler) -> Result<WebGlSampler> {
        if let Some(s) = self.samplers.get(&kind) {
            return Ok(s.clone());
        }
        let sampler = self
            .gl
            .create_sampler()
            .ok_or_else(|| backend("failed to create custom sampler"))?;
        let filter = match kind {
            ShaderSampler::LinearClamp => Gl::LINEAR,
            ShaderSampler::NearestClamp => Gl::NEAREST,
        };
        for parameter in [Gl::TEXTURE_MIN_FILTER, Gl::TEXTURE_MAG_FILTER] {
            self.gl
                .sampler_parameteri(&sampler, parameter, filter as i32);
        }
        for parameter in [Gl::TEXTURE_WRAP_S, Gl::TEXTURE_WRAP_T] {
            self.gl
                .sampler_parameteri(&sampler, parameter, Gl::CLAMP_TO_EDGE as i32);
        }
        self.samplers.insert(kind, sampler.clone());
        Ok(sampler)
    }
    fn pipeline(&mut self, shader: &ShaderHandle, target_format: RenderTargetFormat) -> Result<()> {
        let key = (shader.id(), target_format);
        if self.pipelines.contains_key(&key) {
            return Ok(());
        }
        let translated = shader.translate(ShaderBackend::WebGl2).map_err(backend)?;
        let max_units = self
            .gl
            .get_parameter(Gl::MAX_TEXTURE_IMAGE_UNITS)
            .map_err(backend)?
            .as_f64()
            .unwrap_or(0.0) as usize;
        if translated.texture_sampler_pairs.len() > max_units {
            return Err(RenderTargetError::Unsupported(
                "shader needs more combined texture/sampler units than this browser provides",
            ));
        }
        if self.pipelines.len() >= MAX_PIPELINES {
            let oldest = *self.pipelines.iter().min_by_key(|(_, v)| v.used).unwrap().0;
            destroy_pipeline(&self.gl, self.pipelines.remove(&oldest).unwrap());
        }
        let program = super::link_program(
            &self.gl,
            translated
                .vertex_source
                .as_deref()
                .ok_or_else(|| backend("missing GLSL vertex source"))?,
            &translated.source,
        )
        .map_err(backend)?;
        let mut uniforms = BTreeMap::new();
        for slot in &translated.resources {
            if slot.kind != ShaderResourceSlotKind::Uniform {
                continue;
            }
            let index = self.gl.get_uniform_block_index(&program, &slot.name);
            if index == Gl::INVALID_INDEX {
                continue;
            }
            let buffer = match self.gl.create_buffer() {
                Some(v) => v,
                None => {
                    for uniform in uniforms.values() {
                        let uniform: &Uniform = uniform;
                        self.gl.delete_buffer(Some(&uniform.buffer));
                    }
                    self.gl.delete_program(Some(&program));
                    return Err(backend("failed to create custom uniform buffer"));
                }
            };
            self.gl.uniform_block_binding(&program, index, slot.slot);
            let size = shader
                .resources()
                .iter()
                .find_map(|resource| match &resource.kind {
                    ShaderResourceKind::Uniform(layout) if resource.binding == slot.binding => {
                        Some(layout.size)
                    }
                    _ => None,
                })
                .expect("reflected uniform layout");
            self.gl.bind_buffer(Gl::UNIFORM_BUFFER, Some(&buffer));
            self.gl
                .buffer_data_with_i32(Gl::UNIFORM_BUFFER, size as i32, Gl::DYNAMIC_DRAW);
            self.gl.bind_buffer(Gl::UNIFORM_BUFFER, None);
            uniforms.insert(
                slot.binding,
                Uniform {
                    buffer,
                    slot: slot.slot,
                },
            );
        }
        self.pipelines.insert(
            key,
            Pipeline {
                program,
                uniforms,
                pairs: translated.texture_sampler_pairs,
                used: self.tick,
            },
        );
        #[cfg(test)]
        {
            self.compilations += 1;
        }
        Ok(())
    }
    pub(super) fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<()> {
        self.live()?;
        bindings.validate(shader, target, &self.targets)?;
        self.pipeline(shader, target.descriptor().format)?;
        let key = (shader.id(), target.descriptor().format);
        let pairs = self.pipelines[&key].pairs.clone();
        let mut textures = Vec::new();
        for pair in &pairs {
            let Some(ShaderBinding::Texture(input)) = bindings.get(pair.texture_binding) else {
                unreachable!("validated texture")
            };
            let kind = pair
                .sampler_binding
                .map(|binding| match bindings.get(binding) {
                    Some(ShaderBinding::Sampler(kind)) => *kind,
                    _ => unreachable!("validated sampler"),
                })
                .unwrap_or(ShaderSampler::NearestClamp);
            let sampler = self.sampler(kind)?;
            textures.push((self.targets.get(input)?.image.clone(), sampler));
        }
        let previous = framebuffer_binding(&self.gl)?;
        let viewport = viewport(&self.gl)?;
        let scissor = self.gl.is_enabled(Gl::SCISSOR_TEST);
        self.gl.bind_framebuffer(
            Gl::FRAMEBUFFER,
            Some(&self.targets.get(target)?.framebuffer),
        );
        self.gl.viewport(
            0,
            0,
            target.descriptor().width as i32,
            target.descriptor().height as i32,
        );
        self.gl.disable(Gl::SCISSOR_TEST);
        self.gl.enable(Gl::BLEND);
        self.gl.blend_func_separate(
            Gl::SRC_ALPHA,
            Gl::ONE_MINUS_SRC_ALPHA,
            Gl::ONE,
            Gl::ONE_MINUS_SRC_ALPHA,
        );
        self.gl.clear_color(0.0, 0.0, 0.0, 0.0);
        self.gl.clear(Gl::COLOR_BUFFER_BIT);
        let pipeline = self.pipelines.get_mut(&key).unwrap();
        self.gl.use_program(Some(&pipeline.program));
        self.gl.bind_vertex_array(Some(&self.vao));
        for (&binding, uniform) in &pipeline.uniforms {
            let Some(ShaderBinding::Uniform(bytes)) = bindings.get(binding) else {
                unreachable!("validated uniform")
            };
            self.gl
                .bind_buffer(Gl::UNIFORM_BUFFER, Some(&uniform.buffer));
            self.gl
                .buffer_sub_data_with_i32_and_u8_array(Gl::UNIFORM_BUFFER, 0, bytes);
            self.gl
                .bind_buffer_base(Gl::UNIFORM_BUFFER, uniform.slot, Some(&uniform.buffer));
        }
        for (unit, ((image, sampler), pair)) in textures.iter().zip(&pairs).enumerate() {
            self.gl.active_texture(Gl::TEXTURE0 + unit as u32);
            self.gl.bind_texture(Gl::TEXTURE_2D, Some(image));
            self.gl.bind_sampler(unit as u32, Some(sampler));
            self.gl.uniform1i(
                self.gl
                    .get_uniform_location(&pipeline.program, &pair.uniform_name)
                    .as_ref(),
                unit as i32,
            );
        }
        self.gl.draw_arrays(Gl::TRIANGLES, 0, 3);
        for unit in 0..textures.len() {
            self.gl.bind_sampler(unit as u32, None);
        }
        self.gl.active_texture(Gl::TEXTURE0);
        self.gl.bind_buffer(Gl::UNIFORM_BUFFER, None);
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER, previous.as_ref());
        self.gl
            .viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        if scissor {
            self.gl.enable(Gl::SCISSOR_TEST);
        }
        self.gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);
        self.tick = self.tick.saturating_add(1);
        pipeline.used = self.tick;
        check_error(&self.gl)?;
        target.did_render();
        Ok(())
    }
    pub(super) fn write_target(&mut self, target: &RenderTarget, pixels: &[u8]) -> Result<()> {
        self.live()?;
        let resource = self.targets.get(target)?;
        let descriptor = target.descriptor();
        if pixels.len() as u64 != descriptor.byte_len()? {
            return Err(RenderTargetError::InvalidBindings(
                "target upload requires exact packed pixel bytes".into(),
            ));
        }
        let previous = self
            .gl
            .get_parameter(Gl::TEXTURE_BINDING_2D)
            .map_err(backend)?
            .dyn_into::<WebGlTexture>()
            .ok();
        let alignment = self
            .gl
            .get_parameter(Gl::UNPACK_ALIGNMENT)
            .map_err(backend)?
            .as_f64()
            .unwrap_or(4.0) as i32;
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(&resource.image));
        self.gl.pixel_storei(Gl::UNPACK_ALIGNMENT, 1);
        let format = if descriptor.format == RenderTargetFormat::R8Unorm {
            Gl::RED
        } else {
            Gl::RGBA
        };
        let result = if descriptor.format == RenderTargetFormat::Rgba16Float {
            let values = pixels
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            let values = js_sys::Uint16Array::from(values.as_slice());
            self.gl
                .tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(
                    Gl::TEXTURE_2D,
                    0,
                    0,
                    0,
                    descriptor.width as i32,
                    descriptor.height as i32,
                    format,
                    Gl::HALF_FLOAT,
                    Some(values.as_ref()),
                )
        } else {
            self.gl
                .tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
                    Gl::TEXTURE_2D,
                    0,
                    0,
                    0,
                    descriptor.width as i32,
                    descriptor.height as i32,
                    format,
                    Gl::UNSIGNED_BYTE,
                    Some(pixels),
                )
        };
        self.gl.bind_texture(Gl::TEXTURE_2D, previous.as_ref());
        self.gl.pixel_storei(Gl::UNPACK_ALIGNMENT, alignment);
        result.map_err(backend)?;
        let error = self.gl.get_error();
        if error != Gl::NO_ERROR {
            return Err(backend(format!("WebGL target upload error {error:#x}")));
        }
        target.did_render();
        Ok(())
    }

    pub(super) fn read(&mut self, target: &RenderTarget) -> Result<RenderTargetReadback> {
        self.live()?;
        let descriptor = target.descriptor();
        let framebuffer = self.targets.get(target)?.framebuffer.clone();
        let components = u64::from(descriptor.width) * u64::from(descriptor.height) * 4;
        let staging_bytes = components
            * if descriptor.format == RenderTargetFormat::Rgba16Float {
                4
            } else {
                1
            };
        if staging_bytes > MAX_READBACK {
            return Err(RenderTargetError::BudgetExceeded);
        }
        let previous = framebuffer_binding(&self.gl)?;
        self.gl
            .bind_framebuffer(Gl::FRAMEBUFFER, Some(&framebuffer));
        self.gl.pixel_storei(Gl::PACK_ALIGNMENT, 1);
        let result = if descriptor.format == RenderTargetFormat::Rgba16Float {
            let data = js_sys::Float32Array::new_with_length(components as u32);
            self.gl
                .read_pixels_with_opt_array_buffer_view(
                    0,
                    0,
                    descriptor.width as i32,
                    descriptor.height as i32,
                    Gl::RGBA,
                    Gl::FLOAT,
                    Some(data.as_ref()),
                )
                .map_err(backend)
                .map(|()| {
                    let mut pixels = Vec::with_capacity(components as usize * 2);
                    for value in data.to_vec() {
                        pixels.extend_from_slice(&f32_to_f16(value).to_le_bytes());
                    }
                    pixels
                })
        } else {
            let mut rgba = vec![0; components as usize];
            self.gl
                .read_pixels_with_opt_u8_array(
                    0,
                    0,
                    descriptor.width as i32,
                    descriptor.height as i32,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    Some(&mut rgba),
                )
                .map_err(backend)
                .map(|()| {
                    if descriptor.format == RenderTargetFormat::R8Unorm {
                        rgba.chunks_exact(4).map(|v| v[0]).collect()
                    } else {
                        rgba
                    }
                })
        };
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER, previous.as_ref());
        let pixels = result?;
        check_error(&self.gl)?;
        // The translated WGSL vertex flips Y for GL: its canonical top row is
        // stored at GL row zero, so readPixels already returns top-first rows.
        Ok(RenderTargetReadback { descriptor, pixels })
    }
    pub(super) fn draw(
        &mut self,
        surface: &PaintSurface,
        target: &RenderTarget,
        viewport: Size<DevicePixels>,
    ) -> Result<()> {
        self.live()?;
        let image = self.targets.get(target)?.image.clone();
        if self.display.is_none() {
            let program =
                super::link_program(&self.gl, DISPLAY_VERTEX, DISPLAY_FRAGMENT).map_err(backend)?;
            let buffer = match self.gl.create_buffer() {
                Some(v) => v,
                None => {
                    self.gl.delete_program(Some(&program));
                    return Err(backend("failed to create display uniform"));
                }
            };
            let block = self.gl.get_uniform_block_index(&program, "Params");
            self.gl.uniform_block_binding(&program, block, 0);
            self.gl.bind_buffer(Gl::UNIFORM_BUFFER, Some(&buffer));
            self.gl.buffer_data_with_i32(
                Gl::UNIFORM_BUFFER,
                std::mem::size_of::<RenderTargetDisplayParams>() as i32,
                Gl::DYNAMIC_DRAW,
            );
            self.gl.bind_buffer(Gl::UNIFORM_BUFFER, None);
            self.display = Some(Display { program, buffer });
        }
        let sampling = self.sampler(ShaderSampler::LinearClamp)?;
        let display = self.display.as_ref().unwrap();
        let params = RenderTargetDisplayParams::new(surface, target, viewport);
        self.gl.use_program(Some(&display.program));
        self.gl.bind_vertex_array(Some(&self.vao));
        self.gl
            .bind_buffer(Gl::UNIFORM_BUFFER, Some(&display.buffer));
        self.gl.buffer_sub_data_with_i32_and_u8_array(
            Gl::UNIFORM_BUFFER,
            0,
            bytemuck::bytes_of(&params),
        );
        self.gl
            .bind_buffer_base(Gl::UNIFORM_BUFFER, 0, Some(&display.buffer));
        self.gl.active_texture(Gl::TEXTURE0);
        self.gl.bind_texture(Gl::TEXTURE_2D, Some(&image));
        self.gl.bind_sampler(0, Some(&sampling));
        self.gl.uniform1i(
            self.gl
                .get_uniform_location(&display.program, "image")
                .as_ref(),
            0,
        );
        self.gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);
        self.gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
        self.gl.bind_sampler(0, None);
        self.gl.bind_buffer(Gl::UNIFORM_BUFFER, None);
        check_error(&self.gl)
    }
}
impl Drop for WebCustomRenderer {
    fn drop(&mut self) {
        for target in self.targets.invalidate_and_drain() {
            destroy_target(&self.gl, target);
        }
        for (_, pipeline) in std::mem::take(&mut self.pipelines) {
            destroy_pipeline(&self.gl, pipeline);
        }
        for (_, sampler) in std::mem::take(&mut self.samplers) {
            self.gl.delete_sampler(Some(&sampler));
        }
        if let Some(display) = self.display.take() {
            self.gl.delete_program(Some(&display.program));
            self.gl.delete_buffer(Some(&display.buffer));
        }
        self.gl.delete_vertex_array(Some(&self.vao));
    }
}
fn destroy_target(gl: &Gl, target: Target) {
    gl.delete_framebuffer(Some(&target.framebuffer));
    gl.delete_texture(Some(&target.image));
}
fn destroy_pipeline(gl: &Gl, pipeline: Pipeline) {
    gl.delete_program(Some(&pipeline.program));
    for uniform in pipeline.uniforms.into_values() {
        gl.delete_buffer(Some(&uniform.buffer));
    }
}
fn backend(error: impl std::fmt::Debug) -> RenderTargetError {
    RenderTargetError::Backend(format!("{error:?}"))
}
fn check_error(gl: &Gl) -> Result<()> {
    let error = gl.get_error();
    if error == Gl::NO_ERROR {
        Ok(())
    } else {
        Err(backend(format!("WebGL error 0x{error:04x}")))
    }
}
fn framebuffer_binding(gl: &Gl) -> Result<Option<WebGlFramebuffer>> {
    let value = gl.get_parameter(Gl::FRAMEBUFFER_BINDING).map_err(backend)?;
    Ok(if value.is_null() {
        None
    } else {
        Some(value.unchecked_into())
    })
}
fn viewport(gl: &Gl) -> Result<[i32; 4]> {
    let value = gl.get_parameter(Gl::VIEWPORT).map_err(backend)?;
    let values = js_sys::Int32Array::new(&value).to_vec();
    values
        .try_into()
        .map_err(|_| backend("invalid GL viewport"))
}
fn format(format: RenderTargetFormat) -> u32 {
    match format {
        RenderTargetFormat::Rgba8Unorm => Gl::RGBA8,
        RenderTargetFormat::Rgba8UnormSrgb | RenderTargetFormat::Bgra8UnormSrgb => Gl::SRGB8_ALPHA8,
        RenderTargetFormat::Rgba16Float => Gl::RGBA16F,
        RenderTargetFormat::R8Unorm => Gl::R8,
    }
}
// IEEE binary16 conversion with round-to-nearest, ties-to-even. Browser readback
// exposes f32 components even when the attachment stores half-precision texels.
fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7fffff;
    if exp == 255 {
        return sign | 0x7c00 | if mant == 0 { 0 } else { 0x0200 };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 31 {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let normalized = mant | 0x800000;
        let shift = (14 - half_exp) as u32;
        let result = (normalized >> shift)
            + u32::from(
                (normalized & ((1 << shift) - 1)) > (1 << (shift - 1))
                    || ((normalized & ((1 << shift) - 1)) == (1 << (shift - 1))
                        && ((normalized >> shift) & 1) != 0),
            );
        return sign | result as u16;
    }
    let rounded = mant + 0x0fff + ((mant >> 13) & 1);
    sign | (((half_exp as u32) << 10) + (rounded >> 13)) as u16
}
const DISPLAY_VERTEX: &str = r#"#version 300 es
precision highp float;
layout(std140) uniform Params {vec4 bounds;vec4 mask;vec4 corners;vec4 rounded_clip;vec4 rounded_corners;vec4 transform;vec2 translation;vec2 viewport;vec4 color_filter;float opacity;uint scalar;uvec2 padding;};
out vec2 uv;out vec2 local;
void main(){uv=vec2(float(gl_VertexID&1),float((gl_VertexID>>1)&1));local=bounds.xy+uv*bounds.zw;vec2 position=vec2(dot(transform.xy,local),dot(transform.zw,local))+translation;gl_Position=vec4(position/viewport*vec2(2.0,-2.0)+vec2(-1.0,1.0),0.0,1.0);}
"#;
const DISPLAY_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
layout(std140) uniform Params {vec4 bounds;vec4 mask;vec4 corners;vec4 rounded_clip;vec4 rounded_corners;vec4 transform;vec2 translation;vec2 viewport;vec4 color_filter;float opacity;uint scalar;uvec2 padding;};
uniform sampler2D image;in vec2 uv;in vec2 local;out vec4 output_color;
float coverage(vec2 position,vec4 area,vec4 radii){if(all(equal(radii,vec4(0.0))))return 1.0;vec2 centered=position-area.xy-area.zw*0.5;float radius=centered.y<0.0?(centered.x<0.0?radii.x:radii.y):(centered.x<0.0?radii.w:radii.z);vec2 q=abs(centered)-area.zw*0.5+radius;float distance=length(max(q,0.0))+min(max(q.x,q.y),0.0)-radius;return clamp(0.5-distance,0.0,1.0);}
void main(){vec2 position=vec2(gl_FragCoord.x,viewport.y-gl_FragCoord.y);if(any(lessThan(position,mask.xy))||any(greaterThanEqual(position,mask.xy+mask.zw)))discard;vec4 color=texture(image,uv);if(scalar!=0u)color=vec4(color.rrr,1.0);if(color.a>0.0){vec3 straight=((color.rgb/color.a-0.5)*color_filter.w+0.5)*color_filter.z;vec3 gray=vec3(dot(straight,vec3(0.2126,0.7152,0.0722)));straight=mix(gray,straight,color_filter.y);gray=vec3(dot(straight,vec3(0.2126,0.7152,0.0722)));color.rgb=mix(straight,gray,color_filter.x)*color.a;}output_color=color*(opacity*coverage(local,bounds,corners)*coverage(position,rounded_clip,rounded_corners));}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, ContentMask, PaintSurfaceSource, ShaderDescriptor, point, size};
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_browser);
    fn renderer() -> (web_sys::HtmlCanvasElement, WebCustomRenderer) {
        let canvas: web_sys::HtmlCanvasElement = web_sys::window()
            .unwrap()
            .document()
            .unwrap()
            .create_element("canvas")
            .unwrap()
            .unchecked_into();
        canvas.set_width(8);
        canvas.set_height(8);
        let gl: Gl = canvas
            .get_context("webgl2")
            .unwrap()
            .expect("WebGL2 required for pixel tests")
            .unchecked_into();
        console_log!(
            "WebGL VERSION={:?} VENDOR={:?} RENDERER={:?}",
            gl.get_parameter(Gl::VERSION).unwrap(),
            gl.get_parameter(Gl::VENDOR).unwrap(),
            gl.get_parameter(Gl::RENDERER).unwrap()
        );
        if gl
            .get_extension("WEBGL_debug_renderer_info")
            .unwrap()
            .is_some()
        {
            console_log!(
                "WebGL unmasked VENDOR={:?} RENDERER={:?}",
                gl.get_parameter(0x9245).unwrap(),
                gl.get_parameter(0x9246).unwrap()
            );
        }
        (canvas, WebCustomRenderer::new(&gl).unwrap())
    }
    fn shader(source: &str) -> ShaderHandle {
        ShaderHandle::compile_fragment(ShaderDescriptor::fragment("web_pixel", source, "fs_main"))
            .unwrap()
    }
    #[wasm_bindgen_test]
    fn web_authored_loop_budget_and_exact_pixel_uploads() {
        let (_, mut renderer) = renderer();
        let target = renderer
            .create(RenderTargetDescriptor::rgba8(1, 1))
            .unwrap();
        let program = shader(
            "fn work() -> u32 { var count=0u; loop { loop { count+=1u; } } return count; } @fragment fn fs_main() -> @location(0) vec4<f32> { let n=work(); return vec4<f32>(f32(n)/65536.0,0.0,0.0,1.0); }",
        );
        renderer
            .render(&target, &program, &ShaderBindings::new())
            .unwrap();
        assert_eq!(renderer.read(&target).unwrap().pixels, [255, 0, 0, 255]);
        for format in [
            RenderTargetFormat::Rgba8Unorm,
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::Rgba16Float,
            RenderTargetFormat::R8Unorm,
        ] {
            let target = renderer
                .create(RenderTargetDescriptor {
                    width: 2,
                    height: 2,
                    format,
                })
                .unwrap();
            let pixels = match format {
                RenderTargetFormat::Rgba16Float => {
                    [0x00, 0x38, 0x00, 0x34, 0x00, 0x3c, 0x00, 0x3c].repeat(4)
                }
                RenderTargetFormat::R8Unorm => vec![17, 23, 31, 47],
                _ => [17, 23, 31, 255].repeat(4),
            };
            renderer.write_target(&target, &pixels).unwrap();
            assert_eq!(renderer.read(&target).unwrap().pixels, pixels);
            let revision = target.revision();
            assert!(renderer.write_target(&target, &[]).is_err());
            assert_eq!(target.revision(), revision);
        }
    }
    #[wasm_bindgen_test]
    fn web_custom_fragment_pixel_orientation_pipeline_reuse_and_lifecycle() {
        let (_canvas, mut renderer) = renderer();
        let target = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        assert!(
            renderer
                .read(&target)
                .unwrap()
                .pixels
                .iter()
                .all(|&v| v == 0)
        );
        let color = shader(
            "@fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {return vec4<f32>(uv.x, uv.y, 1.0, 0.5);}",
        );
        for _ in 0..2 {
            renderer
                .render(&target, &color, &ShaderBindings::new())
                .unwrap();
        }
        assert_eq!(renderer.compilations, 1);
        assert_eq!(target.revision(), 2);
        let read = renderer.read(&target).unwrap();
        for y in 0usize..4 {
            for x in 0usize..4 {
                let pixel = &read.pixels[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
                let expected = [
                    ((x as f32 + 0.5) / 4.0 * 0.5 * 255.0).round() as u8,
                    ((y as f32 + 0.5) / 4.0 * 0.5 * 255.0).round() as u8,
                    128,
                    128,
                ];
                for (&v, e) in pixel.iter().zip(expected) {
                    assert!(
                        v.abs_diff(e) <= 1,
                        "{x},{y}: {pixel:?}, expected{expected:?}"
                    );
                }
            }
        }
        renderer.shed_memory();
        assert!(target.is_valid());
        let (_other, foreign) = self::renderer();
        assert!(matches!(
            foreign.validate(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        drop(renderer);
        assert!(!target.is_valid());
    }
    #[wasm_bindgen_test]
    fn web_sparse_uniform_texture_sampler_update_and_feedback_validation() {
        let (_canvas, mut renderer) = renderer();
        let input = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let output = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let uniform = shader(
            "struct Params {color:vec4<f32>} @group(0) @binding(7) var<uniform> params:Params; @fragment fn fs_main()->@location(0) vec4<f32>{return params.color;}",
        );
        for values in [[0.25f32, 0.5, 1.0, 0.5], [1.0, 0.25, 0.5, 0.75]] {
            let bindings = ShaderBindings::new().with(
                7,
                ShaderBinding::Uniform(bytemuck::cast_slice(&values).into()),
            );
            renderer.render(&input, &uniform, &bindings).unwrap();
        }
        let copy = shader(
            "@group(0) @binding(9) var image:texture_2d<f32>; @group(0) @binding(12) var sampling:sampler; @fragment fn fs_main(@location(0) uv:vec2<f32>)->@location(0) vec4<f32>{let color=textureSample(image,sampling,uv);return vec4<f32>(color.rgb/color.a,color.a);}",
        );
        let bindings = ShaderBindings::new()
            .with(9, ShaderBinding::Texture(input.clone()))
            .with(12, ShaderBinding::Sampler(ShaderSampler::NearestClamp));
        renderer.render(&output, &copy, &bindings).unwrap();
        assert_eq!(
            renderer.read(&input).unwrap().pixels,
            renderer.read(&output).unwrap().pixels
        );
        assert!(matches!(
            renderer.render(&input, &copy, &bindings),
            Err(RenderTargetError::FeedbackLoop)
        ));
        assert_eq!(renderer.compilations, 2);
    }
    #[wasm_bindgen_test]
    fn web_all_formats_srgb_scalar_hdr_readback() {
        let (_canvas, mut renderer) = renderer();
        let color = shader(
            "@fragment fn fs_main()->@location(0) vec4<f32>{return vec4<f32>(0.25,0.5,1.0,1.0);}",
        );
        for format in [
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::R8Unorm,
            RenderTargetFormat::Rgba16Float,
        ] {
            let target = renderer
                .create(RenderTargetDescriptor {
                    width: 4,
                    height: 4,
                    format,
                })
                .unwrap();
            renderer
                .render(&target, &color, &ShaderBindings::new())
                .unwrap();
            let read = renderer.read(&target).unwrap();
            if format == RenderTargetFormat::R8Unorm {
                assert!(read.pixels.iter().all(|v| v.abs_diff(64) <= 1));
            } else if format == RenderTargetFormat::Rgba16Float {
                assert_eq!(&read.pixels[..8], &[0, 52, 0, 56, 0, 60, 0, 60]);
            } else {
                for pixel in read.pixels.chunks_exact(4) {
                    for (&v, e) in pixel.iter().zip([137u8, 188, 255, 255]) {
                        assert!(v.abs_diff(e) <= 2, "{pixel:?}");
                    }
                }
            }
        }
        assert_eq!(f32_to_f16(2.0), 0x4000);
        assert_eq!(f32_to_f16(f32::INFINITY), 0x7c00);
        assert_eq!(f32_to_f16(-0.0), 0x8000);
    }
    #[wasm_bindgen_test]
    fn web_gpu_display_rounded_opacity_and_clip_no_readback_upload() {
        let (_canvas, mut renderer) = renderer();
        let input = renderer
            .create(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        renderer.render(&input,&shader("@fragment fn fs_main()->@location(0) vec4<f32>{return vec4<f32>(0.0,1.0,0.0,1.0);}"),&ShaderBindings::new()).unwrap();
        let bounds = Bounds::new(
            point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
            size(crate::ScaledPixels(8.0), crate::ScaledPixels(8.0)),
        );
        let paint = crate::render_target::RenderTargetPaint {
            opacity: 0.5,
            corner_radii: crate::Corners {
                top_left: crate::ScaledPixels(4.0),
                top_right: crate::ScaledPixels(4.0),
                bottom_left: crate::ScaledPixels(4.0),
                bottom_right: crate::ScaledPixels(4.0),
            },
            ..Default::default()
        };
        let surface = PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            source: PaintSurfaceSource::RenderTarget {
                target: input.clone(),
                revision: input.revision(),
                paint,
            },
        };
        renderer.gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
        renderer.gl.viewport(0, 0, 8, 8);
        renderer.gl.enable(Gl::BLEND);
        renderer.gl.clear_color(0.0, 0.0, 0.0, 0.0);
        renderer.gl.clear(Gl::COLOR_BUFFER_BIT);
        renderer
            .draw(&surface, &input, size(DevicePixels(8), DevicePixels(8)))
            .unwrap();
        let mut rgba = vec![0u8; 8 * 8 * 4];
        renderer
            .gl
            .read_pixels_with_opt_u8_array(0, 0, 8, 8, Gl::RGBA, Gl::UNSIGNED_BYTE, Some(&mut rgba))
            .unwrap();
        assert_eq!(&rgba[..4], &[0, 0, 0, 0]);
        let center = &rgba[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4];
        assert!(
            center[1].abs_diff(128) <= 1 && center[3].abs_diff(128) <= 1,
            "{center:?}"
        );
    }
    #[wasm_bindgen_test(async)]
    async fn web_context_loss_invalidates_gpu_handles_and_recovery_has_new_owner() {
        let (canvas, mut renderer) = renderer();
        let target = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let retained_program = shader(
            "@fragment fn fs_main()->@location(0) vec4<f32>{return vec4<f32>(1.0,0.0,0.0,1.0);}",
        );
        let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
            |event: web_sys::Event| event.prevent_default(),
        );
        canvas
            .add_event_listener_with_callback("webglcontextlost", listener.as_ref().unchecked_ref())
            .unwrap();
        let extension = renderer
            .gl
            .get_extension("WEBGL_lose_context")
            .unwrap()
            .expect("context loss extension required for regression");
        let lose: js_sys::Function = js_sys::Reflect::get(&extension, &"loseContext".into())
            .unwrap()
            .unchecked_into();
        lose.call0(&extension).unwrap();
        yield_browser(20).await;
        assert!(matches!(
            renderer.read(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        assert!(!target.is_valid());
        let restore: js_sys::Function = js_sys::Reflect::get(&extension, &"restoreContext".into())
            .unwrap()
            .unchecked_into();
        restore.call0(&extension).unwrap();
        for _ in 0..100 {
            if !renderer.gl.is_context_lost() {
                break;
            }
            yield_browser(20).await;
        }
        assert!(
            !renderer.gl.is_context_lost(),
            "browser must restore the test context"
        );
        let gl: Gl = canvas
            .get_context("webgl2")
            .unwrap()
            .unwrap()
            .unchecked_into();
        let mut recovered = WebCustomRenderer::new(&gl).unwrap();
        let output = recovered
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        assert!(matches!(
            recovered.validate(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        recovered
            .render(&output, &retained_program, &ShaderBindings::new())
            .unwrap();
        assert_eq!(
            &recovered.read(&output).unwrap().pixels[..4],
            &[255, 0, 0, 255]
        );
        canvas
            .remove_event_listener_with_callback(
                "webglcontextlost",
                listener.as_ref().unchecked_ref(),
            )
            .unwrap();
    }
    async fn yield_browser(milliseconds: i32) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds)
                .unwrap();
        });
        wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
    }
}
