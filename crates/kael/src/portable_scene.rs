//! Bounded retained 2D drawing commands shared by native and browser renderers.

use std::{
    collections::HashMap,
    fmt,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

use refineable::Refineable as _;

use crate::{
    App, Bounds, ContentMask, Corners, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, Path, Pixels, Point, RenderImage, Size, Style,
    StyleRefinement, Styled, TransformationMatrix, Window, point, px, quad, size,
    transparent_black,
};

/// Maximum draw commands accepted by one portable scene.
pub const PORTABLE_SCENE_MAX_COMMANDS: usize = 100_000;
/// Maximum logical objects accepted by one portable scene.
pub const PORTABLE_SCENE_MAX_OBJECTS: usize = 100_000;
/// Maximum tessellated path vertices accepted by one portable scene.
pub const PORTABLE_SCENE_MAX_PATH_VERTICES: usize = 1_000_000;
/// Maximum distinct image/frame resources referenced by one portable scene.
pub const PORTABLE_SCENE_MAX_IMAGE_RESOURCES: usize = 256;
/// Maximum decoded image bytes referenced by one portable scene.
pub const PORTABLE_SCENE_MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum estimated retained payload bytes accepted by one portable scene.
pub const PORTABLE_SCENE_MAX_RETAINED_BYTES: usize = 128 * 1024 * 1024;
/// Maximum saved transform/clip/opacity states in one portable scene.
pub const PORTABLE_SCENE_MAX_STATE_DEPTH: usize = 256;
/// Largest absolute logical coordinate accepted by the portable scene API.
pub const PORTABLE_SCENE_MAX_ABS_COORDINATE: f32 = 16_777_216.0;
/// Largest absolute affine-matrix component accepted by the portable scene API.
pub const PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT: f32 = 1_000_000.0;

/// Resource category used by a portable-scene limit or allocation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableSceneResource {
    /// Retained draw commands.
    Commands,
    /// Logical objects represented by commands.
    Objects,
    /// Tessellated vector/triangle vertices.
    PathVertices,
    /// Distinct decoded image frames.
    ImageResources,
    /// Decoded image payload bytes.
    ImageBytes,
    /// Total estimated retained payload bytes.
    RetainedBytes,
    /// Saved transform/clip/opacity states.
    StateDepth,
}

impl fmt::Display for PortableSceneResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Commands => "commands",
            Self::Objects => "objects",
            Self::PathVertices => "path vertices",
            Self::ImageResources => "image resources",
            Self::ImageBytes => "image bytes",
            Self::RetainedBytes => "retained bytes",
            Self::StateDepth => "state depth",
        })
    }
}

/// Portable-scene feature that can be queried before choosing a rendering path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableSceneFeature {
    /// Instanced solid and rounded quads.
    SolidQuads,
    /// Atlas-backed decoded image sprites.
    ImageSprites,
    /// Tessellated filled vector paths.
    FilledPaths,
    /// Solid triangle batches represented by tessellated paths.
    TriangleBatches,
    /// Finite 2D affine transforms.
    AffineTransforms,
    /// Axis-aligned rectangular clips.
    RectangularClips,
    /// Premultiplied source-over alpha composition.
    SourceOverAlpha,
    /// Destination-dependent or custom blend modes.
    CustomBlendModes,
    /// User-supplied GPU shaders.
    CustomShaders,
    /// General-purpose GPU compute.
    Compute,
    /// Depth-tested 3D rendering.
    ThreeDimensional,
}

/// Support level for a portable-scene feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortableSceneFeatureSupport {
    /// The feature uses the same retained Kael scene on native and browser targets.
    Full,
    /// The feature is intentionally outside this portable 2D surface.
    Unsupported,
}

/// Return the stable support level for a portable-scene feature.
pub const fn portable_scene_feature_support(
    feature: PortableSceneFeature,
) -> PortableSceneFeatureSupport {
    match feature {
        PortableSceneFeature::SolidQuads
        | PortableSceneFeature::ImageSprites
        | PortableSceneFeature::FilledPaths
        | PortableSceneFeature::TriangleBatches
        | PortableSceneFeature::AffineTransforms
        | PortableSceneFeature::RectangularClips
        | PortableSceneFeature::SourceOverAlpha => PortableSceneFeatureSupport::Full,
        PortableSceneFeature::CustomBlendModes
        | PortableSceneFeature::CustomShaders
        | PortableSceneFeature::Compute
        | PortableSceneFeature::ThreeDimensional => PortableSceneFeatureSupport::Unsupported,
    }
}

/// Typed failure returned by bounded portable-scene operations.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PortableSceneError {
    /// A coordinate, size, color, opacity, transform, or resource index was invalid.
    #[error("invalid portable scene input: {field}")]
    InvalidInput {
        /// Stable field label suitable for diagnostics.
        field: &'static str,
    },
    /// A stable scene resource ceiling would be exceeded.
    #[error("portable scene {resource} limit {limit} exceeded by attempted value {attempted}")]
    LimitExceeded {
        /// Resource whose ceiling was reached.
        resource: PortableSceneResource,
        /// Configured ceiling.
        limit: usize,
        /// Value the operation attempted to retain.
        attempted: usize,
    },
    /// Memory reservation failed before scene state was mutated.
    #[error("portable scene could not reserve {resource}")]
    AllocationFailed {
        /// Resource whose storage could not be reserved.
        resource: PortableSceneResource,
    },
    /// A restore was requested without a matching saved state.
    #[error("portable scene restore has no matching save")]
    UnbalancedState,
    /// An append transaction callback panicked and was rolled back.
    #[error("portable scene recording callback panicked; appended work was rolled back")]
    RecordingPanicked,
    /// The requested feature is not part of Kael's portable 2D contract.
    #[error("portable scene feature is unsupported: {feature:?}")]
    Unsupported {
        /// Feature that was requested.
        feature: PortableSceneFeature,
    },
    /// A prevalidated image could not be inserted into the renderer's atlas.
    #[error("portable scene image resource failed at command {command_index}")]
    ImageResourceUnavailable {
        /// Content-free command index for diagnostics.
        command_index: usize,
    },
}

/// Per-scene ceilings. Values can be lowered but cannot exceed Kael's public hard limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PortableSceneLimits {
    /// Maximum retained draw commands.
    pub max_commands: usize,
    /// Maximum logical objects represented by commands.
    pub max_objects: usize,
    /// Maximum tessellated path vertices.
    pub max_path_vertices: usize,
    /// Maximum distinct image/frame resources.
    pub max_image_resources: usize,
    /// Maximum decoded image bytes.
    pub max_image_bytes: usize,
    /// Maximum estimated retained payload bytes.
    pub max_retained_bytes: usize,
    /// Maximum saved drawing states.
    pub max_state_depth: usize,
}

impl Default for PortableSceneLimits {
    fn default() -> Self {
        Self {
            max_commands: PORTABLE_SCENE_MAX_COMMANDS,
            max_objects: PORTABLE_SCENE_MAX_OBJECTS,
            max_path_vertices: PORTABLE_SCENE_MAX_PATH_VERTICES,
            max_image_resources: PORTABLE_SCENE_MAX_IMAGE_RESOURCES,
            max_image_bytes: PORTABLE_SCENE_MAX_IMAGE_BYTES,
            max_retained_bytes: PORTABLE_SCENE_MAX_RETAINED_BYTES,
            max_state_depth: PORTABLE_SCENE_MAX_STATE_DEPTH,
        }
    }
}

impl PortableSceneLimits {
    fn validate(self) -> Result<Self, PortableSceneError> {
        let limits = [
            (
                "max_commands",
                self.max_commands,
                PORTABLE_SCENE_MAX_COMMANDS,
            ),
            ("max_objects", self.max_objects, PORTABLE_SCENE_MAX_OBJECTS),
            (
                "max_path_vertices",
                self.max_path_vertices,
                PORTABLE_SCENE_MAX_PATH_VERTICES,
            ),
            (
                "max_image_resources",
                self.max_image_resources,
                PORTABLE_SCENE_MAX_IMAGE_RESOURCES,
            ),
            (
                "max_image_bytes",
                self.max_image_bytes,
                PORTABLE_SCENE_MAX_IMAGE_BYTES,
            ),
            (
                "max_retained_bytes",
                self.max_retained_bytes,
                PORTABLE_SCENE_MAX_RETAINED_BYTES,
            ),
            (
                "max_state_depth",
                self.max_state_depth,
                PORTABLE_SCENE_MAX_STATE_DEPTH,
            ),
        ];
        if let Some((field, _, _)) = limits
            .into_iter()
            .find(|(_, requested, hard_limit)| requested > hard_limit)
        {
            return Err(PortableSceneError::InvalidInput { field });
        }
        Ok(self)
    }
}

/// Content-safe counters for one retained portable scene.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PortableSceneStats {
    /// Retained draw command count.
    pub command_count: usize,
    /// Logical object count, including every triangle in a triangle batch.
    pub object_count: usize,
    /// Solid/rounded quad count.
    pub solid_quad_count: usize,
    /// Image sprite count.
    pub image_sprite_count: usize,
    /// Filled vector path command count.
    pub path_count: usize,
    /// Triangle count across triangle batches.
    pub triangle_count: usize,
    /// Tessellated path vertex count.
    pub path_vertex_count: usize,
    /// Distinct image/frame resource count.
    pub image_resource_count: usize,
    /// Referenced decoded image bytes.
    pub image_bytes: usize,
    /// Estimated retained payload bytes covered by the byte budget.
    pub estimated_retained_bytes: usize,
    /// Currently saved drawing states.
    pub state_stack_depth: usize,
}

impl PortableSceneStats {
    /// Return a content-free summary for performance dashboards and smoke tests.
    pub fn to_text(&self) -> String {
        format!(
            "portable scene: {} commands, {} objects, quads {}, sprites {}, paths {}, triangles {}, vertices {}, image resources {}, image bytes {}, retained bytes {}, saved states {}",
            self.command_count,
            self.object_count,
            self.solid_quad_count,
            self.image_sprite_count,
            self.path_count,
            self.triangle_count,
            self.path_vertex_count,
            self.image_resource_count,
            self.image_bytes,
            self.estimated_retained_bytes,
            self.state_stack_depth,
        )
    }
}

/// One solid or rounded quad submitted to a [`PortableScene2d`].
#[derive(Clone, Debug, PartialEq)]
pub struct PortableSolidQuad {
    /// Canvas-local bounds.
    pub bounds: Bounds<Pixels>,
    /// Per-corner radii.
    pub corner_radii: Corners<Pixels>,
    /// Premultiplied source-over fill color.
    pub color: Hsla,
}

impl PortableSolidQuad {
    /// Construct an axis-aligned solid quad.
    pub fn new(bounds: Bounds<Pixels>, color: impl Into<Hsla>) -> Self {
        Self {
            bounds,
            corner_radii: Corners::default(),
            color: color.into(),
        }
    }

    /// Set the quad's corner radii.
    pub fn corner_radii(mut self, radii: impl Into<Corners<Pixels>>) -> Self {
        self.corner_radii = radii.into();
        self
    }
}

/// One atlas-backed image sprite submitted to a [`PortableScene2d`].
#[derive(Clone)]
pub struct PortableImageSprite {
    image: Arc<RenderImage>,
    bounds: Bounds<Pixels>,
    corner_radii: Corners<Pixels>,
    frame_index: usize,
    grayscale: bool,
}

impl fmt::Debug for PortableImageSprite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortableImageSprite")
            .field("image_id", &self.image.id)
            .field("bounds", &self.bounds)
            .field("corner_radii", &self.corner_radii)
            .field("frame_index", &self.frame_index)
            .field("grayscale", &self.grayscale)
            .finish()
    }
}

impl PortableImageSprite {
    /// Construct a sprite using the first frame of an image.
    pub fn new(image: Arc<RenderImage>, bounds: Bounds<Pixels>) -> Self {
        Self {
            image,
            bounds,
            corner_radii: Corners::default(),
            frame_index: 0,
            grayscale: false,
        }
    }

    /// Select a decoded image frame.
    pub fn frame(mut self, frame_index: usize) -> Self {
        self.frame_index = frame_index;
        self
    }

    /// Set per-corner clipping radii.
    pub fn corner_radii(mut self, radii: impl Into<Corners<Pixels>>) -> Self {
        self.corner_radii = radii.into();
        self
    }

    /// Request grayscale sampling for this sprite.
    pub fn grayscale(mut self, grayscale: bool) -> Self {
        self.grayscale = grayscale;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PortableDrawState {
    transform: TransformationMatrix,
    opacity: f32,
    clip: Option<Bounds<Pixels>>,
}

impl Default for PortableDrawState {
    fn default() -> Self {
        Self {
            transform: TransformationMatrix::unit(),
            opacity: 1.0,
            clip: None,
        }
    }
}

#[derive(Clone)]
enum PortableDrawCommand {
    SolidQuad {
        quad: PortableSolidQuad,
        state: PortableDrawState,
    },
    ImageSprite {
        sprite: PortableImageSprite,
        state: PortableDrawState,
    },
    FilledPath {
        path: Path<Pixels>,
        color: Hsla,
        triangle_count: usize,
        state: PortableDrawState,
    },
}

impl PortableDrawCommand {
    fn state(&self) -> &PortableDrawState {
        match self {
            Self::SolidQuad { state, .. }
            | Self::ImageSprite { state, .. }
            | Self::FilledPath { state, .. } => state,
        }
    }
}

type ImageResourceKey = (crate::ImageId, usize);

#[derive(Clone)]
struct RecordingCheckpoint {
    command_len: usize,
    stats: PortableSceneStats,
    state: PortableDrawState,
    state_stack: Vec<PortableDrawState>,
    image_resources: HashMap<ImageResourceKey, usize>,
}

/// A bounded retained 2D command list rendered by Kael's existing native or WebGL scene.
///
/// This is a high-throughput 2D surface, not raw GPU access. It deliberately does not expose
/// renderer handles, custom shaders, compute, depth buffers, or a 3D pipeline.
#[derive(Clone)]
pub struct PortableScene2d {
    limits: PortableSceneLimits,
    commands: Vec<PortableDrawCommand>,
    stats: PortableSceneStats,
    current_state: PortableDrawState,
    state_stack: Vec<PortableDrawState>,
    image_resources: HashMap<ImageResourceKey, usize>,
}

impl Default for PortableScene2d {
    fn default() -> Self {
        Self::new()
    }
}

impl PortableScene2d {
    /// Construct an empty scene using Kael's public hard ceilings.
    pub fn new() -> Self {
        Self {
            limits: PortableSceneLimits::default(),
            commands: Vec::new(),
            stats: PortableSceneStats::default(),
            current_state: PortableDrawState::default(),
            state_stack: Vec::new(),
            image_resources: HashMap::new(),
        }
    }

    /// Construct an empty scene with ceilings at or below Kael's public hard limits.
    pub fn with_limits(limits: PortableSceneLimits) -> Result<Self, PortableSceneError> {
        Ok(Self {
            limits: limits.validate()?,
            ..Self::new()
        })
    }

    /// Return the active resource ceilings.
    pub fn limits(&self) -> PortableSceneLimits {
        self.limits
    }

    /// Return content-safe retained-scene counters.
    pub fn stats(&self) -> PortableSceneStats {
        let mut stats = self.stats;
        stats.state_stack_depth = self.state_stack.len();
        stats
    }

    /// Return whether the scene contains no draw commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Remove all commands and resources while retaining reusable allocations.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.stats = PortableSceneStats::default();
        self.current_state = PortableDrawState::default();
        self.state_stack.clear();
        self.image_resources.clear();
    }

    /// Reserve bounded command storage for a known game-loop workload.
    pub fn try_reserve_commands(&mut self, additional: usize) -> Result<(), PortableSceneError> {
        let attempted = checked_add(self.commands.len(), additional, "command count")?;
        ensure_limit(
            PortableSceneResource::Commands,
            self.limits.max_commands,
            attempted,
        )?;
        self.commands.try_reserve_exact(additional).map_err(|_| {
            PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::Commands,
            }
        })
    }

    /// Return whether a portable-scene feature is fully available.
    pub const fn supports(feature: PortableSceneFeature) -> bool {
        matches!(
            portable_scene_feature_support(feature),
            PortableSceneFeatureSupport::Full
        )
    }

    /// Require a feature, returning typed `Unsupported` for shaders, compute, 3D, and custom
    /// blending instead of exposing a backend-specific escape hatch.
    pub fn require_feature(feature: PortableSceneFeature) -> Result<(), PortableSceneError> {
        if Self::supports(feature) {
            Ok(())
        } else {
            Err(PortableSceneError::Unsupported { feature })
        }
    }

    /// Append one validated solid or rounded quad.
    pub fn push_solid_quad(&mut self, quad: PortableSolidQuad) -> Result<(), PortableSceneError> {
        self.push_solid_quads(std::slice::from_ref(&quad))
    }

    /// Append a validated solid-quad batch after one bounded reservation.
    pub fn push_solid_quads(
        &mut self,
        quads: &[PortableSolidQuad],
    ) -> Result<(), PortableSceneError> {
        for quad in quads {
            validate_bounds(quad.bounds, "quad bounds", false)?;
            validate_corners(&quad.corner_radii)?;
            validate_color(quad.color)?;
        }
        self.ensure_growth(quads.len(), quads.len(), 0, 0, 0)?;
        self.commands.try_reserve_exact(quads.len()).map_err(|_| {
            PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::Commands,
            }
        })?;
        for quad in quads {
            self.commands.push(PortableDrawCommand::SolidQuad {
                quad: quad.clone(),
                state: self.current_state.clone(),
            });
        }
        self.stats.command_count += quads.len();
        self.stats.object_count += quads.len();
        self.stats.solid_quad_count += quads.len();
        self.stats.estimated_retained_bytes += command_bytes(quads.len())?;
        Ok(())
    }

    /// Append one validated atlas-backed image sprite.
    pub fn push_image_sprite(
        &mut self,
        sprite: PortableImageSprite,
    ) -> Result<(), PortableSceneError> {
        self.push_image_sprites(std::slice::from_ref(&sprite))
    }

    /// Append a validated image-sprite batch and account for distinct decoded frames once.
    pub fn push_image_sprites(
        &mut self,
        sprites: &[PortableImageSprite],
    ) -> Result<(), PortableSceneError> {
        let mut new_resources = HashMap::new();
        new_resources
            .try_reserve(self.limits.max_image_resources.min(sprites.len()))
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::ImageResources,
            })?;
        for sprite in sprites {
            validate_bounds(sprite.bounds, "image bounds", false)?;
            validate_corners(&sprite.corner_radii)?;
            let Some(bytes) = sprite.image.as_bytes(sprite.frame_index) else {
                return Err(PortableSceneError::InvalidInput {
                    field: "image frame index",
                });
            };
            let key = (sprite.image.id, sprite.frame_index);
            if !self.image_resources.contains_key(&key) && !new_resources.contains_key(&key) {
                if checked_add(
                    self.image_resources.len(),
                    new_resources.len(),
                    "image resources",
                )? >= self.limits.max_image_resources
                {
                    return Err(PortableSceneError::LimitExceeded {
                        resource: PortableSceneResource::ImageResources,
                        limit: self.limits.max_image_resources,
                        attempted: self.image_resources.len() + new_resources.len() + 1,
                    });
                }
                new_resources.insert(key, bytes.len());
            }
        }
        let new_image_bytes = new_resources.values().try_fold(0usize, |total, bytes| {
            total
                .checked_add(*bytes)
                .ok_or(PortableSceneError::InvalidInput {
                    field: "image byte count",
                })
        })?;
        self.ensure_growth(
            sprites.len(),
            sprites.len(),
            0,
            new_resources.len(),
            new_image_bytes,
        )?;
        self.commands
            .try_reserve_exact(sprites.len())
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::Commands,
            })?;
        self.image_resources
            .try_reserve(new_resources.len())
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::ImageResources,
            })?;
        self.image_resources.extend(new_resources);
        for sprite in sprites {
            self.commands.push(PortableDrawCommand::ImageSprite {
                sprite: sprite.clone(),
                state: self.current_state.clone(),
            });
        }
        self.stats.command_count += sprites.len();
        self.stats.object_count += sprites.len();
        self.stats.image_sprite_count += sprites.len();
        self.stats.image_resource_count = self.image_resources.len();
        self.stats.image_bytes += new_image_bytes;
        self.stats.estimated_retained_bytes += checked_add(
            command_bytes(sprites.len())?,
            new_image_bytes,
            "retained bytes",
        )?;
        Ok(())
    }

    /// Append a pre-tessellated filled path. Its retained hit-test outline is discarded from
    /// the scene copy so only renderer-owned triangle vertices count against the byte budget.
    pub fn push_filled_path(
        &mut self,
        path: Path<Pixels>,
        color: impl Into<Hsla>,
    ) -> Result<(), PortableSceneError> {
        let color = color.into();
        validate_color(color)?;
        let vertex_count = path.render_vertex_count();
        if vertex_count == 0 || !path.render_vertices_are_bounded(PORTABLE_SCENE_MAX_ABS_COORDINATE)
        {
            return Err(PortableSceneError::InvalidInput {
                field: "path vertices",
            });
        }
        let path = path.into_transformed(self.current_state.transform);
        if !path.render_vertices_are_bounded(PORTABLE_SCENE_MAX_ABS_COORDINATE) {
            return Err(PortableSceneError::InvalidInput {
                field: "transformed path vertices",
            });
        }
        self.ensure_growth(1, 1, vertex_count, 0, 0)?;
        self.commands
            .try_reserve_exact(1)
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::Commands,
            })?;
        let mut state = self.current_state.clone();
        state.transform = TransformationMatrix::unit();
        self.commands.push(PortableDrawCommand::FilledPath {
            path: path.into_render_only(),
            color,
            triangle_count: 0,
            state,
        });
        self.stats.command_count += 1;
        self.stats.object_count += 1;
        self.stats.path_count += 1;
        self.stats.path_vertex_count += vertex_count;
        self.stats.estimated_retained_bytes += checked_add(
            command_bytes(1)?,
            path_vertex_bytes(vertex_count)?,
            "retained bytes",
        )?;
        Ok(())
    }

    /// Append one solid triangle batch as a single renderer path command.
    pub fn push_triangles(
        &mut self,
        triangles: &[[Point<Pixels>; 3]],
        color: impl Into<Hsla>,
    ) -> Result<(), PortableSceneError> {
        if triangles.is_empty() {
            return Ok(());
        }
        let color = color.into();
        validate_color(color)?;
        for triangle in triangles {
            for vertex in triangle {
                validate_point(*vertex, "triangle vertex")?;
            }
        }
        let vertex_count =
            triangles
                .len()
                .checked_mul(3)
                .ok_or(PortableSceneError::InvalidInput {
                    field: "triangle count",
                })?;
        self.ensure_growth(1, triangles.len(), vertex_count, 0, 0)?;
        self.commands
            .try_reserve_exact(1)
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::Commands,
            })?;
        let mut path = Path::new(triangles[0][0]);
        path.try_reserve_triangles(triangles.len()).map_err(|_| {
            PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::PathVertices,
            }
        })?;
        let st = (point(0.0, 1.0), point(0.0, 1.0), point(0.0, 1.0));
        for [a, b, c] in triangles {
            path.push_triangle((*a, *b, *c), st);
        }
        let path = path.into_transformed(self.current_state.transform);
        if !path.render_vertices_are_bounded(PORTABLE_SCENE_MAX_ABS_COORDINATE) {
            return Err(PortableSceneError::InvalidInput {
                field: "transformed triangle vertices",
            });
        }
        let mut state = self.current_state.clone();
        state.transform = TransformationMatrix::unit();
        self.commands.push(PortableDrawCommand::FilledPath {
            path,
            color,
            triangle_count: triangles.len(),
            state,
        });
        self.stats.command_count += 1;
        self.stats.object_count += triangles.len();
        self.stats.path_count += 1;
        self.stats.triangle_count += triangles.len();
        self.stats.path_vertex_count += vertex_count;
        self.stats.estimated_retained_bytes += checked_add(
            command_bytes(1)?,
            path_vertex_bytes(vertex_count)?,
            "retained bytes",
        )?;
        Ok(())
    }

    /// Save the current transform, opacity, and clip.
    pub fn save(&mut self) -> Result<(), PortableSceneError> {
        let attempted = checked_add(self.state_stack.len(), 1, "state depth")?;
        ensure_limit(
            PortableSceneResource::StateDepth,
            self.limits.max_state_depth,
            attempted,
        )?;
        self.state_stack.try_reserve_exact(1).map_err(|_| {
            PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::StateDepth,
            }
        })?;
        self.state_stack.push(self.current_state.clone());
        Ok(())
    }

    /// Restore the most recently saved state.
    pub fn restore(&mut self) -> Result<(), PortableSceneError> {
        self.current_state = self
            .state_stack
            .pop()
            .ok_or(PortableSceneError::UnbalancedState)?;
        Ok(())
    }

    /// Replace the current finite 2D affine transform.
    pub fn set_transform(
        &mut self,
        transform: TransformationMatrix,
    ) -> Result<(), PortableSceneError> {
        validate_transform(transform)?;
        self.current_state.transform = transform;
        Ok(())
    }

    /// Reset the current transform to the identity matrix.
    pub fn reset_transform(&mut self) {
        self.current_state.transform = TransformationMatrix::unit();
    }

    /// Compose a canvas-local translation onto the current transform.
    pub fn translate(&mut self, x: Pixels, y: Pixels) -> Result<(), PortableSceneError> {
        validate_scalar(x.0, PORTABLE_SCENE_MAX_ABS_COORDINATE, "translation x")?;
        validate_scalar(y.0, PORTABLE_SCENE_MAX_ABS_COORDINATE, "translation y")?;
        self.set_transform(self.current_state.transform.compose(TransformationMatrix {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [x.0, y.0],
        }))
    }

    /// Compose a clockwise rotation, in radians, onto the current transform.
    pub fn rotate(&mut self, radians: f32) -> Result<(), PortableSceneError> {
        validate_scalar(radians, PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT, "rotation")?;
        self.set_transform(self.current_state.transform.rotate(crate::Radians(radians)))
    }

    /// Compose independent finite x/y scale factors onto the current transform.
    pub fn scale(&mut self, x: f32, y: f32) -> Result<(), PortableSceneError> {
        validate_scalar(x, PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT, "scale x")?;
        validate_scalar(y, PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT, "scale y")?;
        self.set_transform(self.current_state.transform.scale(size(x, y)))
    }

    /// Set source-over opacity for subsequent commands.
    pub fn set_opacity(&mut self, opacity: f32) -> Result<(), PortableSceneError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(PortableSceneError::InvalidInput { field: "opacity" });
        }
        self.current_state.opacity = opacity;
        Ok(())
    }

    /// Intersect the current clip with a rectangle transformed at call time into scene space.
    /// Rotated rectangles use their axis-aligned bounding box on every backend.
    pub fn clip_rect(&mut self, bounds: Bounds<Pixels>) -> Result<(), PortableSceneError> {
        validate_bounds(bounds, "clip bounds", true)?;
        let transformed = transform_bounds(bounds, self.current_state.transform);
        validate_bounds(transformed, "transformed clip bounds", true)?;
        self.current_state.clip = Some(match self.current_state.clip {
            Some(existing) => existing.intersect(&transformed),
            None => transformed,
        });
        Ok(())
    }

    /// Remove the additional portable-scene clip for subsequent commands.
    pub fn reset_clip(&mut self) {
        self.current_state.clip = None;
    }

    /// Atomically append commands through a restricted recorder.
    ///
    /// Returned errors and unwinding panics both restore commands, resource accounting, and
    /// drawing state to the pre-transaction checkpoint.
    pub fn transaction<R>(
        &mut self,
        record: impl FnOnce(&mut PortableSceneRecorder<'_>) -> Result<R, PortableSceneError>,
    ) -> Result<R, PortableSceneError> {
        let checkpoint = self.checkpoint()?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut recorder = PortableSceneRecorder { scene: self };
            record(&mut recorder)
        }));
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => {
                self.rollback(checkpoint);
                Err(error)
            }
            Err(_) => {
                self.rollback(checkpoint);
                Err(PortableSceneError::RecordingPanicked)
            }
        }
    }

    /// Paint this scene at `bounds.origin` during an element paint phase.
    ///
    /// Commands are inserted into the existing Kael scene; backend handles and renderer
    /// ownership never cross this API boundary.
    pub fn paint(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Result<PortableSceneStats, PortableSceneError> {
        validate_bounds(bounds, "paint bounds", true)?;
        let mut start = 0;
        while start < self.commands.len() {
            let state = self.commands[start].state();
            let mut end = start + 1;
            while end < self.commands.len() && self.commands[end].state() == state {
                end += 1;
            }
            let mask = state.clip.map(|clip| ContentMask {
                bounds: offset_bounds(clip, bounds.origin),
            });
            window.with_content_mask(mask, |window| {
                window.with_element_opacity(Some(state.opacity), |window| {
                    for (offset, command) in self.commands[start..end].iter().enumerate() {
                        self.paint_command(command, start + offset, bounds.origin, window)?;
                    }
                    Ok(())
                })
            })?;
            start = end;
        }
        Ok(self.stats())
    }

    fn paint_command(
        &self,
        command: &PortableDrawCommand,
        command_index: usize,
        origin: Point<Pixels>,
        window: &mut Window,
    ) -> Result<(), PortableSceneError> {
        match command {
            PortableDrawCommand::SolidQuad { quad: solid, state } => {
                let mut paint = quad(
                    offset_bounds(solid.bounds, origin),
                    solid.corner_radii,
                    solid.color,
                    px(0.0),
                    transparent_black(),
                    crate::BorderStyle::Solid,
                );
                paint.transform = resolve_quad_transform(origin, state.transform);
                window.paint_quad(paint);
                Ok(())
            }
            PortableDrawCommand::ImageSprite { sprite, state } => {
                let transform = resolve_quad_transform(origin, state.transform);
                window.with_element_transform(Some(transform), |window| {
                    window
                        .paint_image(
                            offset_bounds(sprite.bounds, origin),
                            sprite.corner_radii,
                            sprite.image.clone(),
                            sprite.frame_index,
                            sprite.grayscale,
                        )
                        .map_err(|_| PortableSceneError::ImageResourceUnavailable { command_index })
                })
            }
            PortableDrawCommand::FilledPath {
                path,
                color,
                triangle_count,
                state,
            } => {
                let _ = triangle_count;
                let path = path.transformed(full_path_transform(origin, state.transform));
                window.paint_path(path, *color);
                Ok(())
            }
        }
    }

    fn ensure_growth(
        &self,
        commands: usize,
        objects: usize,
        path_vertices: usize,
        image_resources: usize,
        image_bytes: usize,
    ) -> Result<(), PortableSceneError> {
        let attempted_commands = checked_add(self.stats.command_count, commands, "commands")?;
        let attempted_objects = checked_add(self.stats.object_count, objects, "objects")?;
        let attempted_vertices =
            checked_add(self.stats.path_vertex_count, path_vertices, "path vertices")?;
        let attempted_image_resources = checked_add(
            self.stats.image_resource_count,
            image_resources,
            "image resources",
        )?;
        let attempted_image_bytes =
            checked_add(self.stats.image_bytes, image_bytes, "image bytes")?;
        let added_retained = checked_add(
            command_bytes(commands)?,
            checked_add(
                path_vertex_bytes(path_vertices)?,
                image_bytes,
                "retained bytes",
            )?,
            "retained bytes",
        )?;
        let attempted_retained = checked_add(
            self.stats.estimated_retained_bytes,
            added_retained,
            "retained bytes",
        )?;
        for (resource, limit, attempted) in [
            (
                PortableSceneResource::Commands,
                self.limits.max_commands,
                attempted_commands,
            ),
            (
                PortableSceneResource::Objects,
                self.limits.max_objects,
                attempted_objects,
            ),
            (
                PortableSceneResource::PathVertices,
                self.limits.max_path_vertices,
                attempted_vertices,
            ),
            (
                PortableSceneResource::ImageResources,
                self.limits.max_image_resources,
                attempted_image_resources,
            ),
            (
                PortableSceneResource::ImageBytes,
                self.limits.max_image_bytes,
                attempted_image_bytes,
            ),
            (
                PortableSceneResource::RetainedBytes,
                self.limits.max_retained_bytes,
                attempted_retained,
            ),
        ] {
            ensure_limit(resource, limit, attempted)?;
        }
        Ok(())
    }

    fn checkpoint(&self) -> Result<RecordingCheckpoint, PortableSceneError> {
        let mut state_stack = Vec::new();
        state_stack
            .try_reserve_exact(self.state_stack.len())
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::StateDepth,
            })?;
        state_stack.extend(self.state_stack.iter().cloned());
        let mut image_resources = HashMap::new();
        image_resources
            .try_reserve(self.image_resources.len())
            .map_err(|_| PortableSceneError::AllocationFailed {
                resource: PortableSceneResource::ImageResources,
            })?;
        image_resources.extend(
            self.image_resources
                .iter()
                .map(|(key, bytes)| (*key, *bytes)),
        );
        Ok(RecordingCheckpoint {
            command_len: self.commands.len(),
            stats: self.stats,
            state: self.current_state.clone(),
            state_stack,
            image_resources,
        })
    }

    fn rollback(&mut self, checkpoint: RecordingCheckpoint) {
        self.commands.truncate(checkpoint.command_len);
        self.stats = checkpoint.stats;
        self.current_state = checkpoint.state;
        self.state_stack = checkpoint.state_stack;
        self.image_resources = checkpoint.image_resources;
    }
}

/// Restricted append-only view used by [`PortableScene2d::transaction`].
pub struct PortableSceneRecorder<'a> {
    scene: &'a mut PortableScene2d,
}

impl PortableSceneRecorder<'_> {
    /// Append one solid/rounded quad.
    pub fn push_solid_quad(&mut self, quad: PortableSolidQuad) -> Result<(), PortableSceneError> {
        self.scene.push_solid_quad(quad)
    }

    /// Append a solid/rounded quad batch.
    pub fn push_solid_quads(
        &mut self,
        quads: &[PortableSolidQuad],
    ) -> Result<(), PortableSceneError> {
        self.scene.push_solid_quads(quads)
    }

    /// Append one image sprite.
    pub fn push_image_sprite(
        &mut self,
        sprite: PortableImageSprite,
    ) -> Result<(), PortableSceneError> {
        self.scene.push_image_sprite(sprite)
    }

    /// Append one image-sprite batch.
    pub fn push_image_sprites(
        &mut self,
        sprites: &[PortableImageSprite],
    ) -> Result<(), PortableSceneError> {
        self.scene.push_image_sprites(sprites)
    }

    /// Append one pre-tessellated filled path.
    pub fn push_filled_path(
        &mut self,
        path: Path<Pixels>,
        color: impl Into<Hsla>,
    ) -> Result<(), PortableSceneError> {
        self.scene.push_filled_path(path, color)
    }

    /// Append one solid triangle batch.
    pub fn push_triangles(
        &mut self,
        triangles: &[[Point<Pixels>; 3]],
        color: impl Into<Hsla>,
    ) -> Result<(), PortableSceneError> {
        self.scene.push_triangles(triangles, color)
    }

    /// Save transform, opacity, and clip.
    pub fn save(&mut self) -> Result<(), PortableSceneError> {
        self.scene.save()
    }

    /// Restore transform, opacity, and clip.
    pub fn restore(&mut self) -> Result<(), PortableSceneError> {
        self.scene.restore()
    }

    /// Replace the current affine transform.
    pub fn set_transform(
        &mut self,
        transform: TransformationMatrix,
    ) -> Result<(), PortableSceneError> {
        self.scene.set_transform(transform)
    }

    /// Reset the current affine transform.
    pub fn reset_transform(&mut self) {
        self.scene.reset_transform();
    }

    /// Compose a translation.
    pub fn translate(&mut self, x: Pixels, y: Pixels) -> Result<(), PortableSceneError> {
        self.scene.translate(x, y)
    }

    /// Compose a clockwise rotation in radians.
    pub fn rotate(&mut self, radians: f32) -> Result<(), PortableSceneError> {
        self.scene.rotate(radians)
    }

    /// Compose independent x/y scale factors.
    pub fn scale(&mut self, x: f32, y: f32) -> Result<(), PortableSceneError> {
        self.scene.scale(x, y)
    }

    /// Set source-over opacity.
    pub fn set_opacity(&mut self, opacity: f32) -> Result<(), PortableSceneError> {
        self.scene.set_opacity(opacity)
    }

    /// Intersect the current clip with a rectangle.
    pub fn clip_rect(&mut self, bounds: Bounds<Pixels>) -> Result<(), PortableSceneError> {
        self.scene.clip_rect(bounds)
    }

    /// Remove the additional clip.
    pub fn reset_clip(&mut self) {
        self.scene.reset_clip();
    }
}

/// Element that paints an immutable retained portable scene.
pub struct PortableSceneElement {
    scene: Arc<PortableScene2d>,
    style: StyleRefinement,
}

/// Construct an element that paints a retained 2D scene through the active Kael renderer.
pub fn portable_scene(
    size: Size<Pixels>,
    scene: impl Into<Arc<PortableScene2d>>,
) -> PortableSceneElement {
    PortableSceneElement {
        scene: scene.into(),
        style: StyleRefinement::default(),
    }
    .w(size.width)
    .h(size.height)
}

impl IntoElement for PortableSceneElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PortableSceneElement {
    type RequestLayoutState = Style;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style.clone(), [], cx);
        (layout_id, style)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Style,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        style: &mut Style,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        style.paint(bounds, window, cx, |window, _cx| {
            if let Err(error) = self.scene.paint(bounds, window) {
                log::error!("portable scene paint failed: {error}");
            }
        });
    }
}

impl Styled for PortableSceneElement {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn checked_add(
    left: usize,
    right: usize,
    field: &'static str,
) -> Result<usize, PortableSceneError> {
    left.checked_add(right)
        .ok_or(PortableSceneError::InvalidInput { field })
}

fn command_bytes(command_count: usize) -> Result<usize, PortableSceneError> {
    command_count
        .checked_mul(size_of::<PortableDrawCommand>())
        .ok_or(PortableSceneError::InvalidInput {
            field: "command byte count",
        })
}

fn path_vertex_bytes(vertex_count: usize) -> Result<usize, PortableSceneError> {
    vertex_count
        .checked_mul(size_of::<crate::scene::PathVertex<Pixels>>())
        .ok_or(PortableSceneError::InvalidInput {
            field: "path vertex byte count",
        })
}

fn ensure_limit(
    resource: PortableSceneResource,
    limit: usize,
    attempted: usize,
) -> Result<(), PortableSceneError> {
    if attempted > limit {
        Err(PortableSceneError::LimitExceeded {
            resource,
            limit,
            attempted,
        })
    } else {
        Ok(())
    }
}

fn validate_scalar(
    value: f32,
    maximum: f32,
    field: &'static str,
) -> Result<(), PortableSceneError> {
    if value.is_finite() && value.abs() <= maximum {
        Ok(())
    } else {
        Err(PortableSceneError::InvalidInput { field })
    }
}

fn validate_point(value: Point<Pixels>, field: &'static str) -> Result<(), PortableSceneError> {
    validate_scalar(value.x.0, PORTABLE_SCENE_MAX_ABS_COORDINATE, field)?;
    validate_scalar(value.y.0, PORTABLE_SCENE_MAX_ABS_COORDINATE, field)
}

fn validate_bounds(
    bounds: Bounds<Pixels>,
    field: &'static str,
    allow_empty: bool,
) -> Result<(), PortableSceneError> {
    validate_point(bounds.origin, field)?;
    validate_scalar(
        bounds.size.width.0,
        PORTABLE_SCENE_MAX_ABS_COORDINATE,
        field,
    )?;
    validate_scalar(
        bounds.size.height.0,
        PORTABLE_SCENE_MAX_ABS_COORDINATE,
        field,
    )?;
    if bounds.size.width < px(0.0)
        || bounds.size.height < px(0.0)
        || (!allow_empty && (bounds.size.width == px(0.0) || bounds.size.height == px(0.0)))
    {
        return Err(PortableSceneError::InvalidInput { field });
    }
    validate_point(bounds.bottom_right(), field)
}

fn validate_corners(corners: &Corners<Pixels>) -> Result<(), PortableSceneError> {
    for radius in [
        corners.top_left,
        corners.top_right,
        corners.bottom_right,
        corners.bottom_left,
    ] {
        validate_scalar(radius.0, PORTABLE_SCENE_MAX_ABS_COORDINATE, "corner radius")?;
        if radius < px(0.0) {
            return Err(PortableSceneError::InvalidInput {
                field: "corner radius",
            });
        }
    }
    Ok(())
}

fn validate_color(color: Hsla) -> Result<(), PortableSceneError> {
    if [color.h, color.s, color.l, color.a]
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
    {
        Ok(())
    } else {
        Err(PortableSceneError::InvalidInput { field: "color" })
    }
}

fn validate_transform(transform: TransformationMatrix) -> Result<(), PortableSceneError> {
    for value in transform
        .rotation_scale
        .into_iter()
        .flatten()
        .chain(transform.translation)
    {
        validate_scalar(value, PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT, "transform")?;
    }
    Ok(())
}

fn full_path_transform(
    canvas_origin: Point<Pixels>,
    transform: TransformationMatrix,
) -> TransformationMatrix {
    translation_matrix(canvas_origin).compose(transform)
}

fn resolve_quad_transform(
    canvas_origin: Point<Pixels>,
    transform: TransformationMatrix,
) -> TransformationMatrix {
    translation_matrix(canvas_origin)
        .compose(transform)
        .compose(TransformationMatrix {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [-canvas_origin.x.0, -canvas_origin.y.0],
        })
}

fn translation_matrix(origin: Point<Pixels>) -> TransformationMatrix {
    TransformationMatrix {
        rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
        translation: [origin.x.0, origin.y.0],
    }
}

fn offset_bounds(bounds: Bounds<Pixels>, offset: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(bounds.origin + offset, bounds.size)
}

fn transform_bounds(bounds: Bounds<Pixels>, transform: TransformationMatrix) -> Bounds<Pixels> {
    let mut transformed = Bounds::default();
    for point in [
        bounds.origin,
        bounds.top_right(),
        bounds.bottom_right(),
        bounds.bottom_left(),
    ] {
        transformed = transformed.union(&Bounds::new(transform.apply(point), Size::default()));
    }
    transformed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(index: usize) -> PortableSolidQuad {
        PortableSolidQuad::new(
            Bounds::new(point(px(index as f32), px(0.0)), size(px(1.0), px(1.0))),
            crate::white(),
        )
    }

    #[test]
    fn supports_only_the_cross_backend_2d_contract() {
        assert!(PortableScene2d::supports(PortableSceneFeature::SolidQuads));
        assert!(PortableScene2d::supports(
            PortableSceneFeature::TriangleBatches
        ));
        for feature in [
            PortableSceneFeature::CustomBlendModes,
            PortableSceneFeature::CustomShaders,
            PortableSceneFeature::Compute,
            PortableSceneFeature::ThreeDimensional,
        ] {
            assert_eq!(
                PortableScene2d::require_feature(feature),
                Err(PortableSceneError::Unsupported { feature })
            );
        }
    }

    #[test]
    fn command_and_byte_limits_fail_without_partial_mutation() {
        let limits = PortableSceneLimits {
            max_commands: 2,
            max_objects: 2,
            max_retained_bytes: command_bytes(2).unwrap(),
            ..PortableSceneLimits::default()
        };
        let mut scene = PortableScene2d::with_limits(limits).unwrap();
        scene.push_solid_quads(&[rect(0), rect(1)]).unwrap();
        let before = scene.stats();
        assert_eq!(
            scene.push_solid_quad(rect(2)),
            Err(PortableSceneError::LimitExceeded {
                resource: PortableSceneResource::Commands,
                limit: 2,
                attempted: 3,
            })
        );
        assert_eq!(scene.stats(), before);
    }

    #[test]
    fn invalid_values_are_typed_and_do_not_enter_the_scene() {
        let mut scene = PortableScene2d::new();
        let invalid = PortableSolidQuad::new(
            Bounds::new(point(px(f32::NAN), px(0.0)), size(px(1.0), px(1.0))),
            crate::white(),
        );
        assert_eq!(
            scene.push_solid_quad(invalid),
            Err(PortableSceneError::InvalidInput {
                field: "quad bounds"
            })
        );
        assert_eq!(
            scene.set_opacity(f32::NAN),
            Err(PortableSceneError::InvalidInput { field: "opacity" })
        );
        assert!(scene.is_empty());
    }

    #[test]
    fn transactions_roll_back_returned_errors_and_panics() {
        let mut scene = PortableScene2d::new();
        scene.push_solid_quad(rect(0)).unwrap();
        let before = scene.stats();

        let returned = scene.transaction(|record| {
            record.push_solid_quad(rect(1))?;
            Err::<(), _>(PortableSceneError::InvalidInput { field: "probe" })
        });
        assert_eq!(
            returned,
            Err(PortableSceneError::InvalidInput { field: "probe" })
        );
        assert_eq!(scene.stats(), before);

        let panicked: Result<(), PortableSceneError> = scene.transaction(|record| {
            record.push_solid_quad(rect(2))?;
            panic!("contained recorder panic")
        });
        assert_eq!(panicked, Err(PortableSceneError::RecordingPanicked));
        assert_eq!(scene.stats(), before);
    }

    #[test]
    fn state_depth_and_restore_transitions_are_deterministic() {
        let mut scene = PortableScene2d::with_limits(PortableSceneLimits {
            max_state_depth: 1,
            ..PortableSceneLimits::default()
        })
        .unwrap();
        scene.save().unwrap();
        assert_eq!(scene.stats().state_stack_depth, 1);
        assert_eq!(
            scene.save(),
            Err(PortableSceneError::LimitExceeded {
                resource: PortableSceneResource::StateDepth,
                limit: 1,
                attempted: 2,
            })
        );
        scene.restore().unwrap();
        assert_eq!(scene.restore(), Err(PortableSceneError::UnbalancedState));
    }

    #[test]
    fn triangle_batch_accounts_objects_and_vertices_without_one_command_per_triangle() {
        let triangles = [
            [
                point(px(0.0), px(0.0)),
                point(px(1.0), px(0.0)),
                point(px(0.0), px(1.0)),
            ],
            [
                point(px(2.0), px(0.0)),
                point(px(3.0), px(0.0)),
                point(px(2.0), px(1.0)),
            ],
        ];
        let mut scene = PortableScene2d::new();
        scene.push_triangles(&triangles, crate::white()).unwrap();
        let stats = scene.stats();
        assert_eq!(stats.command_count, 1);
        assert_eq!(stats.object_count, 2);
        assert_eq!(stats.path_count, 1);
        assert_eq!(stats.triangle_count, 2);
        assert_eq!(stats.path_vertex_count, 6);
    }

    #[test]
    fn decoded_image_resources_are_deduplicated_by_image_and_frame() {
        let buffer = image::ImageBuffer::from_pixel(2, 2, image::Rgba([1u8, 2, 3, 255]));
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        let sprite = PortableImageSprite::new(
            image,
            Bounds::new(point(px(0.0), px(0.0)), size(px(2.0), px(2.0))),
        );
        let mut scene = PortableScene2d::new();
        scene.push_image_sprites(&[sprite.clone(), sprite]).unwrap();
        let stats = scene.stats();
        assert_eq!(stats.image_sprite_count, 2);
        assert_eq!(stats.image_resource_count, 1);
        assert_eq!(stats.image_bytes, 16);
    }

    #[test]
    fn one_hundred_thousand_quads_fit_the_default_release_budget() {
        let mut scene = PortableScene2d::new();
        scene
            .try_reserve_commands(PORTABLE_SCENE_MAX_OBJECTS)
            .unwrap();
        let batch: Vec<_> = (0..PORTABLE_SCENE_MAX_OBJECTS).map(rect).collect();
        scene.push_solid_quads(&batch).unwrap();
        let stats = scene.stats();
        assert_eq!(stats.command_count, PORTABLE_SCENE_MAX_COMMANDS);
        assert_eq!(stats.object_count, PORTABLE_SCENE_MAX_OBJECTS);
        assert!(stats.estimated_retained_bytes <= PORTABLE_SCENE_MAX_RETAINED_BYTES);
    }

    #[test]
    fn portable_scene_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PortableScene2d>();
    }

    #[test]
    fn path_transforms_are_baked_once_and_bounded() {
        let triangle = [[
            point(px(1.0), px(2.0)),
            point(px(3.0), px(2.0)),
            point(px(1.0), px(4.0)),
        ]];
        let mut scene = PortableScene2d::new();
        scene.translate(px(10.0), px(20.0)).unwrap();
        scene.push_triangles(&triangle, crate::white()).unwrap();

        let PortableDrawCommand::FilledPath { path, state, .. } = &scene.commands[0] else {
            panic!("triangle batch should record one filled path")
        };
        assert_eq!(state.transform, TransformationMatrix::unit());
        assert_eq!(path.vertices[0].xy_position, point(px(11.0), px(22.0)));

        scene.clear();
        scene
            .scale(PORTABLE_SCENE_MAX_TRANSFORM_COMPONENT, 1.0)
            .unwrap();
        let oversized_after_transform = [[
            point(px(20.0), px(2.0)),
            point(px(21.0), px(2.0)),
            point(px(20.0), px(4.0)),
        ]];
        assert_eq!(
            scene.push_triangles(&oversized_after_transform, crate::white()),
            Err(PortableSceneError::InvalidInput {
                field: "transformed triangle vertices"
            })
        );
        assert!(scene.is_empty());
    }
}
