//! Identical graph contracts require actual WARP execution in Windows CI.
use super::custom_shaders::DirectXCustomRenderer;
use crate::render_graph::{
    GpuGraphRenderer,
    gpu_tests::{NativeGraphTestRenderer, native_graph_tests},
};
use crate::{
    RenderTarget, RenderTargetDescriptor, RenderTargetError, ShaderBindings, ShaderHandle,
};
use windows::Win32::{
    Foundation::HMODULE,
    Graphics::{Direct3D::*, Direct3D11::*},
};

struct WarpGraphRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    renderer: DirectXCustomRenderer,
}
impl GpuGraphRenderer for WarpGraphRenderer {
    fn context_id(&self) -> u64 {
        self as *const Self as usize as u64
    }
    fn validate(&self, target: &RenderTarget) -> Result<(), RenderTargetError> {
        self.renderer.validate(&self.device, target)
    }
    fn create(
        &mut self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget, RenderTargetError> {
        self.renderer
            .create(&self.device, &self.context, descriptor)
    }
    fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<(), RenderTargetError> {
        self.renderer
            .render(&self.device, &self.context, target, shader, bindings)
    }
    fn validate_buffer(&self, buffer: &crate::GpuBuffer) -> Result<(), RenderTargetError> {
        self.renderer.validate_buffer(&self.device, buffer)
    }
    fn create_buffer(
        &mut self,
        descriptor: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer, RenderTargetError> {
        self.renderer
            .create_buffer(&self.device, &self.context, descriptor)
    }
    fn dispatch(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<(), RenderTargetError> {
        self.renderer
            .dispatch(&self.device, &self.context, shader, bindings, groups)
    }
}
impl NativeGraphTestRenderer for WarpGraphRenderer {
    fn new() -> Self {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .expect("required WARP graph device");
        Self {
            device: device.unwrap(),
            context: context.unwrap(),
            renderer: DirectXCustomRenderer::default(),
        }
    }
    fn read_target(
        &mut self,
        target: &RenderTarget,
    ) -> Result<crate::RenderTargetReadback, RenderTargetError> {
        self.renderer.read(&self.device, &self.context, target)
    }
    fn read_buffer(&mut self, buffer: &crate::GpuBuffer) -> Result<Vec<u8>, RenderTargetError> {
        self.renderer
            .read_buffer(&self.device, &self.context, buffer)
    }
    fn allocated_bytes(&self) -> u64 {
        self.renderer.graph_allocated_bytes()
    }
}
native_graph_tests!(WarpGraphRenderer);
