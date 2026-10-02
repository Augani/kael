//! Run with `cargo run -p kael --example custom_shader --features custom-shaders`.
//! Procedural WGSL renders into a reusable GPU target displayed without readback.

use kael::prelude::*;
use kael::{
    App, Application, Bounds, Context, ObjectFit, Render, RenderTarget, RenderTargetDescriptor,
    ShaderBinding, ShaderBindings, ShaderDescriptor, Window, WindowBounds, WindowOptions, div, px,
    render_target, rgb, size,
};

const FRAGMENT: &str = r#"
struct Palette { controls: vec4<f32> }
@group(0) @binding(7) var<uniform> palette: Palette;

@fragment
fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let p = (uv - 0.5) * vec2<f32>(1.6, 1.0);
    let bend = sin(p.x * 4.0 + p.y * 2.0 + palette.controls.x) * 0.16;
    let ribbon = exp(-abs(p.y + bend) * 12.0);
    let glow = exp(-length(p - vec2<f32>(0.35, -0.2)) * 3.0);
    let grain = fract(sin(dot(uv, vec2<f32>(12.9898, 78.233))) * 43758.5453) * 0.012;
    let ink = vec3<f32>(0.016, 0.025, 0.07);
    let violet = vec3<f32>(0.44, 0.20, 1.0);
    let mint = vec3<f32>(0.10, 0.95, 0.68);
    let accent = mix(violet, mint, smoothstep(0.1, 0.85, uv.x));
    let color = ink + accent * ribbon * 0.76 + violet * glow * 0.24 + grain;
    return vec4<f32>(color, 1.0);
}
"#;

struct ShaderView {
    target: RenderTarget,
}

impl Render for ShaderView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(22.0))
            .p(px(32.0))
            .bg(rgb(0x080b17))
            .text_color(rgb(0xf3f5ff))
            .child(div().text_size(px(28.0)).child("A canvas made of light"))
            .child(
                div()
                    .text_size(px(14.0))
                    .text_color(rgb(0x919ab7))
                    .child("Validated WGSL · cached GPU pipeline · direct texture composition"),
            )
            .child(
                render_target(self.target.clone())
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .object_fit(ObjectFit::Fill)
                    .rounded(px(22.0))
                    .border_1()
                    .border_color(rgb(0x26324d)),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(rgb(0x919ab7))
                    .child("One reusable target. No media decoder, image upload, or CPU readback."),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let shader = cx
            .register_fragment_shader(ShaderDescriptor::fragment(
                "light_ribbon",
                FRAGMENT,
                "fs_main",
            ))
            .expect("valid portable shader");
        let bounds = Bounds::centered(None, size(px(900.0), px(640.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("Kael custom shaders");
                    let target = window
                        .create_render_target(RenderTargetDescriptor::rgba8(1280, 800))
                        .expect("bounded GPU target");
                    let uniform: Vec<u8> = [0.7f32, 0.0, 0.0, 0.0]
                        .into_iter()
                        .flat_map(f32::to_ne_bytes)
                        .collect();
                    window
                        .render_shader(
                            &target,
                            &shader,
                            &ShaderBindings::new().with(7, ShaderBinding::Uniform(uniform.into())),
                        )
                        .expect("GPU fragment execution");
                    cx.new(|_| ShaderView { target })
                },
            )
            .expect("open shader example window");
        cx.activate(true);
        if let Some(output) = std::env::var_os("KAEL_SHADER_CAPTURE_PATH") {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let image = window
                    .update(cx, |_, window, _| window.export_frame_png())
                    .expect("live shader window")
                    .expect("presented shader frame");
                std::fs::write(&output, image.bytes()).expect("write shader capture");
                println!("CUSTOM_SHADER_GPU_UI_CAPTURE: {} bytes", image.byte_len());
                cx.update(|cx| cx.quit()).expect("quit capture app");
            })
            .detach();
        }
    });
}
