//! GPU execution of declared fragment/compute DAGs, with bounded transient reuse.

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod gpu_tests;

use crate::{
    ComputeBinding, ComputeBindings, ComputeHandle, ComputeResourceKind, GpuBuffer,
    GpuBufferDescriptor, MemoryPressureLevel, RenderTarget, RenderTargetDescriptor,
    RenderTargetError, ShaderBinding, ShaderBindings, ShaderHandle, ShaderResourceKind,
    ShaderSampler, Window,
};
use kael_render_graph::{CompiledGraph, PassDesc, PassId, RenderGraph, ResourceId, ResourceKind};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_RESOURCES: usize = 256;
const MAX_PASSES: usize = 256;
const MAX_SLOTS: usize = 64;
const MAX_UNIFORM_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_BYTE_BUDGET: u64 = 256 * 1024 * 1024;

/// A resource value for one binding in a GPU graph pass.
#[derive(Clone, Debug)]
pub enum GpuGraphBinding {
    /// Bytes following the registered shader's exact reflected uniform layout.
    Uniform(Arc<[u8]>),
    /// A declared graph texture, resolved to its imported or transient target.
    Texture(ResourceId),
    /// A declared storage buffer for a compute binding.
    Buffer(ResourceId),
    /// Sampling behavior for a declared sampler binding.
    Sampler(ShaderSampler),
}

/// A native compute program and its exact resource bindings.
///
/// Write-only storage textures use `GpuGraphBinding::Texture`; storage buffers
/// use `GpuGraphBinding::Buffer`. A writable storage buffer is declared as a
/// graph write, and that pass always dispatches because read/write kernels can
/// depend on the buffer's previous contents. Kernels must initialize transient
/// outputs before reading them, including when a physical slot is reused.
#[derive(Clone, Debug)]
pub struct GpuComputePass {
    /// Validated native compute program.
    pub shader: ComputeHandle,
    /// Workgroups, independently validated before resource allocation.
    pub groups: [u32; 3],
    bindings: BTreeMap<u32, GpuGraphBinding>,
}
impl GpuComputePass {
    /// Describe a compute pass without bindings.
    pub fn new(shader: ComputeHandle, groups: [u32; 3]) -> Self {
        Self {
            shader,
            groups,
            bindings: BTreeMap::new(),
        }
    }
    /// Supply one reflected group-zero binding.
    pub fn with(mut self, binding: u32, value: GpuGraphBinding) -> Self {
        self.bindings.insert(binding, value);
        self
    }
}

/// Executable program for one declared graph pass.
#[derive(Clone, Debug)]
pub enum GpuGraphPass {
    /// Portable fullscreen fragment pass with one texture output.
    Fragment(GpuFragmentPass),
    /// Native compute pass with one or more texture/buffer outputs.
    Compute(GpuComputePass),
}
impl GpuGraphPass {
    fn bindings(&self) -> &BTreeMap<u32, GpuGraphBinding> {
        match self {
            Self::Fragment(p) => &p.bindings,
            Self::Compute(p) => &p.bindings,
        }
    }
    fn shader_id(&self) -> u64 {
        match self {
            Self::Fragment(p) => p.shader.id(),
            Self::Compute(p) => p.shader.id(),
        }
    }
    fn groups(&self) -> Option<[u32; 3]> {
        match self {
            Self::Fragment(_) => None,
            Self::Compute(p) => Some(p.groups),
        }
    }
    fn cacheable(&self) -> bool {
        match self {
            Self::Fragment(_) => true,
            Self::Compute(p) => !p.shader.resources().iter().any(|r| {
                matches!(
                    r.kind,
                    ComputeResourceKind::StorageBuffer { writable: true, .. }
                )
            }),
        }
    }
}

/// Exact physical descriptor for a graph transient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuGraphResourceDescriptor {
    /// Two-dimensional color/scalar target.
    Texture(RenderTargetDescriptor),
    /// Native storage buffer.
    Buffer(GpuBufferDescriptor),
}
impl GpuGraphResourceDescriptor {
    fn kind(self) -> ResourceKind {
        match self {
            Self::Texture(_) => ResourceKind::Texture,
            Self::Buffer(_) => ResourceKind::Buffer,
        }
    }
    fn byte_len(self) -> Result<u64, RenderTargetError> {
        match self {
            Self::Texture(d) => d.byte_len(),
            Self::Buffer(d) => d.validate(),
        }
    }
}

/// An imported or exported native GPU resource, pinned by an owned handle.
#[derive(Clone, Debug)]
pub enum GpuGraphResource {
    /// A target belonging to this window.
    Texture(RenderTarget),
    /// A storage buffer belonging to this window.
    Buffer(GpuBuffer),
}
impl GpuGraphResource {
    fn descriptor(&self) -> GpuGraphResourceDescriptor {
        match self {
            Self::Texture(t) => GpuGraphResourceDescriptor::Texture(t.descriptor()),
            Self::Buffer(b) => GpuGraphResourceDescriptor::Buffer(b.descriptor()),
        }
    }
    fn is_valid(&self) -> bool {
        match self {
            Self::Texture(t) => t.is_valid(),
            Self::Buffer(b) => b.is_valid(),
        }
    }
    fn identity(&self) -> ResourceIdentity {
        match self {
            Self::Texture(t) => ResourceIdentity {
                kind: 0,
                owner: t.owner(),
                id: t.id(),
                revision: t.revision(),
            },
            Self::Buffer(b) => ResourceIdentity {
                kind: 1,
                owner: b.owner(),
                id: b.id(),
                revision: b.revision(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ResourceIdentity {
    kind: u8,
    owner: u64,
    id: u64,
    revision: u64,
}

/// Fragment program and group-zero bindings for one declared graph pass.
#[derive(Clone, Debug)]
pub struct GpuFragmentPass {
    /// The validated portable fragment program.
    pub shader: ShaderHandle,
    bindings: BTreeMap<u32, GpuGraphBinding>,
}

impl GpuFragmentPass {
    /// Describe a pass without resources.
    pub fn new(shader: ShaderHandle) -> Self {
        Self {
            shader,
            bindings: BTreeMap::new(),
        }
    }

    /// Supply a value for one authored WGSL binding number.
    pub fn with(mut self, binding: u32, value: GpuGraphBinding) -> Self {
        self.bindings.insert(binding, value);
        self
    }
}

/// Outputs and work counts from a successful GPU graph execution.
#[derive(Debug)]
pub struct GpuGraphExecution {
    /// Requested exports, held alive independently of the executor's cache.
    pub outputs: BTreeMap<ResourceId, RenderTarget>,
    /// Requested storage-buffer exports. WebGL2 has no storage buffers.
    pub buffers: BTreeMap<ResourceId, GpuBuffer>,
    /// Number of fragment/compute passes submitted to the renderer.
    pub executed_passes: usize,
    /// Number of passes whose exact input signature and resident output matched.
    pub skipped_passes: usize,
}

/// An invalid graph, resource contract, or GPU operation.
#[derive(Debug, thiserror::Error)]
pub enum GpuGraphError {
    /// Invalid topology, missing bindings/imports, incompatible alias classes,
    /// or a resource/pass beyond this executor's supported contract.
    #[error("invalid GPU graph: {0}")]
    Invalid(String),
    /// Physical targets, pass metadata, or retained uniform bytes exceed bounds.
    #[error("GPU graph resource limit exceeded: {0}")]
    ResourceLimit(&'static str),
    /// Native allocation, ownership, compilation, or submission failed.
    #[error(transparent)]
    Target(#[from] RenderTargetError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SignatureValue {
    Uniform(Arc<[u8]>),
    Resource(ResourceId, ResourceIdentity),
    Output(ResourceId, u8, u64, u64),
    Sampler(ShaderSampler),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    graph_key: u128,
    shader: u64,
    groups: Option<[u32; 3]>,
    bindings: Vec<(u32, SignatureValue)>,
}

struct CachedPass {
    signature: Signature,
    outputs: Vec<(ResourceId, ResourceIdentity)>,
}

/// A reusable GPU fragment/compute graph backed by one window's renderer.
///
/// Construction validates and schedules an immutable graph. Every real pass
/// declares every sampled/read-only resource as a read and every output as a
/// write. Fragment passes write exactly one texture. Compute passes may write
/// multiple storage textures/buffers. This executor accepts at most 256 resources/passes and
/// 64 physical target slots, including slots required to preserve exports.
///
/// Export lifetimes extend to the end of execution, so an output is never
/// returned after another logical resource has overwritten its aliased slot.
/// All actual dimensions/formats are checked against allocation classes before
/// GPU allocation. Cache reuse compares uniform bytes and actual input target
/// identities/revisions, rather than trusting a caller's parameter hash alone.
/// Aliased outputs are reused only while that exact logical resource remains
/// resident. Submission is ordered through the renderer's ordinary pass path.
pub struct GpuRenderGraph {
    graph: RenderGraph,
    compiled: CompiledGraph,
    order: Vec<PassId>,
    exports: Vec<ResourceId>,
    descriptors: BTreeMap<ResourceId, GpuGraphResourceDescriptor>,
    slot_of: Vec<Option<usize>>,
    slot_descriptors: Vec<GpuGraphResourceDescriptor>,
    slots: Vec<GpuGraphResource>,
    resident: Vec<Option<ResourceId>>,
    cache: BTreeMap<PassId, CachedPass>,
    context: Option<u64>,
    gpu_owner: Option<u64>,
    byte_budget: u64,
    physical_bytes: u64,
}

impl GpuRenderGraph {
    /// Compile a graph and describe every used transient texture and export.
    ///
    /// Imported resources are supplied on each execution. The graph remains
    /// immutable; rebuild this executor when topology or target sizes change.
    /// Pass shader/uniform values may change on every execution.
    pub fn new(
        graph: RenderGraph,
        descriptors: &[(ResourceId, RenderTargetDescriptor)],
        exports: &[ResourceId],
    ) -> Result<Self, GpuGraphError> {
        let typed: Vec<_> = descriptors
            .iter()
            .map(|&(id, d)| (id, GpuGraphResourceDescriptor::Texture(d)))
            .collect();
        Self::new_with_resources(graph, &typed, exports)
    }

    /// Compile a mixed fragment/compute graph with exact buffer/texture types.
    /// Resource kinds and alias classes must agree with these descriptors.
    pub fn new_with_resources(
        mut graph: RenderGraph,
        descriptors: &[(ResourceId, GpuGraphResourceDescriptor)],
        exports: &[ResourceId],
    ) -> Result<Self, GpuGraphError> {
        if descriptors.len() > MAX_RESOURCES || exports.len() > MAX_SLOTS {
            return Err(GpuGraphError::ResourceLimit("resource/export count"));
        }
        if graph.resource(ResourceId(MAX_RESOURCES as u32)).is_some()
            || graph.pass(PassId(MAX_PASSES as u32)).is_some()
        {
            return Err(GpuGraphError::ResourceLimit("256 resources and passes"));
        }
        if exports.is_empty() {
            return Err(invalid("at least one output export is required"));
        }
        let mut unique_exports = BTreeSet::new();
        let mut tail = PassDesc::new("kael_gpu_exports");
        for &id in exports {
            if graph.resource(id).is_none() || !unique_exports.insert(id) {
                return Err(invalid("unknown or duplicated output export"));
            }
            tail = tail.read(id);
        }
        let tail_id = graph
            .try_add_pass(tail)
            .map_err(|error| invalid(error.to_string()))?;
        let compiled = graph
            .compile()
            .map_err(|error| invalid(error.to_string()))?;
        let order: Vec<_> = compiled
            .execution_order()
            .iter()
            .copied()
            .filter(|id| *id != tail_id)
            .collect();
        if order.is_empty() {
            return Err(invalid("at least one executable GPU pass is required"));
        }
        let mut typed = BTreeMap::new();
        for &(id, desc) in descriptors {
            let Some(resource) = graph.resource(id) else {
                return Err(invalid("unknown target descriptor"));
            };
            if resource.imported || resource.kind != desc.kind() || typed.insert(id, desc).is_some()
            {
                return Err(invalid(
                    "descriptors must uniquely match transient resource kinds",
                ));
            }
            desc.byte_len()?;
        }
        let allocation = compiled.assign_transient_memory();
        if allocation.slot_count > MAX_SLOTS {
            return Err(GpuGraphError::ResourceLimit("64 physical target slots"));
        }
        let mut slot_descriptors = vec![None; allocation.slot_count];
        let mut classes = BTreeMap::new();
        for index in 0..MAX_RESOURCES {
            let id = ResourceId(index as u32);
            let Some(resource) = graph.resource(id) else {
                break;
            };
            if compiled.lifetime(id).is_none() {
                continue;
            }
            if resource.imported {
                continue;
            }
            let desc = *typed
                .get(&id)
                .ok_or_else(|| invalid("missing used transient target descriptor"))?;
            if classes
                .insert((resource.kind as u8, resource.allocation_class), desc)
                .is_some_and(|prior| prior != desc)
            {
                return Err(invalid(
                    "an allocation class contains different dimensions or formats",
                ));
            }
            let slot = allocation.slot_of[index]
                .ok_or_else(|| invalid("used transient has no allocation slot"))?;
            if slot_descriptors[slot]
                .replace(desc)
                .is_some_and(|prior| prior != desc)
            {
                return Err(invalid("incompatible transient alias slot"));
            }
        }
        for &id in &order {
            let pass = graph
                .pass(id)
                .ok_or_else(|| invalid("unknown compiled pass"))?;
            if pass.writes.is_empty() || pass.writes.len() > 8 {
                return Err(invalid("each GPU pass must write one to eight resources"));
            }
            if pass.reads.iter().copied().collect::<BTreeSet<_>>().len() != pass.reads.len() {
                return Err(invalid("a pass contains duplicate declared reads"));
            }
        }
        let slot_descriptors: Vec<_> = slot_descriptors
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid("empty physical target slot"))?;
        let physical_bytes = slot_descriptors.iter().try_fold(0u64, |sum, desc| {
            sum.checked_add(desc.byte_len()?)
                .ok_or(GpuGraphError::ResourceLimit("target bytes overflow"))
        })?;
        if physical_bytes > DEFAULT_BYTE_BUDGET {
            return Err(GpuGraphError::ResourceLimit(
                "256 MiB default target budget",
            ));
        }
        Ok(Self {
            graph,
            compiled,
            order,
            exports: exports.to_vec(),
            descriptors: typed,
            slot_of: allocation.slot_of,
            resident: vec![None; slot_descriptors.len()],
            slot_descriptors,
            slots: Vec::new(),
            cache: BTreeMap::new(),
            context: None,
            gpu_owner: None,
            byte_budget: DEFAULT_BYTE_BUDGET,
            physical_bytes,
        })
    }

    /// A collision-free allocation class for these target dimensions/format.
    pub fn allocation_class(descriptor: RenderTargetDescriptor) -> u64 {
        // Valid target dimensions fit in 16 bits. Format discriminants occupy
        // the upper word and remain separate from both dimensions.
        u64::from(descriptor.width)
            | (u64::from(descriptor.height) << 16)
            | ((descriptor.format as u64) << 32)
    }

    /// The scheduled graph, including the final lifetime-only export pass.
    pub fn compiled(&self) -> &CompiledGraph {
        &self.compiled
    }

    /// Physical slots required by the validated alias plan.
    pub fn physical_slot_count(&self) -> usize {
        self.slot_descriptors.len()
    }

    /// Payload bytes required by all physical slots, excluding driver overhead.
    pub fn physical_byte_len(&self) -> u64 {
        self.physical_bytes
    }

    /// Change this executor's payload ceiling. A lower ceiling releases its
    /// cache; subsequent execution rejects an over-budget graph before allocation.
    /// The window's independent live-target budget also applies.
    pub fn set_byte_budget(&mut self, bytes: u64) {
        self.byte_budget = bytes;
        if self.physical_bytes > bytes {
            self.clear_cache();
        }
    }

    /// Release cached outputs, uniform signatures and all owned target handles.
    /// Exports already held by callers/scenes remain valid on their device.
    pub fn clear_cache(&mut self) {
        self.slots = Vec::new();
        self.cache = BTreeMap::new();
        self.resident.fill(None);
        self.context = None;
        self.gpu_owner = None;
    }

    /// Shed retained graph work on warning/critical pressure. Applications can
    /// call this from [`crate::App::on_memory_pressure`].
    pub fn handle_memory_pressure(&mut self, level: MemoryPressureLevel) {
        if level != MemoryPressureLevel::Normal {
            self.clear_cache();
        }
    }

    pub(crate) fn execute_with(
        &mut self,
        renderer: &mut impl GpuGraphRenderer,
        imports: &[(ResourceId, RenderTarget)],
        programs: &[(PassId, GpuFragmentPass)],
    ) -> Result<GpuGraphExecution, GpuGraphError> {
        let imports: Vec<_> = imports
            .iter()
            .map(|(id, t)| (*id, GpuGraphResource::Texture(t.clone())))
            .collect();
        let programs: Vec<_> = programs
            .iter()
            .map(|(id, p)| (*id, GpuGraphPass::Fragment(p.clone())))
            .collect();
        self.execute_programs_with(renderer, &imports, &programs)
    }

    pub(crate) fn execute_programs_with(
        &mut self,
        renderer: &mut impl GpuGraphRenderer,
        imports: &[(ResourceId, GpuGraphResource)],
        programs: &[(PassId, GpuGraphPass)],
    ) -> Result<GpuGraphExecution, GpuGraphError> {
        if imports.len() > MAX_SLOTS || programs.len() > MAX_PASSES {
            return Err(GpuGraphError::ResourceLimit("import/program count"));
        }
        if self.physical_bytes > self.byte_budget {
            return Err(GpuGraphError::ResourceLimit(
                "configured resource byte budget",
            ));
        }
        if self.slots.iter().any(|resource| !resource.is_valid()) {
            self.clear_cache();
        }
        if self.context.is_some_and(|id| id != renderer.context_id()) {
            return Err(RenderTargetError::WrongDevice.into());
        }
        let mut imported = BTreeMap::new();
        let mut owner = None;
        for (id, resource) in imports {
            if !self
                .graph
                .resource(*id)
                .is_some_and(|desc| desc.imported && desc.kind == resource.descriptor().kind())
                || imported.insert(*id, resource.clone()).is_some()
            {
                return Err(invalid(
                    "unknown, transient, wrong-kind or duplicate import",
                ));
            }
            if !resource.is_valid() {
                return Err(RenderTargetError::WrongDevice.into());
            }
            validate_resource(renderer, resource)?;
            let key = resource.identity();
            if owner
                .replace(key.owner)
                .is_some_and(|prior| prior != key.owner)
            {
                return Err(RenderTargetError::WrongDevice.into());
            }
        }
        if let (Some(owner), Some(expected)) = (owner, self.gpu_owner) {
            if owner != expected {
                return Err(RenderTargetError::WrongDevice.into());
            }
        }
        let identities: BTreeSet<_> = imported
            .values()
            .map(|resource| {
                let key = resource.identity();
                (key.kind, key.owner, key.id)
            })
            .collect();
        if identities.len() != imported.len() {
            return Err(invalid(
                "imported resources alias the same physical resource",
            ));
        }
        for index in 0..MAX_RESOURCES {
            let id = ResourceId(index as u32);
            let Some(desc) = self.graph.resource(id) else {
                break;
            };
            if desc.imported && self.compiled.lifetime(id).is_some() && !imported.contains_key(&id)
            {
                return Err(invalid("missing used imported resource"));
            }
        }
        let mut definitions = BTreeMap::new();
        let mut uniform_bytes = 0usize;
        for (id, program) in programs {
            if !self.order.contains(id) || definitions.insert(*id, program).is_some() {
                return Err(invalid("unknown or duplicate pass program"));
            }
            let pass = self.graph.pass(*id).expect("validated pass");
            let bytes = validate_program(program, pass, |id| {
                self.descriptors
                    .get(&id)
                    .copied()
                    .or_else(|| imported.get(&id).map(GpuGraphResource::descriptor))
            })?;
            uniform_bytes = uniform_bytes
                .checked_add(bytes)
                .filter(|bytes| *bytes <= MAX_UNIFORM_BYTES)
                .ok_or(GpuGraphError::ResourceLimit(
                    "16 MiB uniform signature bytes",
                ))?;
        }
        if definitions.len() != self.order.len() {
            return Err(invalid("missing pass program"));
        }
        #[cfg(target_arch = "wasm32")]
        if definitions
            .values()
            .any(|program| matches!(program, GpuGraphPass::Compute(_)))
        {
            return Err(RenderTargetError::Unsupported("WebGL2 has no compute stage").into());
        }
        if self.slots.is_empty() {
            let mut created = Vec::with_capacity(self.slot_descriptors.len());
            for &desc in &self.slot_descriptors {
                created.push(match desc {
                    GpuGraphResourceDescriptor::Texture(d) => {
                        GpuGraphResource::Texture(renderer.create(d)?)
                    }
                    GpuGraphResourceDescriptor::Buffer(d) => {
                        GpuGraphResource::Buffer(renderer.create_buffer(d)?)
                    }
                });
            }
            self.slots = created;
            if let Some(resource) = self.slots.first() {
                self.context = Some(renderer.context_id());
                self.gpu_owner = Some(resource.identity().owner);
            }
        }
        if let (Some(owner), Some(resource)) = (owner, self.slots.first()) {
            if owner != resource.identity().owner {
                return Err(RenderTargetError::WrongDevice.into());
            }
        }
        let mut resources = imported;
        for &id in self.descriptors.keys() {
            if let Some(slot) = self.slot_of.get(id.0 as usize).copied().flatten() {
                resources.insert(id, self.slots[slot].clone());
            }
        }
        let mut executed_passes = 0;
        let mut skipped_passes = 0;
        for &id in &self.order {
            let program = definitions[&id];
            let outputs = &self.graph.pass(id).expect("validated pass").writes;
            let mut signature = Signature {
                shader: program.shader_id(),
                groups: program.groups(),
                graph_key: self
                    .compiled
                    .cache_key(id)
                    .expect("compiled pass key")
                    .as_u128(),
                bindings: Vec::with_capacity(program.bindings().len()),
            };
            for (&binding, value) in program.bindings() {
                let value = match value {
                    GpuGraphBinding::Uniform(bytes) => SignatureValue::Uniform(bytes.clone()),
                    GpuGraphBinding::Sampler(sampler) => SignatureValue::Sampler(*sampler),
                    GpuGraphBinding::Texture(resource) | GpuGraphBinding::Buffer(resource) => {
                        let key = resources
                            .get(resource)
                            .ok_or_else(|| invalid("missing bound resource"))?
                            .identity();
                        if outputs.contains(resource) {
                            SignatureValue::Output(*resource, key.kind, key.owner, key.id)
                        } else {
                            SignatureValue::Resource(*resource, key)
                        }
                    }
                };
                signature.bindings.push((binding, value));
            }
            let output_keys: Vec<_> = outputs
                .iter()
                .map(|output| {
                    Ok((
                        *output,
                        resources
                            .get(output)
                            .ok_or_else(|| invalid("missing output resource"))?
                            .identity(),
                    ))
                })
                .collect::<Result<_, GpuGraphError>>()?;
            let resident = outputs.iter().all(|output| {
                self.slot_of
                    .get(output.0 as usize)
                    .copied()
                    .flatten()
                    .is_none_or(|slot| self.resident[slot] == Some(*output))
            });
            let hit = program.cacheable()
                && resident
                && self.cache.get(&id).is_some_and(|entry| {
                    entry.signature == signature && entry.outputs == output_keys
                });
            if hit {
                skipped_passes += 1;
                continue;
            }
            if let Err(error) = submit_program(renderer, program, outputs, &resources) {
                self.cache.clear();
                return Err(error.into());
            }
            if outputs.iter().any(|output| !resources[output].is_valid()) {
                self.clear_cache();
                return Err(RenderTargetError::WrongDevice.into());
            }
            for &output in outputs {
                if let Some(slot) = self.slot_of.get(output.0 as usize).copied().flatten() {
                    self.resident[slot] = Some(output);
                }
            }
            self.cache.insert(
                id,
                CachedPass {
                    signature,
                    outputs: outputs
                        .iter()
                        .map(|output| (*output, resources[output].identity()))
                        .collect(),
                },
            );
            executed_passes += 1;
        }
        self.context = Some(renderer.context_id());
        self.gpu_owner = self
            .slots
            .first()
            .or_else(|| resources.values().next())
            .map(|r| r.identity().owner);
        let mut outputs = BTreeMap::new();
        let mut buffers = BTreeMap::new();
        for id in &self.exports {
            match resources
                .get(id)
                .ok_or_else(|| invalid("missing export resource"))?
            {
                GpuGraphResource::Texture(t) => {
                    outputs.insert(*id, t.clone());
                }
                GpuGraphResource::Buffer(b) => {
                    buffers.insert(*id, b.clone());
                }
            }
        }
        Ok(GpuGraphExecution {
            outputs,
            buffers,
            executed_passes,
            skipped_passes,
        })
    }
}

fn invalid(message: impl Into<String>) -> GpuGraphError {
    GpuGraphError::Invalid(message.into())
}

fn validate_resource(
    renderer: &impl GpuGraphRenderer,
    resource: &GpuGraphResource,
) -> Result<(), RenderTargetError> {
    match resource {
        GpuGraphResource::Texture(t) => renderer.validate(t),
        GpuGraphResource::Buffer(b) => renderer.validate_buffer(b),
    }
}

fn validate_program(
    program: &GpuGraphPass,
    pass: &PassDesc,
    descriptor: impl Fn(ResourceId) -> Option<GpuGraphResourceDescriptor>,
) -> Result<usize, GpuGraphError> {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut bytes = 0usize;
    let mut uniform = |size: u32, value: &GpuGraphBinding| -> Result<(), GpuGraphError> {
        let GpuGraphBinding::Uniform(data) = value else {
            return Err(invalid("expected uniform bytes"));
        };
        if data.len() != size as usize {
            return Err(invalid("uniform size does not match reflection"));
        }
        bytes = bytes
            .checked_add(data.len())
            .filter(|n| *n <= MAX_UNIFORM_BYTES)
            .ok_or(GpuGraphError::ResourceLimit("uniform signature bytes"))?;
        Ok(())
    };
    let texture =
        |value: &GpuGraphBinding| -> Result<(ResourceId, RenderTargetDescriptor), GpuGraphError> {
            let GpuGraphBinding::Texture(id) = value else {
                return Err(invalid("expected texture binding"));
            };
            let Some(GpuGraphResourceDescriptor::Texture(desc)) = descriptor(*id) else {
                return Err(invalid("unknown or wrong-kind texture binding"));
            };
            Ok((*id, desc))
        };
    match program {
        GpuGraphPass::Fragment(p) => {
            if pass.writes.len() != 1
                || !matches!(
                    descriptor(pass.writes[0]),
                    Some(GpuGraphResourceDescriptor::Texture(_))
                )
            {
                return Err(invalid("fragment pass must write exactly one texture"));
            }
            writes.insert(pass.writes[0]);
            if p.bindings.len() != p.shader.resources().len() {
                return Err(invalid("fragment binding count mismatch"));
            }
            for resource in p.shader.resources() {
                let value = p
                    .bindings
                    .get(&resource.binding)
                    .ok_or_else(|| invalid("missing fragment binding"))?;
                match &resource.kind {
                    ShaderResourceKind::Uniform(layout) => uniform(layout.size, value)?,
                    ShaderResourceKind::Texture2d => {
                        reads.insert(texture(value)?.0);
                    }
                    ShaderResourceKind::Sampler if matches!(value, GpuGraphBinding::Sampler(_)) => {
                    }
                    _ => return Err(invalid("fragment resource kind mismatch")),
                }
            }
        }
        GpuGraphPass::Compute(p) => {
            if p.groups.contains(&0)
                || p.groups.iter().any(|&n| n > 65535)
                || p.groups
                    .iter()
                    .try_fold(1u64, |n, &g| n.checked_mul(u64::from(g)))
                    .is_none_or(|n| n > 16_777_216)
            {
                return Err(invalid("compute workgroup count exceeds portable limits"));
            }
            if p.bindings.len() != p.shader.resources().len() {
                return Err(invalid("compute binding count mismatch"));
            }
            for resource in p.shader.resources() {
                let value = p
                    .bindings
                    .get(&resource.binding)
                    .ok_or_else(|| invalid("missing compute binding"))?;
                match &resource.kind {
                    ComputeResourceKind::Uniform(layout) => uniform(layout.size, value)?,
                    ComputeResourceKind::Texture2d => {
                        reads.insert(texture(value)?.0);
                    }
                    ComputeResourceKind::Sampler
                        if matches!(value, GpuGraphBinding::Sampler(_)) => {}
                    ComputeResourceKind::StorageTexture(format) => {
                        let (id, desc) = texture(value)?;
                        if desc.format != *format || !writes.insert(id) {
                            return Err(invalid("storage texture format or duplicate write"));
                        }
                    }
                    ComputeResourceKind::StorageBuffer { layout, writable } => {
                        let GpuGraphBinding::Buffer(id) = value else {
                            return Err(invalid("expected storage buffer"));
                        };
                        let Some(GpuGraphResourceDescriptor::Buffer(desc)) = descriptor(*id) else {
                            return Err(invalid("unknown or wrong-kind buffer binding"));
                        };
                        if desc.byte_len < u64::from(layout.min_size)
                            || layout
                                .runtime_array_offset
                                .zip(layout.runtime_array_stride)
                                .is_some_and(|(offset, stride)| {
                                    stride == 0
                                        || desc
                                            .byte_len
                                            .checked_sub(u64::from(offset))
                                            .is_none_or(|n| n % u64::from(stride) != 0)
                                })
                        {
                            return Err(invalid(
                                "storage buffer minimum size/runtime-array stride mismatch",
                            ));
                        }
                        if *writable {
                            if !writes.insert(*id) {
                                return Err(invalid("duplicate writable buffer binding"));
                            }
                        } else {
                            reads.insert(*id);
                        }
                    }
                    _ => return Err(invalid("compute resource kind mismatch")),
                }
            }
        }
    }
    if reads != pass.reads.iter().copied().collect()
        || writes != pass.writes.iter().copied().collect()
    {
        return Err(invalid(
            "reflected resource accesses must exactly match declared graph reads/writes",
        ));
    }
    if reads.iter().any(|id| writes.contains(id)) {
        return Err(RenderTargetError::FeedbackLoop.into());
    }
    Ok(bytes)
}

fn submit_program(
    renderer: &mut impl GpuGraphRenderer,
    program: &GpuGraphPass,
    outputs: &[ResourceId],
    resources: &BTreeMap<ResourceId, GpuGraphResource>,
) -> Result<(), RenderTargetError> {
    let wrong_type = || RenderTargetError::InvalidBindings("graph resource kind mismatch".into());
    match program {
        GpuGraphPass::Fragment(p) => {
            let Some(GpuGraphResource::Texture(target)) = resources.get(&outputs[0]) else {
                return Err(wrong_type());
            };
            let mut bindings = ShaderBindings::new();
            for (&binding, value) in &p.bindings {
                let value = match value {
                    GpuGraphBinding::Uniform(bytes) => ShaderBinding::Uniform(bytes.clone()),
                    GpuGraphBinding::Sampler(sampler) => ShaderBinding::Sampler(*sampler),
                    GpuGraphBinding::Texture(id) => {
                        let Some(GpuGraphResource::Texture(t)) = resources.get(id) else {
                            return Err(wrong_type());
                        };
                        ShaderBinding::Texture(t.clone())
                    }
                    GpuGraphBinding::Buffer(_) => return Err(wrong_type()),
                };
                bindings = bindings.with(binding, value);
            }
            renderer.render(target, &p.shader, &bindings)
        }
        GpuGraphPass::Compute(p) => {
            let mut bindings = ComputeBindings::new();
            for resource in p.shader.resources() {
                let value = match &p.bindings[&resource.binding] {
                    GpuGraphBinding::Uniform(bytes) => ComputeBinding::Uniform(bytes.clone()),
                    GpuGraphBinding::Sampler(sampler) => ComputeBinding::Sampler(*sampler),
                    GpuGraphBinding::Texture(id) => {
                        let Some(GpuGraphResource::Texture(t)) = resources.get(id) else {
                            return Err(wrong_type());
                        };
                        if matches!(resource.kind, ComputeResourceKind::StorageTexture(_)) {
                            ComputeBinding::StorageTexture(t.clone())
                        } else {
                            ComputeBinding::Texture(t.clone())
                        }
                    }
                    GpuGraphBinding::Buffer(id) => {
                        let Some(GpuGraphResource::Buffer(b)) = resources.get(id) else {
                            return Err(wrong_type());
                        };
                        ComputeBinding::StorageBuffer(b.clone())
                    }
                };
                bindings = bindings.with(resource.binding, value);
            }
            renderer.dispatch(&p.shader, &bindings, p.groups)
        }
    }
}

pub(crate) trait GpuGraphRenderer {
    fn context_id(&self) -> u64;
    fn validate(&self, target: &RenderTarget) -> Result<(), RenderTargetError>;
    fn create(
        &mut self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget, RenderTargetError>;
    fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<(), RenderTargetError>;
    fn validate_buffer(&self, _buffer: &GpuBuffer) -> Result<(), RenderTargetError> {
        Err(RenderTargetError::Unsupported("storage buffers"))
    }
    fn create_buffer(
        &mut self,
        _descriptor: GpuBufferDescriptor,
    ) -> Result<GpuBuffer, RenderTargetError> {
        Err(RenderTargetError::Unsupported("storage buffers"))
    }
    fn dispatch(
        &mut self,
        _shader: &ComputeHandle,
        _bindings: &ComputeBindings,
        _groups: [u32; 3],
    ) -> Result<(), RenderTargetError> {
        Err(RenderTargetError::Unsupported("compute dispatch"))
    }
}

impl GpuGraphRenderer for Window {
    fn context_id(&self) -> u64 {
        self.window_handle().window_id().as_u64()
    }
    fn validate(&self, target: &RenderTarget) -> Result<(), RenderTargetError> {
        self.validate_render_target(target)
    }
    fn create(
        &mut self,
        descriptor: RenderTargetDescriptor,
    ) -> Result<RenderTarget, RenderTargetError> {
        self.create_render_target(descriptor)
    }
    fn render(
        &mut self,
        target: &RenderTarget,
        shader: &ShaderHandle,
        bindings: &ShaderBindings,
    ) -> Result<(), RenderTargetError> {
        self.render_shader(target, shader, bindings)
    }
    fn validate_buffer(&self, buffer: &GpuBuffer) -> Result<(), RenderTargetError> {
        self.validate_gpu_buffer(buffer)
    }
    fn create_buffer(
        &mut self,
        descriptor: GpuBufferDescriptor,
    ) -> Result<GpuBuffer, RenderTargetError> {
        self.create_gpu_buffer(descriptor)
    }
    fn dispatch(
        &mut self,
        shader: &ComputeHandle,
        bindings: &ComputeBindings,
        groups: [u32; 3],
    ) -> Result<(), RenderTargetError> {
        self.dispatch_compute(shader, bindings, groups)
    }
}

impl Window {
    /// Execute a declared mixed fragment/compute DAG with typed imports and
    /// buffer/texture exports. WebGL2 rejects compute before graph allocation.
    pub fn execute_gpu_graph(
        &mut self,
        executor: &mut GpuRenderGraph,
        imports: &[(ResourceId, GpuGraphResource)],
        programs: &[(PassId, GpuGraphPass)],
    ) -> Result<GpuGraphExecution, GpuGraphError> {
        executor.execute_programs_with(self, imports, programs)
    }
    /// Execute a declared fragment DAG on this window's GPU, without CPU pixel
    /// copies. Resource/binding/byte admission is checked before submission.
    /// Failed backend work may leave a submitted prefix; its cache is invalidated.
    pub fn execute_render_graph(
        &mut self,
        executor: &mut GpuRenderGraph,
        imports: &[(ResourceId, RenderTarget)],
        programs: &[(PassId, GpuFragmentPass)],
    ) -> Result<GpuGraphExecution, GpuGraphError> {
        executor.execute_with(self, imports, programs)
    }
}
