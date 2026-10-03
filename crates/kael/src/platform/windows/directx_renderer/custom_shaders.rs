use std::{collections::BTreeMap, ffi::CString};

use windows::{
    Win32::Graphics::{Direct3D::Fxc::*, Direct3D::*, Direct3D11::*, Dxgi::Common::*},
    core::PCSTR,
};

use crate::{
    DevicePixels, PaintSurface, RenderTarget, RenderTargetDescriptor, RenderTargetError,
    RenderTargetFormat, RenderTargetReadback, ShaderBackend, ShaderBinding, ShaderBindings,
    ShaderHandle, ShaderResourceSlot, ShaderResourceSlotKind, ShaderSampler, Size,
    render_target::{RenderTargetDisplayParams, TargetRegistry},
};

type Result<T> = std::result::Result<T, RenderTargetError>;

struct TargetTexture {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    output: ID3D11RenderTargetView,
    storage: Option<ID3D11UnorderedAccessView>,
}

struct StorageBuffer {
    raw: ID3D11Buffer,
    read: ID3D11ShaderResourceView,
    write: ID3D11UnorderedAccessView,
}
struct ComputePipeline {
    state: ID3D11ComputeShader,
    translation: crate::ComputeTranslation,
    uniforms: BTreeMap<u32, ID3D11Buffer>,
    used: u64,
}
struct CustomPipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    blend: ID3D11BlendState,
    resources: Vec<ShaderResourceSlot>,
    uniforms: BTreeMap<u32, ID3D11Buffer>,
    used: u64,
}

struct DisplayPipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    blend: ID3D11BlendState,
    params: ID3D11Buffer,
}

pub(super) struct DirectXCustomRenderer {
    targets: TargetRegistry<TargetTexture>,
    buffers: crate::compute::BufferRegistry<StorageBuffer>,
    compute: BTreeMap<u64, ComputePipeline>,
    pipelines: BTreeMap<(u64, RenderTargetFormat), CustomPipeline>,
    display: Option<DisplayPipeline>,
    samplers: BTreeMap<ShaderSampler, ID3D11SamplerState>,
    tick: u64,
    #[cfg(test)]
    compilations: usize,
}

impl Default for DirectXCustomRenderer {
    fn default() -> Self {
        let targets = TargetRegistry::default();
        let buffers = crate::compute::BufferRegistry::new(targets.device_budget());
        Self {
            targets,
            buffers,
            compute: Default::default(),
            pipelines: Default::default(),
            display: None,
            samplers: Default::default(),
            tick: 0,
            #[cfg(test)]
            compilations: 0,
        }
    }
}

fn require_compute(device: &ID3D11Device) -> Result<()> {
    if unsafe { device.GetFeatureLevel() }.0 < D3D_FEATURE_LEVEL_11_0.0 {
        return Err(RenderTargetError::Unsupported(
            "compute storage requires DirectX feature level 11.0",
        ));
    }
    Ok(())
}
fn backend(error: impl std::fmt::Display) -> RenderTargetError {
    RenderTargetError::Backend(error.to_string())
}

fn format(format: RenderTargetFormat) -> DXGI_FORMAT {
    match format {
        RenderTargetFormat::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
        RenderTargetFormat::Rgba8UnormSrgb => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        RenderTargetFormat::Bgra8UnormSrgb => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        RenderTargetFormat::Rgba16Float => DXGI_FORMAT_R16G16B16A16_FLOAT,
        RenderTargetFormat::R8Unorm => DXGI_FORMAT_R8_UNORM,
    }
}

fn texture_desc(descriptor: RenderTargetDescriptor) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: descriptor.width,
        Height: descriptor.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: format(descriptor.format),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    }
}

impl DirectXCustomRenderer {
    #[cfg(test)]
    pub(super) fn graph_allocated_bytes(&self) -> u64 {
        self.targets.used_bytes()
    }
    fn check_device(&self, device: &ID3D11Device) -> Result<()> {
        if let Err(error) = unsafe { device.GetDeviceRemovedReason() } {
            self.targets.invalidate_device();
            return Err(backend(error));
        }
        Ok(())
    }

    pub(super) fn create(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget> {
        self.check_device(device)?;
        let bytes = self.targets.check_request(descriptor)?;
        if unsafe { device.GetFeatureLevel() }.0 < D3D_FEATURE_LEVEL_11_0.0
            && (descriptor.width > 8192 || descriptor.height > 8192)
        {
            return Err(RenderTargetError::Unsupported(
                "DirectX 10.1 targets cannot exceed 8192 pixels",
            ));
        }
        let support =
            unsafe { device.CheckFormatSupport(format(descriptor.format)) }.map_err(backend)?;
        let required = (D3D11_FORMAT_SUPPORT_TEXTURE2D.0
            | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0
            | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0
            | D3D11_FORMAT_SUPPORT_BLENDABLE.0) as u32;
        if support & required != required {
            return Err(RenderTargetError::Unsupported(
                "DirectX target format is not renderable, blendable and sampleable",
            ));
        }
        // Impossible requests preserve all residency. D3D11 owns GPU references
        // after submission and defers destruction of released COM resources.
        drop(self.targets.take_unused());
        drop(self.buffers.take_unused());
        self.targets.check_allocation(descriptor)?;
        let mut texture = None;
        let mut native_desc = texture_desc(descriptor);
        let supports_storage = unsafe { device.GetFeatureLevel() }.0 >= D3D_FEATURE_LEVEL_11_0.0
            && matches!(
                descriptor.format,
                RenderTargetFormat::Rgba8Unorm | RenderTargetFormat::Rgba16Float
            );
        if supports_storage {
            native_desc.BindFlags |= D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        }
        unsafe { device.CreateTexture2D(&native_desc, None, Some(&mut texture)) }
            .map_err(backend)?;
        let texture = super::require_com_output(texture, "CreateTexture2D for custom target")
            .map_err(backend)?;
        let mut view = None;
        let mut output = None;
        unsafe {
            device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
                .map_err(backend)?;
            device
                .CreateRenderTargetView(&texture, None, Some(&mut output))
                .map_err(backend)?;
        }
        let mut storage = None;
        if supports_storage {
            unsafe { device.CreateUnorderedAccessView(&texture, None, Some(&mut storage)) }
                .map_err(backend)?;
        }
        let resource = TargetTexture {
            storage,
            texture,
            view: super::require_com_output(view, "CreateShaderResourceView for custom target")
                .map_err(backend)?,
            output: super::require_com_output(output, "CreateRenderTargetView for custom target")
                .map_err(backend)?,
        };
        // Newly created targets have defined transparent contents even before
        // their first render. Clear is ordered on the same immediate context.
        unsafe { context.ClearRenderTargetView(&resource.output, &[0.0; 4]) };
        self.targets.insert(descriptor, resource, bytes)
    }

    pub(super) fn validate(&self, device: &ID3D11Device, target: &RenderTarget) -> Result<()> {
        self.check_device(device)?;
        self.targets.get(target).map(|_| ())
    }

    pub(super) fn set_budget(&mut self, bytes: u64) {
        self.targets.set_budget(bytes);
    }

    pub(super) fn shed(&mut self) {
        drop(self.targets.take_unused());
        self.pipelines.clear();
        self.compute.clear();
        drop(self.buffers.take_unused());
        self.samplers.clear();
        self.display = None;
    }

    fn next_tick(&mut self) -> u64 {
        if self.tick == u64::MAX {
            let mut age: Vec<_> = self
                .pipelines
                .iter()
                .map(|(key, pipeline)| (*key, pipeline.used))
                .collect();
            age.sort_by_key(|(key, used)| (*used, *key));
            for (rank, (key, _)) in age.into_iter().enumerate() {
                self.pipelines.get_mut(&key).unwrap().used = rank as u64;
            }
            self.tick = self.pipelines.len() as u64;
        }
        self.tick += 1;
        self.tick
    }

    pub(super) fn render(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<()> {
        self.check_device(device)?;
        bindings.validate(shader, target, &self.targets)?;
        let key = (shader.id(), target.descriptor().format);
        let tick = self.next_tick();
        if !self.pipelines.contains_key(&key) {
            let translation = shader
                .translate(ShaderBackend::DirectX11)
                .map_err(backend)?;
            let (vertex, fragment) = compile_pipeline(
                device,
                &translation.source,
                &translation.vertex_entry,
                &translation.fragment_entry,
            )?;
            let blend = super::create_blend_state(device).map_err(backend)?;
            if self.pipelines.len() >= 64 {
                let oldest = self
                    .pipelines
                    .iter()
                    .min_by_key(|(key, pipeline)| (pipeline.used, **key))
                    .map(|(key, _)| *key)
                    .unwrap();
                self.pipelines.remove(&oldest);
            }
            self.pipelines.insert(
                key,
                CustomPipeline {
                    vertex,
                    fragment,
                    blend,
                    resources: translation.resources,
                    uniforms: BTreeMap::new(),
                    used: tick,
                },
            );
            #[cfg(test)]
            {
                self.compilations += 1;
            }
        }
        // Allocate every transient binding before clearing the output. A failed
        // uniform/sampler allocation leaves its previously rendered pixels intact.
        let pipeline = self.pipelines.get_mut(&key).unwrap();
        pipeline.used = tick;
        let mut buffers: [Option<ID3D11Buffer>; 8] = Default::default();
        let mut textures: [Option<ID3D11ShaderResourceView>; 16] = Default::default();
        let mut samplers: [Option<ID3D11SamplerState>; 16] = Default::default();
        for slot in &pipeline.resources {
            match (slot.kind, bindings.get(slot.binding)) {
                (ShaderResourceSlotKind::Uniform, Some(ShaderBinding::Uniform(bytes))) => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        pipeline.uniforms.entry(slot.slot)
                    {
                        entry.insert(uniform_buffer(device, bytes.len())?);
                    }
                    let buffer = &pipeline.uniforms[&slot.slot];
                    update_uniform(context, buffer, bytes)?;
                    buffers[slot.slot as usize] = Some(buffer.clone());
                }
                (ShaderResourceSlotKind::Texture, Some(ShaderBinding::Texture(input))) => {
                    textures[slot.slot as usize] = Some(self.targets.get(input)?.view.clone());
                }
                (ShaderResourceSlotKind::Sampler, Some(ShaderBinding::Sampler(kind))) => {
                    if !self.samplers.contains_key(kind) {
                        self.samplers.insert(*kind, sampler(device, *kind)?);
                    }
                    samplers[slot.slot as usize] = self.samplers.get(kind).cloned();
                }
                _ => unreachable!("reflected bindings were validated before rendering"),
            }
        }
        let output = &self.targets.get(target)?.output;
        let _cleanup = BindingCleanup {
            context,
            detach_output: true,
        };
        unsafe {
            clear_texture_bindings(context);
            context.OMSetRenderTargets(Some(&[Some(output.clone())]), None);
            context.ClearRenderTargetView(output, &[0.0; 4]);
            context.RSSetViewports(Some(&[viewport(target.size())]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&pipeline.vertex, None);
            context.PSSetShader(&pipeline.fragment, None);
            context.VSSetConstantBuffers(0, Some(&[const { None }; 8]));
            context.PSSetConstantBuffers(0, Some(&buffers));
            context.PSSetShaderResources(0, Some(&textures));
            context.PSSetSamplers(0, Some(&samplers));
            context.OMSetBlendState(&pipeline.blend, None, u32::MAX);
            context.Draw(3, 0);
        }
        if let Err(error) = unsafe { device.GetDeviceRemovedReason() } {
            target.invalidate_device();
            return Err(backend(error));
        }
        target.did_render();
        Ok(())
    }

    pub(super) fn create_buffer(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        descriptor: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer> {
        self.check_device(device)?;
        require_compute(device)?;
        descriptor.validate()?;
        drop(self.targets.take_unused());
        drop(self.buffers.take_unused());
        let bytes = self.buffers.check(descriptor)?;
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: bytes as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_UNORDERED_ACCESS.0) as u32,
            MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
            ..Default::default()
        };
        let mut raw = None;
        unsafe { device.CreateBuffer(&desc, None, Some(&mut raw)) }.map_err(backend)?;
        let raw = super::require_com_output(raw, "compute storage buffer").map_err(backend)?;
        let read_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_TYPELESS,
            ViewDimension: D3D11_SRV_DIMENSION_BUFFEREX,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                BufferEx: D3D11_BUFFEREX_SRV {
                    FirstElement: 0,
                    NumElements: bytes as u32 / 4,
                    Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
                },
            },
        };
        let mut read = None;
        unsafe { device.CreateShaderResourceView(&raw, Some(&read_desc), Some(&mut read)) }
            .map_err(backend)?;
        let write_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: DXGI_FORMAT_R32_TYPELESS,
            ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_UAV {
                    FirstElement: 0,
                    NumElements: bytes as u32 / 4,
                    Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
                },
            },
        };
        let mut write = None;
        unsafe { device.CreateUnorderedAccessView(&raw, Some(&write_desc), Some(&mut write)) }
            .map_err(backend)?;
        let resource = StorageBuffer {
            raw,
            read: super::require_com_output(read, "compute SRV").map_err(backend)?,
            write: super::require_com_output(write, "compute UAV").map_err(backend)?,
        };
        unsafe {
            context.ClearUnorderedAccessViewUint(&resource.write, &[0; 4]);
        }
        self.buffers.insert(descriptor, resource, bytes)
    }
    pub(super) fn validate_buffer(
        &self,
        device: &ID3D11Device,
        buffer: &crate::GpuBuffer,
    ) -> Result<()> {
        self.check_device(device)?;
        self.buffers.get(buffer).map(|_| ())
    }
    pub(super) fn write_buffer(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        buffer: &crate::GpuBuffer,
        offset: u64,
        bytes: &[u8],
    ) -> Result<()> {
        self.check_device(device)?;
        let resource = self.buffers.get(buffer)?;
        crate::compute::validate_buffer_write(buffer, offset, bytes)?;
        if bytes.is_empty() {
            return Ok(());
        }
        let range = D3D11_BOX {
            left: offset as u32,
            right: (offset + bytes.len() as u64) as u32,
            top: 0,
            bottom: 1,
            front: 0,
            back: 1,
        };
        unsafe {
            context.UpdateSubresource(&resource.raw, 0, Some(&range), bytes.as_ptr().cast(), 0, 0);
        }
        buffer.did_write();
        Ok(())
    }
    pub(super) fn read_buffer(
        &self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        buffer: &crate::GpuBuffer,
    ) -> Result<Vec<u8>> {
        self.check_device(device)?;
        let resource = self.buffers.get(buffer)?;
        let bytes = buffer.descriptor().byte_len;
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: bytes as u32,
            Usage: D3D11_USAGE_STAGING,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..Default::default()
        };
        let mut staging = None;
        unsafe { device.CreateBuffer(&desc, None, Some(&mut staging)) }.map_err(backend)?;
        let staging =
            super::require_com_output(staging, "compute readback staging").map_err(backend)?;
        unsafe {
            context.CopyResource(&staging, &resource.raw);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let started = std::time::Instant::now();
        loop {
            match unsafe {
                context.Map(
                    &staging,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            } {
                Ok(()) => break,
                Err(e)
                    if e.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING =>
                {
                    if started.elapsed() >= std::time::Duration::from_secs(10) {
                        return Err(backend("compute readback deadline exceeded"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(e) => {
                    buffer.invalidate_device();
                    return Err(backend(e));
                }
            }
        }
        let result = if mapped.pData.is_null() {
            Err(backend("compute readback mapping is null"))
        } else {
            Ok(
                unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), bytes as usize) }
                    .to_vec(),
            )
        };
        unsafe {
            context.Unmap(&staging, 0);
        }
        result
    }
    pub(super) fn write_target(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        target: &RenderTarget,
        pixels: &[u8],
    ) -> Result<()> {
        self.check_device(device)?;
        let resource = self.targets.get(target)?;
        let d = target.descriptor();
        if pixels.len() as u64 != d.byte_len()? {
            return Err(RenderTargetError::InvalidBindings(
                "target upload requires exact packed pixel bytes".into(),
            ));
        }
        let swapped = if d.format == RenderTargetFormat::Bgra8UnormSrgb {
            let mut bytes = pixels.to_vec();
            for p in bytes.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            Some(bytes)
        } else {
            None
        };
        let bytes = swapped.as_deref().unwrap_or(pixels);
        unsafe {
            context.UpdateSubresource(
                &resource.texture,
                0,
                None,
                bytes.as_ptr().cast(),
                d.width * d.format.bytes_per_pixel(),
                0,
            );
        }
        target.did_render();
        Ok(())
    }
    pub(super) fn dispatch(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<()> {
        self.check_device(device)?;
        require_compute(device)?;
        bindings.validate(shader, groups, &self.targets, &self.buffers)?;
        let tick = self.next_tick();
        if !self.compute.contains_key(&shader.id()) {
            let t = shader
                .translate(ShaderBackend::DirectX11)
                .map_err(backend)?;
            let code = compile(&t.source, &t.entry_point, "cs_5_0")?;
            let mut state = None;
            unsafe { device.CreateComputeShader(&code, None, Some(&mut state)) }
                .map_err(backend)?;
            let state = super::require_com_output(state, "compute shader").map_err(backend)?;
            if self.compute.len() >= 64 {
                let id = *self.compute.iter().min_by_key(|(_, p)| p.used).unwrap().0;
                self.compute.remove(&id);
            }
            self.compute.insert(
                shader.id(),
                ComputePipeline {
                    state,
                    translation: t,
                    uniforms: Default::default(),
                    used: tick,
                },
            );
        }
        let pipeline = self.compute.get_mut(&shader.id()).unwrap();
        pipeline.used = tick;
        let mut uniforms: [Option<ID3D11Buffer>; 4] = Default::default();
        let mut reads: [Option<ID3D11ShaderResourceView>; 16] = Default::default();
        let mut writes: [Option<ID3D11UnorderedAccessView>; 8] = Default::default();
        let mut samplers: [Option<ID3D11SamplerState>; 8] = Default::default();
        for slot in &pipeline.translation.resources {
            match bindings.get(slot.binding).unwrap() {
                crate::ComputeBinding::Uniform(bytes) => {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        pipeline.uniforms.entry(slot.slot)
                    {
                        entry.insert(uniform_buffer(device, bytes.len())?);
                    }
                    let raw = &pipeline.uniforms[&slot.slot];
                    update_uniform(context, raw, bytes)?;
                    uniforms[slot.slot as usize] = Some(raw.clone());
                }
                crate::ComputeBinding::Texture(t) => {
                    reads[slot.slot as usize] = Some(self.targets.get(t)?.view.clone())
                }
                crate::ComputeBinding::StorageTexture(t) => {
                    writes[slot.slot as usize] =
                        Some(
                            self.targets.get(t)?.storage.clone().ok_or(
                                RenderTargetError::Unsupported("target has no storage view"),
                            )?,
                        )
                }
                crate::ComputeBinding::StorageBuffer(b) => {
                    let raw = self.buffers.get(b)?;
                    match slot.kind {
                        crate::ComputeSlotKind::ReadBuffer => {
                            reads[slot.slot as usize] = Some(raw.read.clone())
                        }
                        crate::ComputeSlotKind::WriteBuffer => {
                            writes[slot.slot as usize] = Some(raw.write.clone())
                        }
                        _ => unreachable!(),
                    }
                }
                crate::ComputeBinding::Sampler(kind) => {
                    if !self.samplers.contains_key(kind) {
                        self.samplers.insert(*kind, sampler(device, *kind)?);
                    }
                    samplers[slot.slot as usize] = self.samplers.get(kind).cloned();
                }
            }
        }
        let _output = OutputBindingRestore::detach(context);
        unsafe {
            clear_texture_bindings(context);
            context.CSSetShader(&pipeline.state, None);
            context.CSSetConstantBuffers(0, Some(&uniforms));
            context.CSSetShaderResources(0, Some(&reads));
            context.CSSetSamplers(0, Some(&samplers));
            context.CSSetUnorderedAccessViews(0, 8, Some(writes.as_ptr()), None);
            context.Dispatch(groups[0], groups[1], groups[2]);
            context.CSSetUnorderedAccessViews(0, 8, Some([const { None }; 8].as_ptr()), None);
            context.CSSetShaderResources(0, Some(&[const { None }; 16]));
            context.CSSetConstantBuffers(0, Some(&[const { None }; 4]));
            context.CSSetSamplers(0, Some(&[const { None }; 8]));
            context.CSSetShader(None, None);
        }
        if let Err(e) = unsafe { device.GetDeviceRemovedReason() } {
            for value in bindings.values.values() {
                match value {
                    crate::ComputeBinding::StorageTexture(t) => t.invalidate_device(),
                    crate::ComputeBinding::StorageBuffer(b) => b.invalidate_device(),
                    _ => {}
                }
            }
            return Err(backend(e));
        }
        bindings.did_dispatch(shader);
        Ok(())
    }

    pub(super) fn read(
        &self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        target: &RenderTarget,
    ) -> Result<RenderTargetReadback> {
        self.check_device(device)?;
        let texture = self.targets.get(target)?;
        let descriptor = target.descriptor();
        let packed_len = usize::try_from(descriptor.byte_len()?).map_err(backend)?;
        let row_len = descriptor.width as usize * descriptor.format.bytes_per_pixel() as usize;
        let mut desc = texture_desc(descriptor);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(backend)?;
        let staging = super::require_com_output(staging, "CreateTexture2D for custom readback")
            .map_err(backend)?;
        let _output_binding = OutputBindingRestore::detach(context);
        unsafe {
            context.CopyResource(&staging, &texture.texture);
            context.Flush();
        };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let started = std::time::Instant::now();
        loop {
            match unsafe {
                context.Map(
                    &staging,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            } {
                Ok(()) => break,
                Err(error)
                    if error.code()
                        == windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING =>
                {
                    if started.elapsed() >= std::time::Duration::from_secs(10) {
                        return Err(backend("DirectX target readback deadline exceeded"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(error) => {
                    if unsafe { device.GetDeviceRemovedReason() }.is_err() {
                        target.invalidate_device();
                    }
                    return Err(backend(error));
                }
            }
        }
        let _mapping = MappedTexture {
            context,
            texture: &staging,
        };
        let stride = mapped.RowPitch as usize;
        let source_len = stride
            .checked_mul(descriptor.height as usize)
            .ok_or_else(|| backend("mapped target row layout overflowed"))?;
        if mapped.pData.is_null() || stride < row_len || source_len > 512 * 1024 * 1024 {
            return Err(backend("mapped target storage has an invalid row layout"));
        }
        let mut pixels = vec![0; packed_len];
        // SAFETY: Map succeeded and validated pitch bounds every copied row.
        // The mapping guard unmaps on every return, including layout failures.
        unsafe {
            for (y, row) in pixels.chunks_exact_mut(row_len).enumerate() {
                let source = mapped.pData.cast::<u8>().add(y * stride);
                // Row padding need not be initialized by the GPU copy.
                row.copy_from_slice(std::slice::from_raw_parts(source, row_len));
            }
        }
        if descriptor.format == RenderTargetFormat::Bgra8UnormSrgb {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        Ok(RenderTargetReadback { descriptor, pixels })
    }

    pub(super) fn draw(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        surface: &PaintSurface,
        target: &RenderTarget,
        size: Size<DevicePixels>,
    ) -> Result<()> {
        self.check_device(device)?;
        let input = self.targets.get(target)?;
        if self.display.is_none() {
            let (vertex, fragment) = compile_pipeline(
                device,
                DISPLAY_SHADER,
                "kael_target_vertex",
                "kael_target_fragment",
            )?;
            self.display = Some(DisplayPipeline {
                vertex,
                fragment,
                blend: super::create_premultiplied_blend_state(device).map_err(backend)?,
                params: uniform_buffer(device, std::mem::size_of::<RenderTargetDisplayParams>())?,
            });
        }
        let params = RenderTargetDisplayParams::new(surface, target, size);
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.samplers.entry(ShaderSampler::LinearClamp)
        {
            entry.insert(sampler(device, ShaderSampler::LinearClamp)?);
        }
        let pipeline = self.display.as_ref().unwrap();
        update_uniform(context, &pipeline.params, bytemuck::bytes_of(&params))?;
        let _cleanup = BindingCleanup {
            context,
            detach_output: false,
        };
        unsafe {
            clear_texture_bindings(context);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.RSSetViewports(Some(&[viewport(size)]));
            context.VSSetShader(&pipeline.vertex, None);
            context.PSSetShader(&pipeline.fragment, None);
            context.VSSetConstantBuffers(0, Some(&[Some(pipeline.params.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(pipeline.params.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(input.view.clone())]));
            context.PSSetSamplers(
                0,
                Some(&[self.samplers.get(&ShaderSampler::LinearClamp).cloned()]),
            );
            context.OMSetBlendState(&pipeline.blend, None, u32::MAX);
            context.Draw(4, 0);
        }
        Ok(())
    }
}

struct MappedTexture<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
}

struct OutputBindingRestore<'a> {
    context: &'a ID3D11DeviceContext,
    outputs: [Option<ID3D11RenderTargetView>; 8],
    depth: Option<ID3D11DepthStencilView>,
}
impl<'a> OutputBindingRestore<'a> {
    fn detach(context: &'a ID3D11DeviceContext) -> Self {
        let mut outputs: [Option<ID3D11RenderTargetView>; 8] = Default::default();
        let mut depth = None;
        unsafe {
            context.OMGetRenderTargets(Some(&mut outputs), Some(&mut depth));
            context.OMSetRenderTargets(None, None);
        }
        Self {
            context,
            outputs,
            depth,
        }
    }
}
impl Drop for OutputBindingRestore<'_> {
    fn drop(&mut self) {
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&self.outputs), self.depth.as_ref())
        };
    }
}
impl Drop for MappedTexture<'_> {
    fn drop(&mut self) {
        unsafe { self.context.Unmap(self.texture, 0) };
    }
}

struct BindingCleanup<'a> {
    context: &'a ID3D11DeviceContext,
    detach_output: bool,
}
impl Drop for BindingCleanup<'_> {
    fn drop(&mut self) {
        unsafe {
            clear_texture_bindings(self.context);
            self.context
                .VSSetConstantBuffers(0, Some(&[const { None }; 8]));
            self.context
                .PSSetConstantBuffers(0, Some(&[const { None }; 8]));
            self.context.PSSetSamplers(0, Some(&[const { None }; 16]));
            if self.detach_output {
                self.context.OMSetRenderTargets(None, None);
            }
        }
    }
}

unsafe fn clear_texture_bindings(context: &ID3D11DeviceContext) {
    unsafe {
        context.VSSetShaderResources(0, Some(&[const { None }; 16]));
        context.PSSetShaderResources(0, Some(&[const { None }; 16]));
    }
}

fn viewport(size: Size<DevicePixels>) -> D3D11_VIEWPORT {
    D3D11_VIEWPORT {
        Width: size.width.0 as f32,
        Height: size.height.0 as f32,
        MaxDepth: 1.0,
        ..Default::default()
    }
}

fn uniform_buffer(device: &ID3D11Device, bytes: usize) -> Result<ID3D11Buffer> {
    let width = bytes
        .checked_add(15)
        .map(|bytes| bytes & !15)
        .filter(|bytes| *bytes > 0 && *bytes <= 65_536)
        .ok_or_else(|| backend("invalid uniform byte count"))?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: width as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }.map_err(backend)?;
    super::require_com_output(buffer, "CreateBuffer for custom uniforms").map_err(backend)
}

fn update_uniform(
    context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    bytes: &[u8],
) -> Result<()> {
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { buffer.GetDesc(&mut desc) };
    if bytes.len() > desc.ByteWidth as usize {
        return Err(backend("uniform bytes exceed allocated buffer"));
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }
        .map_err(backend)?;
    struct Mapping<'a>(&'a ID3D11DeviceContext, &'a ID3D11Buffer);
    impl Drop for Mapping<'_> {
        fn drop(&mut self) {
            unsafe { self.0.Unmap(self.1, 0) };
        }
    }
    let _mapping = Mapping(context, buffer);
    if mapped.pData.is_null() {
        return Err(backend("uniform buffer is not mapped"));
    }
    // WRITE_DISCARD lets the D3D11 driver rename backing storage while earlier
    // draws are in flight. The immutable reflected layout bounds this copy.
    unsafe {
        std::ptr::write_bytes(mapped.pData.cast::<u8>(), 0, desc.ByteWidth as usize);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.pData.cast(), bytes.len());
    }
    Ok(())
}

fn sampler(device: &ID3D11Device, kind: ShaderSampler) -> Result<ID3D11SamplerState> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: match kind {
            ShaderSampler::LinearClamp => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            ShaderSampler::NearestClamp => D3D11_FILTER_MIN_MAG_MIP_POINT,
        },
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        MaxLOD: D3D11_FLOAT32_MAX,
        ..Default::default()
    };
    let mut state = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut state)) }.map_err(backend)?;
    super::require_com_output(state, "CreateSamplerState for custom shader").map_err(backend)
}

fn compile_pipeline(
    device: &ID3D11Device,
    source: &str,
    vertex: &str,
    fragment: &str,
) -> Result<(ID3D11VertexShader, ID3D11PixelShader)> {
    let modern = unsafe { device.GetFeatureLevel() }.0 >= D3D_FEATURE_LEVEL_11_0.0;
    let vertex = compile(source, vertex, if modern { "vs_5_0" } else { "vs_4_1" })?;
    let fragment = compile(source, fragment, if modern { "ps_5_0" } else { "ps_4_1" })?;
    Ok((
        super::create_vertex_shader(device, &vertex).map_err(backend)?,
        super::create_fragment_shader(device, &fragment).map_err(backend)?,
    ))
}

fn compile(source: &str, entry: &str, profile: &str) -> Result<Vec<u8>> {
    let entry = CString::new(entry).map_err(backend)?;
    let profile = CString::new(profile).map_err(backend)?;
    let mut blob = None;
    let mut errors = None;
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR::from_raw(entry.as_ptr().cast()),
            PCSTR::from_raw(profile.as_ptr().cast()),
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut blob,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let diagnostic = errors.as_ref().and_then(|blob| {
            let len = unsafe { blob.GetBufferSize() }.min(65_536);
            let data = unsafe { blob.GetBufferPointer() }.cast::<u8>();
            (!data.is_null() && len > 0).then(|| unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(data, len))
                    .trim_end_matches('\0')
                    .to_owned()
            })
        });
        return Err(backend(diagnostic.unwrap_or_else(|| error.to_string())));
    }
    let blob = super::require_com_output(blob, "D3DCompile custom shader").map_err(backend)?;
    let len = unsafe { blob.GetBufferSize() };
    let data = unsafe { blob.GetBufferPointer() }.cast::<u8>();
    if data.is_null() || len == 0 || len > 16 * 1024 * 1024 {
        return Err(backend("compiled shader has an invalid byte count"));
    }
    Ok(unsafe { std::slice::from_raw_parts(data, len) }.to_vec())
}

const DISPLAY_SHADER: &str = r#"
cbuffer Params : register(b0) {
    float4 bounds; float4 mask; float4 corners; float4 rounded_clip;
    float4 rounded_corners; float4 transform; float2 translation; float2 viewport;
    float4 color_filter; float opacity; uint scalar; uint2 padding;
};
Texture2D<float4> target : register(t0);
SamplerState sampling : register(s0);
struct Varying { float4 position : SV_Position; float2 uv : TEXCOORD0; float2 local : TEXCOORD1; };
Varying kael_target_vertex(uint id : SV_VertexID) {
    float2 uv = float2(float(id & 1), float((id >> 1) & 1));
    float2 position = bounds.xy + uv * bounds.zw;
    Varying result;
    float2 transformed = float2(dot(transform.xy, position), dot(transform.zw, position)) + translation;
    result.position = float4(transformed / viewport * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    result.uv = uv; result.local = position; return result;
}
float rounded_coverage(float2 position, float4 rect, float4 radii) {
    if (all(radii == 0.0)) return 1.0;
    float2 centered = position - rect.xy - rect.zw * 0.5;
    float radius = centered.y < 0.0 ? (centered.x < 0.0 ? radii.x : radii.y) : (centered.x < 0.0 ? radii.w : radii.z);
    float2 q = abs(centered) - rect.zw * 0.5 + radius;
    float distance = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - distance, 0.0, 1.0);
}
float4 kael_target_fragment(Varying input) : SV_Target {
    if (any(input.position.xy < mask.xy) || any(input.position.xy >= mask.xy + mask.zw)) discard;
    float4 color = target.Sample(sampling, input.uv);
    if (scalar != 0) color = float4(color.rrr, 1.0);
    if (color.a > 0.0) {
        float3 straight = color.rgb / color.a;
        straight = ((straight - 0.5) * color_filter.w + 0.5) * color_filter.z;
        float3 grayscale = dot(straight, float3(0.2126, 0.7152, 0.0722)).xxx;
        straight = lerp(grayscale, straight, color_filter.y);
        grayscale = dot(straight, float3(0.2126, 0.7152, 0.0722)).xxx;
        color.rgb = lerp(straight, grayscale, color_filter.x) * color.a;
    }
    float coverage = rounded_coverage(input.local, bounds, corners);
    coverage *= rounded_coverage(input.position.xy, rounded_clip, rounded_corners);
    return color * (opacity * coverage);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, ContentMask, PaintSurfaceSource, ShaderDescriptor, point, size};
    use windows::Win32::Foundation::HMODULE;

    // WARP is required on Windows CI. Failure is explicit: these GPU tests never
    // count an unavailable renderer or compiler as successful pixel evidence.
    fn warp(level: D3D_FEATURE_LEVEL) -> (ID3D11Device, ID3D11DeviceContext) {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[level]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .expect("Windows GPU regression requires a WARP device");
        (device.unwrap(), context.unwrap())
    }

    fn shader(source: &str) -> ShaderHandle {
        ShaderHandle::compile_fragment(ShaderDescriptor::fragment(
            "dx_pixel_regression",
            source,
            "fs_main",
        ))
        .unwrap()
    }

    #[test]
    fn warp_compute_runtime_arrays_exact_uploads_and_guarded_loops() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_11_0);
        let mut renderer = DirectXCustomRenderer::default();
        let input = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let pixels = [32, 64, 128, 128].repeat(16);
        renderer
            .write_target(&device, &context, &input, &pixels)
            .unwrap();
        assert_eq!(
            renderer.read(&device, &context, &input).unwrap().pixels,
            pixels
        );
        let output = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let buffer = renderer
            .create_buffer(
                &device,
                &context,
                crate::GpuBufferDescriptor { byte_len: 64 },
            )
            .unwrap();
        assert_eq!(
            renderer.read_buffer(&device, &context, &buffer).unwrap(),
            [0; 64]
        );
        let compute=crate::ComputeHandle::compile(crate::ComputeDescriptor::new("WARP compute",r#"
            @group(0) @binding(7) var source: texture_2d<f32>;
            @group(0) @binding(11) var<storage, read_write> values: array<u32>;
            @group(0) @binding(19) var destination: texture_storage_2d<rgba8unorm, write>;
            @compute @workgroup_size(2,2) fn main(@builtin(global_invocation_id) id:vec3<u32>){if(id.x>=4u||id.y>=4u){return;}let index=id.y*4u+id.x;if(index<arrayLength(&values)){values[index]=index+100u;}textureStore(destination,vec2<i32>(id.xy),textureLoad(source,vec2<i32>(id.xy),0));}
        "#,"main")).unwrap();
        let bindings = crate::ComputeBindings::new()
            .with(7, crate::ComputeBinding::Texture(input.clone()))
            .with(11, crate::ComputeBinding::StorageBuffer(buffer.clone()))
            .with(19, crate::ComputeBinding::StorageTexture(output.clone()));
        renderer
            .dispatch(&device, &context, &compute, &bindings, [2, 2, 1])
            .unwrap();
        assert_eq!(
            renderer.read(&device, &context, &output).unwrap().pixels,
            pixels
        );
        assert_eq!(
            renderer.read_buffer(&device, &context, &buffer).unwrap(),
            (100u32..116).flat_map(u32::to_le_bytes).collect::<Vec<_>>()
        );
        renderer
            .write_buffer(&device, &context, &buffer, 4, &999u32.to_le_bytes())
            .unwrap();
        assert_eq!(
            &renderer.read_buffer(&device, &context, &buffer).unwrap()[4..8],
            &999u32.to_le_bytes()
        );
        let sync = crate::ComputeHandle::compile(crate::ComputeDescriptor::new(
            "synchronized loops",
            crate::compute::SYNC_BARRIER_REGRESSION,
            "main",
        ))
        .unwrap();
        assert_eq!(sync.loop_body_limit(), None);
        renderer
            .dispatch(
                &device,
                &context,
                &sync,
                &crate::ComputeBindings::new()
                    .with(31, crate::ComputeBinding::StorageBuffer(buffer.clone())),
                [1, 1, 1],
            )
            .unwrap();
        assert_eq!(
            &renderer.read_buffer(&device, &context, &buffer).unwrap()[..8],
            &[65555u32, 19]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert!(
            renderer
                .dispatch(
                    &device,
                    &context,
                    &compute,
                    &bindings
                        .clone()
                        .with(19, crate::ComputeBinding::StorageTexture(input)),
                    [2, 2, 1]
                )
                .is_err()
        );
        let program = shader(
            "fn work() -> u32 { var count=0u; loop { loop { count+=1u; } } return count; } @fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(f32(work())/65536.0,0.0,0.0,1.0); }",
        );
        renderer
            .render(&device, &context, &output, &program, &ShaderBindings::new())
            .unwrap();
        assert_eq!(
            &renderer.read(&device, &context, &output).unwrap().pixels[..4],
            &[255, 0, 0, 255]
        );
        for format in [
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::Rgba16Float,
            RenderTargetFormat::R8Unorm,
        ] {
            let target = renderer
                .create(
                    &device,
                    &context,
                    RenderTargetDescriptor {
                        width: 2,
                        height: 2,
                        format,
                    },
                )
                .unwrap();
            let pixels = match format {
                RenderTargetFormat::Rgba16Float => {
                    [0x00, 0x38, 0x00, 0x34, 0x00, 0x3c, 0x00, 0x3c].repeat(4)
                }
                RenderTargetFormat::R8Unorm => vec![17, 23, 31, 47],
                _ => [17, 23, 31, 255].repeat(4),
            };
            renderer
                .write_target(&device, &context, &target, &pixels)
                .unwrap();
            assert_eq!(
                renderer.read(&device, &context, &target).unwrap().pixels,
                pixels
            );
            if format == RenderTargetFormat::Rgba16Float {
                let kernel = crate::ComputeHandle::compile(crate::ComputeDescriptor::new(
                    "HDR storage",
                    crate::compute::HDR_STORAGE_REGRESSION,
                    "main",
                ))
                .unwrap();
                renderer
                    .dispatch(
                        &device,
                        &context,
                        &kernel,
                        &crate::ComputeBindings::new()
                            .with(5, crate::ComputeBinding::StorageTexture(target.clone())),
                        [1, 1, 1],
                    )
                    .unwrap();
                assert_eq!(
                    renderer.read(&device, &context, &target).unwrap().pixels,
                    [0, 64, 0, 56, 0, 52, 0, 60].repeat(4)
                );
            }
        }
        let (legacy, legacy_context) = warp(D3D_FEATURE_LEVEL_10_1);
        assert!(matches!(
            DirectXCustomRenderer::default().create_buffer(
                &legacy,
                &legacy_context,
                crate::GpuBufferDescriptor { byte_len: 4 }
            ),
            Err(RenderTargetError::Unsupported(_))
        ));
    }
    const COLOR: &str = "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.5, 0.25, 0.5); }";

    fn assert_pixel(actual: &[u8], expected: &[u8], tolerance: u8) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                actual.abs_diff(*expected) <= tolerance,
                "actual {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn warp_all_target_formats_clear_and_preserve_premultiplied_hdr_and_srgb_storage() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_11_0);
        let mut renderer = DirectXCustomRenderer::default();
        let color = shader(COLOR);
        for format in [
            RenderTargetFormat::Rgba8Unorm,
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::Rgba16Float,
            RenderTargetFormat::R8Unorm,
        ] {
            let target = renderer
                .create(
                    &device,
                    &context,
                    RenderTargetDescriptor {
                        width: 3,
                        height: 2,
                        format,
                    },
                )
                .unwrap();
            assert!(
                renderer
                    .read(&device, &context, &target)
                    .unwrap()
                    .pixels
                    .iter()
                    .all(|byte| *byte == 0)
            );
            renderer
                .render(&device, &context, &target, &color, &ShaderBindings::new())
                .unwrap();
            let result = renderer.read(&device, &context, &target).unwrap();
            assert_eq!(
                result.pixels.len(),
                target.descriptor().byte_len().unwrap() as usize
            );
            match format {
                RenderTargetFormat::Rgba8Unorm => {
                    for pixel in result.pixels.chunks_exact(4) {
                        assert_pixel(pixel, &[128, 64, 32, 128], 1);
                    }
                }
                RenderTargetFormat::Rgba8UnormSrgb | RenderTargetFormat::Bgra8UnormSrgb => {
                    for pixel in result.pixels.chunks_exact(4) {
                        assert_pixel(pixel, &[188, 137, 99, 128], 1);
                    }
                }
                RenderTargetFormat::R8Unorm => {
                    assert!(result.pixels.iter().all(|value| value.abs_diff(128) <= 1))
                }
                RenderTargetFormat::Rgba16Float => {
                    for pixel in result.pixels.chunks_exact(8) {
                        assert_eq!(pixel, &[0, 0x38, 0, 0x34, 0, 0x30, 0, 0x38]);
                    }
                }
            }
        }
        let hdr = renderer
            .create(
                &device,
                &context,
                RenderTargetDescriptor {
                    width: 1,
                    height: 1,
                    format: RenderTargetFormat::Rgba16Float,
                },
            )
            .unwrap();
        let color = shader(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(4.0, 2.0, 1.0, 0.5); }",
        );
        renderer
            .render(&device, &context, &hdr, &color, &ShaderBindings::new())
            .unwrap();
        assert_eq!(
            renderer.read(&device, &context, &hdr).unwrap().pixels,
            [0, 0x40, 0, 0x3c, 0, 0x38, 0, 0x38]
        );
    }

    #[test]
    fn warp_sparse_bindings_validate_before_mutation_and_dynamic_uniforms_update() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_11_0);
        let mut renderer = DirectXCustomRenderer::default();
        let source = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(3, 2))
            .unwrap();
        let output = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(3, 2))
            .unwrap();
        renderer
            .render(
                &device,
                &context,
                &source,
                &shader(COLOR),
                &ShaderBindings::new(),
            )
            .unwrap();
        let pass = shader(
            r#"
struct Params { tint: vec4<f32> }
@group(0) @binding(7) var<uniform> params: Params;
@group(0) @binding(19) var source: texture_2d<f32>;
@group(0) @binding(31) var sampling: sampler;
@fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let color = textureSample(source, sampling, uv);
    return vec4<f32>(color.rgb / max(color.a, 0.0001) * params.tint.rgb, color.a * params.tint.a);
}"#,
        );
        let bindings = |target: &RenderTarget, tint: [f32; 4]| {
            ShaderBindings::new()
                .with(
                    7,
                    ShaderBinding::Uniform(std::sync::Arc::from(bytemuck::cast_slice::<f32, u8>(
                        &tint,
                    ))),
                )
                .with(19, ShaderBinding::Texture(target.clone()))
                .with(31, ShaderBinding::Sampler(ShaderSampler::NearestClamp))
        };
        renderer
            .render(
                &device,
                &context,
                &output,
                &pass,
                &bindings(&source, [0.5, 1.0, 1.0, 0.5]),
            )
            .unwrap();
        let before = renderer.read(&device, &context, &output).unwrap().pixels;
        for pixel in before.chunks_exact(4) {
            assert_pixel(pixel, &[32, 32, 16, 64], 1);
        }
        assert!(matches!(
            renderer.render(
                &device,
                &context,
                &output,
                &pass,
                &bindings(&output, [1.0; 4])
            ),
            Err(RenderTargetError::FeedbackLoop)
        ));
        assert!(matches!(
            renderer.render(&device, &context, &output, &pass, &ShaderBindings::new()),
            Err(RenderTargetError::InvalidBindings(_))
        ));
        assert_eq!(
            renderer.read(&device, &context, &output).unwrap().pixels,
            before
        );
        renderer
            .render(
                &device,
                &context,
                &output,
                &pass,
                &bindings(&source, [1.0; 4]),
            )
            .unwrap();
        for pixel in renderer
            .read(&device, &context, &output)
            .unwrap()
            .pixels
            .chunks_exact(4)
        {
            assert_pixel(pixel, &[128, 64, 32, 128], 1);
        }
        let mut srvs: [Option<ID3D11ShaderResourceView>; 16] = Default::default();
        let mut outputs = [None];
        unsafe {
            context.PSGetShaderResources(0, Some(&mut srvs));
            context.OMGetRenderTargets(Some(&mut outputs), None);
        }
        assert!(srvs.iter().all(Option::is_none));
        assert!(outputs[0].is_none());
    }

    fn surface(
        target: &RenderTarget,
        paint: crate::render_target::RenderTargetPaint,
        bounds: Bounds<crate::ScaledPixels>,
        mask: Bounds<crate::ScaledPixels>,
    ) -> PaintSurface {
        PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds: mask },
            source: PaintSurfaceSource::RenderTarget {
                target: target.clone(),
                revision: target.revision(),
                paint,
            },
        }
    }

    #[test]
    fn warp_direct_display_uses_source_over_clipping_opacity_filters_and_transform() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_11_0);
        super::super::set_rasterizer_state(&device, &context).unwrap();
        let mut renderer = DirectXCustomRenderer::default();
        let input = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(2, 2))
            .unwrap();
        let output = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        renderer.render(&device, &context, &input, &shader("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 0.5); }"), &ShaderBindings::new()).unwrap();
        let output_view = renderer.targets.get(&output).unwrap().output.clone();
        let viewport = size(DevicePixels(4), DevicePixels(4));
        let bounds = Bounds::new(
            point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
            size(crate::ScaledPixels(4.0), crate::ScaledPixels(4.0)),
        );
        let mut mask = bounds;
        mask.size.width.0 = 2.0;
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.opacity = 0.5;
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(output_view.clone())]), None);
            context.ClearRenderTargetView(&output_view, &[0.0, 0.0, 1.0, 1.0]);
        }
        renderer
            .draw(
                &device,
                &context,
                &surface(&input, paint, bounds, mask),
                &input,
                viewport,
            )
            .unwrap();
        let result = renderer.read(&device, &context, &output).unwrap().pixels;
        for (index, pixel) in result.chunks_exact(4).enumerate() {
            assert_pixel(
                pixel,
                if index % 4 < 2 {
                    &[64, 0, 191, 255]
                } else {
                    &[0, 0, 255, 255]
                },
                1,
            );
        }
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.color_filter.brightness = 0.5;
        paint.transform.translation = [1.0, 0.0];
        let mut smaller = bounds;
        smaller.size.width.0 = 2.0;
        unsafe {
            context.ClearRenderTargetView(&output_view, &[0.0; 4]);
        }
        renderer
            .draw(
                &device,
                &context,
                &surface(&input, paint, smaller, bounds),
                &input,
                viewport,
            )
            .unwrap();
        for (index, pixel) in renderer
            .read(&device, &context, &output)
            .unwrap()
            .pixels
            .chunks_exact(4)
            .enumerate()
        {
            assert_pixel(
                pixel,
                if (1..3).contains(&(index % 4)) {
                    &[64, 0, 0, 128]
                } else {
                    &[0; 4]
                },
                1,
            );
        }
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.corner_radii = crate::Corners::all(crate::ScaledPixels(2.0));
        unsafe {
            context.ClearRenderTargetView(&output_view, &[0.0; 4]);
        }
        renderer
            .draw(
                &device,
                &context,
                &surface(&input, paint, bounds, bounds),
                &input,
                viewport,
            )
            .unwrap();
        let result = renderer.read(&device, &context, &output).unwrap().pixels;
        assert!(result[3] < result[23]);
        assert_pixel(&result[20..24], &[128, 0, 0, 128], 1);
    }

    #[test]
    fn warp_pressure_preserves_live_targets_bounds_pipelines_and_invalidates_device_owners() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_11_0);
        let mut renderer = DirectXCustomRenderer::default();
        let live = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(1, 1))
            .unwrap();
        let released = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(2, 2))
            .unwrap();
        let color = shader(COLOR);
        renderer
            .render(&device, &context, &live, &color, &ShaderBindings::new())
            .unwrap();
        let pixels = renderer.read(&device, &context, &live).unwrap().pixels;
        renderer.set_budget(0);
        assert!(matches!(
            renderer.create(&device, &context, RenderTargetDescriptor::rgba8(1, 1)),
            Err(RenderTargetError::BudgetExceeded)
        ));
        assert!(renderer.validate(&device, &released).is_ok());
        drop(released);
        unsafe {
            context.ClearState();
        }
        renderer.shed();
        unsafe {
            context.Flush();
        }
        assert_eq!(renderer.targets.used_bytes(), 4);
        assert!(renderer.pipelines.is_empty());
        assert_eq!(
            renderer.read(&device, &context, &live).unwrap().pixels,
            pixels
        );
        for index in 0..65 {
            let pass = shader(&format!(
                "@fragment fn fs_main() -> @location(0) vec4<f32> {{ return vec4<f32>({:.8}, 0.0, 0.0, 1.0); }}",
                index as f32 / 128.0
            ));
            renderer
                .render(&device, &context, &live, &pass, &ShaderBindings::new())
                .unwrap();
        }
        assert_eq!(renderer.pipelines.len(), 64);
        renderer.tick = u64::MAX;
        renderer
            .render(&device, &context, &live, &color, &ShaderBindings::new())
            .unwrap();
        assert_eq!(renderer.pipelines.len(), 64);
        assert_eq!(renderer.tick, 65);
        let mut other = DirectXCustomRenderer::default();
        assert!(matches!(
            other.render(&device, &context, &live, &color, &ShaderBindings::new()),
            Err(RenderTargetError::WrongDevice)
        ));
        drop(renderer);
        assert!(!live.is_valid());
    }

    #[test]
    fn warp_feature_level_10_1_compiles_the_portable_fragment_subset() {
        let (device, context) = warp(D3D_FEATURE_LEVEL_10_1);
        let mut renderer = DirectXCustomRenderer::default();
        let target = renderer
            .create(&device, &context, RenderTargetDescriptor::rgba8(2, 1))
            .unwrap();
        renderer
            .render(
                &device,
                &context,
                &target,
                &shader(COLOR),
                &ShaderBindings::new(),
            )
            .unwrap();
        for pixel in renderer
            .read(&device, &context, &target)
            .unwrap()
            .pixels
            .chunks_exact(4)
        {
            assert_pixel(pixel, &[128, 64, 32, 128], 1);
        }
    }

    struct NativeWindow(windows::Win32::Foundation::HWND);
    impl Drop for NativeWindow {
        fn drop(&mut self) {
            unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0) }.unwrap();
        }
    }

    fn native_warp_renderer() -> (NativeWindow, super::super::DirectXRenderer) {
        use windows::{
            Win32::{Graphics::Dxgi::*, UI::WindowsAndMessaging::*},
            core::{Interface, w},
        };
        let (device, device_context) = warp(D3D_FEATURE_LEVEL_11_0);
        let dxgi_device: IDXGIDevice = device.cast().unwrap();
        let adapter = unsafe { dxgi_device.GetAdapter() }.unwrap().cast().unwrap();
        let dxgi_factory: IDXGIFactory6 =
            unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let devices = super::super::DirectXDevices {
            adapter,
            dxgi_factory,
            device,
            device_context,
        };
        let window = NativeWindow(
            unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("Kael GPU regression"),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    32,
                    32,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap(),
        );
        let renderer = super::super::DirectXRenderer::new(window.0, &devices, true).unwrap();
        (window, renderer)
    }

    #[test]
    fn warp_packed_sprite_filtering_isolates_neighbor_texels_and_preserves_interpolation() {
        let (_window, mut renderer) = native_warp_renderer();
        renderer
            .resize(size(DevicePixels(32), DevicePixels(40)))
            .unwrap();
        let scene = crate::scene::sprite_sampling_tests::packed_sprite_scene(&*renderer.atlas);
        let frame = renderer.render_scene_to_bgra(&scene).unwrap();
        crate::scene::sprite_sampling_tests::assert_packed_sprite_pixels(&frame.premultiplied_bgra);
    }

    #[test]
    fn warp_atlas_pressure_retains_replayed_pixels_and_reuploads_after_retirement() {
        use crate::PlatformAtlas;
        use crate::scene::sprite_sampling_tests::*;
        let (_window, mut renderer) = native_warp_renderer();
        renderer
            .resize(size(DevicePixels(32), DevicePixels(40)))
            .unwrap();
        let atlas = renderer.atlas.clone();
        let scene = packed_sprite_scene(&*atlas);
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bgra(&scene)
                .unwrap()
                .premultiplied_bgra,
        );
        let identities: Vec<_> = scene.atlas_tiles().map(|tile| tile.texture_id).collect();
        remove_packed_sprite_keys(&*atlas);
        reject_packed_sprite_growth_before_raster(&*atlas);
        assert_eq!(atlas.evict_to_budget_keeping(0, 4), 0);
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bgra(&scene)
                .unwrap()
                .premultiplied_bgra,
        );
        for id in &identities {
            assert!(atlas.get_texture(*id).is_ok());
        }
        for _ in 0..4 {
            atlas.advance_frame();
        }
        for id in &identities {
            assert!(atlas.get_texture(*id).is_err());
        }
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits::default());
        let restored = packed_sprite_scene(&*atlas);
        assert!(
            restored
                .atlas_tiles()
                .all(|tile| !identities.contains(&tile.texture_id))
        );
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bgra(&restored)
                .unwrap()
                .premultiplied_bgra,
        );
    }

    #[test]
    fn warp_atlas_device_reset_rejects_old_scene_identity_and_rebuilds_pixels() {
        use crate::scene::sprite_sampling_tests::*;
        let (_window, mut renderer) = native_warp_renderer();
        renderer
            .resize(size(DevicePixels(32), DevicePixels(40)))
            .unwrap();
        let atlas = renderer.atlas.clone();
        let old_scene = packed_sprite_scene(&*atlas);
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bgra(&old_scene)
                .unwrap()
                .premultiplied_bgra,
        );
        let identities: Vec<_> = old_scene
            .atlas_tiles()
            .map(|tile| tile.texture_id)
            .collect();
        atlas.handle_device_lost(&renderer.devices.device, &renderer.devices.device_context);
        for id in &identities {
            assert!(atlas.get_texture(*id).is_err());
        }
        let restored = packed_sprite_scene(&*atlas);
        assert!(
            restored
                .atlas_tiles()
                .all(|tile| !identities.contains(&tile.texture_id))
        );
        // Direct3D propagates a checked stale-ID error, instead of binding a
        // replacement texture under the previous device generation's identity.
        assert!(renderer.render_scene_to_bgra(&old_scene).is_err());
        assert_packed_sprite_pixels(
            &renderer
                .render_scene_to_bgra(&restored)
                .unwrap()
                .premultiplied_bgra,
        );
    }

    #[test]
    fn warp_surviving_atlas_page_reuse_rejects_retired_tile_before_gpu_submission() {
        use crate::PlatformAtlas;
        use crate::scene::sprite_sampling_tests::*;
        let (_window, mut renderer) = native_warp_renderer();
        renderer
            .resize(size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        let atlas = renderer.atlas.clone();
        let first = surviving_page_tile(&*atlas, 994, [0, 0, 255, 255]);
        let survivor = surviving_page_tile(&*atlas, 995, [0, 255, 0, 255]);
        assert_eq!(first.texture_id, survivor.texture_id);
        let old_scene = surviving_page_scene(first.clone());
        let red = renderer.render_scene_to_bgra(&old_scene).unwrap();
        assert_eq!(
            &red.premultiplied_bgra[(8 * 16 + 8) * 4..][..4],
            &[0, 0, 255, 255]
        );
        atlas.remove(&surviving_page_key(994));
        for _ in 0..4 {
            atlas.advance_frame();
        }
        let replacement = surviving_page_tile(&*atlas, 996, [255, 0, 0, 255]);
        assert_eq!(replacement.texture_id, first.texture_id);
        assert_eq!(replacement.bounds, first.bounds);
        let blue = renderer
            .render_scene_to_bgra(&surviving_page_scene(replacement))
            .unwrap();
        assert_eq!(
            &blue.premultiplied_bgra[(8 * 16 + 8) * 4..][..4],
            &[255, 0, 0, 255]
        );
        assert!(renderer.render_scene_to_bgra(&old_scene).is_err());
    }

    #[test]
    fn warp_native_renderer_allocates_scratch_lazily_and_rebuilds_after_pressure_and_resize() {
        use crate::platform::PlatformAtlas;
        use crate::{
            AtlasKey, Background, BlurRect, CachedSurfaceParams, CachedSurfaceSnapshot, Corners,
            Hsla, Quad, ScaledPixels, Scene, hsla,
        };
        let (_window, mut renderer) = native_warp_renderer();
        let viewport = size(DevicePixels(16), DevicePixels(16));
        renderer.resize(viewport).unwrap();
        let bounds = Bounds::new(
            point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size(ScaledPixels(16.0), ScaledPixels(16.0)),
        );
        let quad = Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: Background::from(hsla(0.0, 1.0, 0.5, 1.0)),
            ..Default::default()
        };
        let mut plain = Scene::default();
        plain.insert_primitive(quad.clone());
        plain.finish();
        assert!(
            renderer.resources.path.is_none()
                && renderer.resources.blur.is_none()
                && renderer.resources.cached.is_none()
        );
        for pixel in renderer
            .render_scene_to_bgra(&plain)
            .unwrap()
            .premultiplied_bgra
            .chunks_exact(4)
        {
            assert_pixel(pixel, &[0, 0, 255, 255], 1);
        }
        assert!(
            renderer.resources.path.is_none()
                && renderer.resources.blur.is_none()
                && renderer.resources.cached.is_none()
        );
        let mut builder = crate::PathBuilder::fill();
        builder.move_to(point(crate::px(0.0), crate::px(0.0)));
        builder.line_to(point(crate::px(16.0), crate::px(0.0)));
        builder.line_to(point(crate::px(16.0), crate::px(16.0)));
        builder.line_to(point(crate::px(0.0), crate::px(16.0)));
        builder.close();
        let mut path = builder.build().unwrap();
        path.color = Background::from(hsla(2.0 / 3.0, 1.0, 0.5, 0.5));
        path.content_mask = ContentMask {
            bounds: path.bounds,
        };
        let tile = renderer
            .atlas
            .get_or_insert_with(
                &AtlasKey::CachedSurface(CachedSurfaceParams {
                    cache_id: 123,
                    size: viewport,
                }),
                &mut || {
                    Ok(Some((
                        viewport,
                        std::borrow::Cow::Owned(vec![0; 16 * 16 * 4]),
                    )))
                },
            )
            .unwrap()
            .unwrap();
        let mut scene = Scene::default();
        scene.insert_primitive(quad.clone());
        scene.insert_primitive(path.scale(1.0));
        scene.insert_primitive(BlurRect {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            blur_radius: ScaledPixels(1.0),
            corner_radii: Corners::default(),
            tint: Hsla::transparent_black(),
            saturation: 1.0,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
        });
        scene.request_cached_surface_snapshot(CachedSurfaceSnapshot {
            paint_operations: 0..scene.paint_operations.len(),
            source_bounds: Bounds::new(point(DevicePixels(0), DevicePixels(0)), viewport),
            target: tile,
        });
        scene.finish();
        let before = renderer
            .render_scene_to_bgra(&scene)
            .unwrap()
            .premultiplied_bgra;
        assert!(
            renderer.resources.path.is_some()
                && renderer.resources.blur.is_some()
                && renderer.resources.cached.is_some()
        );
        let target = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(2, 2))
            .unwrap();
        renderer
            .render_shader(&target, &shader(COLOR), &ShaderBindings::new())
            .unwrap();
        renderer.shed_memory(crate::MemoryPressureLevel::Critical);
        assert!(
            renderer.resources.path.is_none()
                && renderer.resources.blur.is_none()
                && renderer.resources.cached.is_none()
        );
        assert!(target.is_valid());
        assert_pixel(
            &renderer.read_render_target(&target).unwrap().pixels[..4],
            &[128, 64, 32, 128],
            1,
        );
        assert_eq!(
            renderer
                .render_scene_to_bgra(&scene)
                .unwrap()
                .premultiplied_bgra,
            before
        );
        renderer
            .resize(size(DevicePixels(17), DevicePixels(15)))
            .unwrap();
        assert!(
            renderer.resources.path.is_none()
                && renderer.resources.blur.is_none()
                && renderer.resources.cached.is_none()
        );
        let output = renderer.render_scene_to_bgra(&plain).unwrap();
        assert_eq!((output.width, output.height), (17, 15));
        assert!(
            renderer.resources.path.is_none()
                && renderer.resources.blur.is_none()
                && renderer.resources.cached.is_none()
        );
        let mut many = Scene::default();
        for _ in 0..100 {
            many.insert_primitive(quad.clone());
        }
        many.finish();
        renderer.render_scene_to_bgra(&many).unwrap();
        assert!(renderer.pipelines.quad_pipeline.buffer_size > 64);
        renderer.shed_memory(crate::MemoryPressureLevel::Warning);
        assert_eq!(renderer.pipelines.quad_pipeline.buffer_size, 64);
        renderer.render_scene_to_bgra(&many).unwrap();
        assert!(renderer.pipelines.quad_pipeline.buffer_size > 64);
    }
}
