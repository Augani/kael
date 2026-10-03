//! Identical actual-device graph contracts across every native backend.
#![cfg(any(
    target_os = "macos",
    target_os = "windows",
    all(
        any(target_os = "linux", target_os = "freebsd"),
        any(feature = "x11", feature = "wayland"),
        not(feature = "webview-wayland-gtk4")
    )
))]
use crate::render_graph::GpuGraphRenderer;
use crate::{
    GpuFragmentPass, GpuGraphBinding, GpuGraphError, GpuRenderGraph, MemoryPressureLevel,
    RenderTarget, RenderTargetDescriptor, RenderTargetError, ShaderBindings, ShaderDescriptor,
    ShaderHandle,
};
use kael_render_graph::{PassDesc, RenderGraph, ResourceDesc, ResourceId};
pub(crate) trait NativeGraphTestRenderer: GpuGraphRenderer {
    fn new() -> Self;
    fn read_target(
        &mut self,
        target: &RenderTarget,
    ) -> Result<crate::RenderTargetReadback, RenderTargetError>;
    fn read_buffer(&mut self, buffer: &crate::GpuBuffer) -> Result<Vec<u8>, RenderTargetError>;
    /// Native driver bytes where available; tracked target bytes otherwise.
    fn allocated_bytes(&self) -> u64;
}

#[cfg(all(
    feature = "custom-shaders",
    any(
        target_os = "macos",
        target_os = "windows",
        all(
            any(target_os = "linux", target_os = "freebsd"),
            any(feature = "x11", feature = "wayland"),
            not(feature = "webview-wayland-gtk4")
        )
    )
))]
macro_rules! native_graph_tests {
    ($renderer:ty) => {
        #[test]
        fn graph_executes_native_dag_and_invalidates_exact_inputs_and_outputs() { $crate::render_graph::gpu_tests::graph_executes_native_dag_and_invalidates_exact_inputs_and_outputs::<$renderer>(); }
        #[test]
        fn graph_aliases_dead_intermediates_and_preserves_all_exports() { $crate::render_graph::gpu_tests::graph_aliases_dead_intermediates_and_preserves_all_exports::<$renderer>(); }
        #[test]
        fn graph_admits_uniforms_and_imports_before_gpu_submission() { $crate::render_graph::gpu_tests::graph_admits_uniforms_and_imports_before_gpu_submission::<$renderer>(); }
        #[test]
        fn mixed_graph_executes_buffer_compute_texture_compute_and_fragment_on_gpu() { $crate::render_graph::gpu_tests::mixed_graph_executes_buffer_compute_texture_compute_and_fragment_on_gpu::<$renderer>(); }
        #[test]
        fn graph_rejects_foreign_imports_and_recovers_after_output_invalidation() { $crate::render_graph::gpu_tests::graph_rejects_foreign_imports_and_recovers_after_output_invalidation::<$renderer>(); }
        #[test]
        fn image_compute_graph_caches_exact_uniforms_and_rejects_invalid_groups_before_allocation() { $crate::render_graph::gpu_tests::image_compute_graph_caches_exact_uniforms_and_rejects_invalid_groups_before_allocation::<$renderer>(); }
    };
}
#[cfg(all(
    feature = "custom-shaders",
    any(
        target_os = "macos",
        target_os = "windows",
        all(
            any(target_os = "linux", target_os = "freebsd"),
            any(feature = "x11", feature = "wayland"),
            not(feature = "webview-wayland-gtk4")
        )
    )
))]
pub(crate) use native_graph_tests;

fn shader(source: &str) -> ShaderHandle {
    ShaderHandle::compile_fragment(ShaderDescriptor::fragment(
        "graph_pixels",
        source,
        "fs_main",
    ))
    .unwrap()
}

const COPY: &str = r#"
@group(0) @binding(5) var input: texture_2d<f32>;
@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(input, vec2<i32>(p.xy), 0);
}"#;

fn copy(id: ResourceId) -> GpuFragmentPass {
    GpuFragmentPass::new(shader(COPY)).with(5, GpuGraphBinding::Texture(id))
}

fn expect_pixel<R: NativeGraphTestRenderer>(
    renderer: &mut R,
    target: &RenderTarget,
    expected: [u8; 4],
) {
    let readback = renderer.read_target(target).unwrap();
    for pixel in readback.pixels.chunks_exact(4) {
        for (actual, expected) in pixel.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 1,
                "actual {pixel:?}, expected {expected}"
            );
        }
    }
}

pub(crate) fn graph_executes_native_dag_and_invalidates_exact_inputs_and_outputs<
    R: NativeGraphTestRenderer,
>() {
    let mut renderer = R::new();
    let desc = RenderTargetDescriptor::rgba8(8, 8);
    let imported = renderer.create(desc).unwrap();
    let red = shader(
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }",
    );
    let green = shader(
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }",
    );
    renderer
        .render(&imported, &red, &ShaderBindings::new())
        .unwrap();
    let mut graph = RenderGraph::new();
    let input = graph.add_resource(ResourceDesc::imported_texture("input"));
    let intermediate = graph.add_resource(ResourceDesc::transient_texture("intermediate"));
    let output = graph.add_resource(ResourceDesc::transient_texture("output"));
    let first = graph.add_pass(PassDesc::new("copy_input").read(input).write(intermediate));
    let second = graph.add_pass(
        PassDesc::new("copy_intermediate")
            .read(intermediate)
            .write(output),
    );
    let mut executor =
        GpuRenderGraph::new(graph, &[(intermediate, desc), (output, desc)], &[output]).unwrap();
    assert_eq!(executor.physical_slot_count(), 2);
    assert_eq!(executor.compiled().barriers().len(), 2);
    let programs = [(first, copy(input)), (second, copy(intermediate))];
    let result = executor
        .execute_with(&mut renderer, &[(input, imported.clone())], &programs)
        .unwrap();
    assert_eq!((result.executed_passes, result.skipped_passes), (2, 0));
    expect_pixel(&mut renderer, &result.outputs[&output], [255, 0, 0, 255]);
    let cached = executor
        .execute_with(&mut renderer, &[(input, imported.clone())], &programs)
        .unwrap();
    assert_eq!((cached.executed_passes, cached.skipped_passes), (0, 2));
    renderer
        .render(&imported, &green, &ShaderBindings::new())
        .unwrap();
    let changed = executor
        .execute_with(&mut renderer, &[(input, imported.clone())], &programs)
        .unwrap();
    assert_eq!(changed.executed_passes, 2);
    expect_pixel(&mut renderer, &changed.outputs[&output], [0, 255, 0, 255]);
    // A caller can write an exported target; exact output revisions prevent
    // treating that externally changed image as a valid cached graph result.
    renderer
        .render(&changed.outputs[&output], &red, &ShaderBindings::new())
        .unwrap();
    let repaired = executor
        .execute_with(&mut renderer, &[(input, imported.clone())], &programs)
        .unwrap();
    assert_eq!((repaired.executed_passes, repaired.skipped_passes), (1, 1));
    expect_pixel(&mut renderer, &repaired.outputs[&output], [0, 255, 0, 255]);
    executor.handle_memory_pressure(MemoryPressureLevel::Critical);
    assert!(repaired.outputs[&output].is_valid());
    let after_pressure = executor
        .execute_with(&mut renderer, &[(input, imported)], &programs)
        .unwrap();
    assert_eq!(after_pressure.executed_passes, 2);
    assert_ne!(after_pressure.outputs[&output], repaired.outputs[&output]);
    expect_pixel(
        &mut renderer,
        &after_pressure.outputs[&output],
        [0, 255, 0, 255],
    );
}

pub(crate) fn graph_aliases_dead_intermediates_and_preserves_all_exports<
    R: NativeGraphTestRenderer,
>() {
    let mut renderer = R::new();
    let desc = RenderTargetDescriptor::rgba8(4, 4);
    let mut graph = RenderGraph::new();
    let resources: Vec<_> = (0..4)
        .map(|index| graph.add_resource(ResourceDesc::transient_texture(format!("r{index}"))))
        .collect();
    let producer = graph.add_pass(PassDesc::new("produce").write(resources[0]));
    let consumers: Vec<_> = (1..4)
        .map(|index| {
            graph.add_pass(
                PassDesc::new(format!("copy{index}"))
                    .read(resources[index - 1])
                    .write(resources[index]),
            )
        })
        .collect();
    let mut executor = GpuRenderGraph::new(
        graph,
        &resources.iter().map(|id| (*id, desc)).collect::<Vec<_>>(),
        &[resources[1], resources[3]],
    )
    .unwrap();
    // r0 and r2 share storage; r1 remains live until exported at the tail.
    assert_eq!(executor.physical_slot_count(), 3);
    assert_eq!(executor.physical_byte_len(), 3 * 4 * 4 * 4);
    let blue = shader(
        "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }",
    );
    let mut programs = vec![(producer, GpuFragmentPass::new(blue))];
    programs.extend(
        consumers
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, copy(resources[i]))),
    );
    for _ in 0..2 {
        let result = executor
            .execute_with(&mut renderer, &[], &programs)
            .unwrap();
        // Alias residency prevents a stale producer hit on the second run.
        assert_eq!(result.executed_passes, 4);
        for target in result.outputs.values() {
            expect_pixel(&mut renderer, target, [0, 0, 255, 255]);
        }
        assert_ne!(result.outputs[&resources[1]], result.outputs[&resources[3]]);
    }
}

pub(crate) fn graph_admits_uniforms_and_imports_before_gpu_submission<
    R: NativeGraphTestRenderer,
>() {
    let mut renderer = R::new();
    let desc = RenderTargetDescriptor::rgba8(4, 4);
    let mut graph = RenderGraph::new();
    let output = graph.add_resource(ResourceDesc::transient_texture("output"));
    let pass = graph.add_pass(PassDesc::new("uniform_color").write(output));
    let mut executor = GpuRenderGraph::new(graph, &[(output, desc)], &[output]).unwrap();
    let uniform = shader(
        "struct Color { rgba: vec4<f32> } @group(0) @binding(11) var<uniform> color: Color; @fragment fn fs_main() -> @location(0) vec4<f32> { return color.rgba; }",
    );
    let invalid =
        GpuFragmentPass::new(uniform.clone()).with(11, GpuGraphBinding::Uniform(vec![0; 4].into()));
    assert!(matches!(
        executor.execute_with(&mut renderer, &[], &[(pass, invalid)]),
        Err(GpuGraphError::Invalid(_))
    ));
    let bytes = |color: [f32; 4]| {
        color
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
            .into()
    };
    let program = |color| {
        GpuFragmentPass::new(uniform.clone()).with(11, GpuGraphBinding::Uniform(bytes(color)))
    };
    let result = executor
        .execute_with(&mut renderer, &[], &[(pass, program([1.0, 0.0, 0.0, 1.0]))])
        .unwrap();
    expect_pixel(&mut renderer, &result.outputs[&output], [255, 0, 0, 255]);
    let result = executor
        .execute_with(&mut renderer, &[], &[(pass, program([0.0, 1.0, 0.0, 1.0]))])
        .unwrap();
    assert_eq!(result.executed_passes, 1);
    expect_pixel(&mut renderer, &result.outputs[&output], [0, 255, 0, 255]);
    executor.set_byte_budget(0);
    assert!(matches!(
        executor.execute_with(&mut renderer, &[], &[(pass, program([1.0; 4]))]),
        Err(GpuGraphError::ResourceLimit(_))
    ));
}

pub(crate) fn mixed_graph_executes_buffer_compute_texture_compute_and_fragment_on_gpu<
    R: NativeGraphTestRenderer,
>() {
    use crate::{
        ComputeDescriptor, ComputeHandle, GpuBufferDescriptor, GpuComputePass, GpuGraphPass,
        GpuGraphResourceDescriptor,
    };
    let mut renderer = R::new();
    let desc = RenderTargetDescriptor::rgba8(8, 8);
    let buffer_desc = GpuBufferDescriptor { byte_len: 64 * 16 };
    let fill = ComputeHandle::compile(ComputeDescriptor::new(
        "graph_fill",
        r#"
@group(0) @binding(3) var<storage, read_write> values: array<vec4<f32>>;
@compute @workgroup_size(4) fn cs_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x < arrayLength(&values) { values[id.x] = vec4<f32>(0.0, 1.0, 0.0, 1.0); }
}"#,
        "cs_main",
    ))
    .unwrap();
    let image = ComputeHandle::compile(ComputeDescriptor::new(
        "graph_image",
        r#"
@group(0) @binding(4) var<storage, read> values: array<vec4<f32>>;
@group(0) @binding(9) var image: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(4,4) fn cs_main(@builtin(global_invocation_id) id:vec3<u32>) {
    let dimensions = textureDimensions(image);
    if all(id.xy < dimensions) {
        textureStore(image, vec2<i32>(id.xy), values[id.x + id.y * dimensions.x]);
    }
}"#,
        "cs_main",
    ))
    .unwrap();
    let mut graph = RenderGraph::new();
    let buffer = graph.add_resource(
        ResourceDesc::transient_buffer("values").allocation_class(buffer_desc.byte_len),
    );
    let texture = graph.add_resource(ResourceDesc::transient_texture("compute_image"));
    let output = graph.add_resource(ResourceDesc::transient_texture("fragment_image"));
    let first = graph.add_pass(PassDesc::new("fill").write(buffer));
    let second = graph.add_pass(PassDesc::new("image").read(buffer).write(texture));
    let third = graph.add_pass(PassDesc::new("display").read(texture).write(output));
    let mut executor = GpuRenderGraph::new_with_resources(
        graph,
        &[
            (buffer, GpuGraphResourceDescriptor::Buffer(buffer_desc)),
            (texture, GpuGraphResourceDescriptor::Texture(desc)),
            (output, GpuGraphResourceDescriptor::Texture(desc)),
        ],
        &[buffer, output],
    )
    .unwrap();
    assert_eq!(executor.physical_slot_count(), 3);
    let programs = [
        (
            first,
            GpuGraphPass::Compute(
                GpuComputePass::new(fill, [16, 1, 1]).with(3, GpuGraphBinding::Buffer(buffer)),
            ),
        ),
        (
            second,
            GpuGraphPass::Compute(
                GpuComputePass::new(image, [2, 2, 1])
                    .with(4, GpuGraphBinding::Buffer(buffer))
                    .with(9, GpuGraphBinding::Texture(texture)),
            ),
        ),
        (third, GpuGraphPass::Fragment(copy(texture))),
    ];
    for _ in 0..2 {
        let result = executor
            .execute_programs_with(&mut renderer, &[], &programs)
            .unwrap();
        assert_eq!(
            result.executed_passes, 3,
            "read/write buffer kernels must not be incorrectly cached"
        );
        expect_pixel(&mut renderer, &result.outputs[&output], [0, 255, 0, 255]);
        let bytes = renderer.read_buffer(&result.buffers[&buffer]).unwrap();
        assert_eq!(bytes.len(), 1024);
        for value in bytes.chunks_exact(16) {
            let channels: Vec<_> = value
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(channels, [0.0, 1.0, 0.0, 1.0]);
        }
    }
}

pub(crate) fn graph_rejects_foreign_imports_and_recovers_after_output_invalidation<
    R: NativeGraphTestRenderer,
>() {
    let mut renderer = R::new();
    let mut other = R::new();
    let desc = RenderTargetDescriptor::rgba8(4, 4);
    let foreign = other.create(desc).unwrap();
    let own = renderer.create(desc).unwrap();
    let mut graph = RenderGraph::new();
    let input = graph.add_resource(ResourceDesc::imported_texture("input"));
    let output = graph.add_resource(ResourceDesc::transient_texture("output"));
    let pass = graph.add_pass(PassDesc::new("copy").read(input).write(output));
    let mut executor = GpuRenderGraph::new(graph, &[(output, desc)], &[output]).unwrap();
    let programs = [(pass, copy(input))];
    let before = renderer.allocated_bytes();
    assert!(matches!(
        executor.execute_with(&mut renderer, &[(input, foreign)], &programs),
        Err(GpuGraphError::Target(RenderTargetError::WrongDevice))
    ));
    assert_eq!(
        renderer.allocated_bytes(),
        before,
        "reject wrong-device imports before creating graph targets"
    );
    let green = shader(
        "@fragment fn fs_main()->@location(0) vec4<f32>{return vec4<f32>(0.0,1.0,0.0,1.0);}",
    );
    renderer
        .render(&own, &green, &ShaderBindings::new())
        .unwrap();
    let result = executor
        .execute_with(&mut renderer, &[(input, own.clone())], &programs)
        .unwrap();
    result.outputs[&output].invalidate();
    let recovered = executor
        .execute_with(&mut renderer, &[(input, own)], &programs)
        .unwrap();
    assert_eq!(recovered.executed_passes, 1);
    assert_ne!(recovered.outputs[&output], result.outputs[&output]);
    expect_pixel(&mut renderer, &recovered.outputs[&output], [0, 255, 0, 255]);
}

pub(crate) fn image_compute_graph_caches_exact_uniforms_and_rejects_invalid_groups_before_allocation<
    R: NativeGraphTestRenderer,
>() {
    use crate::{ComputeDescriptor, ComputeHandle, GpuComputePass, GpuGraphPass};
    let mut renderer = R::new();
    let desc = RenderTargetDescriptor::rgba8(8, 8);
    let mut graph = RenderGraph::new();
    let image = graph.add_resource(ResourceDesc::transient_texture("image"));
    let output = graph.add_resource(ResourceDesc::transient_texture("output"));
    let first = graph.add_pass(PassDesc::new("compute").write(image));
    let second = graph.add_pass(PassDesc::new("fragment").read(image).write(output));
    let shader = ComputeHandle::compile(ComputeDescriptor::new(
        "graph_color",
        r#"
struct Color { rgba: vec4<f32> }
@group(0) @binding(0) var<uniform> color: Color;
@group(0) @binding(7) var image: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(4,4) fn cs_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if all(id.xy < textureDimensions(image)) { textureStore(image,vec2<i32>(id.xy),color.rgba); }
}"#,
        "cs_main",
    ))
    .unwrap();
    let program = |groups, color: [f32; 4]| {
        GpuGraphPass::Compute(
            GpuComputePass::new(shader.clone(), groups)
                .with(
                    0,
                    GpuGraphBinding::Uniform(
                        color
                            .into_iter()
                            .flat_map(f32::to_le_bytes)
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                )
                .with(7, GpuGraphBinding::Texture(image)),
        )
    };
    let copy = GpuGraphPass::Fragment(copy(image));
    let mut executor =
        GpuRenderGraph::new(graph, &[(image, desc), (output, desc)], &[output]).unwrap();
    let before = renderer.allocated_bytes();
    assert!(matches!(
        executor.execute_programs_with(
            &mut renderer,
            &[],
            &[
                (first, program([0, 2, 1], [1.0, 0.0, 0.0, 1.0])),
                (second, copy.clone())
            ]
        ),
        Err(GpuGraphError::Invalid(_))
    ));
    assert_eq!(renderer.allocated_bytes(), before);
    for (color, expected, executed) in [
        ([1.0, 0.0, 0.0, 1.0], [255, 0, 0, 255], 2),
        ([1.0, 0.0, 0.0, 1.0], [255, 0, 0, 255], 0),
        ([0.0, 1.0, 0.0, 1.0], [0, 255, 0, 255], 2),
    ] {
        let result = executor
            .execute_programs_with(
                &mut renderer,
                &[],
                &[(first, program([2, 2, 1], color)), (second, copy.clone())],
            )
            .unwrap();
        assert_eq!(result.executed_passes, executed);
        assert_eq!(result.skipped_passes, 2 - executed);
        expect_pixel(&mut renderer, &result.outputs[&output], expected);
    }
}
