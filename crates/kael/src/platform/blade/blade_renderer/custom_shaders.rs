//! GPU-resident custom fragment passes. Submitted resources are retained until
//! their queue fence completes; synchronous exports have a bounded wait.
use crate::render_target::{RenderTargetDisplayParams, TargetRegistry};
use crate::{
    DevicePixels, PaintSurface, RenderTarget, RenderTargetDescriptor, RenderTargetError,
    RenderTargetFormat, RenderTargetReadback, ShaderBackend, ShaderBinding, ShaderBindings,
    ShaderHandle, ShaderResourceSlot, ShaderResourceSlotKind, ShaderSampler, Size,
};
use blade_graphics as gpu;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

type Result<T> = std::result::Result<T, RenderTargetError>;
const MAX_PIPELINES: usize = 64;
const DEADLINE_MS: u32 = 10_000;
const MAX_READBACK: u64 = 256 * 1024 * 1024;
const UNIFORM_NAMES: [&str; 4] = [
    "kael_uniform_a",
    "kael_uniform_b",
    "kael_uniform_c",
    "kael_uniform_d",
];
const TEXTURE_NAMES: [&str; 8] = [
    "kael_texture_a",
    "kael_texture_b",
    "kael_texture_c",
    "kael_texture_d",
    "kael_texture_e",
    "kael_texture_f",
    "kael_texture_g",
    "kael_texture_h",
];
const SAMPLER_NAMES: [&str; 8] = [
    "kael_sampler_a",
    "kael_sampler_b",
    "kael_sampler_c",
    "kael_sampler_d",
    "kael_sampler_e",
    "kael_sampler_f",
    "kael_sampler_g",
    "kael_sampler_h",
];

#[derive(Clone, Copy)]
struct Target {
    texture: gpu::Texture,
    view: gpu::TextureView,
}
struct Pipeline {
    raw: gpu::RenderPipeline,
    slots: Vec<ShaderResourceSlot>,
    used: u64,
}
struct ComputePipeline {
    raw: gpu::ComputePipeline,
    slots: Vec<crate::ComputeSlot>,
    used: u64,
}
struct Pending {
    fence: gpu::SyncPoint,
    buffers: Vec<gpu::Buffer>,
}
#[derive(Clone, Copy)]
enum Value {
    Buffer(gpu::BufferPiece),
    Texture(gpu::TextureView),
    Sampler(gpu::Sampler),
}
struct Data<'a>(&'a [Value]);
impl gpu::ShaderData for Data<'_> {
    fn layout() -> gpu::ShaderDataLayout {
        gpu::ShaderDataLayout::EMPTY.clone()
    }
    fn fill(&self, mut context: gpu::PipelineContext) {
        use gpu::ShaderBindable as _;
        for (index, value) in self.0.iter().enumerate() {
            match value {
                Value::Buffer(v) => v.bind_to(&mut context, index as u32),
                Value::Texture(v) => v.bind_to(&mut context, index as u32),
                Value::Sampler(v) => v.bind_to(&mut context, index as u32),
            }
        }
    }
}
#[derive(blade_macros::ShaderData)]
struct DisplayData {
    params: RenderTargetDisplayParams,
    image: gpu::TextureView,
    sampling: gpu::Sampler,
}

pub(super) struct BladeCustomRenderer {
    gpu: Arc<gpu::Context>,
    targets: TargetRegistry<Target>,
    buffers: crate::compute::BufferRegistry<gpu::Buffer>,
    compute: BTreeMap<u64, ComputePipeline>,
    encoder: gpu::CommandEncoder,
    pending: VecDeque<Pending>,
    ui_fence: Option<gpu::SyncPoint>,
    pipelines: BTreeMap<(u64, RenderTargetFormat), Pipeline>,
    display: BTreeMap<gpu::TextureFormat, gpu::RenderPipeline>,
    samplers: BTreeMap<ShaderSampler, gpu::Sampler>,
    tick: u64,
    #[cfg(test)]
    compilations: u32,
}
impl BladeCustomRenderer {
    pub(super) fn invalidate_device(&mut self) {
        self.targets.invalidate_device();
    }
    #[cfg(test)]
    pub(super) fn graph_allocated_bytes(&self) -> u64 {
        #[cfg(target_os = "macos")]
        {
            use objc2_metal::MTLDevice as _;
            self.gpu.metal_device().currentAllocatedSize() as u64
        }
        #[cfg(not(target_os = "macos"))]
        self.targets.used_bytes()
    }
    pub(super) fn new(gpu: Arc<gpu::Context>) -> Self {
        let encoder = gpu.create_command_encoder(gpu::CommandEncoderDesc {
            name: "custom fragments",
            buffer_count: 3,
        });
        let targets = TargetRegistry::default();
        let buffers = crate::compute::BufferRegistry::new(targets.device_budget());
        Self {
            gpu,
            targets,
            buffers,
            compute: Default::default(),
            encoder,
            pending: Default::default(),
            ui_fence: None,
            pipelines: Default::default(),
            display: Default::default(),
            samplers: Default::default(),
            tick: 0,
            #[cfg(test)]
            compilations: 0,
        }
    }
    fn ready(&mut self, timeout: u32) -> Result<bool> {
        while let Some(first) = self.pending.front() {
            if !self.gpu.wait_for(&first.fence, timeout).map_err(|error| {
                self.targets.invalidate_device();
                backend(error)
            })? {
                return Ok(false);
            }
            let done = self.pending.pop_front().unwrap();
            for buffer in done.buffers {
                self.gpu.destroy_buffer(buffer);
            }
        }
        if let Some(fence) = &self.ui_fence {
            if !self.gpu.wait_for(fence, timeout).map_err(|error| {
                self.targets.invalidate_device();
                backend(error)
            })? {
                return Ok(false);
            }
            self.ui_fence = None;
        }
        Ok(true)
    }
    fn prepare(&mut self) -> Result<()> {
        self.ready(0)?;
        // Three encoder command buffers, with at most two older submissions.
        if self.pending.len() >= 2 && !self.ready(DEADLINE_MS)? {
            return Err(RenderTargetError::Backend(
                "custom fragment GPU deadline exceeded".into(),
            ));
        }
        self.encoder.start();
        Ok(())
    }
    fn submit(&mut self, buffers: Vec<gpu::Buffer>) -> gpu::SyncPoint {
        let fence = self.gpu.submit(&mut self.encoder);
        self.pending.push_back(Pending {
            fence: fence.clone(),
            buffers,
        });
        fence
    }
    fn prune(&mut self) -> Result<()> {
        if self.ready(0)? {
            for buffer in self.buffers.take_unused() {
                self.gpu.destroy_buffer(buffer);
            }
            for target in self.targets.take_unused() {
                destroy_target(&self.gpu, target);
            }
        }
        Ok(())
    }
    pub(super) fn after_frame(&mut self, fence: &gpu::SyncPoint) {
        self.ui_fence = Some(fence.clone());
    }
    pub(super) fn validate(&self, target: &RenderTarget) -> Result<()> {
        self.targets.get(target).map(|_| ())
    }
    pub(super) fn set_budget(&mut self, bytes: u64) {
        self.targets.set_budget(bytes);
        let _ = self.prune();
    }
    pub(super) fn shed_memory(&mut self) {
        let _ = self.prune();
        if self.ready(0).unwrap_or(false) {
            for (_, mut pipeline) in std::mem::take(&mut self.compute) {
                self.gpu.destroy_compute_pipeline(&mut pipeline.raw);
            }
            for (_, mut pipeline) in std::mem::take(&mut self.pipelines) {
                self.gpu.destroy_render_pipeline(&mut pipeline.raw);
            }
        }
    }
    pub(super) fn create(&mut self, descriptor: RenderTargetDescriptor) -> Result<RenderTarget> {
        self.targets.check_request(descriptor)?;
        self.prune()?;
        let bytes = self.targets.check_allocation(descriptor)?;
        self.prepare()?;
        let format = format(descriptor.format);
        let texture = self.gpu.create_texture(gpu::TextureDesc {
            name: "custom target",
            format,
            size: extent(descriptor),
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage: gpu::TextureUsage::TARGET
                | gpu::TextureUsage::RESOURCE
                | gpu::TextureUsage::COPY
                | if matches!(
                    descriptor.format,
                    RenderTargetFormat::Rgba8Unorm | RenderTargetFormat::Rgba16Float
                ) {
                    gpu::TextureUsage::STORAGE
                } else {
                    gpu::TextureUsage::empty()
                },
            external: None,
        });
        let view = self.gpu.create_texture_view(
            texture,
            gpu::TextureViewDesc {
                name: "custom target",
                format,
                dimension: gpu::ViewDimension::D2,
                subresources: &Default::default(),
            },
        );
        let resource = Target { texture, view };
        let target = match self.targets.insert(descriptor, resource, bytes) {
            Ok(target) => target,
            Err(error) => {
                destroy_target(&self.gpu, resource);
                return Err(error);
            }
        };
        self.encoder.init_texture(texture);
        let colors = [gpu::RenderTarget {
            view,
            init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
            finish_op: gpu::FinishOp::Store,
        }];
        drop(self.encoder.render(
            "clear custom target",
            gpu::RenderTargetSet {
                colors: &colors,
                depth_stencil: None,
            },
        ));
        self.submit(Vec::new());
        Ok(target)
    }
    pub(super) fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<()> {
        bindings.validate(shader, target, &self.targets)?;
        let key = (shader.id(), target.descriptor().format);
        if !self.pipelines.contains_key(&key) {
            if self.pipelines.len() >= MAX_PIPELINES {
                if !self.ready(DEADLINE_MS)? {
                    return Err(RenderTargetError::ResourceLimit);
                }
                let oldest = *self.pipelines.iter().min_by_key(|(_, v)| v.used).unwrap().0;
                let mut old = self.pipelines.remove(&oldest).unwrap();
                self.gpu.destroy_render_pipeline(&mut old.raw);
            }
            let translated = shader.translate(ShaderBackend::Blade).map_err(backend)?;
            let program = self
                .gpu
                .try_create_shader(gpu::ShaderDesc {
                    source: &translated.source,
                    naga_module: None,
                })
                .map_err(backend)?;
            let layouts: Vec<_> = translated
                .resources
                .chunks(gpu::limits::RESOURCES_IN_GROUP as usize)
                .map(|slots| gpu::ShaderDataLayout {
                    bindings: slots
                        .iter()
                        .map(|slot| match slot.kind {
                            ShaderResourceSlotKind::Uniform => (
                                UNIFORM_NAMES[slot.slot as usize],
                                gpu::ShaderBinding::Buffer,
                            ),
                            ShaderResourceSlotKind::Texture => (
                                TEXTURE_NAMES[slot.slot as usize],
                                gpu::ShaderBinding::Texture,
                            ),
                            ShaderResourceSlotKind::Sampler => (
                                SAMPLER_NAMES[slot.slot as usize],
                                gpu::ShaderBinding::Sampler,
                            ),
                        })
                        .collect(),
                })
                .collect();
            let references: Vec<_> = layouts.iter().collect();
            let raw = self.gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "custom fragment",
                data_layouts: &references,
                vertex: program.at(&translated.vertex_entry),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(program.at(&translated.fragment_entry)),
                color_targets: &[gpu::ColorTargetState {
                    format: format(target.descriptor().format),
                    blend: Some(gpu::BlendState::ALPHA_BLENDING),
                    write_mask: Default::default(),
                }],
                multisample_state: Default::default(),
            });
            self.pipelines.insert(
                key,
                Pipeline {
                    raw,
                    slots: translated.resources,
                    used: self.tick,
                },
            );
            #[cfg(test)]
            {
                self.compilations += 1;
            }
        }
        self.prepare()?;
        let mut retained = Vec::new();
        let mut values = Vec::new();
        for slot in &self.pipelines[&key].slots {
            match bindings.get(slot.binding).expect("validated binding") {
                ShaderBinding::Uniform(bytes) => {
                    let buffer = self.gpu.create_buffer(gpu::BufferDesc {
                        name: "custom uniform",
                        size: bytes.len() as u64,
                        memory: gpu::Memory::Shared,
                    });
                    // SAFETY: Shared buffers expose their full allocation and the
                    // reflected uniform byte count was validated before encoding.
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.data(), bytes.len());
                    }
                    retained.push(buffer);
                    values.push(Value::Buffer(buffer.into()));
                }
                ShaderBinding::Texture(input) => {
                    values.push(Value::Texture(self.targets.get(input)?.view))
                }
                ShaderBinding::Sampler(kind) => {
                    let sampler = *self.samplers.entry(*kind).or_insert_with(|| {
                        self.gpu.create_sampler(gpu::SamplerDesc {
                            name: "custom sampler",
                            address_modes: [gpu::AddressMode::ClampToEdge; 3],
                            min_filter: filter(*kind),
                            mag_filter: filter(*kind),
                            lod_max_clamp: Some(0.0),
                            anisotropy_clamp: 1,
                            ..Default::default()
                        })
                    });
                    values.push(Value::Sampler(sampler));
                }
            }
        }
        self.tick = self.tick.saturating_add(1);
        let pipeline = self.pipelines.get_mut(&key).unwrap();
        pipeline.used = self.tick;
        let colors = [gpu::RenderTarget {
            view: self.targets.get(target)?.view,
            init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
            finish_op: gpu::FinishOp::Store,
        }];
        {
            let mut pass = self.encoder.render(
                "custom fragment",
                gpu::RenderTargetSet {
                    colors: &colors,
                    depth_stencil: None,
                },
            );
            let mut encoder = pass.with(&pipeline.raw);
            for (group, chunk) in values
                .chunks(gpu::limits::RESOURCES_IN_GROUP as usize)
                .enumerate()
            {
                encoder.bind(group as u32, &Data(chunk));
            }
            encoder.draw(0, 3, 0, 1);
        }
        self.submit(retained);
        target.did_render();
        Ok(())
    }
    pub(super) fn create_buffer(
        &mut self,
        descriptor: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer> {
        descriptor.validate()?;
        self.prune()?;
        let size = self.buffers.check(descriptor)?;
        self.prepare()?;
        let raw = self.gpu.create_buffer(gpu::BufferDesc {
            name: "compute storage",
            size,
            memory: gpu::Memory::Device,
        });
        let buffer = match self.buffers.insert(descriptor, raw, size) {
            Ok(b) => b,
            Err(e) => {
                self.gpu.destroy_buffer(raw);
                return Err(e);
            }
        };
        self.encoder
            .transfer("clear compute buffer")
            .fill_buffer(raw.into(), size, 0);
        self.submit(Vec::new());
        Ok(buffer)
    }
    pub(super) fn validate_buffer(&self, buffer: &crate::GpuBuffer) -> Result<()> {
        self.buffers.get(buffer).map(|_| ())
    }
    pub(super) fn write_buffer(
        &mut self,
        buffer: &crate::GpuBuffer,
        offset: u64,
        bytes: &[u8],
    ) -> Result<()> {
        let raw = *self.buffers.get(buffer)?;
        crate::compute::validate_buffer_write(buffer, offset, bytes)?;
        if bytes.is_empty() {
            return Ok(());
        }
        self.prepare()?;
        let staging = self.gpu.create_buffer(gpu::BufferDesc {
            name: "compute upload",
            size: bytes.len() as u64,
            memory: gpu::Memory::Shared,
        });
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), staging.data(), bytes.len());
        }
        self.encoder
            .transfer("compute upload")
            .copy_buffer_to_buffer(staging.into(), raw.at(offset), bytes.len() as u64);
        self.submit(vec![staging]);
        buffer.did_write();
        Ok(())
    }
    pub(super) fn read_buffer(&mut self, buffer: &crate::GpuBuffer) -> Result<Vec<u8>> {
        let raw = *self.buffers.get(buffer)?;
        self.prepare()?;
        let size = buffer.descriptor().byte_len;
        let staging = self.gpu.create_buffer(gpu::BufferDesc {
            name: "compute readback",
            size,
            memory: gpu::Memory::Shared,
        });
        self.encoder
            .transfer("compute readback")
            .copy_buffer_to_buffer(raw.into(), staging.into(), size);
        let fence = self.submit(vec![staging]);
        if !self.gpu.wait_for(&fence, DEADLINE_MS).map_err(backend)? {
            return Err(backend("compute readback deadline exceeded"));
        }
        let result = unsafe { std::slice::from_raw_parts(staging.data(), size as usize) }.to_vec();
        self.ready(0)?;
        Ok(result)
    }
    pub(super) fn write_target(&mut self, target: &RenderTarget, pixels: &[u8]) -> Result<()> {
        let raw = *self.targets.get(target)?;
        let d = target.descriptor();
        if pixels.len() as u64 != d.byte_len()? {
            return Err(RenderTargetError::InvalidBindings(
                "target upload requires exact packed pixel bytes".into(),
            ));
        }
        let row = d.width * d.format.bytes_per_pixel();
        let stride = (row + 255) & !255;
        let total = u64::from(stride) * u64::from(d.height);
        if total > MAX_READBACK {
            return Err(RenderTargetError::BudgetExceeded);
        }
        self.prepare()?;
        let staging = self.gpu.create_buffer(gpu::BufferDesc {
            name: "target upload",
            size: total,
            memory: gpu::Memory::Shared,
        });
        unsafe {
            for (y, source) in pixels.chunks_exact(row as usize).enumerate() {
                let dest = std::slice::from_raw_parts_mut(
                    staging.data().add(y * stride as usize),
                    row as usize,
                );
                dest.copy_from_slice(source);
                if d.format == RenderTargetFormat::Bgra8UnormSrgb {
                    for p in dest.chunks_exact_mut(4) {
                        p.swap(0, 2);
                    }
                }
            }
        }
        self.encoder
            .transfer("target upload")
            .copy_buffer_to_texture(staging.into(), stride, raw.texture.into(), extent(d));
        self.submit(vec![staging]);
        target.did_render();
        Ok(())
    }
    pub(super) fn dispatch(
        &mut self,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<()> {
        bindings.validate(shader, groups, &self.targets, &self.buffers)?;
        self.tick = self.tick.saturating_add(1);
        if !self.compute.contains_key(&shader.id()) {
            if self.compute.len() >= 64 {
                if !self.ready(DEADLINE_MS)? {
                    return Err(RenderTargetError::ResourceLimit);
                }
                let id = *self.compute.iter().min_by_key(|(_, p)| p.used).unwrap().0;
                let mut pipeline = self.compute.remove(&id).unwrap();
                self.gpu.destroy_compute_pipeline(&mut pipeline.raw);
            }
            let t = shader.translate(ShaderBackend::Blade).map_err(backend)?;
            let program = self
                .gpu
                .try_create_shader(gpu::ShaderDesc {
                    source: &t.source,
                    naga_module: None,
                })
                .map_err(backend)?;
            let layouts: Vec<_> = t
                .resources
                .chunks(gpu::limits::RESOURCES_IN_GROUP as usize)
                .map(|slots| gpu::ShaderDataLayout {
                    bindings: slots
                        .iter()
                        .map(|slot| {
                            (
                                compute_name(slot),
                                match slot.kind {
                                    crate::ComputeSlotKind::Uniform
                                    | crate::ComputeSlotKind::ReadBuffer
                                    | crate::ComputeSlotKind::WriteBuffer => {
                                        gpu::ShaderBinding::Buffer
                                    }
                                    crate::ComputeSlotKind::Texture
                                    | crate::ComputeSlotKind::WriteTexture => {
                                        gpu::ShaderBinding::Texture
                                    }
                                    crate::ComputeSlotKind::Sampler => gpu::ShaderBinding::Sampler,
                                },
                            )
                        })
                        .collect(),
                })
                .collect();
            let refs: Vec<_> = layouts.iter().collect();
            let raw = self.gpu.create_compute_pipeline(gpu::ComputePipelineDesc {
                name: "custom compute",
                data_layouts: &refs,
                compute: program.at(&t.entry_point),
            });
            self.compute.insert(
                shader.id(),
                ComputePipeline {
                    raw,
                    slots: t.resources,
                    used: self.tick,
                },
            );
        }
        self.prepare()?;
        let mut retained = Vec::new();
        let mut values = Vec::new();
        for slot in &self.compute[&shader.id()].slots {
            match bindings.get(slot.binding).unwrap() {
                crate::ComputeBinding::Uniform(bytes) => {
                    let raw = self.gpu.create_buffer(gpu::BufferDesc {
                        name: "compute uniform",
                        size: bytes.len() as u64,
                        memory: gpu::Memory::Shared,
                    });
                    unsafe {
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), raw.data(), bytes.len());
                    }
                    retained.push(raw);
                    values.push(Value::Buffer(raw.into()));
                }
                crate::ComputeBinding::StorageBuffer(buffer) => {
                    values.push(Value::Buffer((*self.buffers.get(buffer)?).into()))
                }
                crate::ComputeBinding::Texture(target)
                | crate::ComputeBinding::StorageTexture(target) => {
                    values.push(Value::Texture(self.targets.get(target)?.view))
                }
                crate::ComputeBinding::Sampler(kind) => {
                    let raw = *self.samplers.entry(*kind).or_insert_with(|| {
                        self.gpu.create_sampler(gpu::SamplerDesc {
                            name: "compute sampler",
                            address_modes: [gpu::AddressMode::ClampToEdge; 3],
                            min_filter: filter(*kind),
                            mag_filter: filter(*kind),
                            lod_max_clamp: Some(0.0),
                            anisotropy_clamp: 1,
                            ..Default::default()
                        })
                    });
                    values.push(Value::Sampler(raw));
                }
            }
        }
        let pipeline = self.compute.get_mut(&shader.id()).unwrap();
        pipeline.used = self.tick;
        {
            let mut pass = self.encoder.compute("custom compute");
            let mut encoder = pass.with(&pipeline.raw);
            for (group, chunk) in values
                .chunks(gpu::limits::RESOURCES_IN_GROUP as usize)
                .enumerate()
            {
                encoder.bind(group as u32, &Data(chunk));
            }
            encoder.dispatch(groups);
        }
        self.submit(retained);
        bindings.did_dispatch(shader);
        Ok(())
    }

    pub(super) fn read(&mut self, target: &RenderTarget) -> Result<RenderTargetReadback> {
        let descriptor = target.descriptor();
        let resource = *self.targets.get(target)?;
        let row = descriptor
            .width
            .checked_mul(descriptor.format.bytes_per_pixel())
            .ok_or(RenderTargetError::InvalidDimensions)?;
        let stride = row
            .checked_add(255)
            .ok_or(RenderTargetError::InvalidDimensions)?
            & !255;
        let total = u64::from(stride) * u64::from(descriptor.height);
        if total > MAX_READBACK {
            return Err(RenderTargetError::BudgetExceeded);
        }
        self.prepare()?;
        let buffer = self.gpu.create_buffer(gpu::BufferDesc {
            name: "custom target readback",
            size: total,
            memory: gpu::Memory::Shared,
        });
        self.encoder
            .transfer("custom target readback")
            .copy_texture_to_buffer(
                resource.texture.into(),
                buffer.into(),
                stride,
                extent(descriptor),
            );
        let fence = self.submit(vec![buffer]);
        if !self.gpu.wait_for(&fence, DEADLINE_MS).map_err(backend)? {
            return Err(RenderTargetError::Backend(
                "custom target readback deadline exceeded".into(),
            ));
        }
        let mut pixels = vec![0; descriptor.byte_len()? as usize];
        // SAFETY: The copy fence completed, and the padded staging allocation
        // and tightly packed destination sizes were checked before allocation.
        unsafe {
            for (y, dest) in pixels.chunks_exact_mut(row as usize).enumerate() {
                std::ptr::copy_nonoverlapping(
                    buffer.data().add(y * stride as usize),
                    dest.as_mut_ptr(),
                    dest.len(),
                );
            }
        }
        if descriptor.format == RenderTargetFormat::Bgra8UnormSrgb {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        self.ready(0)?;
        Ok(RenderTargetReadback { descriptor, pixels })
    }
    pub(super) fn draw(
        &mut self,
        surface: &PaintSurface,
        target: &RenderTarget,
        viewport: Size<DevicePixels>,
        output: gpu::TextureFormat,
        pass: &mut gpu::RenderCommandEncoder<'_>,
    ) -> Result<()> {
        use gpu::ShaderData as _;
        let image = self.targets.get(target)?.view;
        if !self.display.contains_key(&output) {
            let program = self
                .gpu
                .try_create_shader(gpu::ShaderDesc {
                    source: DISPLAY_WGSL,
                    naga_module: None,
                })
                .map_err(backend)?;
            let layout = DisplayData::layout();
            let pipeline = self.gpu.create_render_pipeline(gpu::RenderPipelineDesc {
                name: "custom target display",
                data_layouts: &[&layout],
                vertex: program.at("display_vertex"),
                vertex_fetches: &[],
                primitive: gpu::PrimitiveState {
                    topology: gpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                fragment: Some(program.at("display_fragment")),
                color_targets: &[gpu::ColorTargetState {
                    format: output,
                    blend: Some(gpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: Default::default(),
                }],
                multisample_state: Default::default(),
            });
            self.display.insert(output, pipeline);
        }
        let sampling = *self
            .samplers
            .entry(ShaderSampler::LinearClamp)
            .or_insert_with(|| {
                self.gpu.create_sampler(gpu::SamplerDesc {
                    name: "custom display sampler",
                    address_modes: [gpu::AddressMode::ClampToEdge; 3],
                    min_filter: gpu::FilterMode::Linear,
                    mag_filter: gpu::FilterMode::Linear,
                    lod_max_clamp: Some(0.0),
                    anisotropy_clamp: 1,
                    ..Default::default()
                })
            });
        let mut encoder = pass.with(&self.display[&output]);
        encoder.bind(
            0,
            &DisplayData {
                params: RenderTargetDisplayParams::new(surface, target, viewport),
                image,
                sampling,
            },
        );
        encoder.draw(0, 4, 0, 1);
        Ok(())
    }
}
impl Drop for BladeCustomRenderer {
    fn drop(&mut self) {
        let resources = self.targets.invalidate_and_drain();
        let buffers = self.buffers.invalidate_and_drain();
        if !self.ready(DEADLINE_MS).unwrap_or(false) {
            // A timed-out GPU may still reference every raw allocation. Retain
            // its context and allocations instead of freeing in-flight memory.
            std::mem::forget(self.gpu.clone());
            return;
        }
        for buffer in buffers {
            self.gpu.destroy_buffer(buffer);
        }
        for (_, mut pipeline) in std::mem::take(&mut self.compute) {
            self.gpu.destroy_compute_pipeline(&mut pipeline.raw);
        }
        for target in resources {
            destroy_target(&self.gpu, target);
        }
        for (_, mut pipeline) in std::mem::take(&mut self.pipelines) {
            self.gpu.destroy_render_pipeline(&mut pipeline.raw);
        }
        for (_, mut pipeline) in std::mem::take(&mut self.display) {
            self.gpu.destroy_render_pipeline(&mut pipeline);
        }
        for (_, sampler) in std::mem::take(&mut self.samplers) {
            self.gpu.destroy_sampler(sampler);
        }
        self.gpu.destroy_command_encoder(&mut self.encoder);
    }
}
fn compute_name(slot: &crate::ComputeSlot) -> &'static str {
    let index = (slot.name.as_bytes().last().unwrap() - b'a') as usize;
    match slot.kind {
        crate::ComputeSlotKind::Uniform => UNIFORM_NAMES[index],
        crate::ComputeSlotKind::Texture => TEXTURE_NAMES[index],
        crate::ComputeSlotKind::Sampler => SAMPLER_NAMES[index],
        crate::ComputeSlotKind::ReadBuffer => [
            "kael_read_buffer_a",
            "kael_read_buffer_b",
            "kael_read_buffer_c",
            "kael_read_buffer_d",
            "kael_read_buffer_e",
            "kael_read_buffer_f",
            "kael_read_buffer_g",
            "kael_read_buffer_h",
        ][index],
        crate::ComputeSlotKind::WriteBuffer => [
            "kael_write_buffer_a",
            "kael_write_buffer_b",
            "kael_write_buffer_c",
            "kael_write_buffer_d",
            "kael_write_buffer_e",
            "kael_write_buffer_f",
            "kael_write_buffer_g",
            "kael_write_buffer_h",
        ][index],
        crate::ComputeSlotKind::WriteTexture => [
            "kael_write_texture_a",
            "kael_write_texture_b",
            "kael_write_texture_c",
            "kael_write_texture_d",
            "kael_write_texture_e",
            "kael_write_texture_f",
            "kael_write_texture_g",
            "kael_write_texture_h",
        ][index],
    }
}
fn backend(error: impl std::fmt::Debug) -> RenderTargetError {
    RenderTargetError::Backend(format!("{error:?}"))
}
fn destroy_target(gpu: &gpu::Context, target: Target) {
    gpu.destroy_texture_view(target.view);
    gpu.destroy_texture(target.texture);
}
fn filter(kind: ShaderSampler) -> gpu::FilterMode {
    match kind {
        ShaderSampler::LinearClamp => gpu::FilterMode::Linear,
        ShaderSampler::NearestClamp => gpu::FilterMode::Nearest,
    }
}
fn extent(descriptor: RenderTargetDescriptor) -> gpu::Extent {
    gpu::Extent {
        width: descriptor.width,
        height: descriptor.height,
        depth: 1,
    }
}
fn format(value: RenderTargetFormat) -> gpu::TextureFormat {
    match value {
        RenderTargetFormat::Rgba8Unorm => gpu::TextureFormat::Rgba8Unorm,
        RenderTargetFormat::Rgba8UnormSrgb => gpu::TextureFormat::Rgba8UnormSrgb,
        RenderTargetFormat::Bgra8UnormSrgb => gpu::TextureFormat::Bgra8UnormSrgb,
        RenderTargetFormat::Rgba16Float => gpu::TextureFormat::Rgba16Float,
        RenderTargetFormat::R8Unorm => gpu::TextureFormat::R8Unorm,
    }
}
const DISPLAY_WGSL: &str = r#"
struct RenderTargetDisplayParams { bounds: vec4<f32>, mask: vec4<f32>, corners: vec4<f32>, rounded_clip: vec4<f32>, rounded_corners: vec4<f32>, transform: vec4<f32>, translation: vec2<f32>, viewport: vec2<f32>, color_filter: vec4<f32>, opacity: f32, scalar: u32, padding: vec2<u32> }
var<uniform> params: RenderTargetDisplayParams;
var image: texture_2d<f32>;
var sampling: sampler;
struct Varying { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) local: vec2<f32> }
@vertex fn display_vertex(@builtin(vertex_index) id: u32) -> Varying {
    let uv = vec2<f32>(f32(id & 1u), f32((id >> 1u) & 1u));
    let local = params.bounds.xy + uv * params.bounds.zw;
    let transformed = vec2<f32>(dot(params.transform.xy, local), dot(params.transform.zw, local)) + params.translation;
    return Varying(vec4<f32>(transformed / params.viewport * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv, local);
}
fn coverage(position: vec2<f32>, bounds: vec4<f32>, corners: vec4<f32>) -> f32 {
    if all(corners == vec4<f32>(0.0)) { return 1.0; }
    let centered = position - bounds.xy - bounds.zw * 0.5;
    let radius = select(select(corners.z, corners.w, centered.x < 0.0), select(corners.y, corners.x, centered.x < 0.0), centered.y < 0.0);
    let q = abs(centered) - bounds.zw * 0.5 + radius;
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - distance, 0.0, 1.0);
}
@fragment fn display_fragment(input: Varying) -> @location(0) vec4<f32> {
    if any(input.position.xy < params.mask.xy) || any(input.position.xy >= params.mask.xy + params.mask.zw) { discard; }
    var color = textureSample(image, sampling, input.uv);
    if params.scalar != 0u { color = vec4<f32>(color.rrr, 1.0); }
    if color.a > 0.0 {
        var straight = ((color.rgb / color.a - vec3<f32>(0.5)) * params.color_filter.w + vec3<f32>(0.5)) * params.color_filter.z;
        var gray = vec3<f32>(dot(straight, vec3<f32>(0.2126, 0.7152, 0.0722)));
        straight = mix(gray, straight, params.color_filter.y);
        gray = vec3<f32>(dot(straight, vec3<f32>(0.2126, 0.7152, 0.0722)));
        color = vec4<f32>(mix(straight, gray, params.color_filter.x) * color.a, color.a);
    }
    return color * (params.opacity * coverage(input.local, params.bounds, params.corners) * coverage(input.position.xy, params.rounded_clip, params.rounded_corners));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, ContentMask, PaintSurfaceSource, ShaderDescriptor, point, size};
    fn renderer() -> BladeCustomRenderer {
        // SAFETY: Tests own a headless device and destroy all resources after fences complete.
        let gpu = unsafe {
            gpu::Context::init(gpu::ContextDesc {
                presentation: false,
                validation: false,
                ..Default::default()
            })
        }
        .expect("native Blade device");
        BladeCustomRenderer::new(Arc::new(gpu))
    }
    fn shader(source: &str) -> ShaderHandle {
        ShaderHandle::compile_fragment(ShaderDescriptor::fragment("blade_pixel", source, "fs_main"))
            .unwrap()
    }
    #[test]
    fn blade_compute_runtime_arrays_uploads_and_loop_budget() {
        let mut renderer = renderer();
        let input = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let pixels = [32, 64, 128, 128].repeat(16);
        renderer.write_target(&input, &pixels).unwrap();
        assert_eq!(renderer.read(&input).unwrap().pixels, pixels);
        let output = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let buffer = renderer
            .create_buffer(crate::GpuBufferDescriptor { byte_len: 64 })
            .unwrap();
        assert_eq!(renderer.read_buffer(&buffer).unwrap(), [0; 64]);
        let compute=crate::ComputeHandle::compile(crate::ComputeDescriptor::new("blade compute",r#"
            @group(0) @binding(7) var source: texture_2d<f32>;
            @group(0) @binding(11) var<storage, read_write> values: array<u32>;
            @group(0) @binding(19) var destination: texture_storage_2d<rgba8unorm, write>;
            @compute @workgroup_size(2,2) fn main(@builtin(global_invocation_id) id:vec3<u32>) { if(id.x>=4u||id.y>=4u){return;}let index=id.y*4u+id.x;if(index<arrayLength(&values)){values[index]=index+100u;}textureStore(destination,vec2<i32>(id.xy),textureLoad(source,vec2<i32>(id.xy),0)); }
        "#,"main")).unwrap();
        let bindings = crate::ComputeBindings::new()
            .with(7, crate::ComputeBinding::Texture(input.clone()))
            .with(11, crate::ComputeBinding::StorageBuffer(buffer.clone()))
            .with(19, crate::ComputeBinding::StorageTexture(output.clone()));
        renderer.dispatch(&compute, &bindings, [2, 2, 1]).unwrap();
        assert_eq!(renderer.read(&output).unwrap().pixels, pixels);
        assert_eq!(
            renderer.read_buffer(&buffer).unwrap(),
            (100u32..116).flat_map(u32::to_le_bytes).collect::<Vec<_>>()
        );
        renderer
            .write_buffer(&buffer, 4, &999u32.to_le_bytes())
            .unwrap();
        assert_eq!(
            &renderer.read_buffer(&buffer).unwrap()[4..8],
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
                &sync,
                &crate::ComputeBindings::new()
                    .with(31, crate::ComputeBinding::StorageBuffer(buffer.clone())),
                [1, 1, 1],
            )
            .unwrap();
        assert_eq!(
            &renderer.read_buffer(&buffer).unwrap()[..8],
            &[65555u32, 19]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        assert!(
            renderer
                .dispatch(
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
            .render(&output, &program, &ShaderBindings::new())
            .unwrap();
        assert_eq!(
            &renderer.read(&output).unwrap().pixels[..4],
            &[255, 0, 0, 255]
        );
        for format in [
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
            if format == RenderTargetFormat::Rgba16Float {
                let kernel = crate::ComputeHandle::compile(crate::ComputeDescriptor::new(
                    "HDR storage",
                    crate::compute::HDR_STORAGE_REGRESSION,
                    "main",
                ))
                .unwrap();
                renderer
                    .dispatch(
                        &kernel,
                        &crate::ComputeBindings::new()
                            .with(5, crate::ComputeBinding::StorageTexture(target.clone())),
                        [1, 1, 1],
                    )
                    .unwrap();
                assert_eq!(
                    renderer.read(&target).unwrap().pixels,
                    [0, 64, 0, 56, 0, 52, 0, 60].repeat(4)
                );
            }
        }
    }
    #[test]
    fn blade_custom_fragment_premultiplied_storage_pipeline_reuse_and_lifecycle() {
        let mut renderer = renderer();
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
        let shader = shader(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.25, 0.5, 1.0, 0.5); }",
        );
        for _ in 0..2 {
            renderer
                .render(&target, &shader, &ShaderBindings::new())
                .unwrap();
        }
        for pixel in renderer.read(&target).unwrap().pixels.chunks_exact(4) {
            for (&v, e) in pixel.iter().zip([32u8, 64, 128, 128]) {
                assert!(v.abs_diff(e) <= 1, "{pixel:?}");
            }
        }
        assert_eq!(renderer.compilations, 1);
        assert_eq!(target.revision(), 2);
        renderer.shed_memory();
        assert!(target.is_valid());
        let other = self::renderer();
        assert!(matches!(
            other.validate(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        drop(renderer);
        assert!(!target.is_valid());
    }
    #[test]
    fn blade_sparse_uniform_texture_sampler_and_feedback_validation() {
        let mut renderer = renderer();
        let input = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let output = renderer
            .create(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let uniform = shader(
            "struct Params { color: vec4<f32> } @group(0) @binding(7) var<uniform> params: Params; @fragment fn fs_main() -> @location(0) vec4<f32> { return params.color; }",
        );
        let bytes: Arc<[u8]> = bytemuck::cast_slice(&[0.25f32, 0.5, 1.0, 0.5]).into();
        renderer
            .render(
                &input,
                &uniform,
                &ShaderBindings::new().with(7, ShaderBinding::Uniform(bytes)),
            )
            .unwrap();
        let copy = shader(
            "@group(0) @binding(9) var image: texture_2d<f32>; @group(0) @binding(12) var sampling: sampler; @fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { let c = textureSample(image, sampling, uv); return vec4<f32>(c.rgb / c.a, c.a); }",
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
    }
    #[test]
    fn blade_srgb_scalar_and_hdr_target_formats() {
        let mut renderer = renderer();
        let color = shader(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.25, 0.5, 1.0, 1.0); }",
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
    }
    #[test]
    fn blade_gpu_display_obeys_premultiplied_alpha_opacity_and_rounded_clip() {
        let mut renderer = renderer();
        let input = renderer
            .create(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        let output = renderer
            .create(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        renderer.render(&input, &shader("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }"), &ShaderBindings::new()).unwrap();
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
        let view = renderer.targets.get(&output).unwrap().view;
        let mut command = renderer
            .gpu
            .create_command_encoder(gpu::CommandEncoderDesc {
                name: "display test",
                buffer_count: 1,
            });
        command.start();
        let colors = [gpu::RenderTarget {
            view,
            init_op: gpu::InitOp::Clear(gpu::TextureColor::TransparentBlack),
            finish_op: gpu::FinishOp::Store,
        }];
        {
            let mut pass = command.render(
                "display",
                gpu::RenderTargetSet {
                    colors: &colors,
                    depth_stencil: None,
                },
            );
            renderer
                .draw(
                    &surface,
                    &input,
                    size(DevicePixels(8), DevicePixels(8)),
                    gpu::TextureFormat::Rgba8Unorm,
                    &mut pass,
                )
                .unwrap();
        }
        let fence = renderer.gpu.submit(&mut command);
        renderer.after_frame(&fence);
        let read = renderer.read(&output).unwrap();
        assert_eq!(&read.pixels[..4], &[0, 0, 0, 0]);
        let center = &read.pixels[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4];
        assert!(
            center[1].abs_diff(128) <= 1 && center[3].abs_diff(128) <= 1,
            "{center:?}"
        );
        assert!(renderer.gpu.wait_for(&fence, DEADLINE_MS).unwrap());
        renderer.gpu.destroy_command_encoder(&mut command);
    }
}
