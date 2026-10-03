use std::{
    collections::{BTreeMap, VecDeque},
    ffi::c_void,
    mem,
};

use block::ConcreteBlock;
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{DeviceRef, MTLPixelFormat, MTLResourceOptions};
use objc2::msg_send;

use crate::{
    DevicePixels, PaintSurface, RenderTarget, RenderTargetDescriptor, RenderTargetError,
    RenderTargetFormat, RenderTargetReadback, ShaderBackend, ShaderBinding, ShaderBindings,
    ShaderHandle, ShaderResourceSlot, ShaderResourceSlotKind, ShaderSampler, Size,
    render_target::TargetRegistry,
};

type Result<T> = std::result::Result<T, RenderTargetError>;

struct CustomPipeline {
    state: metal::RenderPipelineState,
    resources: Vec<ShaderResourceSlot>,
    used: u64,
}
struct ComputePipeline {
    state: metal::ComputePipelineState,
    translation: crate::ComputeTranslation,
    used: u64,
}

struct PendingSubmission {
    command: metal::CommandBuffer,
    uniforms: Vec<metal::Buffer>,
}

pub(super) struct MetalCustomRenderer {
    targets: TargetRegistry<metal::Texture>,
    buffers: crate::compute::BufferRegistry<metal::Buffer>,
    compute: BTreeMap<u64, ComputePipeline>,
    pipelines: BTreeMap<(u64, RenderTargetFormat), CustomPipeline>,
    display_pipelines: BTreeMap<u64, metal::RenderPipelineState>,
    samplers: BTreeMap<ShaderSampler, metal::SamplerState>,
    tick: u64,
    pending: VecDeque<PendingSubmission>,
    uniform_pool: Vec<metal::Buffer>,
    #[cfg(test)]
    compilation_count: usize,
}
impl Default for MetalCustomRenderer {
    fn default() -> Self {
        let targets = TargetRegistry::default();
        let buffers = crate::compute::BufferRegistry::new(targets.device_budget());
        Self {
            targets,
            buffers,
            compute: Default::default(),
            pipelines: Default::default(),
            display_pipelines: Default::default(),
            samplers: Default::default(),
            tick: 0,
            pending: Default::default(),
            uniform_pool: Default::default(),
            #[cfg(test)]
            compilation_count: 0,
        }
    }
}

fn wait_for_command(command: &metal::CommandBufferRef) -> Result<()> {
    let started = std::time::Instant::now();
    while !matches!(
        command.status(),
        metal::MTLCommandBufferStatus::Completed | metal::MTLCommandBufferStatus::Error
    ) {
        if started.elapsed() >= std::time::Duration::from_secs(10) {
            // Metal retains encoded resources until completion; the queue also
            // retains this command. Never map or recycle pending storage.
            return Err(backend("Metal custom command deadline exceeded"));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    Ok(())
}

fn backend(error: impl std::fmt::Display) -> RenderTargetError {
    RenderTargetError::Backend(error.to_string())
}

fn pixel_format(format: RenderTargetFormat) -> MTLPixelFormat {
    match format {
        RenderTargetFormat::Rgba8Unorm => MTLPixelFormat::RGBA8Unorm,
        RenderTargetFormat::Rgba8UnormSrgb => MTLPixelFormat::RGBA8Unorm_sRGB,
        RenderTargetFormat::Bgra8UnormSrgb => MTLPixelFormat::BGRA8Unorm_sRGB,
        RenderTargetFormat::Rgba16Float => MTLPixelFormat::RGBA16Float,
        RenderTargetFormat::R8Unorm => MTLPixelFormat::R8Unorm,
    }
}

impl MetalCustomRenderer {
    fn reap(&mut self) {
        while self.pending.front().is_some_and(|pending| {
            matches!(
                pending.command.status(),
                metal::MTLCommandBufferStatus::Completed | metal::MTLCommandBufferStatus::Error
            )
        }) {
            let completed = self.pending.pop_front().unwrap();
            if completed.command.status() == metal::MTLCommandBufferStatus::Error {
                self.targets.invalidate_device();
            }
            for buffer in completed.uniforms {
                // Upload/readback staging shares the pending-resource list.
                // Never retain a large completed staging allocation in the
                // uniform pool: its eight entries are capped at 64 KiB each.
                if buffer.length() <= 65_536 && self.uniform_pool.len() < 8 {
                    self.uniform_pool.push(buffer);
                }
            }
        }
    }
    fn prepare_submission(&mut self) -> Result<()> {
        self.reap();
        if self.pending.len() >= 2 {
            wait_for_command(&self.pending.front().unwrap().command)?;
            self.reap();
        }
        Ok(())
    }
    fn submit(&mut self, command: &metal::CommandBufferRef, uniforms: Vec<metal::Buffer>) {
        command.commit();
        self.pending.push_back(PendingSubmission {
            command: command.to_owned(),
            uniforms,
        });
    }

    pub(super) fn create(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget> {
        // An impossible request must not disturb existing resources.
        let requested = self.targets.check_request(descriptor)?;
        self.reap();
        if self.pending.is_empty() {
            drop(self.targets.take_unused());
            drop(self.buffers.take_unused());
        }
        self.targets.check_allocation(descriptor)?;
        self.prepare_submission()?;
        let metal_descriptor = metal::TextureDescriptor::new();
        metal_descriptor.set_width(u64::from(descriptor.width));
        metal_descriptor.set_height(u64::from(descriptor.height));
        metal_descriptor.set_pixel_format(pixel_format(descriptor.format));
        metal_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        metal_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        if matches!(
            descriptor.format,
            RenderTargetFormat::Rgba8Unorm | RenderTargetFormat::Rgba16Float
        ) {
            metal_descriptor
                .set_usage(metal_descriptor.usage() | metal::MTLTextureUsage::ShaderWrite);
        }
        // Metal's convenience binding assumes allocation succeeds. Preserve a
        // typed failure if the driver cannot allocate a legal bounded request.
        let texture_ptr: *mut objc2::runtime::AnyObject = unsafe {
            msg_send![device.as_ptr().cast::<objc2::runtime::AnyObject>(), newTextureWithDescriptor: metal_descriptor.as_ptr().cast::<objc2::runtime::AnyObject>()]
        };
        if texture_ptr.is_null() {
            return Err(backend("Metal target allocation failed"));
        }
        let texture = unsafe { metal::Texture::from_ptr(texture_ptr.cast()) };
        let actual = texture.allocated_size().max(requested);
        let target = self.targets.insert(descriptor, texture, actual)?;
        let command = queue.new_command_buffer();
        let encoder = super::new_texture_command_encoder(
            command,
            self.targets.get(&target)?,
            target.size(),
            metal::MTLLoadAction::Clear,
            0.0,
        );
        encoder.end_encoding();
        self.submit(command, Vec::new());
        Ok(target)
    }

    pub(super) fn validate(&self, target: &RenderTarget) -> Result<()> {
        self.targets.get(target).map(|_| ())
    }
    pub(super) fn set_budget(&mut self, bytes: u64) {
        self.targets.set_budget(bytes);
    }

    pub(super) fn shed(&mut self) {
        drop(self.targets.take_unused());
        drop(self.buffers.take_unused());
        self.pipelines.clear();
        self.compute.clear();
        self.display_pipelines.clear();
        self.samplers.clear();
        self.uniform_pool.clear();
    }

    pub(super) fn render(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<()> {
        bindings.validate(shader, target, &self.targets)?;
        self.prepare_submission()?;
        let key = (shader.id(), target.descriptor().format);
        self.tick = self.tick.saturating_add(1);
        if !self.pipelines.contains_key(&key) {
            let translation = shader.translate(ShaderBackend::Metal).map_err(backend)?;
            let library = device
                .new_library_with_source(&translation.source, &metal::CompileOptions::new())
                .map_err(backend)?;
            let state = super::build_pipeline_state(
                device,
                &library,
                "custom_fragment",
                &translation.vertex_entry,
                &translation.fragment_entry,
                pixel_format(target.descriptor().format),
            )
            .map_err(backend)?;
            if self.pipelines.len() >= 64 {
                let oldest = self
                    .pipelines
                    .iter()
                    .min_by_key(|(_, pipeline)| pipeline.used)
                    .map(|(&key, _)| key)
                    .expect("full pipeline cache has an oldest entry");
                self.pipelines.remove(&oldest);
            }
            self.pipelines.insert(
                key,
                CustomPipeline {
                    state,
                    resources: translation.resources,
                    used: self.tick,
                },
            );
            #[cfg(test)]
            {
                self.compilation_count += 1;
            }
        }
        // Samplers are cached independently of the authored binding numbers.
        for value in bindings.values.values() {
            if let ShaderBinding::Sampler(kind) = value {
                self.samplers.entry(*kind).or_insert_with(|| {
                    let descriptor = metal::SamplerDescriptor::new();
                    let filter = match kind {
                        ShaderSampler::LinearClamp => metal::MTLSamplerMinMagFilter::Linear,
                        ShaderSampler::NearestClamp => metal::MTLSamplerMinMagFilter::Nearest,
                    };
                    descriptor.set_min_filter(filter);
                    descriptor.set_mag_filter(filter);
                    descriptor.set_address_mode_s(metal::MTLSamplerAddressMode::ClampToEdge);
                    descriptor.set_address_mode_t(metal::MTLSamplerAddressMode::ClampToEdge);
                    device.new_sampler(&descriptor)
                });
            }
        }
        let pipeline = self
            .pipelines
            .get_mut(&key)
            .expect("custom pipeline was compiled");
        pipeline.used = self.tick;
        let command = queue.new_command_buffer();
        let encoder = super::new_texture_command_encoder(
            command,
            self.targets.get(target)?,
            target.size(),
            metal::MTLLoadAction::Clear,
            0.0,
        );
        encoder.set_render_pipeline_state(&pipeline.state);
        let mut uniforms = Vec::new();
        for slot in &pipeline.resources {
            match (slot.kind, bindings.get(slot.binding)) {
                (ShaderResourceSlotKind::Uniform, Some(ShaderBinding::Uniform(bytes))) => {
                    let buffer = self
                        .uniform_pool
                        .iter()
                        .position(|buffer| buffer.length() == bytes.len() as u64)
                        .map(|index| self.uniform_pool.swap_remove(index))
                        .unwrap_or_else(|| {
                            device.new_buffer(
                                bytes.len() as u64,
                                MTLResourceOptions::StorageModeShared,
                            )
                        });
                    // SAFETY: This shared buffer is new or came from a completed
                    // command, and its exact length equals the validated layout.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            buffer.contents().cast(),
                            bytes.len(),
                        );
                    }
                    encoder.set_fragment_buffer(u64::from(slot.slot), Some(&buffer), 0);
                    uniforms.push(buffer);
                }
                (ShaderResourceSlotKind::Texture, Some(ShaderBinding::Texture(input))) => {
                    encoder
                        .set_fragment_texture(u64::from(slot.slot), Some(self.targets.get(input)?));
                }
                (ShaderResourceSlotKind::Sampler, Some(ShaderBinding::Sampler(kind))) => {
                    encoder.set_fragment_sampler_state(
                        u64::from(slot.slot),
                        self.samplers.get(kind).map(|sampler| sampler.as_ref()),
                    );
                }
                _ => unreachable!("reflected shader bindings were validated before encoding"),
            }
        }
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 3);
        encoder.end_encoding();
        let handle = target.clone();
        let completed = ConcreteBlock::new(move |command: &metal::CommandBufferRef| {
            if command.status() == metal::MTLCommandBufferStatus::Error {
                handle.invalidate_device();
            }
        })
        .copy();
        command.add_completed_handler(&completed);
        self.submit(command, uniforms);
        target.did_render();
        Ok(())
    }

    pub(super) fn create_buffer(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        descriptor: crate::GpuBufferDescriptor,
    ) -> Result<crate::GpuBuffer> {
        descriptor.validate()?;
        self.prepare_submission()?;
        if self.pending.is_empty() {
            drop(self.targets.take_unused());
            drop(self.buffers.take_unused());
        }
        let bytes = self.buffers.check(descriptor)?;
        let pointer: *mut objc2::runtime::AnyObject = unsafe {
            msg_send![device.as_ptr().cast::<objc2::runtime::AnyObject>(), newBufferWithLength: bytes, options: MTLResourceOptions::StorageModePrivate.bits()]
        };
        if pointer.is_null() {
            return Err(backend("Metal storage buffer allocation failed"));
        }
        let raw = unsafe { metal::Buffer::from_ptr(pointer.cast()) };
        let actual = raw.allocated_size().max(bytes);
        let buffer = self.buffers.insert(descriptor, raw, actual)?;
        let command = queue.new_command_buffer();
        let blit = command.new_blit_command_encoder();
        blit.fill_buffer(
            self.buffers.get(&buffer)?,
            metal::NSRange {
                location: 0,
                length: bytes,
            },
            0,
        );
        blit.end_encoding();
        self.submit(command, Vec::new());
        Ok(buffer)
    }
    pub(super) fn validate_buffer(&self, buffer: &crate::GpuBuffer) -> Result<()> {
        self.buffers.get(buffer).map(|_| ())
    }
    pub(super) fn write_buffer(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        buffer: &crate::GpuBuffer,
        offset: u64,
        bytes: &[u8],
    ) -> Result<()> {
        self.buffers.get(buffer)?;
        crate::compute::validate_buffer_write(buffer, offset, bytes)?;
        if bytes.is_empty() {
            return Ok(());
        }
        self.prepare_submission()?;
        let staging = device.new_buffer_with_data(
            bytes.as_ptr().cast(),
            bytes.len() as u64,
            MTLResourceOptions::StorageModeShared,
        );
        let command = queue.new_command_buffer();
        let blit = command.new_blit_command_encoder();
        blit.copy_from_buffer(
            &staging,
            0,
            self.buffers.get(buffer)?,
            offset,
            bytes.len() as u64,
        );
        blit.end_encoding();
        self.submit(command, vec![staging]);
        buffer.did_write();
        Ok(())
    }
    pub(super) fn read_buffer(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        buffer: &crate::GpuBuffer,
    ) -> Result<Vec<u8>> {
        self.buffers.get(buffer)?;
        self.prepare_submission()?;
        let bytes = buffer.descriptor().byte_len;
        let staging = device.new_buffer(bytes, MTLResourceOptions::StorageModeShared);
        let command = queue.new_command_buffer();
        let blit = command.new_blit_command_encoder();
        blit.copy_from_buffer(self.buffers.get(buffer)?, 0, &staging, 0, bytes);
        blit.end_encoding();
        self.submit(command, vec![staging.clone()]);
        wait_for_command(command)?;
        if command.status() == metal::MTLCommandBufferStatus::Error {
            buffer.invalidate_device();
            return Err(backend("Metal buffer readback failed"));
        }
        if staging.contents().is_null() {
            return Err(backend("Metal buffer mapping failed"));
        }
        Ok(
            unsafe { std::slice::from_raw_parts(staging.contents().cast::<u8>(), bytes as usize) }
                .to_vec(),
        )
    }
    pub(super) fn write_target(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        target: &RenderTarget,
        pixels: &[u8],
    ) -> Result<()> {
        self.targets.get(target)?;
        if pixels.len() as u64 != target.descriptor().byte_len()? {
            return Err(RenderTargetError::InvalidBindings(
                "target upload requires exact packed pixel bytes".into(),
            ));
        }
        let descriptor = target.descriptor();
        let (stride, length, _) = super::checked_readback_layout(
            u64::from(descriptor.width),
            u64::from(descriptor.height),
            u64::from(descriptor.format.bytes_per_pixel()),
        )
        .map_err(backend)?;
        self.prepare_submission()?;
        let staging = device.new_buffer(length, MTLResourceOptions::StorageModeShared);
        if staging.contents().is_null() {
            return Err(backend("Metal upload staging allocation failed"));
        }
        let row_len = descriptor.width as usize * descriptor.format.bytes_per_pixel() as usize;
        unsafe {
            for (row, source) in pixels.chunks_exact(row_len).enumerate() {
                let destination = std::slice::from_raw_parts_mut(
                    staging.contents().cast::<u8>().add(row * stride as usize),
                    row_len,
                );
                destination.copy_from_slice(source);
                if descriptor.format == RenderTargetFormat::Bgra8UnormSrgb {
                    for p in destination.chunks_exact_mut(4) {
                        p.swap(0, 2);
                    }
                }
            }
        }
        let command = queue.new_command_buffer();
        let blit = command.new_blit_command_encoder();
        blit.copy_from_buffer_to_texture(
            &staging,
            0,
            stride,
            length,
            metal::MTLSize {
                width: u64::from(descriptor.width),
                height: u64::from(descriptor.height),
                depth: 1,
            },
            self.targets.get(target)?,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();
        self.submit(command, vec![staging]);
        target.did_render();
        Ok(())
    }
    pub(super) fn dispatch(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        shader: &crate::ComputeHandle,
        bindings: &crate::ComputeBindings,
        groups: [u32; 3],
    ) -> Result<()> {
        bindings.validate(shader, groups, &self.targets, &self.buffers)?;
        self.prepare_submission()?;
        self.tick = self.tick.saturating_add(1);
        if !self.compute.contains_key(&shader.id()) {
            let translation = shader.translate(ShaderBackend::Metal).map_err(backend)?;
            let library = device
                .new_library_with_source(&translation.source, &metal::CompileOptions::new())
                .map_err(backend)?;
            let function = library
                .get_function(&translation.entry_point, None)
                .map_err(backend)?;
            let state = device
                .new_compute_pipeline_state_with_function(&function)
                .map_err(backend)?;
            let workgroup = shader.workgroup_size();
            if workgroup.iter().map(|&n| u64::from(n)).product::<u64>()
                > state.max_total_threads_per_threadgroup()
            {
                return Err(RenderTargetError::Unsupported(
                    "compute workgroup exceeds this Metal pipeline's limit",
                ));
            }
            if translation
                .metal_workgroup_sizes
                .iter()
                .map(|&n| u64::from(n))
                .sum::<u64>()
                > device.max_threadgroup_memory_length()
            {
                return Err(RenderTargetError::Unsupported(
                    "compute shared memory exceeds this Metal device's limit",
                ));
            }
            if self.compute.len() >= 64 {
                let id = *self.compute.iter().min_by_key(|(_, p)| p.used).unwrap().0;
                self.compute.remove(&id);
            }
            self.compute.insert(
                shader.id(),
                ComputePipeline {
                    state,
                    translation,
                    used: self.tick,
                },
            );
        }
        for value in bindings.values.values() {
            if let crate::ComputeBinding::Sampler(kind) = value {
                self.samplers.entry(*kind).or_insert_with(|| {
                    let d = metal::SamplerDescriptor::new();
                    let f = match kind {
                        ShaderSampler::LinearClamp => metal::MTLSamplerMinMagFilter::Linear,
                        ShaderSampler::NearestClamp => metal::MTLSamplerMinMagFilter::Nearest,
                    };
                    d.set_min_filter(f);
                    d.set_mag_filter(f);
                    d.set_address_mode_s(metal::MTLSamplerAddressMode::ClampToEdge);
                    d.set_address_mode_t(metal::MTLSamplerAddressMode::ClampToEdge);
                    device.new_sampler(&d)
                });
            }
        }
        let pipeline = self.compute.get_mut(&shader.id()).unwrap();
        pipeline.used = self.tick;
        let command = queue.new_command_buffer();
        let encoder = command.new_compute_command_encoder();
        encoder.set_compute_pipeline_state(&pipeline.state);
        for (index, &bytes) in pipeline
            .translation
            .metal_workgroup_sizes
            .iter()
            .enumerate()
        {
            encoder.set_threadgroup_memory_length(index as u64, u64::from(bytes));
        }
        let mut uniforms = Vec::new();
        for slot in &pipeline.translation.resources {
            match bindings.get(slot.binding).unwrap() {
                crate::ComputeBinding::Uniform(bytes) => {
                    let buffer = device.new_buffer_with_data(
                        bytes.as_ptr().cast(),
                        bytes.len() as u64,
                        MTLResourceOptions::StorageModeShared,
                    );
                    encoder.set_buffer(u64::from(slot.slot), Some(&buffer), 0);
                    uniforms.push(buffer);
                }
                crate::ComputeBinding::StorageBuffer(buffer) => {
                    encoder.set_buffer(u64::from(slot.slot), Some(self.buffers.get(buffer)?), 0)
                }
                crate::ComputeBinding::Texture(target)
                | crate::ComputeBinding::StorageTexture(target) => {
                    encoder.set_texture(u64::from(slot.slot), Some(self.targets.get(target)?))
                }
                crate::ComputeBinding::Sampler(kind) => encoder.set_sampler_state(
                    u64::from(slot.slot),
                    self.samplers.get(kind).map(|s| s.as_ref()),
                ),
            }
        }
        let sizes = pipeline
            .translation
            .metal_size_bindings
            .iter()
            .map(|binding| match bindings.get(*binding).unwrap() {
                crate::ComputeBinding::StorageBuffer(b) => b.descriptor().byte_len as u32,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        if !sizes.is_empty() {
            encoder.set_bytes(30, (sizes.len() * 4) as u64, sizes.as_ptr().cast());
        }
        let workgroup = shader.workgroup_size();
        encoder.dispatch_thread_groups(
            metal::MTLSize {
                width: u64::from(groups[0]),
                height: u64::from(groups[1]),
                depth: u64::from(groups[2]),
            },
            metal::MTLSize {
                width: u64::from(workgroup[0]),
                height: u64::from(workgroup[1]),
                depth: u64::from(workgroup[2]),
            },
        );
        encoder.end_encoding();
        let owned = bindings.clone();
        let completed = ConcreteBlock::new(move |command: &metal::CommandBufferRef| {
            if command.status() == metal::MTLCommandBufferStatus::Error {
                for value in owned.values.values() {
                    match value {
                        crate::ComputeBinding::StorageTexture(t) => t.invalidate_device(),
                        crate::ComputeBinding::StorageBuffer(b) => b.invalidate_device(),
                        _ => {}
                    }
                }
            }
        })
        .copy();
        command.add_completed_handler(&completed);
        self.submit(command, uniforms);
        bindings.did_dispatch(shader);
        Ok(())
    }

    pub(super) fn read(
        &mut self,
        device: &DeviceRef,
        queue: &metal::CommandQueueRef,
        target: &RenderTarget,
    ) -> Result<RenderTargetReadback> {
        self.targets.get(target)?;
        let descriptor = target.descriptor();
        let (stride, buffer_len, packed_len) = super::checked_readback_layout(
            u64::from(descriptor.width),
            u64::from(descriptor.height),
            u64::from(descriptor.format.bytes_per_pixel()),
        )
        .map_err(backend)?;
        self.prepare_submission()?;
        let texture = self.targets.get(target)?.clone();
        let staging = device.new_buffer(buffer_len, MTLResourceOptions::StorageModeShared);
        let command = queue.new_command_buffer();
        let blit = command.new_blit_command_encoder();
        blit.copy_from_texture_to_buffer(
            &texture,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
            metal::MTLSize {
                width: u64::from(descriptor.width),
                height: u64::from(descriptor.height),
                depth: 1,
            },
            &staging,
            0,
            stride,
            buffer_len,
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();
        self.submit(command, Vec::new());
        wait_for_command(command)?;
        if command.status() == metal::MTLCommandBufferStatus::Error {
            target.invalidate_device();
            return Err(backend("Metal target readback command failed"));
        }
        let mut pixels = vec![0; packed_len];
        let row_len = descriptor.width as usize * descriptor.format.bytes_per_pixel() as usize;
        // SAFETY: the completed blit initialized every pixel copied below; the
        // checked layout bounds both mapped storage and destination row sizes.
        unsafe {
            let mapped = staging.contents() as *const u8;
            if mapped.is_null() {
                return Err(backend("Metal staging buffer is not mapped"));
            }
            let source = std::slice::from_raw_parts(mapped, buffer_len as usize);
            for (y, destination) in pixels.chunks_exact_mut(row_len).enumerate() {
                destination
                    .copy_from_slice(&source[y * stride as usize..y * stride as usize + row_len]);
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
        device: &DeviceRef,
        surface: &PaintSurface,
        target: &RenderTarget,
        viewport: Size<DevicePixels>,
        encoder: &metal::RenderCommandEncoderRef,
        output_format: MTLPixelFormat,
    ) -> Result<()> {
        let texture = self.targets.get(target)?;
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.display_pipelines.entry(output_format as u64)
        {
            let library = device
                .new_library_with_source(DISPLAY_SHADER, &metal::CompileOptions::new())
                .map_err(backend)?;
            let pipeline = super::build_premultiplied_pipeline_state(
                device,
                &library,
                "custom_target_display",
                "kael_target_vertex",
                "kael_target_fragment",
                output_format,
            )
            .map_err(backend)?;
            entry.insert(pipeline);
        }
        let params =
            crate::render_target::RenderTargetDisplayParams::new(surface, target, viewport);
        encoder.set_render_pipeline_state(
            self.display_pipelines
                .get(&(output_format as u64))
                .expect("target display pipeline exists"),
        );
        encoder.set_vertex_bytes(
            0,
            mem::size_of::<crate::render_target::RenderTargetDisplayParams>() as u64,
            &params as *const _ as *const c_void,
        );
        encoder.set_fragment_bytes(
            0,
            mem::size_of::<crate::render_target::RenderTargetDisplayParams>() as u64,
            &params as *const _ as *const c_void,
        );
        encoder.set_fragment_texture(0, Some(texture));
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        Ok(())
    }
}

const DISPLAY_SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct Params { float4 bounds; float4 mask; float4 corners; float4 rounded_clip; float4 rounded_corners; float4 transform; float2 translation; float2 viewport; float4 color_filter; float opacity; uint scalar; uint2 padding; };
struct Varying { float4 position [[position]]; float2 uv; float2 local; };
vertex Varying kael_target_vertex(uint id [[vertex_id]], constant Params &params [[buffer(0)]]) {
    float2 uv = float2(float(id & 1), float((id >> 1) & 1));
    float2 position = params.bounds.xy + uv * params.bounds.zw;
    Varying result;
    float2 transformed = float2(dot(params.transform.xy, position), dot(params.transform.zw, position)) + params.translation;
    result.position = float4(transformed / params.viewport * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    result.uv = uv;
    result.local = position;
    return result;
}
float rounded_coverage(float2 position, float4 bounds, float4 corners) {
    if (all(corners == 0.0)) return 1.0;
    float2 centered = position - bounds.xy - bounds.zw * 0.5;
    float radius = centered.y < 0.0 ? (centered.x < 0.0 ? corners.x : corners.y) : (centered.x < 0.0 ? corners.w : corners.z);
    float2 q = abs(centered) - bounds.zw * 0.5 + radius;
    float distance = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - distance, 0.0, 1.0);
}
fragment float4 kael_target_fragment(Varying input [[stage_in]], constant Params &params [[buffer(0)]], texture2d<float> target [[texture(0)]]) {
    if (any(input.position.xy < params.mask.xy) || any(input.position.xy >= params.mask.xy + params.mask.zw)) discard_fragment();
    constexpr sampler sampling(coord::normalized, filter::linear, address::clamp_to_edge);
    float4 color = target.sample(sampling, input.uv);
    if (params.scalar != 0) color = float4(color.rrr, 1.0);
    if (color.a > 0.0) {
        float3 straight = color.rgb / color.a;
        straight = ((straight - 0.5) * params.color_filter.w + 0.5) * params.color_filter.z;
        float3 grayscale = float3(dot(straight, float3(0.2126, 0.7152, 0.0722)));
        straight = mix(grayscale, straight, params.color_filter.y);
        grayscale = float3(dot(straight, float3(0.2126, 0.7152, 0.0722)));
        color.rgb = mix(straight, grayscale, params.color_filter.x) * color.a;
    }
    float coverage = rounded_coverage(input.local, params.bounds, params.corners);
    coverage *= rounded_coverage(input.position.xy, params.rounded_clip, params.rounded_corners);
    return color * (params.opacity * coverage);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, ContentMask, PaintSurfaceSource, Scene, ShaderDescriptor, point, size};
    use std::sync::Arc;

    fn new_renderer() -> super::super::MetalRenderer {
        assert!(
            super::super::metal_is_available(),
            "Metal device required for custom GPU pixel regressions"
        );

        super::super::MetalRenderer::try_new(Arc::new(parking_lot::Mutex::new(
            super::super::InstanceBufferPool::default(),
        )))
        .unwrap()
    }

    fn shader(source: &str) -> ShaderHandle {
        ShaderHandle::compile_fragment(ShaderDescriptor::fragment(
            "pixel_regression",
            source,
            "fs_main",
        ))
        .unwrap()
    }

    #[test]
    fn native_compute_runtime_buffers_uploads_and_gpu_display() {
        let mut renderer = new_renderer();
        let input = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let input_bytes = [32, 64, 128, 128].repeat(16);
        renderer.write_render_target(&input, &input_bytes).unwrap();
        assert_eq!(
            renderer.read_render_target(&input).unwrap().pixels,
            input_bytes
        );
        let output = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        let buffer = renderer
            .create_gpu_buffer(crate::GpuBufferDescriptor { byte_len: 64 })
            .unwrap();
        assert_eq!(renderer.read_gpu_buffer(&buffer).unwrap(), [0; 64]);
        let uniform: Arc<[u8]> = bytemuck::cast_slice(&[1.0f32, 1.0, 1.0, 1.0]).into();
        let compute = crate::ComputeHandle::compile(crate::ComputeDescriptor::new("compute pixels", r#"
            struct Params { multiplier: vec4<f32> }
            @group(0) @binding(3) var<uniform> params: Params;
            @group(0) @binding(7) var source: texture_2d<f32>;
            @group(0) @binding(11) var<storage, read_write> values: array<u32>;
            @group(0) @binding(19) var destination: texture_storage_2d<rgba8unorm, write>;
            @compute @workgroup_size(2,2) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                if (id.x >= 4u || id.y >= 4u) { return; }
                let index = id.y * 4u + id.x;
                if (index < arrayLength(&values)) { values[index] = index + 100u; }
                textureStore(destination, vec2<i32>(id.xy), textureLoad(source, vec2<i32>(id.xy), 0) * params.multiplier);
            }
        "#, "main")).unwrap();
        let bindings = crate::ComputeBindings::new()
            .with(3, crate::ComputeBinding::Uniform(uniform))
            .with(7, crate::ComputeBinding::Texture(input.clone()))
            .with(11, crate::ComputeBinding::StorageBuffer(buffer.clone()))
            .with(19, crate::ComputeBinding::StorageTexture(output.clone()));
        renderer
            .dispatch_compute(&compute, &bindings, [2, 2, 1])
            .unwrap();
        assert_eq!(
            renderer.read_render_target(&output).unwrap().pixels,
            input_bytes
        );
        let values = renderer.read_gpu_buffer(&buffer).unwrap();
        let expected: Vec<u8> = (100u32..116).flat_map(u32::to_le_bytes).collect();
        assert_eq!(values, expected);
        renderer
            .write_gpu_buffer(&buffer, 4, &999u32.to_le_bytes())
            .unwrap();
        assert_eq!(
            &renderer.read_gpu_buffer(&buffer).unwrap()[4..8],
            &999u32.to_le_bytes()
        );
        assert!(renderer.write_gpu_buffer(&buffer, 1, &[0; 4]).is_err());
        let sync = crate::ComputeHandle::compile(crate::ComputeDescriptor::new(
            "synchronized loops",
            crate::compute::SYNC_BARRIER_REGRESSION,
            "main",
        ))
        .unwrap();
        assert_eq!(sync.loop_body_limit(), None);
        renderer
            .dispatch_compute(
                &sync,
                &crate::ComputeBindings::new()
                    .with(31, crate::ComputeBinding::StorageBuffer(buffer.clone())),
                [1, 1, 1],
            )
            .unwrap();
        assert_eq!(
            &renderer.read_gpu_buffer(&buffer).unwrap()[..8],
            &[65555u32, 19]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let large = renderer
            .create_gpu_buffer(crate::GpuBufferDescriptor { byte_len: 131072 })
            .unwrap();
        renderer.read_gpu_buffer(&large).unwrap();
        renderer.custom.reap();
        assert!(
            renderer
                .custom
                .uniform_pool
                .iter()
                .all(|b| b.length() <= 65536)
        );

        assert!(
            renderer
                .dispatch_compute(&compute, &bindings, [0, 1, 1])
                .is_err()
        );
        let feedback = bindings
            .clone()
            .with(19, crate::ComputeBinding::StorageTexture(input));
        assert!(matches!(
            renderer.dispatch_compute(&compute, &feedback, [2, 2, 1]),
            Err(RenderTargetError::FeedbackLoop)
        ));
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: Bounds {
                origin: point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
                size: size(crate::ScaledPixels(4.0), crate::ScaledPixels(4.0)),
            },
            content_mask: ContentMask {
                bounds: Bounds::new(
                    point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
                    size(crate::ScaledPixels(4.0), crate::ScaledPixels(4.0)),
                ),
            },
            source: PaintSurfaceSource::RenderTarget {
                target: output.clone(),
                revision: output.revision(),
                paint: Default::default(),
            },
        });
        scene.finish();
        let image = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(4), DevicePixels(4)))
            .unwrap();
        assert_eq!(&image.bgra[..4], &[128, 64, 32, 128]);
        for format in [
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::Rgba16Float,
            RenderTargetFormat::R8Unorm,
        ] {
            let descriptor = RenderTargetDescriptor {
                width: 2,
                height: 2,
                format,
            };
            let target = renderer.create_render_target(descriptor).unwrap();
            let pixels = match format {
                RenderTargetFormat::Rgba16Float => {
                    [0x00, 0x38, 0x00, 0x34, 0x00, 0x3c, 0x00, 0x3c].repeat(4)
                }
                RenderTargetFormat::R8Unorm => vec![17, 23, 31, 47],
                _ => [17, 23, 31, 255].repeat(4),
            };
            renderer.write_render_target(&target, &pixels).unwrap();
            assert_eq!(renderer.read_render_target(&target).unwrap().pixels, pixels);
            if format == RenderTargetFormat::Rgba16Float {
                let kernel = crate::ComputeHandle::compile(crate::ComputeDescriptor::new(
                    "HDR storage",
                    crate::compute::HDR_STORAGE_REGRESSION,
                    "main",
                ))
                .unwrap();
                renderer
                    .dispatch_compute(
                        &kernel,
                        &crate::ComputeBindings::new()
                            .with(5, crate::ComputeBinding::StorageTexture(target.clone())),
                        [1, 1, 1],
                    )
                    .unwrap();
                assert_eq!(
                    renderer.read_render_target(&target).unwrap().pixels,
                    [0, 64, 0, 56, 0, 52, 0, 60].repeat(4)
                );
            }
        }
    }
    #[test]
    fn authored_infinite_nested_loops_have_one_invocation_budget() {
        let mut renderer = new_renderer();
        let target = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(1, 1))
            .unwrap();
        let shader = shader(
            "fn work() -> u32 { var count=0u; loop { loop { count+=1u; } } return count; } @fragment fn fs_main() -> @location(0) vec4<f32> { let n=work(); return vec4<f32>(f32(n)/65536.0,0.0,0.0,1.0); }",
        );
        renderer
            .render_shader(&target, &shader, &ShaderBindings::new())
            .unwrap();
        assert_eq!(
            renderer.read_render_target(&target).unwrap().pixels,
            [255, 0, 0, 255]
        );
    }

    const COLOR_SHADER: &str = "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.25, 0.5, 1.0, 0.5); }";

    #[test]
    fn custom_fragment_uses_premultiplied_storage_and_reuses_pipeline() {
        let mut renderer = new_renderer();
        let target = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        assert!(
            renderer
                .read_render_target(&target)
                .unwrap()
                .pixels
                .iter()
                .all(|&byte| byte == 0)
        );
        let program = shader(COLOR_SHADER);
        for _ in 0..2 {
            renderer
                .render_shader(&target, &program, &ShaderBindings::new())
                .unwrap();
        }
        let readback = renderer.read_render_target(&target).unwrap();
        for pixel in readback.pixels.chunks_exact(4) {
            for (&actual, expected) in pixel.iter().zip([32u8, 64, 128, 128]) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "RGBA target pixel {pixel:?}"
                );
            }
        }
        assert_eq!(renderer.custom.compilation_count, 1);
        assert_eq!(target.revision(), 2);
        let other = new_renderer();
        assert!(matches!(
            other.validate_render_target(&target),
            Err(RenderTargetError::WrongDevice)
        ));
        renderer.shed_memory(crate::MemoryPressureLevel::Critical);
        assert!(target.is_valid());
        assert_eq!(
            renderer.read_render_target(&target).unwrap().pixels,
            readback.pixels
        );
        drop(renderer);
        assert!(!target.is_valid());
    }

    #[test]
    fn sparse_uniform_texture_and_sampler_bindings_render_and_reject_feedback() {
        let mut renderer = new_renderer();
        let input = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        let output = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        let uniform = shader(
            "struct Params { color: vec4<f32> }; @group(0) @binding(7) var<uniform> params: Params; @fragment fn fs_main() -> @location(0) vec4<f32> { return params.color; }",
        );
        let bytes: Vec<u8> = [0.25f32, 0.5, 1.0, 0.5]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        renderer
            .render_shader(
                &input,
                &uniform,
                &ShaderBindings::new().with(7, ShaderBinding::Uniform(bytes.into())),
            )
            .unwrap();
        let sampling = shader(
            "@group(0) @binding(9) var image: texture_2d<f32>; @group(0) @binding(12) var sampling: sampler; @fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { let color = textureSample(image, sampling, uv); return vec4<f32>(color.rgb / max(color.a, 0.001), color.a); }",
        );
        let bindings = ShaderBindings::new()
            .with(9, ShaderBinding::Texture(input.clone()))
            .with(12, ShaderBinding::Sampler(ShaderSampler::LinearClamp));
        renderer
            .render_shader(&output, &sampling, &bindings)
            .unwrap();
        assert_eq!(
            renderer.read_render_target(&input).unwrap().pixels,
            renderer.read_render_target(&output).unwrap().pixels
        );
        assert!(matches!(
            renderer.render_shader(&input, &sampling, &bindings),
            Err(RenderTargetError::FeedbackLoop)
        ));
        assert!(matches!(
            renderer.render_shader(
                &output,
                &uniform,
                &ShaderBindings::new().with(7, ShaderBinding::Uniform(Arc::from([0u8; 4])))
            ),
            Err(RenderTargetError::InvalidBindings(_))
        ));
    }

    #[test]
    fn custom_target_composes_directly_with_ui_and_preserves_revision_damage() {
        let mut renderer = new_renderer();
        let target = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(4, 4))
            .unwrap();
        renderer
            .render_shader(&target, &shader(COLOR_SHADER), &ShaderBindings::new())
            .unwrap();
        let bounds = Bounds::new(
            point(crate::ScaledPixels(2.0), crate::ScaledPixels(2.0)),
            size(crate::ScaledPixels(4.0), crate::ScaledPixels(4.0)),
        );
        let mut scene = Scene::default();
        let underlay =
            super::super::offscreen_tests::full_viewport_quad(8.0, crate::hsla(0.0, 1.0, 0.5, 1.0));
        scene.insert_primitive(underlay.clone());
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            source: PaintSurfaceSource::RenderTarget {
                target: target.clone(),
                revision: target.revision(),
                paint: Default::default(),
            },
        });
        scene.finish();
        assert!(!scene.has_live_surfaces());
        let checksum = scene.structural_checksum();
        let frame = renderer
            .render_scene_to_bytes(&scene, size(DevicePixels(8), DevicePixels(8)))
            .unwrap();
        let center = &frame.bgra[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4];
        for (&actual, expected) in center.iter().zip([128u8, 64, 159, 255]) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "UI composition pixel {center:?}"
            );
        }
        assert_eq!(&frame.bgra[..4], &[0, 0, 255, 255]);
        renderer.render_shader(&target, &shader("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }"), &ShaderBindings::new()).unwrap();
        assert_eq!(
            scene.structural_checksum(),
            checksum,
            "a captured scene revision must stay immutable"
        );
        let mut next = Scene::default();
        next.insert_primitive(underlay);
        next.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            source: PaintSurfaceSource::RenderTarget {
                target: target.clone(),
                revision: target.revision(),
                paint: Default::default(),
            },
        });
        next.finish();
        assert_ne!(next.structural_checksum(), checksum);
        assert!(matches!(
            next.damage_since(&scene),
            crate::FrameDamage::Region(_)
        ));
    }

    #[test]
    fn custom_targets_support_hdr_srgb_bgra_and_scalar_formats() {
        let mut renderer = new_renderer();
        let program = shader(
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.25, 0.5, 1.0, 1.0); }",
        );
        for format in [
            RenderTargetFormat::Rgba8UnormSrgb,
            RenderTargetFormat::Bgra8UnormSrgb,
            RenderTargetFormat::R8Unorm,
        ] {
            let target = renderer
                .create_render_target(RenderTargetDescriptor {
                    width: 4,
                    height: 4,
                    format,
                })
                .unwrap();
            renderer
                .render_shader(&target, &program, &ShaderBindings::new())
                .unwrap();
            let result = renderer.read_render_target(&target).unwrap();
            if format == RenderTargetFormat::R8Unorm {
                assert_eq!(result.pixels.len(), 16);
                assert!(result.pixels.iter().all(|&red| red.abs_diff(64) <= 1));
            } else {
                for pixel in result.pixels.chunks_exact(4) {
                    for (&actual, expected) in pixel.iter().zip([137u8, 188, 255, 255]) {
                        assert!(actual.abs_diff(expected) <= 2, "sRGB readback {pixel:?}");
                    }
                }
            }
        }
        let hdr = renderer
            .create_render_target(RenderTargetDescriptor {
                width: 4,
                height: 4,
                format: RenderTargetFormat::Rgba16Float,
            })
            .unwrap();
        renderer.render_shader(&hdr, &shader("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(2.0, 0.5, 0.25, 1.0); }"), &ShaderBindings::new()).unwrap();
        let result = renderer.read_render_target(&hdr).unwrap();
        for pixel in result.pixels.chunks_exact(8) {
            let decoded: Vec<_> = pixel
                .chunks_exact(2)
                .map(|bytes| super::super::f16_to_f32(u16::from_le_bytes([bytes[0], bytes[1]])))
                .collect();
            assert_eq!(decoded, [2.0, 0.5, 0.25, 1.0]);
        }
    }

    #[test]
    fn custom_target_display_respects_opacity_rounded_clips_transform_and_color_filter() {
        let mut renderer = new_renderer();
        let target = renderer
            .create_render_target(RenderTargetDescriptor::rgba8(8, 8))
            .unwrap();
        renderer.render_shader(&target, &shader("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(0.0, 1.0, 0.0, 1.0); }"), &ShaderBindings::new()).unwrap();
        let bounds = Bounds::new(
            point(crate::ScaledPixels(0.0), crate::ScaledPixels(0.0)),
            size(crate::ScaledPixels(8.0), crate::ScaledPixels(8.0)),
        );
        let frame = |renderer: &mut super::super::MetalRenderer, paint| {
            let mut scene = Scene::default();
            scene.insert_primitive(PaintSurface {
                order: 0,
                bounds,
                content_mask: ContentMask {
                    bounds: Bounds::new(
                        bounds.origin,
                        size(crate::ScaledPixels(12.0), crate::ScaledPixels(12.0)),
                    ),
                },
                source: PaintSurfaceSource::RenderTarget {
                    target: target.clone(),
                    revision: target.revision(),
                    paint,
                },
            });
            scene.finish();
            renderer
                .render_scene_to_bytes(&scene, size(DevicePixels(12), DevicePixels(12)))
                .unwrap()
        };
        let pixel = |frame: &super::super::OffscreenReadback, x: usize, y: usize| -> [u8; 4] {
            frame.bgra[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4]
                .try_into()
                .unwrap()
        };
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.opacity = 0.5;
        let opacity = frame(&mut renderer, paint);
        let center = pixel(&opacity, 4, 4);
        assert!(
            center[1].abs_diff(128) <= 1 && center[3].abs_diff(128) <= 1,
            "opacity {center:?}"
        );
        let radii = crate::Corners {
            top_left: crate::ScaledPixels(4.0),
            top_right: crate::ScaledPixels(4.0),
            bottom_left: crate::ScaledPixels(4.0),
            bottom_right: crate::ScaledPixels(4.0),
        };
        for ancestor in [false, true] {
            let mut paint = crate::render_target::RenderTargetPaint::default();
            if ancestor {
                paint.rounded_clip_bounds = bounds;
                paint.rounded_clip_radii = radii;
            } else {
                paint.corner_radii = radii;
            }
            let rounded = frame(&mut renderer, paint);
            assert_eq!(pixel(&rounded, 0, 0), [0; 4]);
            assert_eq!(pixel(&rounded, 4, 4), [0, 255, 0, 255]);
        }
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.transform.translation = [2.0, 3.0];
        let translated = frame(&mut renderer, paint);
        assert_eq!(pixel(&translated, 0, 0), [0; 4]);
        assert_eq!(pixel(&translated, 6, 7), [0, 255, 0, 255]);
        let mut paint = crate::render_target::RenderTargetPaint::default();
        paint.color_filter.grayscale = 1.0;
        let filtered = frame(&mut renderer, paint);
        let center = pixel(&filtered, 4, 4);
        assert!(
            center[..3]
                .iter()
                .all(|&channel| channel.abs_diff(182) <= 1),
            "grayscale {center:?}"
        );
        assert_eq!(center[3], 255);
    }
}
