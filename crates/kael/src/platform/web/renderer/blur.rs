//! Separable backdrop Gaussian blur in associated-color, compact GPU scratch.
use super::{
    Gl, UniformCache, bounds, corners, js_error, link_program, rgba_components, uniform1f,
    uniform1i, uniform2f, uniform4f_array,
};
use crate::{BlurRect, DevicePixels, size};
use anyhow::{Context as _, Result};
use web_sys::{WebGlFramebuffer, WebGlProgram, WebGlTexture, WebGlVertexArrayObject};

const MAX_SCRATCH_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SCRATCH_DIMENSION: u32 = 16_384;
const UNIFORMS: &[&str] = &[
    "u_render_size",
    "u_render_origin",
    "u_target_bounds",
    "u_capture_bounds",
    "u_texture_size",
    "u_axis",
    "u_sigma",
    "u_composite",
    "u_texture",
    "u_tint",
    "u_saturation",
    "u_content_mask",
    "u_corner_radii",
    "u_rounded_clip_bounds",
    "u_rounded_clip_radii",
];

struct Pipeline {
    program: WebGlProgram,
    uniforms: UniformCache,
}
struct Scratch {
    source: WebGlTexture,
    horizontal: WebGlTexture,
    framebuffer: WebGlFramebuffer,
    width: u32,
    height: u32,
}

pub(super) struct WebBlurRenderer {
    gl: Gl,
    pipeline: Option<Pipeline>,
    scratch: Option<Scratch>,
    #[cfg(test)]
    pub(super) allocations: u32,
}

impl WebBlurRenderer {
    pub(super) fn new(gl: &Gl) -> Self {
        Self {
            gl: gl.clone(),
            pipeline: None,
            scratch: None,
            #[cfg(test)]
            allocations: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn dimensions(&self) -> Option<(u32, u32)> {
        self.scratch
            .as_ref()
            .map(|scratch| (scratch.width, scratch.height))
    }

    pub(super) fn clear_scratch(&mut self) {
        if let Some(scratch) = self.scratch.take() {
            if !self.gl.is_context_lost() {
                destroy_scratch(&self.gl, scratch);
            }
        }
    }

    pub(super) fn forget_context(&mut self) {
        // Deleting a stale generation after restoration would poison its error queue.
        self.scratch = None;
        self.pipeline = None;
    }

    pub(super) fn destroy(&mut self) {
        if self.gl.is_context_lost() {
            self.forget_context();
            return;
        }
        self.clear_scratch();
        if let Some(pipeline) = self.pipeline.take() {
            self.gl.delete_program(Some(&pipeline.program));
        }
    }

    fn ensure_scratch(&mut self, width: u32, height: u32) -> Result<()> {
        let device_max = self
            .gl
            .get_parameter(Gl::MAX_TEXTURE_SIZE)
            .map_err(js_error)?
            .as_f64()
            .context("browser texture limit unavailable")? as u32;
        checked_layout(width, height, device_max)?;
        if self
            .scratch
            .as_ref()
            .is_some_and(|scratch| width <= scratch.width && height <= scratch.height)
        {
            return Ok(());
        }
        let (width, height) = if let Some(previous) = self.scratch.as_ref() {
            let grown = (width.max(previous.width), height.max(previous.height));
            if checked_layout(grown.0, grown.1, device_max).is_ok() {
                grown
            } else {
                (width, height)
            }
        } else {
            (width, height)
        };
        // Release before replacement so even growth has one bounded scratch pair.
        self.clear_scratch();
        let source = create_texture(&self.gl, width, height)?;
        let horizontal = match create_texture(&self.gl, width, height) {
            Ok(texture) => texture,
            Err(error) => {
                self.gl.delete_texture(Some(&source));
                return Err(error);
            }
        };
        let Some(framebuffer) = self.gl.create_framebuffer() else {
            self.gl.delete_texture(Some(&source));
            self.gl.delete_texture(Some(&horizontal));
            anyhow::bail!("browser blur framebuffer allocation failed");
        };
        let scratch = Scratch {
            source,
            horizontal,
            framebuffer,
            width,
            height,
        };
        self.gl
            .bind_framebuffer(Gl::FRAMEBUFFER, Some(&scratch.framebuffer));
        self.gl.framebuffer_texture_2d(
            Gl::FRAMEBUFFER,
            Gl::COLOR_ATTACHMENT0,
            Gl::TEXTURE_2D,
            Some(&scratch.horizontal),
            0,
        );
        let complete =
            self.gl.check_framebuffer_status(Gl::FRAMEBUFFER) == Gl::FRAMEBUFFER_COMPLETE;
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
        if !complete {
            destroy_scratch(&self.gl, scratch);
            anyhow::bail!("browser blur framebuffer incomplete");
        }
        self.scratch = Some(scratch);
        #[cfg(test)]
        {
            self.allocations += 1;
        }
        Ok(())
    }

    pub(super) fn draw(
        &mut self,
        rects: &[BlurRect],
        viewport: [f32; 2],
        vao: &WebGlVertexArrayObject,
        damage: Option<[i32; 4]>,
    ) -> Result<()> {
        let gl = self.gl.clone();
        let result = (|| -> Result<()> {
            for rect in rects {
                anyhow::ensure!(
                    rect.blur_radius.0.is_finite()
                        && rect.blur_radius.0 >= 0.0
                        && rect.saturation.is_finite(),
                    "invalid browser blur parameters"
                );
                // The authored sigma is unchanged; the bounded kernel only needs
                // the at-most-16-pixel capture margin it can actually sample.
                let mut capture_rect = rect.clone();
                capture_rect.blur_radius.0 = rect.blur_radius.0.min(16.0 / 3.0);
                let capture = capture_rect.capture_bounds(size(
                    DevicePixels(viewport[0] as i32),
                    DevicePixels(viewport[1] as i32),
                ));
                if capture.is_empty() {
                    continue;
                }
                let [x, y, width, height] = bounds(capture);
                self.ensure_scratch(width as u32, height as u32)?;
                if self.pipeline.is_none() {
                    let program = link_program(&gl, VERTEX, FRAGMENT)?;
                    let uniforms = UniformCache::new(&gl, &program, UNIFORMS);
                    self.pipeline = Some(Pipeline { program, uniforms });
                }
                let scratch = self.scratch.as_ref().unwrap();
                let pipeline = self.pipeline.as_ref().unwrap();
                let uniforms = &pipeline.uniforms;
                gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
                gl.active_texture(Gl::TEXTURE0);
                gl.bind_sampler(0, None);
                gl.bind_texture(Gl::TEXTURE_2D, Some(&scratch.source));
                // Framebuffer coordinates use a lower-left origin; both compact
                // scratch images keep the copied rectangle in their lower-left.
                gl.copy_tex_sub_image_2d(
                    Gl::TEXTURE_2D,
                    0,
                    0,
                    0,
                    x as i32,
                    viewport[1] as i32 - (y + height) as i32,
                    width as i32,
                    height as i32,
                );
                gl.use_program(Some(&pipeline.program));
                gl.bind_vertex_array(Some(vao));
                uniform1i(&gl, uniforms, "u_texture", 0);
                uniform2f(
                    &gl,
                    uniforms,
                    "u_texture_size",
                    scratch.width as f32,
                    scratch.height as f32,
                );
                uniform4f_array(&gl, uniforms, "u_capture_bounds", bounds(capture));
                uniform1f(&gl, uniforms, "u_sigma", rect.blur_radius.0);

                gl.bind_framebuffer(Gl::FRAMEBUFFER, Some(&scratch.framebuffer));
                gl.viewport(0, 0, width as i32, height as i32);
                gl.disable(Gl::SCISSOR_TEST);
                gl.disable(Gl::BLEND);
                uniform2f(&gl, uniforms, "u_render_size", width, height);
                uniform2f(&gl, uniforms, "u_render_origin", x, y);
                uniform4f_array(&gl, uniforms, "u_target_bounds", bounds(capture));
                uniform2f(&gl, uniforms, "u_axis", 1.0, 0.0);
                uniform1i(&gl, uniforms, "u_composite", 0);
                gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);

                gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
                gl.viewport(0, 0, viewport[0] as i32, viewport[1] as i32);
                restore_scissor(&gl, damage);
                gl.enable(Gl::BLEND);
                gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);
                gl.bind_texture(Gl::TEXTURE_2D, Some(&scratch.horizontal));
                uniform2f(&gl, uniforms, "u_render_size", viewport[0], viewport[1]);
                uniform2f(&gl, uniforms, "u_render_origin", 0.0, 0.0);
                uniform4f_array(&gl, uniforms, "u_target_bounds", bounds(rect.bounds));
                uniform2f(&gl, uniforms, "u_axis", 0.0, 1.0);
                uniform1i(&gl, uniforms, "u_composite", 1);
                uniform4f_array(&gl, uniforms, "u_tint", rgba_components(rect.tint));
                uniform1f(&gl, uniforms, "u_saturation", rect.saturation);
                uniform4f_array(
                    &gl,
                    uniforms,
                    "u_content_mask",
                    bounds(rect.content_mask.bounds),
                );
                uniform4f_array(&gl, uniforms, "u_corner_radii", corners(rect.corner_radii));
                uniform4f_array(
                    &gl,
                    uniforms,
                    "u_rounded_clip_bounds",
                    bounds(rect.rounded_clip_bounds),
                );
                uniform4f_array(
                    &gl,
                    uniforms,
                    "u_rounded_clip_radii",
                    corners(rect.rounded_clip_radii),
                );
                gl.draw_arrays(Gl::TRIANGLE_STRIP, 0, 4);
            }
            Ok(())
        })();
        // Preserve the main scene target/scissor even when allocation or linking fails.
        gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
        gl.viewport(0, 0, viewport[0] as i32, viewport[1] as i32);
        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::ONE, Gl::ONE_MINUS_SRC_ALPHA);
        restore_scissor(&gl, damage);
        result
    }
}

fn restore_scissor(gl: &Gl, damage: Option<[i32; 4]>) {
    if let Some([x, y, width, height]) = damage {
        gl.enable(Gl::SCISSOR_TEST);
        gl.scissor(x, y, width, height);
    } else {
        gl.disable(Gl::SCISSOR_TEST);
    }
}
fn checked_layout(width: u32, height: u32, device_max: u32) -> Result<u64> {
    anyhow::ensure!(
        width > 0
            && height > 0
            && width <= device_max.min(MAX_SCRATCH_DIMENSION)
            && height <= device_max.min(MAX_SCRATCH_DIMENSION),
        "browser blur capture dimensions exceed device limits"
    );
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(8))
        .context("browser blur scratch dimensions overflow")?;
    anyhow::ensure!(
        bytes <= MAX_SCRATCH_BYTES,
        "browser blur scratch exceeds 64MiB pair budget"
    );
    Ok(bytes)
}
fn create_texture(gl: &Gl, width: u32, height: u32) -> Result<WebGlTexture> {
    let texture = gl
        .create_texture()
        .context("browser blur texture allocation failed")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.tex_storage_2d(Gl::TEXTURE_2D, 1, Gl::RGBA8, width as i32, height as i32);
    for name in [Gl::TEXTURE_MIN_FILTER, Gl::TEXTURE_MAG_FILTER] {
        gl.tex_parameteri(Gl::TEXTURE_2D, name, Gl::LINEAR as i32);
    }
    for name in [Gl::TEXTURE_WRAP_S, Gl::TEXTURE_WRAP_T] {
        gl.tex_parameteri(Gl::TEXTURE_2D, name, Gl::CLAMP_TO_EDGE as i32);
    }
    if gl.get_error() != Gl::NO_ERROR {
        gl.delete_texture(Some(&texture));
        anyhow::bail!("browser blur texture allocation failed");
    }
    Ok(texture)
}
fn destroy_scratch(gl: &Gl, scratch: Scratch) {
    gl.delete_framebuffer(Some(&scratch.framebuffer));
    gl.delete_texture(Some(&scratch.source));
    gl.delete_texture(Some(&scratch.horizontal));
}

const VERTEX: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_unit;
uniform vec2 u_render_size;
uniform vec2 u_render_origin;
uniform vec4 u_target_bounds;
out vec2 v_world;
void main() {
    v_world = u_target_bounds.xy + a_unit * u_target_bounds.zw;
    vec2 clip = (v_world - u_render_origin) / u_render_size * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
    gl_Position = vec4(clip, 0.0, 1.0);
}
"#;
const FRAGMENT: &str = r#"#version 300 es
precision highp float;
in vec2 v_world;
uniform sampler2D u_texture;
uniform vec4 u_capture_bounds;
uniform vec2 u_texture_size;
uniform vec2 u_axis;
uniform float u_sigma;
uniform int u_composite;
uniform vec4 u_target_bounds;
uniform vec4 u_tint;
uniform float u_saturation;
uniform vec4 u_content_mask;
uniform vec4 u_corner_radii;
uniform vec4 u_rounded_clip_bounds;
uniform vec4 u_rounded_clip_radii;
out vec4 out_color;

float rounded_sdf(vec2 p, vec4 bounds, vec4 radii) {
    vec2 local = p - (bounds.xy + bounds.zw * 0.5);
    float radius = local.y < 0.0 ? (local.x < 0.0 ? radii.x : radii.y) : (local.x < 0.0 ? radii.w : radii.z);
    radius = clamp(radius, 0.0, min(bounds.z, bounds.w) * 0.5);
    vec2 q = abs(local) - bounds.zw * 0.5 + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}
float rect_mask(vec2 p, vec4 bounds) {
    if (bounds.z <= 0.0 || bounds.w <= 0.0) return 1.0;
    return step(bounds.x, p.x) * step(bounds.y, p.y) * step(p.x, bounds.x + bounds.z) * step(p.y, bounds.y + bounds.w);
}
void main() {
    float sigma = max(u_sigma, 0.001);
    int radius = int(min(ceil(u_sigma * 3.0), 16.0));
    vec2 sample_min = u_capture_bounds.xy + vec2(0.5);
    vec2 sample_max = sample_min + max(u_capture_bounds.zw - vec2(1.0), vec2(0.0));
    vec4 accum = vec4(0.0);
    float weights = 0.0;
    for (int offset = -16; offset <= 16; ++offset) {
        if (abs(offset) > radius) continue;
        float weight = exp(-0.5 * pow(float(offset) / sigma, 2.0));
        vec2 sample_position = clamp(v_world + u_axis * float(offset), sample_min, sample_max);
        vec2 local = sample_position - u_capture_bounds.xy;
        vec2 uv = vec2(local.x, u_capture_bounds.w - local.y) / u_texture_size;
        accum += texture(u_texture, uv) * weight;
        weights += weight;
    }
    vec4 blurred = accum / weights;
    if (u_composite == 0) { out_color = blurred; return; }
    float coverage = rect_mask(v_world, u_content_mask)
        * clamp(0.5 - rounded_sdf(v_world, u_target_bounds, u_corner_radii), 0.0, 1.0);
    if (u_rounded_clip_bounds.z > 0.0 && u_rounded_clip_bounds.w > 0.0) {
        coverage *= clamp(0.5 - rounded_sdf(v_world, u_rounded_clip_bounds, u_rounded_clip_radii), 0.0, 1.0);
    }
    if (coverage <= 0.0) discard;
    vec3 grayscale = vec3(dot(blurred.rgb, vec3(0.2126, 0.7152, 0.0722)));
    vec3 saturated = mix(grayscale, blurred.rgb, u_saturation);
    vec3 associated = u_tint.rgb * u_tint.a + saturated * (1.0 - u_tint.a);
    float alpha = u_tint.a + blurred.a * (1.0 - u_tint.a);
    out_color = vec4(associated, alpha) * coverage;
}
"#;
