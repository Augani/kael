//! Native compute → fragment → GPU UI composition, without pixel downloads.
//! `cargo run -p kael --example compute_graph --features custom-shaders`
use kael::gpu_graph::{PassDesc, RenderGraph, ResourceDesc};
use kael::prelude::*;
use kael::{
    App, Application, Bounds, ComputeDescriptor, Context, GpuComputePass, GpuFragmentPass,
    GpuGraphBinding, GpuGraphPass, GpuGraphResourceDescriptor, GpuRenderGraph, ObjectFit, Render,
    RenderTarget, RenderTargetDescriptor, ShaderDescriptor, Window, WindowBounds, WindowOptions,
    div, px, render_target, rgb, size,
};

const COMPUTE: &str = r#"
@group(0) @binding(7) var image: texture_storage_2d<rgba16float, write>;
@compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let dimensions = textureDimensions(image);
    if any(id.xy >= dimensions) { return; }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(dimensions);
    let wave = 0.5 + 0.5 * sin(uv.x * 7.0 + sin(uv.y * 6.0));
    let halo = exp(-length((uv - vec2<f32>(0.65,0.45)) * vec2<f32>(1.0,1.4)) * 5.0);
    let dusk = vec3<f32>(0.045,0.065,0.16);
    let dawn = vec3<f32>(1.4,0.42,0.16);
    let color = mix(dusk,dawn,pow(wave,6.0)) + vec3<f32>(0.08,0.45,0.75) * halo;
    textureStore(image,vec2<i32>(id.xy),vec4<f32>(color,1.0));
}"#;
const FRAGMENT: &str = r#"
@group(0) @binding(5) var image: texture_2d<f32>;
@fragment fn main(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> {
    let color = textureLoad(image,vec2<i32>(p.xy),0).rgb;
    return vec4<f32>(color / (vec3<f32>(1.0) + color),1.0);
}"#;

struct GraphView {
    target: RenderTarget,
    // Retain the bounded reusable graph; exported handles also pin outputs.
    _graph: GpuRenderGraph,
}
impl Render for GraphView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().p(px(32.0)).gap(px(20.0))
            .bg(rgb(0x0b101b)).text_color(rgb(0xf5f4f0))
            .child(div().text_size(px(28.0)).child("A landscape in two passes"))
            .child(div().text_size(px(14.0)).text_color(rgb(0xa0abbc))
                .child("Native compute creates an HDR image. A fragment pass maps its light to the display."))
            .child(render_target(self.target.clone()).flex_1().w_full().min_h_0()
                .object_fit(ObjectFit::Fill).rounded(px(20.0)))
    }
}
fn main() {
    Application::new().run(|cx: &mut App| {
        let kernel = cx
            .register_compute_shader(ComputeDescriptor::new("landscape", COMPUTE, "main"))
            .expect("portable native compute program");
        let tone_map = cx
            .register_fragment_shader(ShaderDescriptor::fragment("tone_map", FRAGMENT, "main"))
            .expect("portable fragment program");
        let bounds = Bounds::centered(None, size(px(900.0), px(640.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("Kael compute graph");
                    let hdr = RenderTargetDescriptor {
                        width: 1024,
                        height: 640,
                        format: kael::RenderTargetFormat::Rgba16Float,
                    };
                    let display = RenderTargetDescriptor::rgba8(1024, 640);
                    let mut topology = RenderGraph::new();
                    let source = topology.add_resource(
                        ResourceDesc::transient_texture("hdr")
                            .allocation_class(GpuRenderGraph::allocation_class(hdr)),
                    );
                    let output = topology.add_resource(
                        ResourceDesc::transient_texture("display")
                            .allocation_class(GpuRenderGraph::allocation_class(display)),
                    );
                    let first = topology.add_pass(PassDesc::new("compute_landscape").write(source));
                    let second =
                        topology.add_pass(PassDesc::new("tone_map").read(source).write(output));
                    let mut graph = GpuRenderGraph::new_with_resources(
                        topology,
                        &[
                            (source, GpuGraphResourceDescriptor::Texture(hdr)),
                            (output, GpuGraphResourceDescriptor::Texture(display)),
                        ],
                        &[output],
                    )
                    .expect("checked graph lifetimes");
                    let programs = [
                        (
                            first,
                            GpuGraphPass::Compute(
                                GpuComputePass::new(kernel, [128, 80, 1])
                                    .with(7, GpuGraphBinding::Texture(source)),
                            ),
                        ),
                        (
                            second,
                            GpuGraphPass::Fragment(
                                GpuFragmentPass::new(tone_map)
                                    .with(5, GpuGraphBinding::Texture(source)),
                            ),
                        ),
                    ];
                    let rendered = window
                        .execute_gpu_graph(&mut graph, &[], &programs)
                        .expect("native GPU graph");
                    let cached = window
                        .execute_gpu_graph(&mut graph, &[], &programs)
                        .expect("cached graph");
                    assert_eq!((rendered.executed_passes, cached.skipped_passes), (2, 2));
                    println!(
                        "COMPUTE_GRAPH_GPU_UI: submitted=2 cached=2 physical_bytes={}",
                        graph.physical_byte_len()
                    );
                    cx.new(|_| GraphView {
                        target: rendered.outputs[&output].clone(),
                        _graph: graph,
                    })
                },
            )
            .expect("native graph window");
        cx.activate(true);
        if let Some(path) = std::env::var_os("KAEL_GRAPH_CAPTURE_PATH") {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let image = window
                    .update(cx, |_, window, _| window.export_frame_png())
                    .expect("live graph window")
                    .expect("GPU graph capture");
                std::fs::write(path, image.bytes()).expect("save graph evidence");
                println!("COMPUTE_GRAPH_GPU_UI_CAPTURE: {} bytes", image.byte_len());
                cx.update(|cx| cx.quit()).expect("quit capture app");
            })
            .detach();
        }
    });
}
