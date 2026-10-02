//! Native WGSL compute programs and device-owned buffers.
//!
//! Compute writes associated linear RGBA directly; unlike fragment rendering,
//! no blend stage converts straight colors. WebGL2 returns typed unsupported
//! errors for dispatch and buffer operations.
#[cfg(not(target_arch = "wasm32"))]
use crate::render_target::{DeviceBudget, TargetRegistry};
use crate::{
    RenderTarget, RenderTargetError, RenderTargetFormat, ShaderBackend, ShaderError, ShaderSampler,
    ShaderUniformLayout, ShaderUniformType,
};
use naga_shader as naga;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[cfg(not(target_arch = "wasm32"))]
use std::{collections::BTreeSet, sync::Weak};

const MAX_GPU_BUFFER_BYTES: u64 = 128 * 1024 * 1024;

/// A native compute entry point authored in WGSL.
///
/// Kernels without synchronized loops bound loop-body executions to
/// [`crate::SHADER_MAX_LOOP_BODY_EXECUTIONS`] per invocation. Synchronization
/// kernels retain authored loop/barrier semantics so individual invocations
/// cannot leave a barrier loop early. Inspect [`ComputeHandle::loop_body_limit`]
/// or require bounded loops with [`Self::require_loop_bound`].
#[derive(Clone, Debug)]
pub struct ComputeDescriptor {
    /// Diagnostic name, at most 256 bytes.
    pub label: String,
    /// WGSL source, at most 256 KiB.
    pub source: String,
    /// The sole compute entry point.
    pub entry_point: String,
    /// Reject synchronized-loop kernels that cannot safely use a per-invocation
    /// loop counter. False preserves authored synchronization semantics.
    pub require_loop_bound: bool,
}
impl ComputeDescriptor {
    /// Describe a compute program for validation during registration.
    pub fn new(
        label: impl Into<String>,
        source: impl Into<String>,
        entry_point: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            source: source.into(),
            entry_point: entry_point.into(),
            require_loop_bound: false,
        }
    }
    /// Require the shared per-invocation loop bound. Synchronized loops return
    /// a typed validation error instead of changing workgroup participation.
    pub fn require_loop_bound(mut self) -> Self {
        self.require_loop_bound = true;
        self
    }
}
/// Exact minimum storage layout and optional trailing runtime-array stride.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeBufferLayout {
    /// Fixed prefix bytes; includes one element for a runtime-sized array.
    pub min_size: u32,
    /// Runtime array starts at this byte offset.
    pub runtime_array_offset: Option<u32>,
    /// Bytes between runtime array elements.
    pub runtime_array_stride: Option<u32>,
}
/// Supported native compute bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputeResourceKind {
    /// Read-only portable uniform bytes.
    Uniform(ShaderUniformLayout),
    /// Sampled floating-point 2D texture.
    Texture2d,
    /// Non-comparison sampler.
    Sampler,
    /// Storage buffer, read-only or read/write.
    StorageBuffer {
        /// Checked host-shareable layout.
        layout: ComputeBufferLayout,
        /// Whether stores are allowed.
        writable: bool,
    },
    /// Write-only storage image; linear RGBA8 or RGBA16F.
    StorageTexture(RenderTargetFormat),
}
/// One authored group-zero binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputeResourceBinding {
    /// Authored WGSL binding number.
    pub binding: u32,
    /// Authored global name.
    pub name: String,
    /// Checked resource kind.
    pub kind: ComputeResourceKind,
}
/// Native resource namespace used by a translated compute pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComputeSlotKind {
    /// Constant/read-only uniform transport.
    Uniform,
    /// Sampled texture.
    Texture,
    /// Sampler.
    Sampler,
    /// Read-only storage buffer.
    ReadBuffer,
    /// Read/write storage buffer.
    WriteBuffer,
    /// Write-only storage texture.
    WriteTexture,
}
/// Authored-to-native binding mapping.
#[derive(Clone, Debug)]
pub struct ComputeSlot {
    /// Authored binding.
    pub binding: u32,
    /// Dense native namespace index.
    pub slot: u32,
    /// Resource kind.
    pub kind: ComputeSlotKind,
    /// Emitted variable name.
    pub name: String,
}
/// Already validated native source and runtime-array metadata.
#[derive(Clone, Debug)]
pub struct ComputeTranslation {
    /// Native source.
    pub source: String,
    /// Native entry point.
    pub entry_point: String,
    /// Dense resource slots.
    pub resources: Vec<ComputeSlot>,
    /// Metal sizes-buffer entries in declaration order.
    pub metal_size_bindings: Vec<u32>,
    /// Dynamic threadgroup memory bytes, one entry per used workgroup global
    /// in declaration order. Metal allocations are rounded to 16 bytes.
    pub metal_workgroup_sizes: Vec<u32>,
}
#[derive(Debug)]
struct ComputeProgram {
    id: u64,
    label: String,
    source: String,
    entry: String,
    workgroup: [u32; 3],
    loop_bound: Option<u32>,
    resources: Vec<ComputeResourceBinding>,
    translations: [ComputeTranslation; 3],
    live: Arc<()>,
    bytes: usize,
}
/// Reusable validated native compute program; device pipelines are cached separately.
#[derive(Clone, Debug)]
pub struct ComputeHandle(Arc<ComputeProgram>);
impl ComputeHandle {
    /// Stable identity for device caches.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// Diagnostic label.
    pub fn label(&self) -> &str {
        &self.0.label
    }
    /// Authored WGSL source.
    pub fn source(&self) -> &str {
        &self.0.source
    }
    /// Authored compute entry name.
    pub fn entry_point(&self) -> &str {
        &self.0.entry
    }
    /// Fixed thread count per workgroup.
    pub fn workgroup_size(&self) -> [u32; 3] {
        self.0.workgroup
    }
    /// Applied total loop-body limit. None means authored synchronization and
    /// loop semantics are preserved; those kernels have no execution bound.
    pub fn loop_body_limit(&self) -> Option<u32> {
        self.0.loop_bound
    }
    /// Reflected bindings, sorted by authored binding.
    pub fn resources(&self) -> &[ComputeResourceBinding] {
        &self.0.resources
    }
    /// Translate to a supported native backend. WebGL2 has no compute stage.
    pub fn translate(&self, backend: ShaderBackend) -> Result<ComputeTranslation, ShaderError> {
        let index = match backend {
            ShaderBackend::Metal => 0,
            ShaderBackend::DirectX11 => 1,
            ShaderBackend::Blade => 2,
            ShaderBackend::WebGl2 => {
                return Err(ShaderError::Unsupported(
                    "WebGL2 has no compute stage".into(),
                ));
            }
        };
        Ok(self.0.translations[index].clone())
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.bytes
    }
    pub(crate) fn live(&self) -> &Arc<()> {
        &self.0.live
    }
    pub(crate) fn compile(descriptor: ComputeDescriptor) -> Result<Self, ShaderError> {
        crate::shader::register_compute(descriptor)
    }
}
impl crate::App {
    /// Validate a native WGSL compute program without allocating GPU resources.
    pub fn register_compute_shader(
        &mut self,
        descriptor: ComputeDescriptor,
    ) -> Result<ComputeHandle, ShaderError> {
        ComputeHandle::compile(descriptor)
    }
}

pub(crate) fn compile(descriptor: ComputeDescriptor) -> Result<ComputeHandle, ShaderError> {
    let mut module = naga::front::wgsl::parse_str(&descriptor.source)
        .map_err(|e| ShaderError::Validation(e.emit_to_string(&descriptor.source)))?;
    if module.entry_points.len() != 1
        || module.entry_points[0].stage != naga::ShaderStage::Compute
        || module.entry_points[0].name != descriptor.entry_point
        || !module.overrides.is_empty()
    {
        return Err(ShaderError::Unsupported(
            "exactly one compute entry without overrides is required".into(),
        ));
    }
    if module
        .global_variables
        .iter()
        .any(|(_, g)| g.name.as_deref().is_some_and(|n| n.starts_with("kael_")))
        || module
            .functions
            .iter()
            .any(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with("kael_")))
    {
        return Err(ShaderError::Unsupported(
            "kael_ identifiers are reserved".into(),
        ));
    }
    let workgroup = module.entry_points[0].workgroup_size;
    if workgroup.contains(&0)
        || workgroup[0] > 128
        || workgroup[1] > 128
        || workgroup[2] > 64
        || workgroup
            .iter()
            .try_fold(1u32, |p, &n| p.checked_mul(n))
            .is_none_or(|n| n > 128)
    {
        return Err(ShaderError::ResourceLimit(
            "128 threads per workgroup; axes x/y128,z64",
        ));
    }
    fn control(block: &naga::Block) -> (bool, bool) {
        let mut result = (false, false);
        for statement in block.iter() {
            let parts = match statement {
                naga::Statement::Barrier(_) => (false, true),
                naga::Statement::Loop {
                    body, continuing, ..
                } => {
                    let a = control(body);
                    let b = control(continuing);
                    (true, a.1 || b.1)
                }
                naga::Statement::Block(b) => control(b),
                naga::Statement::If { accept, reject, .. } => {
                    let a = control(accept);
                    let b = control(reject);
                    (a.0 || b.0, a.1 || b.1)
                }
                naga::Statement::Switch { cases, .. } => cases
                    .iter()
                    .map(|c| control(&c.body))
                    .fold((false, false), |a, b| (a.0 || b.0, a.1 || b.1)),
                _ => (false, false),
            };
            result = (result.0 || parts.0, result.1 || parts.1);
        }
        result
    }
    let controls = module
        .functions
        .iter()
        .map(|(_, f)| control(&f.body))
        .chain(
            module
                .entry_points
                .iter()
                .map(|e| control(&e.function.body)),
        )
        .fold((false, false), |a, b| (a.0 || b.0, a.1 || b.1));
    let loop_bound = if controls.0 && controls.1 {
        if descriptor.require_loop_bound {
            return Err(ShaderError::Unsupported("synchronized-loop kernels preserve authored barrier participation and cannot use a per-invocation loop bound".into()));
        }
        None
    } else {
        crate::shader::bound_loops(&mut module);
        Some(crate::SHADER_MAX_LOOP_BODY_EXECUTIONS)
    };
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| ShaderError::Validation(e.emit_to_string(&descriptor.source)))?;
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .map_err(|e| ShaderError::Validation(e.to_string()))?;
    let mut resources = Vec::new();
    let mut counts = [0; 5];
    let mut workgroup_bytes = 0u64;
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = &global.binding else {
            match global.space {
                naga::AddressSpace::Private => continue,
                naga::AddressSpace::WorkGroup => {
                    workgroup_bytes += u64::from(layouter[global.ty].size.next_multiple_of(16));
                    continue;
                }
                _ => {
                    return Err(ShaderError::Unsupported(
                        "only private/workgroup globals may be unbound".into(),
                    ));
                }
            }
        };
        if binding.group != 0 {
            return Err(ShaderError::Unsupported(
                "compute resources must use group zero".into(),
            ));
        }
        let kind = match (global.space, &module.types[global.ty].inner) {
            (naga::AddressSpace::Uniform, _) => {
                counts[0] += 1;
                crate::shader::check_std140(&module, &layouter, global.ty)?;
                let layout = layouter[global.ty];
                if layout.size > 65536 {
                    return Err(ShaderError::ResourceLimit("64 KiB uniform"));
                }
                let reflected = crate::shader::reflect_uniform(&module, &layouter, global.ty, 0)?;
                ComputeResourceKind::Uniform(ShaderUniformLayout {
                    size: layout.size,
                    alignment: layout.alignment * 1,
                    members: match reflected {
                        ShaderUniformType::Struct(m) => m,
                        _ => Vec::new(),
                    },
                })
            }
            (naga::AddressSpace::Storage { access }, _) => {
                counts[3] += 1;
                if u64::from(layouter[global.ty].size) > MAX_GPU_BUFFER_BYTES {
                    return Err(ShaderError::ResourceLimit("128 MiB storage binding"));
                }
                let (offset, stride) = runtime_array(&module, global.ty)?;
                ComputeResourceKind::StorageBuffer {
                    layout: ComputeBufferLayout {
                        min_size: layouter[global.ty].size,
                        runtime_array_offset: offset,
                        runtime_array_stride: stride,
                    },
                    writable: access.contains(naga::StorageAccess::STORE),
                }
            }
            (
                naga::AddressSpace::Handle,
                naga::TypeInner::Image {
                    dim: naga::ImageDimension::D2,
                    arrayed: false,
                    class:
                        naga::ImageClass::Sampled {
                            kind: naga::ScalarKind::Float,
                            multi: false,
                        },
                },
            ) => {
                counts[1] += 1;
                ComputeResourceKind::Texture2d
            }
            (naga::AddressSpace::Handle, naga::TypeInner::Sampler { comparison: false }) => {
                counts[2] += 1;
                ComputeResourceKind::Sampler
            }
            (
                naga::AddressSpace::Handle,
                naga::TypeInner::Image {
                    dim: naga::ImageDimension::D2,
                    arrayed: false,
                    class: naga::ImageClass::Storage { format, access },
                },
            ) if *access == naga::StorageAccess::STORE => {
                counts[4] += 1;
                ComputeResourceKind::StorageTexture(match format {
                    naga::StorageFormat::Rgba8Unorm => RenderTargetFormat::Rgba8Unorm,
                    naga::StorageFormat::Rgba16Float => RenderTargetFormat::Rgba16Float,
                    _ => {
                        return Err(ShaderError::Unsupported(
                            "storage images require rgba8unorm or rgba16float".into(),
                        ));
                    }
                })
            }
            _ => {
                return Err(ShaderError::Unsupported(
                    "unsupported compute resource type".into(),
                ));
            }
        };
        resources.push(ComputeResourceBinding {
            binding: binding.binding,
            name: global
                .name
                .clone()
                .unwrap_or_else(|| format!("binding_{}", binding.binding)),
            kind,
        });
    }
    if counts[0] > 4
        || counts[1] > 8
        || counts[2] > 8
        || counts[3] > 8
        || counts[4] > 8
        || counts[3] + counts[4] > 8
        || workgroup_bytes > 16384
    {
        return Err(ShaderError::ResourceLimit(
            "compute binding/workgroup memory limits",
        ));
    }
    resources.sort_by_key(|r| r.binding);
    let translations = [
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            ShaderBackend::Metal,
        )?,
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            ShaderBackend::DirectX11,
        )?,
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            ShaderBackend::Blade,
        )?,
    ];
    let bytes = descriptor.source.len() * 5
        + descriptor.label.len()
        + translations
            .iter()
            .map(|t| {
                t.source.len()
                    + t.resources
                        .iter()
                        .map(|r| r.name.len() + std::mem::size_of::<ComputeSlot>())
                        .sum::<usize>()
            })
            .sum::<usize>();
    Ok(ComputeHandle(Arc::new(ComputeProgram {
        id: crate::shader::next_program_id()?,
        label: descriptor.label,
        source: descriptor.source,
        entry: descriptor.entry_point,
        workgroup,
        loop_bound,
        resources,
        translations,
        live: Arc::new(()),
        bytes,
    })))
}
fn runtime_array(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
) -> Result<(Option<u32>, Option<u32>), ShaderError> {
    match &module.types[ty].inner {
        naga::TypeInner::Array {
            size: naga::ArraySize::Dynamic,
            stride,
            ..
        } => Ok((Some(0), Some(*stride))),
        naga::TypeInner::Struct { members, .. } => {
            let mut dynamic = (None, None);
            for (index, member) in members.iter().enumerate() {
                let (offset, stride) = runtime_array(module, member.ty)?;
                if let Some(offset) = offset {
                    if index + 1 != members.len() {
                        return Err(ShaderError::Unsupported(
                            "runtime array must be final storage member".into(),
                        ));
                    }
                    dynamic = (Some(member.offset + offset), stride);
                }
            }
            Ok(dynamic)
        }
        naga::TypeInner::Scalar(_)
        | naga::TypeInner::Vector { .. }
        | naga::TypeInner::Matrix { .. }
        | naga::TypeInner::Atomic(_) => Ok((None, None)),
        naga::TypeInner::Array {
            base,
            size: naga::ArraySize::Constant(_),
            ..
        } => {
            if runtime_array(module, *base)?.0.is_some() {
                return Err(ShaderError::Unsupported("nested dynamic arrays".into()));
            }
            Ok((None, None))
        }
        _ => Err(ShaderError::Unsupported(
            "storage buffers must be host-shareable".into(),
        )),
    }
}
fn translate(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    resources: &[ComputeResourceBinding],
    entry: &str,
    backend: ShaderBackend,
) -> Result<ComputeTranslation, ShaderError> {
    let failure = |e: String| ShaderError::Translation {
        backend,
        message: e,
    };
    let mut next = [0u32; 5];
    let mut slots = Vec::new();
    for resource in resources {
        let (kind, index) = match resource.kind {
            ComputeResourceKind::Uniform(_) => (ComputeSlotKind::Uniform, 0),
            ComputeResourceKind::Texture2d => (ComputeSlotKind::Texture, 1),
            ComputeResourceKind::Sampler => (ComputeSlotKind::Sampler, 2),
            ComputeResourceKind::StorageBuffer {
                writable: false, ..
            } => (
                ComputeSlotKind::ReadBuffer,
                if backend == ShaderBackend::Metal {
                    0
                } else {
                    1
                },
            ),
            ComputeResourceKind::StorageBuffer { writable: true, .. } => (
                ComputeSlotKind::WriteBuffer,
                if backend == ShaderBackend::Metal {
                    0
                } else {
                    3
                },
            ),
            ComputeResourceKind::StorageTexture(_) => (
                ComputeSlotKind::WriteTexture,
                if backend == ShaderBackend::Metal {
                    1
                } else {
                    3
                },
            ),
        };
        let slot = next[index];
        next[index] += 1;
        slots.push(ComputeSlot {
            binding: resource.binding,
            slot,
            kind,
            name: resource.name.clone(),
        });
    }
    let mut result = ComputeTranslation {
        source: String::new(),
        entry_point: entry.into(),
        resources: slots,
        metal_size_bindings: Vec::new(),
        metal_workgroup_sizes: Vec::new(),
    };
    match backend {
        ShaderBackend::Metal => {
            let entry_info = info.get_entry_point(0);
            let mut layouter = naga::proc::Layouter::default();
            layouter
                .update(module.to_ctx())
                .map_err(|e| failure(e.to_string()))?;
            for (handle, global) in module.global_variables.iter() {
                if global.space == naga::AddressSpace::WorkGroup && !entry_info[handle].is_empty() {
                    result
                        .metal_workgroup_sizes
                        .push(layouter[global.ty].size.next_multiple_of(16));
                }
            }
            let mut map = naga::back::msl::BindingMap::new();
            for slot in &result.resources {
                let mut target = naga::back::msl::BindTarget::default();
                match slot.kind {
                    ComputeSlotKind::Uniform
                    | ComputeSlotKind::ReadBuffer
                    | ComputeSlotKind::WriteBuffer => target.buffer = Some(slot.slot as u8),
                    ComputeSlotKind::Texture | ComputeSlotKind::WriteTexture => {
                        target.texture = Some(slot.slot as u8)
                    }
                    ComputeSlotKind::Sampler => {
                        target.sampler = Some(naga::back::msl::BindSamplerTarget::Resource(
                            slot.slot as u8,
                        ))
                    }
                };
                map.insert(
                    naga::ResourceBinding {
                        group: 0,
                        binding: slot.binding,
                    },
                    target,
                );
            }
            for (_, global) in module.global_variables.iter() {
                if runtime_array(module, global.ty)
                    .ok()
                    .is_some_and(|(offset, _)| offset.is_some())
                {
                    result
                        .metal_size_bindings
                        .push(global.binding.as_ref().unwrap().binding);
                }
            }
            let mut options = naga::back::msl::Options {
                lang_version: (2, 0),
                fake_missing_bindings: false,
                ..Default::default()
            };
            options.per_entry_point_map.insert(
                entry.into(),
                naga::back::msl::EntryPointResources {
                    resources: map,
                    sizes_buffer: Some(30),
                    ..Default::default()
                },
            );
            let (source, reflection) =
                naga::back::msl::write_string(module, info, &options, &Default::default())
                    .map_err(|e| failure(e.to_string()))?;
            result.source = source;
            result.entry_point = reflection.entry_point_names[0]
                .as_ref()
                .map_err(|e| failure(e.to_string()))?
                .clone();
        }
        ShaderBackend::DirectX11 => {
            let mut options = naga::back::hlsl::Options {
                shader_model: naga::back::hlsl::ShaderModel::V5_0,
                fake_missing_bindings: false,
                ..Default::default()
            };
            for slot in &result.resources {
                options.binding_map.insert(
                    naga::ResourceBinding {
                        group: 0,
                        binding: slot.binding,
                    },
                    naga::back::hlsl::BindTarget {
                        space: 0,
                        register: slot.slot,
                        binding_array_size: None,
                    },
                );
            }
            let reflection = naga::back::hlsl::Writer::new(&mut result.source, &options)
                .write(module, info, None)
                .map_err(|e| failure(e.to_string()))?;
            result.entry_point = reflection.entry_point_names[0]
                .as_ref()
                .map_err(|e| failure(e.to_string()))?
                .clone();
        }
        ShaderBackend::Blade => {
            let mut module = module.clone();
            let mut counts = [0u8; 6];
            for slot in &mut result.resources {
                let (prefix, index) = match slot.kind {
                    ComputeSlotKind::Uniform => ("uniform", 0),
                    ComputeSlotKind::Texture => ("texture", 1),
                    ComputeSlotKind::Sampler => ("sampler", 2),
                    ComputeSlotKind::ReadBuffer => ("read_buffer", 3),
                    ComputeSlotKind::WriteBuffer => ("write_buffer", 4),
                    ComputeSlotKind::WriteTexture => ("write_texture", 5),
                };
                slot.name = format!("kael_{prefix}_{}", char::from(b'a' + counts[index]));
                counts[index] += 1;
                for (_, global) in module.global_variables.iter_mut() {
                    if global
                        .binding
                        .as_ref()
                        .is_some_and(|b| b.binding == slot.binding)
                    {
                        global.name = Some(slot.name.clone());
                        if global.space == naga::AddressSpace::Uniform {
                            global.space = naga::AddressSpace::Storage {
                                access: naga::StorageAccess::LOAD,
                            };
                        }
                    }
                }
            }
            let pointers = module
                .types
                .iter()
                .filter_map(|(h, t)| match t.inner {
                    naga::TypeInner::Pointer {
                        base,
                        space: naga::AddressSpace::Uniform,
                    } => {
                        let mut t = t.clone();
                        t.inner = naga::TypeInner::Pointer {
                            base,
                            space: naga::AddressSpace::Storage {
                                access: naga::StorageAccess::LOAD,
                            },
                        };
                        t.name = Some(format!("KaelReadonlyPointer{}", h.index()));
                        Some((h, t))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            for (h, t) in pointers {
                module.types.replace(h, t);
            }
            for (_, global) in module.global_variables.iter_mut() {
                global.binding = None;
            }
            let info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all() - naga::valid::ValidationFlags::BINDINGS,
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .map_err(|e| failure(e.to_string()))?;
            result.source = naga::back::wgsl::write_string(
                &module,
                &info,
                naga::back::wgsl::WriterFlags::EXPLICIT_TYPES,
            )
            .map_err(|e| failure(e.to_string()))?;
            let emitted =
                naga::front::wgsl::parse_str(&result.source).map_err(|e| failure(e.to_string()))?;
            for slot in &result.resources {
                if !emitted
                    .global_variables
                    .iter()
                    .any(|(_, g)| g.name.as_deref() == Some(&slot.name))
                {
                    return Err(failure(format!("emitted resource {} changed", slot.name)));
                }
            }
        }
        ShaderBackend::WebGl2 => unreachable!(),
    }
    Ok(result)
}

/// Size of a zero-initialized storage buffer owned by one window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuBufferDescriptor {
    /// Nonzero byte length, multiple of four, at most 128 MiB.
    pub byte_len: u64,
}
impl GpuBufferDescriptor {
    /// Check the portable native storage-buffer bounds before allocation.
    pub fn validate(self) -> Result<u64, RenderTargetError> {
        if self.byte_len == 0
            || !self.byte_len.is_multiple_of(4)
            || self.byte_len > MAX_GPU_BUFFER_BYTES
        {
            return Err(RenderTargetError::InvalidBindings(
                "GPU buffer size must be a nonzero multiple of four, at most 128 MiB".into(),
            ));
        }
        Ok(self.byte_len)
    }
}
#[derive(Debug)]
struct BufferLease {
    valid: AtomicBool,
    revision: AtomicU64,
    device_valid: Arc<AtomicBool>,
}
/// Device-owned storage buffer. Clones pin allocation; resources cannot cross windows.
#[derive(Clone, Debug)]
pub struct GpuBuffer {
    owner: u64,
    id: u64,
    descriptor: GpuBufferDescriptor,
    lease: Arc<BufferLease>,
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validate_buffer_write(
    buffer: &GpuBuffer,
    offset: u64,
    bytes: &[u8],
) -> Result<(), RenderTargetError> {
    if !offset.is_multiple_of(4)
        || !bytes.len().is_multiple_of(4)
        || offset
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > buffer.descriptor.byte_len)
    {
        return Err(RenderTargetError::InvalidBindings(
            "GPU buffer writes require four-byte aligned, in-bounds ranges".into(),
        ));
    }
    Ok(())
}
impl PartialEq for GpuBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.id == other.id
    }
}
impl Eq for GpuBuffer {}
impl GpuBuffer {
    /// Buffer size.
    pub fn descriptor(&self) -> GpuBufferDescriptor {
        self.descriptor
    }
    /// Whether the owning device remains alive.
    pub fn is_valid(&self) -> bool {
        self.lease.valid.load(Ordering::Acquire) && self.lease.device_valid.load(Ordering::Acquire)
    }
    pub(crate) fn revision(&self) -> u64 {
        self.lease.revision.load(Ordering::Acquire)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn did_write(&self) {
        if self
            .lease
            .revision
            .fetch_update(Ordering::Release, Ordering::Relaxed, |r| r.checked_add(1))
            .is_err()
        {
            self.invalidate();
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn invalidate(&self) {
        self.lease.valid.store(false, Ordering::Release);
    }
    #[cfg(any(
        target_os = "windows",
        all(target_os = "macos", not(feature = "macos-blade"))
    ))]
    pub(crate) fn invalidate_device(&self) {
        self.lease.device_valid.store(false, Ordering::Release);
    }
    pub(crate) fn owner(&self) -> u64 {
        self.owner
    }
    pub(crate) fn id(&self) -> u64 {
        self.id
    }
}
#[cfg(not(target_arch = "wasm32"))]
struct BufferResource<T> {
    resource: T,
    lease: Weak<BufferLease>,
    bytes: u64,
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct BufferRegistry<T> {
    budget: DeviceBudget,
    next_id: u64,
    resources: BTreeMap<u64, BufferResource<T>>,
}
#[cfg(not(target_arch = "wasm32"))]
impl<T> BufferRegistry<T> {
    pub(crate) fn new(budget: DeviceBudget) -> Self {
        Self {
            budget,
            next_id: 1,
            resources: BTreeMap::new(),
        }
    }
    pub(crate) fn check(&self, descriptor: GpuBufferDescriptor) -> Result<u64, RenderTargetError> {
        let bytes = descriptor.validate()?;
        self.budget.check_allocation(bytes)?;
        if self.next_id == u64::MAX {
            return Err(RenderTargetError::ResourceLimit);
        }
        Ok(bytes)
    }
    pub(crate) fn insert(
        &mut self,
        descriptor: GpuBufferDescriptor,
        resource: T,
        bytes: u64,
    ) -> Result<GpuBuffer, RenderTargetError> {
        self.check(descriptor)?;
        self.budget.reserve(bytes)?;
        let id = self.next_id;
        self.next_id += 1;
        let lease = Arc::new(BufferLease {
            valid: AtomicBool::new(true),
            revision: AtomicU64::new(0),
            device_valid: self.budget.validity(),
        });
        self.resources.insert(
            id,
            BufferResource {
                resource,
                lease: Arc::downgrade(&lease),
                bytes,
            },
        );
        Ok(GpuBuffer {
            owner: self.budget.owner(),
            id,
            descriptor,
            lease,
        })
    }
    pub(crate) fn get(&self, buffer: &GpuBuffer) -> Result<&T, RenderTargetError> {
        if buffer.owner != self.budget.owner() || !buffer.is_valid() {
            return Err(RenderTargetError::WrongDevice);
        }
        self.resources
            .get(&buffer.id)
            .map(|r| &r.resource)
            .ok_or(RenderTargetError::WrongDevice)
    }
    pub(crate) fn take_unused(&mut self) -> Vec<T> {
        let ids = self
            .resources
            .iter()
            .filter_map(|(&id, r)| (r.lease.strong_count() == 0).then_some(id))
            .collect::<Vec<_>>();
        ids.into_iter()
            .map(|id| {
                let r = self.resources.remove(&id).unwrap();
                self.budget.release(r.bytes, 1);
                r.resource
            })
            .collect()
    }
    pub(crate) fn invalidate_and_drain(&mut self) -> Vec<T> {
        std::mem::take(&mut self.resources)
            .into_values()
            .map(|r| {
                if let Some(l) = r.lease.upgrade() {
                    l.valid.store(false, Ordering::Release);
                }
                self.budget.release(r.bytes, 1);
                r.resource
            })
            .collect()
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl<T> Drop for BufferRegistry<T> {
    fn drop(&mut self) {
        drop(self.invalidate_and_drain());
    }
}
/// One native compute resource value.
#[derive(Clone, Debug)]
pub enum ComputeBinding {
    /// Portable uniform bytes including padding.
    Uniform(Arc<[u8]>),
    /// Sampled associated linear target.
    Texture(RenderTarget),
    /// Texture sampling behavior.
    Sampler(ShaderSampler),
    /// Write-only output image.
    StorageTexture(RenderTarget),
    /// Read-only or writable buffer as reflected.
    StorageBuffer(GpuBuffer),
}
/// Compute resources keyed by authored WGSL binding number.
#[derive(Clone, Debug, Default)]
pub struct ComputeBindings {
    pub(crate) values: BTreeMap<u32, ComputeBinding>,
}
impl ComputeBindings {
    /// Empty resource set.
    pub fn new() -> Self {
        Self::default()
    }
    /// Insert one resource.
    pub fn with(mut self, binding: u32, value: ComputeBinding) -> Self {
        self.values.insert(binding, value);
        self
    }
    /// Read one resource.
    pub fn get(&self, binding: u32) -> Option<&ComputeBinding> {
        self.values.get(&binding)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn validate<T, B>(
        &self,
        shader: &ComputeHandle,
        groups: [u32; 3],
        targets: &TargetRegistry<T>,
        buffers: &BufferRegistry<B>,
    ) -> Result<(), RenderTargetError> {
        if groups.contains(&0)
            || groups.iter().any(|&n| n > 65535)
            || groups
                .iter()
                .try_fold(1u64, |p, &n| p.checked_mul(u64::from(n)))
                .is_none_or(|n| n > 16_777_216)
        {
            return Err(RenderTargetError::InvalidBindings(
                "dispatch requires 1..=65535 groups per axis and at most 16,777,216 total groups"
                    .into(),
            ));
        }
        if self.values.len() != shader.resources().len() {
            return Err(RenderTargetError::InvalidBindings(
                "compute resource count mismatch".into(),
            ));
        }
        let mut texture_reads = BTreeSet::new();
        let mut texture_writes = BTreeSet::new();
        let mut buffer_uses: BTreeMap<(u64, u64), bool> = BTreeMap::new();
        for r in shader.resources() {
            let value = self.get(r.binding).ok_or_else(|| {
                RenderTargetError::InvalidBindings(format!("missing compute binding {}", r.binding))
            })?;
            match (&r.kind, value) {
                (ComputeResourceKind::Uniform(layout), ComputeBinding::Uniform(b))
                    if b.len() == layout.size as usize => {}
                (ComputeResourceKind::Texture2d, ComputeBinding::Texture(t)) => {
                    targets.get(t)?;
                    texture_reads.insert((t.owner(), t.id()));
                }
                (ComputeResourceKind::Sampler, ComputeBinding::Sampler(_)) => {}
                (
                    ComputeResourceKind::StorageTexture(format),
                    ComputeBinding::StorageTexture(t),
                ) if t.descriptor().format == *format => {
                    targets.get(t)?;
                    if !texture_writes.insert((t.owner(), t.id())) {
                        return Err(RenderTargetError::FeedbackLoop);
                    }
                }
                (
                    ComputeResourceKind::StorageBuffer { layout, writable },
                    ComputeBinding::StorageBuffer(b),
                ) => {
                    buffers.get(b)?;
                    let size = b.descriptor.byte_len;
                    if size < u64::from(layout.min_size)
                        || layout
                            .runtime_array_offset
                            .zip(layout.runtime_array_stride)
                            .is_some_and(|(offset, stride)| {
                                (size - u64::from(offset)) % u64::from(stride) != 0
                            })
                    {
                        return Err(RenderTargetError::InvalidBindings(format!(
                            "buffer {} violates reflected minimum/runtime array stride",
                            r.binding
                        )));
                    }
                    let key = (b.owner(), b.id());
                    if buffer_uses
                        .get(&key)
                        .is_some_and(|previous| *previous || *writable)
                    {
                        return Err(RenderTargetError::FeedbackLoop);
                    }
                    buffer_uses.insert(key, *writable);
                }
                _ => {
                    return Err(RenderTargetError::InvalidBindings(format!(
                        "compute binding {} has wrong type, format or size",
                        r.binding
                    )));
                }
            }
        }
        if texture_reads.iter().any(|id| texture_writes.contains(id)) {
            return Err(RenderTargetError::FeedbackLoop);
        }
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn did_dispatch(&self, shader: &ComputeHandle) {
        for r in shader.resources() {
            match (&r.kind, self.get(r.binding)) {
                (
                    ComputeResourceKind::StorageTexture(_),
                    Some(ComputeBinding::StorageTexture(t)),
                ) => t.did_render(),
                (
                    ComputeResourceKind::StorageBuffer { writable: true, .. },
                    Some(ComputeBinding::StorageBuffer(b)),
                ) => b.did_write(),
                _ => {}
            }
        }
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) const HDR_STORAGE_REGRESSION: &str = r#"@group(0) @binding(5) var output:texture_storage_2d<rgba16float,write>; @compute @workgroup_size(2,2) fn main(@builtin(global_invocation_id) id:vec3<u32>){if any(id.xy>=textureDimensions(output)){return;}textureStore(output,vec2<i32>(id.xy),vec4<f32>(2.0,0.5,0.25,1.0));}"#;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) const SYNC_BARRIER_REGRESSION: &str = r#"
    var<workgroup> shared_value: u32;
    @group(0) @binding(31) var<storage, read_write> values: array<u32>;
    @compute @workgroup_size(2) fn main(@builtin(local_invocation_index) id:u32) {
        var count=0u;
        if(id==0u) { shared_value=17u; for(var index=0u;index<65536u;index+=1u) { count+=1u; } }
        workgroupBarrier();
        for(var step=0u;step<2u;step+=1u) { if(id==0u) {shared_value+=1u;} workgroupBarrier(); }
        values[id]=count+shared_value;
    }
"#;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn targets_and_buffers_share_budget_and_owner() {
        let mut targets = TargetRegistry::default();
        targets.set_budget(64);
        let mut buffers = BufferRegistry::new(targets.device_budget());
        let target = targets
            .insert(crate::RenderTargetDescriptor::rgba8(2, 2), (), 16)
            .unwrap();
        let buffer = buffers
            .insert(GpuBufferDescriptor { byte_len: 48 }, (), 48)
            .unwrap();
        assert_eq!(target.owner(), buffer.owner());
        assert!(matches!(
            buffers.check(GpuBufferDescriptor { byte_len: 4 }),
            Err(RenderTargetError::BudgetExceeded)
        ));
        assert!(matches!(
            targets.check_allocation(crate::RenderTargetDescriptor::rgba8(1, 1)),
            Err(RenderTargetError::BudgetExceeded)
        ));
        drop(buffer);
        drop(buffers.take_unused());
        assert!(
            targets
                .check_allocation(crate::RenderTargetDescriptor::rgba8(1, 1))
                .is_ok()
        );
        drop(buffers);
        drop(targets);
        assert!(!target.is_valid());
    }
    #[test]
    fn combined_handle_limit_and_device_loss_cover_buffers_and_textures() {
        let mut targets = TargetRegistry::default();
        let mut buffers = BufferRegistry::new(targets.device_budget());
        let textures: Vec<_> = (0..32)
            .map(|_| {
                targets
                    .insert(crate::RenderTargetDescriptor::rgba8(1, 1), (), 4)
                    .unwrap()
            })
            .collect();
        let data: Vec<_> = (0..32)
            .map(|_| {
                buffers
                    .insert(GpuBufferDescriptor { byte_len: 4 }, (), 4)
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            buffers.check(GpuBufferDescriptor { byte_len: 4 }),
            Err(RenderTargetError::ResourceLimit)
        ));
        assert!(matches!(
            targets.check_allocation(crate::RenderTargetDescriptor::rgba8(1, 1)),
            Err(RenderTargetError::ResourceLimit)
        ));
        textures[0].invalidate_device();
        assert!(textures.iter().all(|t| !t.is_valid()));
        assert!(data.iter().all(|b| !b.is_valid()));
        assert!(matches!(
            targets.check_allocation(crate::RenderTargetDescriptor::rgba8(1, 1)),
            Err(RenderTargetError::WrongDevice)
        ));
        assert!(matches!(
            buffers.get(&data[0]),
            Err(RenderTargetError::WrongDevice)
        ));
    }
    #[test]
    fn checked_runtime_stride_buffer_aliases_and_foreign_device() {
        let shader=ComputeHandle::compile(ComputeDescriptor::new("checked arrays",r#"struct Data { prefix: vec4<u32>, values: array<vec4<f32>> } @group(0) @binding(2) var<storage, read> input: Data; @group(0) @binding(7) var<storage, read_write> output: Data; @compute @workgroup_size(1) fn main(){output.values[0]=input.values[0];}"#,"main")).unwrap();
        let targets = TargetRegistry::<()>::default();
        let mut buffers = BufferRegistry::new(targets.device_budget());
        let input = buffers
            .insert(GpuBufferDescriptor { byte_len: 32 }, (), 32)
            .unwrap();
        let output = buffers
            .insert(GpuBufferDescriptor { byte_len: 32 }, (), 32)
            .unwrap();
        let bindings = ComputeBindings::new()
            .with(2, ComputeBinding::StorageBuffer(input.clone()))
            .with(7, ComputeBinding::StorageBuffer(output));
        bindings
            .validate(&shader, [1, 1, 1], &targets, &buffers)
            .unwrap();
        assert!(matches!(
            bindings
                .clone()
                .with(7, ComputeBinding::StorageBuffer(input))
                .validate(&shader, [1, 1, 1], &targets, &buffers),
            Err(RenderTargetError::FeedbackLoop)
        ));
        let unaligned = buffers
            .insert(GpuBufferDescriptor { byte_len: 36 }, (), 36)
            .unwrap();
        assert!(
            bindings
                .clone()
                .with(7, ComputeBinding::StorageBuffer(unaligned))
                .validate(&shader, [1, 1, 1], &targets, &buffers)
                .is_err()
        );
        let foreign_targets = TargetRegistry::<()>::default();
        let mut foreign = BufferRegistry::new(foreign_targets.device_budget());
        let buffer = foreign
            .insert(GpuBufferDescriptor { byte_len: 32 }, (), 32)
            .unwrap();
        assert!(matches!(
            bindings
                .with(7, ComputeBinding::StorageBuffer(buffer))
                .validate(&shader, [1, 1, 1], &targets, &buffers),
            Err(RenderTargetError::WrongDevice)
        ));
    }
    #[test]
    fn portable_compute_limits_and_guarded_barrier_validation() {
        assert!(
            GpuBufferDescriptor {
                byte_len: MAX_GPU_BUFFER_BYTES
            }
            .validate()
            .is_ok()
        );
        assert!(
            GpuBufferDescriptor {
                byte_len: MAX_GPU_BUFFER_BYTES + 4
            }
            .validate()
            .is_err()
        );
        assert!(matches!(
            ComputeHandle::compile(ComputeDescriptor::new(
                "unallocatable storage layout",
                "@group(0) @binding(0) var<storage, read> oversized:array<u32,33554433>; @compute @workgroup_size(1) fn main() { let value=oversized[0]; }",
                "main",
            )),
            Err(ShaderError::ResourceLimit("128 MiB storage binding"))
        ));
        assert!(
            ComputeHandle::compile(ComputeDescriptor::new(
                "limit",
                "@compute @workgroup_size(256) fn main() {}",
                "main"
            ))
            .is_err()
        );
        assert!(
            ComputeHandle::compile(ComputeDescriptor::new(
                "limit",
                "@compute @workgroup_size(1,1,128) fn main() {}",
                "main"
            ))
            .is_err()
        );
        let straight = ComputeHandle::compile(ComputeDescriptor::new(
            "shared memory",
            r#"var<workgroup> value:u32; @compute @workgroup_size(2) fn main(@builtin(local_invocation_index) id:u32){if(id==0u){value=17u;}workgroupBarrier();}"#,
            "main",
        ));
        assert!(straight.is_ok(), "{straight:?}");
        let guarded = ComputeHandle::compile(ComputeDescriptor::new(
            "barrier loop",
            r#"@compute @workgroup_size(2) fn main(){for(var i=0u;i<2u;i+=1u){workgroupBarrier();}}"#,
            "main",
        ));
        let guarded = guarded.unwrap();
        assert_eq!(
            guarded.loop_body_limit(),
            None,
            "synchronized kernels must preserve authored participation"
        );
        assert!(
            ComputeHandle::compile(
                ComputeDescriptor::new("strict loop bound", guarded.source(), "main")
                    .require_loop_bound()
            )
            .is_err()
        );
    }
    #[test]
    fn runtime_arrays_and_native_translations() {
        let shader=ComputeHandle::compile(ComputeDescriptor::new("array",r#"@group(0) @binding(7) var<storage, read_write> values: array<vec4<f32>>; @group(0) @binding(12) var output: texture_storage_2d<rgba8unorm, write>; @compute @workgroup_size(8,8) fn main(@builtin(global_invocation_id) id: vec3<u32>) { if (id.x < arrayLength(&values)) { values[id.x] = vec4<f32>(1.0); textureStore(output, vec2<i32>(id.xy), values[id.x]); } }"#,"main")).unwrap();
        assert_eq!(shader.workgroup_size(), [8, 8, 1]);
        let ComputeResourceKind::StorageBuffer { layout, .. } = &shader.resources()[0].kind else {
            panic!()
        };
        assert_eq!(layout.runtime_array_stride, Some(16));
        for backend in [
            ShaderBackend::Metal,
            ShaderBackend::DirectX11,
            ShaderBackend::Blade,
        ] {
            assert!(!shader.translate(backend).unwrap().source.is_empty());
        }
        assert!(shader.translate(ShaderBackend::WebGl2).is_err());
    }
}
