//! Required actual-Metal graph pixels and lifecycle regressions.
use super::{InstanceBufferPool, MetalRenderer};
use crate::render_graph::{
    GpuGraphRenderer,
    gpu_tests::{NativeGraphTestRenderer, native_graph_tests},
};
use crate::{
    RenderTarget, RenderTargetDescriptor, RenderTargetError, ShaderBindings, ShaderHandle,
};
use parking_lot::Mutex;
use std::sync::Arc;
impl GpuGraphRenderer for MetalRenderer {
    fn context_id(&self) -> u64 {
        self as *const Self as usize as u64
    }
    fn validate(&self, target: &RenderTarget) -> Result<(), RenderTargetError> {
        self.validate_render_target(target)
    }
    fn create(&mut self, desc: RenderTargetDescriptor) -> Result<RenderTarget, RenderTargetError> {
        self.create_render_target(desc)
    }
    fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<(), RenderTargetError> {
        self.render_shader(target, shader, bindings)
    }
    fn validate_buffer(&self, buffer: &crate::GpuBuffer) -> Result<(), RenderTargetError> {
        self.validate_gpu_buffer(buffer)
    }
    fn create_buffer(
        &mut self,
        desc: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer, RenderTargetError> {
        self.create_gpu_buffer(desc)
    }
    fn dispatch(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<(), RenderTargetError> {
        self.dispatch_compute(shader, bindings, groups)
    }
}

impl NativeGraphTestRenderer for MetalRenderer {
    fn new() -> Self {
        assert!(
            super::metal_is_available(),
            "Metal device required; absence is not a passing GPU test"
        );
        MetalRenderer::try_new(Arc::new(Mutex::new(InstanceBufferPool::default()))).unwrap()
    }
    fn read_target(
        &mut self,
        target: &RenderTarget,
    ) -> Result<crate::RenderTargetReadback, RenderTargetError> {
        self.read_render_target(target)
    }
    fn read_buffer(&mut self, buffer: &crate::GpuBuffer) -> Result<Vec<u8>, RenderTargetError> {
        self.read_gpu_buffer(buffer)
    }
    fn allocated_bytes(&self) -> u64 {
        self.device.current_allocated_size()
    }
}
native_graph_tests!(MetalRenderer);
