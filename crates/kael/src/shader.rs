//! Portable, bounded WGSL fragment programs and reflected resource layouts.
//!
//! Enable `custom-shaders` to register programs with
//! [`crate::App::register_fragment_shader`]. Programs take optional normalized
//! top-left UVs at location zero and/or device-pixel fragment position. Return
//! straight **linear** RGBA at location zero; target storage is premultiplied.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use naga::{AddressSpace, Binding, Handle, ScalarKind, ShaderStage, Type, TypeInner};
use naga_shader as naga;

const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_PROGRAM_BYTES: usize = 2 * 1024 * 1024;
const MAX_REGISTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROGRAMS: usize = 128;
/// Maximum loop-body executions across all loops and called functions in one
/// shader invocation. At exhaustion each remaining loop exits, then ordinary
/// control flow continues. Algorithms requiring more work must split it across
/// passes. This bound does not promise a GPU wall-clock deadline.
pub const SHADER_MAX_LOOP_BODY_EXECUTIONS: u32 = 65_536;
const VERTEX_ENTRY: &str = "kael_fullscreen_vertex";
const FULLSCREEN_VERTEX: &str = r#"
struct KaelFullscreenOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex
fn kael_fullscreen_vertex(@builtin(vertex_index) index: u32) -> KaelFullscreenOutput {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var output: KaelFullscreenOutput;
    output.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    output.uv = vec2<f32>(x, y);
    return output;
}
"#;

/// Renderer language used for a validated program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShaderBackend {
    /// Metal Shading Language 2.0.
    Metal,
    /// HLSL Shader Model 5.0 for Direct3D 11.
    DirectX11,
    /// WGSL for Blade's native GPU backend.
    Blade,
    /// GLSL ES 3.00 for WebGL 2.
    WebGl2,
}

impl ShaderBackend {
    fn index(self) -> usize {
        match self {
            Self::Metal => 0,
            Self::DirectX11 => 1,
            Self::Blade => 2,
            Self::WebGl2 => 3,
        }
    }
}

/// Authored WGSL containing exactly one fragment entry point.
///
/// Registration bounds total loop-body executions per invocation to
/// [`SHADER_MAX_LOOP_BODY_EXECUTIONS`]. Work beyond this shared counter is
/// truncated by exiting loops, including nested loops and helper functions.
#[derive(Clone, Debug)]
pub struct ShaderDescriptor {
    /// Diagnostic label, at most 256 bytes.
    pub label: String,
    /// WGSL source, at most 256 KiB.
    pub source: String,
    /// Name of the fragment entry point in `source`.
    pub entry_point: String,
}

impl ShaderDescriptor {
    /// Describe a fragment program. Validation occurs during registration.
    pub fn fragment(
        label: impl Into<String>,
        source: impl Into<String>,
        entry_point: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            source: source.into(),
            entry_point: entry_point.into(),
        }
    }
}

/// A host-shareable 32-bit scalar in a uniform buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderUniformScalar {
    /// IEEE 754 single-precision float.
    F32,
    /// Signed 32-bit integer.
    I32,
    /// Unsigned 32-bit integer.
    U32,
}

/// A recursively reflected WGSL uniform type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderUniformType {
    /// One 32-bit value.
    Scalar(ShaderUniformScalar),
    /// A vector with two, three, or four lanes.
    Vector {
        /// Lane type.
        scalar: ShaderUniformScalar,
        /// Number of lanes.
        lanes: u32,
    },
    /// A column-major floating-point matrix.
    Matrix {
        /// Column count.
        columns: u32,
        /// Row count.
        rows: u32,
        /// Bytes between columns.
        column_stride: u32,
    },
    /// A fixed-length array, with padding included in `stride`.
    Array {
        /// Number of elements.
        length: u32,
        /// Bytes between elements.
        stride: u32,
        /// Element layout.
        element: Box<ShaderUniformType>,
    },
    /// A nested structure. Member offsets are relative to this structure.
    Struct(Vec<ShaderUniformMember>),
}

/// One field of a reflected uniform structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderUniformMember {
    /// Authored WGSL member name.
    pub name: String,
    /// Offset in bytes from the start of the containing structure.
    pub offset: u32,
    /// Size of the member type, excluding following structure padding.
    pub size: u32,
    /// Required byte alignment of the member type.
    pub alignment: u32,
    /// Type and any nested members/strides.
    pub ty: ShaderUniformType,
}

/// Exact layout of a uniform resource, including trailing padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderUniformLayout {
    /// Required binding byte count.
    pub size: u32,
    /// Required WGSL alignment, in bytes.
    pub alignment: u32,
    /// Structure members, empty for a scalar/vector/matrix root.
    pub members: Vec<ShaderUniformMember>,
}

/// Resource types supported by portable fragment programs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderResourceKind {
    /// A read-only host-shareable uniform buffer, at most 64 KiB.
    Uniform(ShaderUniformLayout),
    /// A sampled, non-array, non-multisampled 2D floating-point texture.
    Texture2d,
    /// A non-comparison sampler.
    Sampler,
}

/// One group-zero resource declared by the author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderResourceBinding {
    /// Original WGSL `@binding` number.
    pub binding: u32,
    /// Original global variable name.
    pub name: String,
    /// Resource type and reflected uniform layout.
    pub kind: ShaderResourceKind,
}

/// Native binding namespace, with its own dense slot numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderResourceSlotKind {
    /// Uniform/constant buffers.
    Uniform,
    /// Sampled textures.
    Texture,
    /// Samplers.
    Sampler,
}

/// A mapping from authored binding to a renderer resource slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderResourceSlot {
    /// Original WGSL `@binding` number.
    pub binding: u32,
    /// Dense index within `kind`, independent of other resource namespaces.
    pub slot: u32,
    /// Native resource namespace.
    pub kind: ShaderResourceSlotKind,
    /// GLSL uniform block name, or authored name on native backends.
    pub name: String,
}

/// A WebGL combined sampler uniform generated from a WGSL texture use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderTextureSamplerPair {
    /// Authored texture binding number.
    pub texture_binding: u32,
    /// Authored sampler binding, absent for texture loads.
    pub sampler_binding: Option<u32>,
    /// Exact generated GLSL sampler uniform name.
    pub uniform_name: String,
}

/// Translated source and binding metadata ready for a renderer compiler.
#[derive(Clone, Debug)]
pub struct ShaderTranslation {
    /// Combined native source; GLSL fragment source for WebGL.
    pub source: String,
    /// Separate GLSL vertex source; `None` for combined native modules.
    pub vertex_source: Option<String>,
    /// Translated vertex entry name (`main` for GLSL).
    pub vertex_entry: String,
    /// Translated fragment entry name (`main` for GLSL).
    pub fragment_entry: String,
    /// Dense renderer slots, with GLSL block names when applicable.
    pub resources: Vec<ShaderResourceSlot>,
    /// Combined sampler mappings for WebGL; empty on native backends.
    pub texture_sampler_pairs: Vec<ShaderTextureSamplerPair>,
}

impl ShaderTranslation {
    fn retained_bytes(&self) -> usize {
        self.source.len()
            + self.vertex_source.as_ref().map_or(0, String::len)
            + self.vertex_entry.len()
            + self.fragment_entry.len()
            + self
                .resources
                .iter()
                .map(|r| std::mem::size_of::<ShaderResourceSlot>() + r.name.len())
                .sum::<usize>()
            + self
                .texture_sampler_pairs
                .iter()
                .map(|r| std::mem::size_of::<ShaderTextureSamplerPair>() + r.uniform_name.len())
                .sum::<usize>()
    }
}

/// Validation, portability, or source-registry admission failure.
#[derive(Debug, thiserror::Error)]
pub enum ShaderError {
    /// Invalid WGSL syntax or semantics, with source locations when available.
    #[error("invalid WGSL: {0}")]
    Validation(String),
    /// An interface/resource/layout is outside the supported portable contract.
    #[error("unsupported shader interface: {0}")]
    Unsupported(String),
    /// A backend cannot translate a validated construct.
    #[error("shader translation for {backend:?} failed: {message}")]
    Translation {
        /// Backend that rejected the program.
        backend: ShaderBackend,
        /// Compiler diagnostic.
        message: String,
    },
    /// Source, reflected layout, program count, or retained byte cap was exceeded.
    #[error("shader resource limit exceeded: {0}")]
    ResourceLimit(&'static str),
}

#[derive(Debug)]
struct ShaderProgram {
    live: Arc<()>,
    id: u64,
    label: String,
    source: String,
    fragment_entry: String,
    resources: Vec<ShaderResourceBinding>,
    translations: [ShaderTranslation; 4],
    retained_bytes: usize,
}

/// A shareable validated fragment program. GPU pipelines remain device-owned.
///
/// Registration translates all supported languages before the handle is
/// admitted. Dropping the last clone releases source/translation storage;
/// the registry keeps only weak references. Windows separately bound their
/// device pipeline and target storage.
#[derive(Clone, Debug)]
pub struct ShaderHandle(Arc<ShaderProgram>);

impl ShaderHandle {
    /// Stable program identity for device pipeline caches.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// Diagnostic label.
    pub fn label(&self) -> &str {
        &self.0.label
    }
    /// Validated WGSL, including Kael's generated full-screen vertex stage.
    pub fn source(&self) -> &str {
        &self.0.source
    }
    /// Generated WGSL vertex entry name.
    pub fn vertex_entry(&self) -> &str {
        VERTEX_ENTRY
    }
    /// Authored WGSL fragment entry name.
    pub fn fragment_entry(&self) -> &str {
        &self.0.fragment_entry
    }
    /// Resources, sorted by original WGSL binding number.
    pub fn resources(&self) -> &[ShaderResourceBinding] {
        &self.0.resources
    }
    /// Already validated renderer translation. The returned source is owned.
    pub fn translate(&self, backend: ShaderBackend) -> Result<ShaderTranslation, ShaderError> {
        Ok(self.0.translations[backend.index()].clone())
    }

    pub(crate) fn compile_fragment(descriptor: ShaderDescriptor) -> Result<Self, ShaderError> {
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.prune();
        if registry.programs.len() >= MAX_PROGRAMS {
            return Err(ShaderError::ResourceLimit("128 live programs"));
        }
        // Bound source bytes before parsing or translation, and serialize
        // registration so parallel callers cannot bypass the admission limit.
        if descriptor.source.len() > MAX_SOURCE_BYTES
            || descriptor.label.len() > 256
            || descriptor.entry_point.len() > 256
        {
            return Err(ShaderError::ResourceLimit("source or label length"));
        }
        if registry
            .retained_bytes
            .saturating_add(descriptor.source.len())
            > MAX_REGISTRY_BYTES
        {
            return Err(ShaderError::ResourceLimit(
                "16 MiB retained program storage",
            ));
        }
        let program = compile(descriptor)?;
        if registry
            .retained_bytes
            .saturating_add(program.retained_bytes)
            > MAX_REGISTRY_BYTES
        {
            return Err(ShaderError::ResourceLimit(
                "16 MiB retained program storage",
            ));
        }
        let program = Arc::new(program);
        registry.retained_bytes += program.retained_bytes;
        registry.programs.insert(
            program.id,
            (Arc::downgrade(&program.live), program.retained_bytes),
        );
        Ok(Self(program))
    }
}

#[derive(Default)]
struct ShaderRegistry {
    programs: BTreeMap<u64, (Weak<()>, usize)>,
    retained_bytes: usize,
}

impl ShaderRegistry {
    fn prune(&mut self) {
        self.programs
            .retain(|_, (program, _)| program.strong_count() != 0);
        self.retained_bytes = self.programs.values().map(|(_, bytes)| *bytes).sum();
    }
}

static REGISTRY: LazyLock<Mutex<ShaderRegistry>> =
    LazyLock::new(|| Mutex::new(ShaderRegistry::default()));
static NEXT_PROGRAM_ID: AtomicU64 = AtomicU64::new(1);

/// Compute and fragment registrations share one source/pipeline admission cap.
pub(crate) fn register_compute(
    descriptor: crate::ComputeDescriptor,
) -> Result<crate::ComputeHandle, ShaderError> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    registry.prune();
    if registry.programs.len() >= MAX_PROGRAMS {
        return Err(ShaderError::ResourceLimit("128 live programs"));
    }
    if descriptor.source.len() > MAX_SOURCE_BYTES
        || descriptor.label.len() > 256
        || descriptor.entry_point.len() > 256
    {
        return Err(ShaderError::ResourceLimit("source or label length"));
    }
    if registry
        .retained_bytes
        .saturating_add(descriptor.source.len())
        > MAX_REGISTRY_BYTES
    {
        return Err(ShaderError::ResourceLimit(
            "16 MiB retained program storage",
        ));
    }
    let handle = crate::compute::compile(descriptor)?;
    if handle.retained_bytes() > MAX_PROGRAM_BYTES
        || registry
            .retained_bytes
            .saturating_add(handle.retained_bytes())
            > MAX_REGISTRY_BYTES
    {
        return Err(ShaderError::ResourceLimit("retained program storage"));
    }
    registry.retained_bytes += handle.retained_bytes();
    registry.programs.insert(
        handle.id(),
        (Arc::downgrade(handle.live()), handle.retained_bytes()),
    );
    Ok(handle)
}

pub(crate) fn next_program_id() -> Result<u64, ShaderError> {
    NEXT_PROGRAM_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| ShaderError::ResourceLimit("program identity exhausted"))
}

impl crate::App {
    /// Validate/register portable WGSL without allocating GPU resources.
    ///
    /// The contract supports at most four 64 KiB uniform buffers, eight sampled
    /// 2D textures and eight samplers, all in group zero. No storage resources,
    /// overrides, extra stages, or resource arrays are silently discarded.
    /// Syntax, semantics, entry interfaces, layouts, all translations and
    /// registry admission are checked before returning a handle.
    pub fn register_fragment_shader(
        &mut self,
        descriptor: ShaderDescriptor,
    ) -> Result<ShaderHandle, ShaderError> {
        ShaderHandle::compile_fragment(descriptor)
    }
}

fn compile(descriptor: ShaderDescriptor) -> Result<ShaderProgram, ShaderError> {
    let authored = naga::front::wgsl::parse_str(&descriptor.source)
        .map_err(|error| ShaderError::Validation(error.emit_to_string(&descriptor.source)))?;
    if authored.entry_points.len() != 1
        || authored.entry_points[0].stage != ShaderStage::Fragment
        || authored.entry_points[0].name != descriptor.entry_point
    {
        return Err(ShaderError::Unsupported(
            "exactly one named fragment entry point is required".into(),
        ));
    }
    if !authored.overrides.is_empty() {
        return Err(ShaderError::Unsupported(
            "pipeline overrides are not supported".into(),
        ));
    }
    if authored.global_variables.iter().any(|(_, global)| {
        global
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("kael_"))
    }) || authored.functions.iter().any(|(_, function)| {
        function
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("kael_"))
    }) {
        return Err(ShaderError::Unsupported(
            "the kael_ identifier prefix is reserved".into(),
        ));
    }
    if authored.types.iter().any(|(_, ty)| {
        ty.name
            .as_deref()
            .is_some_and(|name| name.starts_with("kael_") || name.starts_with("Kael"))
    }) {
        return Err(ShaderError::Unsupported(
            "the kael_ and Kael type namespaces are reserved".into(),
        ));
    }
    check_interface(&authored, &authored.entry_points[0])?;
    let source = format!("{}\n{}", descriptor.source, FULLSCREEN_VERTEX);
    let mut module = naga::front::wgsl::parse_str(&source)
        .map_err(|error| ShaderError::Validation(error.emit_to_string(&source)))?;
    bound_loops(&mut module);
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| ShaderError::Validation(error.emit_to_string(&source)))?;
    let resources = reflect_resources(&module)?;
    let translations = [
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            &source,
            ShaderBackend::Metal,
        )?,
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            &source,
            ShaderBackend::DirectX11,
        )?,
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            &source,
            ShaderBackend::Blade,
        )?,
        translate(
            &module,
            &info,
            &resources,
            &descriptor.entry_point,
            &source,
            ShaderBackend::WebGl2,
        )?,
    ];
    let retained_bytes = source.len() + descriptor.label.len() + descriptor.entry_point.len()
        + translations.iter().map(ShaderTranslation::retained_bytes).sum::<usize>()
        // Reflection depth/count is bounded by the parsed source. Include its
        // names, recursive boxes and vector storage conservatively.
        + descriptor.source.len().saturating_mul(4);
    if retained_bytes > MAX_PROGRAM_BYTES {
        return Err(ShaderError::ResourceLimit("2 MiB per program"));
    }
    let id = NEXT_PROGRAM_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| ShaderError::ResourceLimit("program identity exhausted"))?;
    Ok(ShaderProgram {
        live: Arc::new(()),
        id,
        label: descriptor.label,
        source,
        fragment_entry: descriptor.entry_point,
        resources,
        translations,
        retained_bytes,
    })
}

/// Bound the total number of loop body executions across all called functions
/// in one shader invocation. This limits authored loop work, not GPU wall time.
/// Exhaustion exits each remaining loop and continues ordinary shader control
/// flow. Nested loops share the same counter, so their limits cannot multiply.
pub(crate) fn bound_loops(module: &mut naga::Module) {
    fn has_loop(block: &naga::Block) -> bool {
        block.iter().any(|s| match s {
            naga::Statement::Loop { .. } => true,
            naga::Statement::Block(b) => has_loop(b),
            naga::Statement::If { accept, reject, .. } => has_loop(accept) || has_loop(reject),
            naga::Statement::Switch { cases, .. } => cases.iter().any(|c| has_loop(&c.body)),
            _ => false,
        })
    }
    if !module.functions.iter().any(|(_, f)| has_loop(&f.body))
        && !module
            .entry_points
            .iter()
            .any(|e| has_loop(&e.function.body))
    {
        return;
    }
    let ty = module.types.insert(
        naga::Type {
            name: None,
            inner: naga::TypeInner::Scalar(naga::Scalar::U32),
        },
        naga::Span::UNDEFINED,
    );
    let counter = module.global_variables.append(
        naga::GlobalVariable {
            name: Some("kael_loop_budget".into()),
            space: AddressSpace::Private,
            binding: None,
            ty,
            init: None,
        },
        naga::Span::UNDEFINED,
    );
    fn instrument(
        block: &mut naga::Block,
        expressions: &mut naga::Arena<naga::Expression>,
        counter: naga::Handle<naga::GlobalVariable>,
    ) {
        for statement in block.iter_mut() {
            match statement {
                naga::Statement::Loop {
                    body, continuing, ..
                } => {
                    instrument(body, expressions, counter);
                    instrument(continuing, expressions, counter);
                    let append = |expressions: &mut naga::Arena<naga::Expression>, expression| {
                        expressions.append(expression, naga::Span::UNDEFINED)
                    };
                    let pointer = append(expressions, naga::Expression::GlobalVariable(counter));
                    let limit = append(
                        expressions,
                        naga::Expression::Literal(naga::Literal::U32(
                            SHADER_MAX_LOOP_BODY_EXECUTIONS,
                        )),
                    );
                    let one = append(
                        expressions,
                        naga::Expression::Literal(naga::Literal::U32(1)),
                    );
                    let value = append(expressions, naga::Expression::Load { pointer });
                    let condition = append(
                        expressions,
                        naga::Expression::Binary {
                            op: naga::BinaryOperator::GreaterEqual,
                            left: value,
                            right: limit,
                        },
                    );
                    let increment = append(
                        expressions,
                        naga::Expression::Binary {
                            op: naga::BinaryOperator::Add,
                            left: value,
                            right: one,
                        },
                    );
                    body.splice(
                        0..0,
                        naga::Block::from_vec(vec![
                            naga::Statement::Emit(naga::Range::new_from_bounds(value, condition)),
                            naga::Statement::If {
                                condition,
                                accept: naga::Block::from_vec(vec![naga::Statement::Break]),
                                reject: naga::Block::new(),
                            },
                            naga::Statement::Emit(naga::Range::new_from_bounds(
                                increment, increment,
                            )),
                            naga::Statement::Store {
                                pointer,
                                value: increment,
                            },
                        ]),
                    );
                }
                naga::Statement::Block(b) => instrument(b, expressions, counter),
                naga::Statement::If { accept, reject, .. } => {
                    instrument(accept, expressions, counter);
                    instrument(reject, expressions, counter);
                }
                naga::Statement::Switch { cases, .. } => {
                    for case in cases {
                        instrument(&mut case.body, expressions, counter);
                    }
                }
                _ => {}
            }
        }
    }
    for (_, function) in module.functions.iter_mut() {
        instrument(&mut function.body, &mut function.expressions, counter);
    }
    for entry in &mut module.entry_points {
        instrument(
            &mut entry.function.body,
            &mut entry.function.expressions,
            counter,
        );
    }
}

fn check_interface(module: &naga::Module, entry: &naga::EntryPoint) -> Result<(), ShaderError> {
    fn check_input(module: &naga::Module, ty: Handle<Type>, binding: Option<&Binding>) -> bool {
        match (binding, &module.types[ty].inner) {
            (
                Some(Binding::Location {
                    location: 0,
                    interpolation: Some(naga::Interpolation::Perspective),
                    sampling: Some(naga::Sampling::Center),
                    second_blend_source: false,
                }),
                TypeInner::Vector {
                    size: naga::VectorSize::Bi,
                    scalar,
                },
            ) => scalar.kind == ScalarKind::Float && scalar.width == 4,
            (
                Some(Binding::BuiltIn(naga::BuiltIn::Position { .. })),
                TypeInner::Vector {
                    size: naga::VectorSize::Quad,
                    scalar,
                },
            ) => scalar.kind == ScalarKind::Float && scalar.width == 4,
            (None, TypeInner::Struct { members, .. }) => members
                .iter()
                .all(|member| check_input(module, member.ty, member.binding.as_ref())),
            _ => false,
        }
    }
    fn check_output(module: &naga::Module, ty: Handle<Type>, binding: Option<&Binding>) -> bool {
        match (binding, &module.types[ty].inner) {
            (
                Some(Binding::Location {
                    location: 0,
                    second_blend_source: false,
                    ..
                }),
                TypeInner::Vector {
                    size: naga::VectorSize::Quad,
                    scalar,
                },
            ) => scalar.kind == ScalarKind::Float && scalar.width == 4,
            (None, TypeInner::Struct { members, .. }) if members.len() == 1 => {
                check_output(module, members[0].ty, members[0].binding.as_ref())
            }
            _ => false,
        }
    }
    if !entry
        .function
        .arguments
        .iter()
        .all(|argument| check_input(module, argument.ty, argument.binding.as_ref()))
    {
        return Err(ShaderError::Unsupported(
            "fragment inputs must be @location(0) vec2<f32> UV or @builtin(position) vec4<f32>"
                .into(),
        ));
    }
    if !entry
        .function
        .result
        .as_ref()
        .is_some_and(|result| check_output(module, result.ty, result.binding.as_ref()))
    {
        return Err(ShaderError::Unsupported(
            "fragment output must be one @location(0) vec4<f32>".into(),
        ));
    }
    Ok(())
}

fn reflect_resources(module: &naga::Module) -> Result<Vec<ShaderResourceBinding>, ShaderError> {
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .map_err(|error| ShaderError::Validation(error.to_string()))?;
    let mut resources = Vec::new();
    let mut counts = [0usize; 3];
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = global.binding.as_ref() else {
            if matches!(global.space, AddressSpace::Private) {
                continue;
            }
            return Err(ShaderError::Unsupported(
                "only private globals and bound resources are supported".into(),
            ));
        };
        if binding.group != 0 {
            return Err(ShaderError::Unsupported(
                "resources must use @group(0)".into(),
            ));
        }
        let kind = match (global.space, &module.types[global.ty].inner) {
            (AddressSpace::Uniform, _) => {
                counts[0] += 1;
                let layout = layouter[global.ty];
                if layout.size == 0 || layout.size > 65_536 { return Err(ShaderError::ResourceLimit("uniform buffer size")); }
                let ty = reflect_uniform(module, &layouter, global.ty, 0)?;
                // The portable binding contract requires the same byte layout
                // on WebGL std140, WGSL, Metal, and D3D constant buffers.
                check_std140(module, &layouter, global.ty)?;
                let members = match ty { ShaderUniformType::Struct(members) => members, _ => Vec::new() };
                ShaderResourceKind::Uniform(ShaderUniformLayout { size: layout.size, alignment: layout.alignment * 1, members })
            }
            (AddressSpace::Handle, TypeInner::Image { dim: naga::ImageDimension::D2, arrayed: false, class: naga::ImageClass::Sampled { kind: ScalarKind::Float, multi: false } }) => { counts[1] += 1; ShaderResourceKind::Texture2d }
            (AddressSpace::Handle, TypeInner::Sampler { comparison: false }) => { counts[2] += 1; ShaderResourceKind::Sampler }
            _ => return Err(ShaderError::Unsupported("fragment bindings support only uniform buffers, sampled float 2D textures, and ordinary samplers".into())),
        };
        resources.push(ShaderResourceBinding {
            binding: binding.binding,
            name: global
                .name
                .clone()
                .unwrap_or_else(|| format!("binding_{}", binding.binding)),
            kind,
        });
    }
    if counts[0] > 4 || counts[1] > 8 || counts[2] > 8 {
        return Err(ShaderError::ResourceLimit(
            "four uniforms, eight textures, eight samplers",
        ));
    }
    resources.sort_by_key(|resource| resource.binding);
    Ok(resources)
}

fn scalar_type(scalar: naga::Scalar) -> Result<ShaderUniformScalar, ShaderError> {
    match (scalar.kind, scalar.width) {
        (ScalarKind::Float, 4) => Ok(ShaderUniformScalar::F32),
        (ScalarKind::Sint, 4) => Ok(ShaderUniformScalar::I32),
        (ScalarKind::Uint, 4) => Ok(ShaderUniformScalar::U32),
        _ => Err(ShaderError::Unsupported(
            "uniform values must use host-shareable 32-bit scalar types".into(),
        )),
    }
}

pub(crate) fn reflect_uniform(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    ty: Handle<Type>,
    depth: usize,
) -> Result<ShaderUniformType, ShaderError> {
    if depth > 16 {
        return Err(ShaderError::ResourceLimit("uniform nesting depth"));
    }
    Ok(match &module.types[ty].inner {
        TypeInner::Scalar(scalar) => ShaderUniformType::Scalar(scalar_type(*scalar)?),
        TypeInner::Vector { size, scalar } => ShaderUniformType::Vector {
            scalar: scalar_type(*scalar)?,
            lanes: *size as u32,
        },
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } if *scalar == naga::Scalar::F32 => ShaderUniformType::Matrix {
            columns: *columns as u32,
            rows: *rows as u32,
            column_stride: (naga::proc::Alignment::from(*rows)
                * naga::proc::Alignment::from_width(scalar.width))
            .round_up(*rows as u32 * u32::from(scalar.width)),
        },
        TypeInner::Array {
            base,
            size: naga::ArraySize::Constant(length),
            stride,
        } => ShaderUniformType::Array {
            length: length.get(),
            stride: *stride,
            element: Box::new(reflect_uniform(module, layouter, *base, depth + 1)?),
        },
        TypeInner::Struct { members, .. } => ShaderUniformType::Struct(
            members
                .iter()
                .map(|member| {
                    let layout = layouter[member.ty];
                    Ok(ShaderUniformMember {
                        name: member.name.clone().unwrap_or_default(),
                        offset: member.offset,
                        size: layout.size,
                        alignment: layout.alignment * 1,
                        ty: reflect_uniform(module, layouter, member.ty, depth + 1)?,
                    })
                })
                .collect::<Result<_, ShaderError>>()?,
        ),
        _ => {
            return Err(ShaderError::Unsupported(
                "uniform type is not host-shareable or fixed-size".into(),
            ));
        }
    })
}

// Reject layouts for which GLSL std140 would silently change the host bytes.
// Explicit WGSL @align/@size padding remains supported when offsets match.
pub(crate) fn check_std140(
    module: &naga::Module,
    layouter: &naga::proc::Layouter,
    ty: Handle<Type>,
) -> Result<(), ShaderError> {
    fn compatible(
        module: &naga::Module,
        layouter: &naga::proc::Layouter,
        ty: Handle<Type>,
    ) -> bool {
        match &module.types[ty].inner {
            TypeInner::Scalar(_) | TypeInner::Vector { .. } => true,
            TypeInner::Matrix { rows, .. } => *rows != naga::VectorSize::Bi,
            TypeInner::Array { base, stride, .. } => {
                *stride % 16 == 0 && compatible(module, layouter, *base)
            }
            TypeInner::Struct { members, span } => {
                if *span % 16 != 0 {
                    return false;
                }
                let mut end = 0;
                for member in members {
                    let layout = layouter[member.ty];
                    let alignment = if matches!(
                        module.types[member.ty].inner,
                        TypeInner::Struct { .. }
                            | TypeInner::Array { .. }
                            | TypeInner::Matrix { .. }
                    ) {
                        (layout.alignment * 1).max(16)
                    } else {
                        layout.alignment * 1
                    };
                    let offset = (end + alignment - 1) & !(alignment - 1);
                    // Naga's GLSL backend emits no authored @align/@size padding.
                    if offset != member.offset || !compatible(module, layouter, member.ty) {
                        return false;
                    }
                    end = member.offset + layout.size;
                }
                (end + 15) & !15 == *span
            }
            _ => false,
        }
    }
    if !compatible(module, layouter, ty) || !layouter[ty].size.is_multiple_of(16) {
        return Err(ShaderError::Unsupported("uniform layout must match std140 on all backends (use 16-byte struct/array padding and matrices with three or four rows)".into()));
    }
    Ok(())
}

fn slots(resources: &[ShaderResourceBinding]) -> Vec<ShaderResourceSlot> {
    let mut next = [0; 3];
    resources
        .iter()
        .map(|resource| {
            let (kind, index) = match resource.kind {
                ShaderResourceKind::Uniform(_) => (ShaderResourceSlotKind::Uniform, 0),
                ShaderResourceKind::Texture2d => (ShaderResourceSlotKind::Texture, 1),
                ShaderResourceKind::Sampler => (ShaderResourceSlotKind::Sampler, 2),
            };
            let slot = next[index];
            next[index] += 1;
            ShaderResourceSlot {
                binding: resource.binding,
                slot,
                kind,
                name: resource.name.clone(),
            }
        })
        .collect()
}

fn translate(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    resources: &[ShaderResourceBinding],
    fragment: &str,
    _wgsl: &str,
    backend: ShaderBackend,
) -> Result<ShaderTranslation, ShaderError> {
    let failure = |error: String| ShaderError::Translation {
        backend,
        message: error,
    };
    let mut result = ShaderTranslation {
        source: String::new(),
        vertex_source: None,
        vertex_entry: VERTEX_ENTRY.into(),
        fragment_entry: fragment.into(),
        resources: slots(resources),
        texture_sampler_pairs: Vec::new(),
    };
    let vertex_index = module
        .entry_points
        .iter()
        .position(|entry| entry.name == VERTEX_ENTRY)
        .unwrap();
    let fragment_index = module
        .entry_points
        .iter()
        .position(|entry| entry.name == fragment)
        .unwrap();
    match backend {
        ShaderBackend::Blade => {
            // Blade's dynamic buffers use read-only storage transport. Rename
            // resources into a fixed namespace so layouts borrow static names
            // without leaking strings, while public bindings remain authored.
            let mut blade_module = module.clone();
            for resource in &mut result.resources {
                let prefix = match resource.kind {
                    ShaderResourceSlotKind::Uniform => "uniform",
                    ShaderResourceSlotKind::Texture => "texture",
                    ShaderResourceSlotKind::Sampler => "sampler",
                };
                resource.name = format!("kael_{prefix}_{}", char::from(b'a' + resource.slot as u8));
                for (_, global) in blade_module.global_variables.iter_mut() {
                    if global.binding.as_ref().is_some_and(|binding| {
                        binding.group == 0 && binding.binding == resource.binding
                    }) {
                        global.name = Some(resource.name.clone());
                        if global.space == AddressSpace::Uniform {
                            global.space = AddressSpace::Storage {
                                access: naga::StorageAccess::LOAD,
                            };
                        }
                    }
                }
            }
            let pointers = blade_module
                .types
                .iter()
                .filter_map(|(handle, ty)| {
                    if let TypeInner::Pointer {
                        base,
                        space: AddressSpace::Uniform,
                    } = ty.inner
                    {
                        let mut ty = ty.clone();
                        ty.inner = TypeInner::Pointer {
                            base,
                            space: AddressSpace::Storage {
                                access: naga::StorageAccess::LOAD,
                            },
                        };
                        // Keep distinct arena identities, including any original
                        // authored read-only storage pointer (normally rejected).
                        ty.name = Some(format!("KaelReadonlyPointer{}", handle.index()));
                        Some((handle, ty))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for (handle, ty) in pointers {
                blade_module.types.replace(handle, ty);
            }
            // Blade assigns bindings from its static named data layout. It
            // requires unbound globals, while the public descriptor retains
            // the author bindings checked above. Validate every other semantic
            // rule after clearing only this backend's binding decorations.
            for (_, global) in blade_module.global_variables.iter_mut() {
                global.binding = None;
            }
            let blade_info = naga::valid::Validator::new(
                naga::valid::ValidationFlags::all() - naga::valid::ValidationFlags::BINDINGS,
                naga::valid::Capabilities::empty(),
            )
            .validate(&blade_module)
            .map_err(|error| failure(error.to_string()))?;
            result.source = naga::back::wgsl::write_string(
                &blade_module,
                &blade_info,
                naga::back::wgsl::WriterFlags::EXPLICIT_TYPES,
            )
            .map_err(|error| failure(error.to_string()))?;
            let emitted = naga::front::wgsl::parse_str(&result.source)
                .map_err(|error| failure(error.to_string()))?;
            for resource in &result.resources {
                if !emitted
                    .global_variables
                    .iter()
                    .any(|(_, global)| global.name.as_deref() == Some(resource.name.as_str()))
                {
                    return Err(failure(format!(
                        "resource name {} changed during WGSL emission",
                        resource.name
                    )));
                }
            }
        }
        ShaderBackend::Metal => {
            let mut map = naga::back::msl::BindingMap::new();
            for resource in &result.resources {
                let mut target = naga::back::msl::BindTarget::default();
                match resource.kind {
                    ShaderResourceSlotKind::Uniform => target.buffer = Some(resource.slot as u8),
                    ShaderResourceSlotKind::Texture => target.texture = Some(resource.slot as u8),
                    ShaderResourceSlotKind::Sampler => {
                        target.sampler = Some(naga::back::msl::BindSamplerTarget::Resource(
                            resource.slot as u8,
                        ))
                    }
                }
                map.insert(
                    naga::ResourceBinding {
                        group: 0,
                        binding: resource.binding,
                    },
                    target,
                );
            }
            let mut options = naga::back::msl::Options {
                lang_version: (2, 0),
                fake_missing_bindings: false,
                ..Default::default()
            };
            for entry in &module.entry_points {
                options.per_entry_point_map.insert(
                    entry.name.clone(),
                    naga::back::msl::EntryPointResources {
                        resources: map.clone(),
                        ..Default::default()
                    },
                );
            }
            let (source, reflection) =
                naga::back::msl::write_string(module, info, &options, &Default::default())
                    .map_err(|error| failure(error.to_string()))?;
            result.source = source;
            result.vertex_entry = reflection.entry_point_names[vertex_index]
                .as_ref()
                .map_err(|error| failure(error.to_string()))?
                .clone();
            result.fragment_entry = reflection.entry_point_names[fragment_index]
                .as_ref()
                .map_err(|error| failure(error.to_string()))?
                .clone();
        }
        ShaderBackend::DirectX11 => {
            let mut options = naga::back::hlsl::Options {
                shader_model: naga::back::hlsl::ShaderModel::V5_0,
                fake_missing_bindings: false,
                ..Default::default()
            };
            for resource in &result.resources {
                options.binding_map.insert(
                    naga::ResourceBinding {
                        group: 0,
                        binding: resource.binding,
                    },
                    naga::back::hlsl::BindTarget {
                        space: 0,
                        register: resource.slot,
                        binding_array_size: None,
                    },
                );
            }
            let fragment = naga::back::hlsl::FragmentEntryPoint::new(module, fragment);
            let reflection = naga::back::hlsl::Writer::new(&mut result.source, &options)
                .write(module, info, fragment.as_ref())
                .map_err(|error| failure(error.to_string()))?;
            result.vertex_entry = reflection.entry_point_names[vertex_index]
                .as_ref()
                .map_err(|error| failure(error.to_string()))?
                .clone();
            result.fragment_entry = reflection.entry_point_names[fragment_index]
                .as_ref()
                .map_err(|error| failure(error.to_string()))?
                .clone();
        }
        ShaderBackend::WebGl2 => {
            let mut options = naga::back::glsl::Options {
                version: naga::back::glsl::Version::new_gles(300),
                ..Default::default()
            };
            for resource in &result.resources {
                options.binding_map.insert(
                    naga::ResourceBinding {
                        group: 0,
                        binding: resource.binding,
                    },
                    resource.slot as u8,
                );
            }
            let write_stage = |stage, entry: &str, source: &mut String| {
                let pipeline = naga::back::glsl::PipelineOptions {
                    shader_stage: stage,
                    entry_point: entry.into(),
                    multiview: None,
                };
                naga::back::glsl::Writer::new(
                    source,
                    module,
                    info,
                    &options,
                    &pipeline,
                    Default::default(),
                )
                .map_err(|error| failure(error.to_string()))?
                .write()
                .map_err(|error| failure(error.to_string()))
            };
            let mut vertex = String::new();
            write_stage(ShaderStage::Vertex, VERTEX_ENTRY, &mut vertex)?;
            let reflection = write_stage(ShaderStage::Fragment, fragment, &mut result.source)?;
            result.vertex_source = Some(vertex);
            result.vertex_entry = "main".into();
            result.fragment_entry = "main".into();
            for resource in &mut result.resources {
                if resource.kind != ShaderResourceSlotKind::Uniform {
                    continue;
                }
                if let Some((handle, _)) = module.global_variables.iter().find(|(_, global)| {
                    global.binding
                        == Some(naga::ResourceBinding {
                            group: 0,
                            binding: resource.binding,
                        })
                }) {
                    if let Some(name) = reflection.uniforms.get(&handle) {
                        resource.name = name.clone();
                    }
                }
            }
            for (name, mapping) in reflection.texture_mapping {
                let texture = module.global_variables[mapping.texture]
                    .binding
                    .as_ref()
                    .unwrap();
                let sampler = mapping.sampler.and_then(|handle| {
                    module.global_variables[handle]
                        .binding
                        .as_ref()
                        .map(|binding| binding.binding)
                });
                result.texture_sampler_pairs.push(ShaderTextureSamplerPair {
                    texture_binding: texture.binding,
                    sampler_binding: sampler,
                    uniform_name: name,
                });
            }
            result
                .texture_sampler_pairs
                .sort_by(|a, b| a.uniform_name.cmp(&b.uniform_name));
        }
    }
    if result.retained_bytes() > MAX_PROGRAM_BYTES {
        return Err(ShaderError::ResourceLimit("translated source size"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(source: &str) -> Result<ShaderHandle, ShaderError> {
        ShaderHandle::compile_fragment(ShaderDescriptor::fragment("test", source, "fs_main"))
    }

    #[test]
    fn procedural_program_translates_all_backends_and_has_top_left_uv() {
        let shader = program("@fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4<f32>(uv, 0.5, 1.0); }").unwrap();
        assert!(shader.source().contains("1.0 - y * 2.0"));
        for backend in [
            ShaderBackend::Metal,
            ShaderBackend::DirectX11,
            ShaderBackend::Blade,
            ShaderBackend::WebGl2,
        ] {
            let output = shader.translate(backend).unwrap();
            assert!(!output.source.is_empty());
            assert_eq!(
                output.vertex_source.is_some(),
                backend == ShaderBackend::WebGl2
            );
        }
        assert!(shader.resources().is_empty());
    }

    #[test]
    fn reflection_preserves_offsets_nested_strides_and_sparse_binding_namespaces() {
        let shader = program(
            r#"
            struct Nested { color: vec4<f32>, direction: vec3<f32>, strength: f32 }
            struct Settings { transform: mat4x4<f32>, entries: array<Nested, 2>, tint: vec4<f32> }
            @group(0) @binding(29) var<uniform> settings: Settings;
            @group(0) @binding(105) var image: texture_2d<f32>;
            @group(0) @binding(201) var image_sampler: sampler;
            @fragment fn fs_main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
                return textureSample(image, image_sampler, uv) * settings.tint;
            }
        "#,
        )
        .unwrap();
        let ShaderResourceKind::Uniform(layout) = &shader.resources()[0].kind else {
            panic!("uniform")
        };
        assert_eq!(layout.size, 144);
        assert_eq!(
            layout.members.iter().map(|m| m.offset).collect::<Vec<_>>(),
            [0, 64, 128]
        );
        assert!(matches!(
            &layout.members[1].ty,
            ShaderUniformType::Array {
                length: 2,
                stride: 32,
                ..
            }
        ));
        let output = shader.translate(ShaderBackend::Metal).unwrap();
        assert_eq!(
            output
                .resources
                .iter()
                .map(|r| (r.binding, r.slot))
                .collect::<Vec<_>>(),
            [(29, 0), (105, 0), (201, 0)]
        );
        let browser = shader.translate(ShaderBackend::WebGl2).unwrap();
        assert_eq!(browser.texture_sampler_pairs.len(), 1);
        assert_eq!(browser.texture_sampler_pairs[0].texture_binding, 105);
        assert_eq!(browser.texture_sampler_pairs[0].sampler_binding, Some(201));
        assert!(browser.source.contains(&browser.resources[0].name));
    }

    #[test]
    fn semantic_errors_and_unsupported_resources_fail_before_gpu_work() {
        for source in [
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(unknown); }",
            "@fragment fn fs_main() -> @location(0) vec4<f32> { return 7u; }",
            "@fragment fn fs_main(@location(1) uv: vec2<f32>) -> @location(0) vec4<f32> { return vec4<f32>(uv, 0.0, 1.0); }",
            "@group(1) @binding(0) var<uniform> value: vec4<f32>; @fragment fn fs_main() -> @location(0) vec4<f32> { return value; }",
            "@group(0) @binding(0) var<storage, read> value: vec4<f32>; @fragment fn fs_main() -> @location(0) vec4<f32> { return value; }",
            "@fragment fn fs_main() -> @builtin(frag_depth) f32 { return 0.0; }",
        ] {
            assert!(program(source).is_err(), "{source}");
        }
    }

    #[test]
    fn nonportable_uniform_layout_is_rejected_instead_of_reading_wrong_bytes() {
        for ty in ["mat2x2<f32>", "struct Bad { value: f32 }"] {
            let source = if ty.starts_with("struct") {
                format!(
                    "{ty} @group(0) @binding(0) var<uniform> bad: Bad; @fragment fn fs_main() -> @location(0) vec4<f32> {{ return vec4<f32>(bad.value); }}"
                )
            } else {
                format!(
                    "@group(0) @binding(0) var<uniform> bad: {ty}; @fragment fn fs_main() -> @location(0) vec4<f32> {{ return vec4<f32>(bad[0], bad[1]); }}"
                )
            };
            assert!(matches!(program(&source), Err(ShaderError::Unsupported(_))));
        }
    }

    #[test]
    fn source_admission_precedes_parse_and_dead_programs_release_registry_bytes() {
        let oversized = "?".repeat(MAX_SOURCE_BYTES + 1);
        assert!(matches!(
            program(&oversized),
            Err(ShaderError::ResourceLimit(_))
        ));
        let shader =
            program("@fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }")
                .unwrap();
        let id = shader.id();
        let weak = Arc::downgrade(&shader.0);
        let clone = shader.clone();
        drop(shader);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
        let mut registry = REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.prune();
        assert!(!registry.programs.contains_key(&id));
        assert_eq!(
            registry.retained_bytes,
            registry
                .programs
                .values()
                .map(|(_, bytes)| *bytes)
                .sum::<usize>()
        );
    }
}
