use super::*;
use crate::{BlurRect, ContentMask, hsla, point, size};
use wasm_bindgen_test::*;
#[cfg(not(feature = "custom-shaders"))]
wasm_bindgen_test_configure!(run_in_browser);

fn renderer(width: u32, height: u32) -> WebGlSceneRenderer {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .unchecked_into();
    canvas.set_width(width);
    canvas.set_height(height);
    let renderer = WebGlSceneRenderer::new(
        &canvas,
        size(DevicePixels(width as i32), DevicePixels(height as i32)),
    )
    .unwrap();
    console_log!("Web scene blur driver: {:?}", renderer.gpu_specs());
    renderer
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(width), ScaledPixels(height)),
    )
}

fn quad(bounds: Bounds<ScaledPixels>, color: Hsla) -> Quad {
    Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: Background::from(color),
        transform: TransformationMatrix::unit(),
        ..Default::default()
    }
}

fn blur(bounds: Bounds<ScaledPixels>, sigma: f32) -> BlurRect {
    BlurRect {
        bounds,
        content_mask: ContentMask { bounds },
        blur_radius: ScaledPixels(sigma),
        tint: Hsla::transparent_black(),
        saturation: 1.0,
        order: 0,
        corner_radii: Corners::default(),
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
    }
}

fn pixel(renderer: &WebGlSceneRenderer, x: i32, y: i32) -> [u8; 4] {
    let mut pixel = [0; 4];
    renderer
        .gl
        .read_pixels_with_opt_u8_array(
            x,
            renderer.canvas.height() as i32 - y - 1,
            1,
            1,
            Gl::RGBA,
            Gl::UNSIGNED_BYTE,
            Some(&mut pixel),
        )
        .unwrap();
    assert_eq!(renderer.gl.get_error(), Gl::NO_ERROR);
    pixel
}

fn assert_pixel(actual: [u8; 4], expected: [u8; 4]) {
    for (actual_channel, expected_channel) in actual.into_iter().zip(expected) {
        assert!(
            actual_channel.abs_diff(expected_channel) <= 2,
            "pixel {actual:?}, expected {expected:?}"
        );
    }
}

#[wasm_bindgen_test]
fn web_backdrop_gaussian_keeps_absolute_capture_coordinates() {
    let mut renderer = renderer(64, 32);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(rect(0.0, 0.0, 64.0, 32.0), hsla(0.0, 1.0, 0.5, 1.0)));
    scene.insert_primitive(quad(
        rect(20.0, 0.0, 44.0, 32.0),
        hsla(2.0 / 3.0, 1.0, 0.5, 1.0),
    ));
    scene.insert_primitive(blur(rect(12.0, 4.0, 20.0, 24.0), 2.0));
    scene.finish();
    renderer.draw(&scene).unwrap();
    assert_pixel(pixel(&renderer, 18, 16), [198, 0, 57, 255]);
    assert_pixel(pixel(&renderer, 22, 16), [26, 0, 229, 255]);
}

#[wasm_bindgen_test]
fn web_backdrop_fractional_capture_includes_last_visible_texel() {
    let mut renderer = renderer(64, 32);
    seed(&renderer, [1.0, 0.0, 0.0, 1.0]);
    seed_region(&renderer, [32, 0, 1, 32], [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &blur(rect(12.8, 4.2, 20.0, 20.0), 0.0), None);
    assert_pixel(pixel(&renderer, 32, 16), [0, 0, 255, 255]);
}

fn seed(renderer: &WebGlSceneRenderer, color: [f32; 4]) {
    renderer.gl.bind_framebuffer(Gl::FRAMEBUFFER, None);
    renderer.gl.disable(Gl::SCISSOR_TEST);
    renderer
        .gl
        .clear_color(color[0], color[1], color[2], color[3]);
    renderer.gl.clear(Gl::COLOR_BUFFER_BIT);
}

fn seed_region(renderer: &WebGlSceneRenderer, region: [i32; 4], color: [f32; 4]) {
    renderer.gl.enable(Gl::SCISSOR_TEST);
    renderer.gl.scissor(
        region[0],
        renderer.canvas.height() as i32 - region[1] - region[3],
        region[2],
        region[3],
    );
    renderer
        .gl
        .clear_color(color[0], color[1], color[2], color[3]);
    renderer.gl.clear(Gl::COLOR_BUFFER_BIT);
    renderer.gl.disable(Gl::SCISSOR_TEST);
}

fn draw_blur(renderer: &mut WebGlSceneRenderer, rect: &BlurRect, damage: Option<[i32; 4]>) {
    renderer
        .blur
        .draw(
            std::slice::from_ref(rect),
            [
                renderer.canvas.width() as f32,
                renderer.canvas.height() as f32,
            ],
            &renderer.quad_vao,
            damage,
        )
        .unwrap();
    assert_eq!(renderer.gl.get_error(), Gl::NO_ERROR);
}

#[wasm_bindgen_test]
fn web_backdrop_gaussian_clamps_texel_centers_at_both_viewport_edges() {
    let mut renderer = renderer(32, 32);
    let panel = blur(rect(0.0, 0.0, 32.0, 32.0), 1.0);
    seed(&renderer, [1.0, 0.0, 0.0, 1.0]);
    seed_region(&renderer, [31, 0, 1, 32], [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &panel, None);
    assert_pixel(pixel(&renderer, 31, 16), [77, 0, 178, 255]);
    seed(&renderer, [1.0, 0.0, 0.0, 1.0]);
    seed_region(&renderer, [0, 31, 32, 1], [0.0, 1.0, 0.0, 1.0]);
    draw_blur(&mut renderer, &panel, None);
    assert_pixel(pixel(&renderer, 16, 31), [77, 178, 0, 255]);
}

#[wasm_bindgen_test]
fn web_backdrop_preserves_associated_alpha_with_tint_and_saturation() {
    let mut renderer = renderer(32, 32);
    let mut panel = blur(rect(0.0, 0.0, 32.0, 32.0), 1.0);
    seed(&renderer, [0.5, 0.0, 0.0, 0.5]);
    panel.tint = hsla(2.0 / 3.0, 1.0, 0.5, 0.25);
    draw_blur(&mut renderer, &panel, None);
    assert_pixel(pixel(&renderer, 16, 16), [144, 0, 64, 208]);
    seed(&renderer, [0.0, 1.0, 0.0, 1.0]);
    panel.tint = Hsla::transparent_black();
    panel.saturation = 0.0;
    draw_blur(&mut renderer, &panel, None);
    assert_pixel(pixel(&renderer, 16, 16), [182, 182, 182, 255]);
}

#[wasm_bindgen_test]
fn web_backdrop_own_ancestor_masks_and_damage_scissor_restore_scene_state() {
    let mut renderer = renderer(32, 32);
    seed(&renderer, [0.0, 0.0, 1.0, 1.0]);
    let mut panel = blur(rect(0.0, 0.0, 32.0, 32.0), 1.0);
    panel.tint = hsla(0.0, 1.0, 0.5, 1.0);
    panel.corner_radii = Corners::all(ScaledPixels(8.0));
    panel.rounded_clip_bounds = rect(8.0, 8.0, 16.0, 16.0);
    panel.rounded_clip_radii = Corners::all(ScaledPixels(8.0));
    panel.content_mask.bounds = rect(9.0, 8.0, 15.0, 16.0);
    draw_blur(&mut renderer, &panel, None);
    assert_pixel(pixel(&renderer, 16, 16), [255, 0, 0, 255]);
    for (x, y) in [(0, 0), (8, 16), (9, 8), (24, 16)] {
        assert_pixel(pixel(&renderer, x, y), [0, 0, 255, 255]);
    }
    seed(&renderer, [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &panel, Some([14, 14, 4, 4]));
    assert_pixel(pixel(&renderer, 16, 16), [255, 0, 0, 255]);
    assert_pixel(pixel(&renderer, 20, 16), [0, 0, 255, 255]);
    assert!(renderer.gl.is_enabled(Gl::SCISSOR_TEST));
    assert!(renderer.gl.is_enabled(Gl::BLEND));
    assert!(
        renderer
            .gl
            .get_parameter(Gl::FRAMEBUFFER_BINDING)
            .unwrap()
            .is_null()
    );
    let scissor =
        js_sys::Int32Array::new(&renderer.gl.get_parameter(Gl::SCISSOR_BOX).unwrap()).to_vec();
    assert_eq!(scissor, [14, 14, 4, 4]);
    let viewport =
        js_sys::Int32Array::new(&renderer.gl.get_parameter(Gl::VIEWPORT).unwrap()).to_vec();
    assert_eq!(viewport, [0, 0, 32, 32]);
}

#[wasm_bindgen_test]
fn web_backdrop_lazy_scratch_reuses_compact_capacity_and_rejects_before_allocation() {
    let mut renderer = renderer(32, 32);
    assert_eq!(renderer.blur.dimensions(), None);
    let mut scene = Scene::default();
    scene.insert_primitive(quad(rect(0.0, 0.0, 32.0, 32.0), hsla(0.0, 1.0, 0.5, 1.0)));
    scene.finish();
    renderer.draw(&scene).unwrap();
    assert_eq!(renderer.blur.allocations, 0);
    let oversized = blur(rect(0.0, 0.0, 4096.0, 4096.0), 1.0);
    let error = renderer
        .blur
        .draw(&[oversized], [4096.0, 4096.0], &renderer.quad_vao, None)
        .unwrap_err();
    assert!(error.to_string().contains("64MiB"));
    assert_eq!(renderer.blur.allocations, 0);
    assert_eq!(renderer.blur.dimensions(), None);

    let panel = blur(rect(12.0, 12.0, 8.0, 8.0), 1.0);
    seed(&renderer, [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &panel, None);
    assert_eq!(renderer.blur.dimensions(), Some((14, 14)));
    assert_eq!(renderer.blur.allocations, 1);
    draw_blur(&mut renderer, &panel, None);
    assert_eq!(renderer.blur.allocations, 1);
    assert_pixel(pixel(&renderer, 16, 16), [0, 0, 255, 255]);
    // Capture a smaller, differently positioned ROI using the larger images.
    let small = blur(rect(30.0, 30.0, 2.0, 2.0), 0.0);
    seed(&renderer, [0.0, 1.0, 0.0, 1.0]);
    draw_blur(&mut renderer, &small, None);
    assert_eq!(renderer.blur.allocations, 1);
    assert_pixel(pixel(&renderer, 31, 31), [0, 255, 0, 255]);

    renderer.shed_scene_scratch();
    assert_eq!(renderer.blur.dimensions(), None);
    draw_blur(&mut renderer, &panel, None);
    assert_eq!(renderer.blur.allocations, 2);
    renderer.canvas.set_width(16);
    renderer.canvas.set_height(16);
    renderer.resize(size(DevicePixels(16), DevicePixels(16)));
    assert_eq!(renderer.blur.dimensions(), None);
    seed(&renderer, [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &blur(rect(0.0, 0.0, 16.0, 16.0), 1.0), None);
    assert_pixel(pixel(&renderer, 8, 8), [0, 0, 255, 255]);
    assert_eq!(renderer.blur.allocations, 3);
}

#[wasm_bindgen_test]
fn web_backdrop_changed_source_redraws_all_dependent_pixels() {
    let mut renderer = renderer(64, 32);
    let build = |step| {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(rect(0.0, 0.0, 64.0, 32.0), hsla(0.0, 1.0, 0.5, 1.0)));
        scene.insert_primitive(quad(
            rect(step, 0.0, 64.0 - step, 32.0),
            hsla(2.0 / 3.0, 1.0, 0.5, 1.0),
        ));
        scene.insert_primitive(blur(rect(12.0, 4.0, 20.0, 24.0), 2.0));
        scene.finish();
        scene
    };
    renderer.draw(&build(20.0)).unwrap();
    assert_pixel(pixel(&renderer, 18, 16), [198, 0, 57, 255]);
    let allocations = renderer.blur.allocations;
    renderer.draw(&build(21.0)).unwrap();
    assert_eq!(
        renderer
            .canvas
            .get_attribute("data-kael-frame-damage")
            .as_deref(),
        Some("full")
    );
    assert_pixel(pixel(&renderer, 18, 16), [229, 0, 26, 255]);
    assert_eq!(renderer.blur.allocations, allocations);
    renderer.draw(&build(21.0)).unwrap();
    assert_eq!(
        renderer
            .canvas
            .get_attribute("data-kael-frame-damage")
            .as_deref(),
        Some("none")
    );
    assert_pixel(pixel(&renderer, 18, 16), [229, 0, 26, 255]);
}

async fn yield_browser(milliseconds: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, milliseconds)
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
}

#[wasm_bindgen_test(async)]
async fn web_backdrop_context_loss_discards_generation_and_recovers_pixels() {
    let mut renderer = renderer(32, 32);
    let panel = blur(rect(0.0, 0.0, 32.0, 32.0), 1.0);
    seed(&renderer, [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut renderer, &panel, None);
    let before = pixel(&renderer, 16, 16);
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
        |event: web_sys::Event| event.prevent_default(),
    );
    renderer
        .canvas
        .add_event_listener_with_callback("webglcontextlost", listener.as_ref().unchecked_ref())
        .unwrap();
    let extension = renderer
        .gl
        .get_extension("WEBGL_lose_context")
        .unwrap()
        .expect("WebGL context loss extension required");
    let lose: js_sys::Function = js_sys::Reflect::get(&extension, &"loseContext".into())
        .unwrap()
        .unchecked_into();
    lose.call0(&extension).unwrap();
    yield_browser(20).await;
    assert!(renderer.gl.is_context_lost());
    renderer.draw(&Scene::default()).unwrap();
    assert_eq!(renderer.blur.dimensions(), None);
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
        "browser must restore the regression context"
    );
    let mut restored = WebGlSceneRenderer::restored(
        &renderer.canvas,
        size(DevicePixels(32), DevicePixels(32)),
        renderer.atlas(),
        renderer.frame_count(),
        None,
    )
    .unwrap();
    assert_eq!(restored.blur.dimensions(), None);
    seed(&restored, [0.0, 0.0, 1.0, 1.0]);
    draw_blur(&mut restored, &panel, None);
    assert_eq!(pixel(&restored, 16, 16), before);
    assert_eq!(restored.gl.get_error(), Gl::NO_ERROR);
    renderer
        .canvas
        .remove_event_listener_with_callback("webglcontextlost", listener.as_ref().unchecked_ref())
        .unwrap();
}
