//! Shared graph contracts executed by the real Blade backend.
use super::custom_shaders::BladeCustomRenderer;
use crate::render_graph::{
    GpuGraphRenderer,
    gpu_tests::{NativeGraphTestRenderer, native_graph_tests},
};
use crate::{
    RenderTarget, RenderTargetDescriptor, RenderTargetError, ShaderBindings, ShaderHandle,
};
use blade_graphics as gpu;
use std::sync::Arc;

impl GpuGraphRenderer for BladeCustomRenderer {
    fn context_id(&self) -> u64 {
        self as *const Self as usize as u64
    }
    fn validate(&self, target: &RenderTarget) -> Result<(), RenderTargetError> {
        self.validate(target)
    }
    fn create(
        &mut self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget, RenderTargetError> {
        self.create(descriptor)
    }
    fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<(), RenderTargetError> {
        self.render(target, shader, bindings)
    }
    fn validate_buffer(&self, buffer: &crate::GpuBuffer) -> Result<(), RenderTargetError> {
        self.validate_buffer(buffer)
    }
    fn create_buffer(
        &mut self,
        descriptor: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer, RenderTargetError> {
        self.create_buffer(descriptor)
    }
    fn dispatch(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<(), RenderTargetError> {
        self.dispatch(shader, bindings, groups)
    }
}
impl NativeGraphTestRenderer for BladeCustomRenderer {
    fn new() -> Self {
        // No OS window is required; every graph pass uses a native GPU target.
        let context = unsafe {
            gpu::Context::init(gpu::ContextDesc {
                presentation: false,
                validation: false,
                ..Default::default()
            })
        }
        .expect("required native Blade graph device");
        BladeCustomRenderer::new(Arc::new(context))
    }
    fn read_target(
        &mut self,
        target: &RenderTarget,
    ) -> Result<crate::RenderTargetReadback, RenderTargetError> {
        self.read(target)
    }
    fn read_buffer(&mut self, buffer: &crate::GpuBuffer) -> Result<Vec<u8>, RenderTargetError> {
        self.read_buffer(buffer)
    }
    fn allocated_bytes(&self) -> u64 {
        self.graph_allocated_bytes()
    }
}
native_graph_tests!(BladeCustomRenderer);
