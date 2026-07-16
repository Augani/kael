//! Scene graph primitives for canvas and creative applications.
//!
//! Provides frame budget instrumentation, render statistics, spatial indexing
//! for hit testing, viewport pan/zoom transforms, a hierarchical scene graph,
//! transform handles, and alignment snapping.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

const MAX_FRAME_BUDGET_SAMPLES: usize = 100_000;
const MIN_TARGET_FPS: f64 = 0.001;
const MAX_TARGET_FPS: f64 = 1_000_000.0;
const MAX_SPATIAL_ENTRIES: usize = 100_000;
const MAX_SCENE_NODES: usize = 100_000;
const MAX_SCENE_DEPTH: usize = 256;
const MAX_SCENE_NAME_BYTES: usize = 1_024;

/// An axis-aligned rectangle used throughout the scene graph.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SceneRect<T> {
    /// X coordinate of the rectangle origin.
    pub x: T,
    /// Y coordinate of the rectangle origin.
    pub y: T,
    /// Width of the rectangle.
    pub width: T,
    /// Height of the rectangle.
    pub height: T,
}

impl<T> SceneRect<T> {
    /// Create a new rectangle from origin and dimensions.
    pub fn new(x: T, y: T, width: T, height: T) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl SceneRect<f64> {
    /// Return whether every coordinate is finite and dimensions are non-negative.
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f64::is_finite)
            && self.width >= 0.0
            && self.height >= 0.0
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }

    /// Returns `true` if the given point lies inside this rectangle.
    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        self.is_valid()
            && px.is_finite()
            && py.is_finite()
            && px >= self.x
            && px <= self.x + self.width
            && py >= self.y
            && py <= self.y + self.height
    }

    /// Returns `true` if this rectangle intersects another rectangle.
    pub fn intersects(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

// ---------------------------------------------------------------------------
// Frame Budget Instrumentation
// ---------------------------------------------------------------------------

/// Tracks frame timing against a target FPS budget.
pub struct FrameBudget {
    target_fps: f64,
    frame_times: VecDeque<Duration>,
    max_samples: usize,
}

impl FrameBudget {
    /// Create a new frame budget tracker.
    pub fn new(target_fps: f64, max_samples: usize) -> Self {
        let target_fps = if target_fps.is_finite() && target_fps > 0.0 {
            target_fps.clamp(MIN_TARGET_FPS, MAX_TARGET_FPS)
        } else {
            60.0
        };
        let max_samples = max_samples.clamp(1, MAX_FRAME_BUDGET_SAMPLES);
        Self {
            target_fps,
            frame_times: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    /// Create a tracker after validating its target and retention bound.
    pub fn new_checked(target_fps: f64, max_samples: usize) -> Result<Self> {
        anyhow::ensure!(
            target_fps.is_finite() && (MIN_TARGET_FPS..=MAX_TARGET_FPS).contains(&target_fps),
            "target FPS is out of range"
        );
        anyhow::ensure!(
            max_samples > 0 && max_samples <= MAX_FRAME_BUDGET_SAMPLES,
            "frame budget sample count is out of range"
        );
        Ok(Self::new(target_fps, max_samples))
    }

    /// Record a single frame's duration.
    pub fn record_frame(&mut self, duration: Duration) {
        if self.frame_times.len() >= self.max_samples {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(duration);
    }

    /// Target frame time in milliseconds.
    pub fn budget_ms(&self) -> f64 {
        1000.0 / self.target_fps
    }

    /// Average recorded frame time in milliseconds.
    pub fn average_ms(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let total_seconds = self
            .frame_times
            .iter()
            .map(Duration::as_secs_f64)
            .sum::<f64>();
        total_seconds * 1000.0 / self.frame_times.len() as f64
    }

    /// Number of recorded frames that exceeded the budget.
    pub fn over_budget_count(&self) -> usize {
        let budget = Duration::from_secs_f64(1.0 / self.target_fps);
        self.frame_times.iter().filter(|t| **t > budget).count()
    }

    /// Average frame time as a percentage of the budget (0-100+).
    pub fn utilization(&self) -> f64 {
        let budget = self.budget_ms();
        if budget <= 0.0 {
            return 0.0;
        }
        (self.average_ms() / budget) * 100.0
    }
}

// ---------------------------------------------------------------------------
// Render Statistics
// ---------------------------------------------------------------------------

/// Accumulated statistics for a render pass.
pub struct RenderStats {
    /// Number of layout calculations performed.
    pub layout_count: u64,
    /// Number of paint operations performed.
    pub paint_count: u64,
    /// Total scene nodes processed.
    pub scene_node_count: u64,
    /// Bytes uploaded to the GPU this frame.
    pub gpu_upload_bytes: u64,
    /// Current texture atlas memory usage in bytes.
    pub texture_atlas_bytes: u64,
    /// Ratio of pixels drawn to visible pixels.
    pub overdraw_ratio: f64,
}

impl RenderStats {
    /// Create zeroed render statistics.
    pub fn new() -> Self {
        Self {
            layout_count: 0,
            paint_count: 0,
            scene_node_count: 0,
            gpu_upload_bytes: 0,
            texture_atlas_bytes: 0,
            overdraw_ratio: 0.0,
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&mut self) {
        self.layout_count = 0;
        self.paint_count = 0;
        self.scene_node_count = 0;
        self.gpu_upload_bytes = 0;
        self.texture_atlas_bytes = 0;
        self.overdraw_ratio = 0.0;
    }

    /// Increment the layout counter.
    pub fn record_layout(&mut self) {
        self.layout_count = self.layout_count.saturating_add(1);
    }

    /// Increment the paint counter.
    pub fn record_paint(&mut self) {
        self.paint_count = self.paint_count.saturating_add(1);
    }

    /// Set the total scene node count.
    pub fn set_scene_nodes(&mut self, count: u64) {
        self.scene_node_count = count;
    }

    /// Add bytes to the GPU upload counter.
    pub fn add_gpu_upload(&mut self, bytes: u64) {
        self.gpu_upload_bytes = self.gpu_upload_bytes.saturating_add(bytes);
    }

    /// Set the texture atlas byte count.
    pub fn set_atlas_bytes(&mut self, bytes: u64) {
        self.texture_atlas_bytes = bytes;
    }

    /// Set the overdraw ratio.
    pub fn set_overdraw(&mut self, ratio: f64) {
        self.overdraw_ratio = if ratio.is_finite() && ratio >= 0.0 {
            ratio
        } else {
            0.0
        };
    }
}

impl Default for RenderStats {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Spatial Index
// ---------------------------------------------------------------------------

/// An entry in the spatial index associating bounds with arbitrary data.
pub struct SpatialEntry<T> {
    /// Bounding rectangle for this entry.
    pub bounds: SceneRect<f64>,
    /// Payload data associated with these bounds.
    pub data: T,
}

/// A brute-force spatial index for hit testing and region queries.
pub struct SpatialIndex<T> {
    entries: Vec<SpatialEntry<T>>,
}

impl<T> SpatialIndex<T> {
    /// Create an empty spatial index.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Insert an entry with the given bounds and data.
    pub fn insert(&mut self, bounds: SceneRect<f64>, data: T) {
        let _ = self.insert_checked(bounds, data);
    }

    /// Insert a bounded entry after validating its geometry.
    pub fn insert_checked(&mut self, bounds: SceneRect<f64>, data: T) -> Result<()> {
        anyhow::ensure!(bounds.is_valid(), "spatial bounds are invalid");
        anyhow::ensure!(
            self.entries.len() < MAX_SPATIAL_ENTRIES,
            "spatial index cannot exceed {MAX_SPATIAL_ENTRIES} entries"
        );
        self.entries.push(SpatialEntry { bounds, data });
        Ok(())
    }

    /// Return all entries whose bounds contain the given point.
    pub fn query(&self, point_x: f64, point_y: f64) -> Vec<&T> {
        self.entries
            .iter()
            .filter(|e| e.bounds.contains_point(point_x, point_y))
            .map(|e| &e.data)
            .collect()
    }

    /// Return all entries whose bounds intersect the given rectangle.
    pub fn query_rect(&self, rect: &SceneRect<f64>) -> Vec<&T> {
        self.entries
            .iter()
            .filter(|e| e.bounds.intersects(rect))
            .map(|e| &e.data)
            .collect()
    }

    /// Remove and return the entry at the given index, if it exists.
    pub fn remove_at(&mut self, index: usize) -> Option<SpatialEntry<T>> {
        if index < self.entries.len() {
            Some(self.entries.remove(index))
        } else {
            None
        }
    }

    /// Number of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the index contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<T> Default for SpatialIndex<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Viewport Transform
// ---------------------------------------------------------------------------

/// A 2D affine transform representing pan and zoom for a canvas viewport.
pub struct ViewportTransform {
    /// Current zoom scale factor.
    pub scale: f64,
    /// Horizontal scroll offset in screen pixels.
    pub offset_x: f64,
    /// Vertical scroll offset in screen pixels.
    pub offset_y: f64,
    /// Minimum allowed scale factor.
    pub min_scale: f64,
    /// Maximum allowed scale factor.
    pub max_scale: f64,
}

impl ViewportTransform {
    /// Create an identity viewport transform (scale=1, no offset).
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            min_scale: 0.01,
            max_scale: 100.0,
        }
    }

    /// Translate the viewport by `(dx, dy)` in screen space.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let _ = self.pan_checked(dx, dy);
    }

    /// Translate the viewport while rejecting non-finite or overflowing offsets.
    pub fn pan_checked(&mut self, dx: f64, dy: f64) -> Result<()> {
        anyhow::ensure!(
            dx.is_finite() && dy.is_finite(),
            "viewport pan must be finite"
        );
        let offset_x = self.offset_x + dx;
        let offset_y = self.offset_y + dy;
        anyhow::ensure!(
            offset_x.is_finite() && offset_y.is_finite(),
            "viewport pan overflowed"
        );
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        Ok(())
    }

    /// Zoom by `factor` relative to a center point in screen coordinates.
    pub fn zoom(&mut self, factor: f64, center_x: f64, center_y: f64) {
        let _ = self.zoom_checked(factor, center_x, center_y);
    }

    /// Zoom while validating scale limits, factor, center, and resulting offsets.
    pub fn zoom_checked(&mut self, factor: f64, center_x: f64, center_y: f64) -> Result<()> {
        self.validate()?;
        anyhow::ensure!(
            factor.is_finite() && factor > 0.0,
            "viewport zoom factor must be finite and positive"
        );
        anyhow::ensure!(
            center_x.is_finite() && center_y.is_finite(),
            "viewport zoom center must be finite"
        );
        let new_scale = (self.scale * factor).clamp(self.min_scale, self.max_scale);
        let actual_factor = new_scale / self.scale;
        let offset_x = center_x - actual_factor * (center_x - self.offset_x);
        let offset_y = center_y - actual_factor * (center_y - self.offset_y);
        anyhow::ensure!(
            new_scale.is_finite() && offset_x.is_finite() && offset_y.is_finite(),
            "viewport zoom overflowed"
        );
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        self.scale = new_scale;
        Ok(())
    }

    /// Set the scale, clamped to `[min_scale, max_scale]`.
    pub fn set_scale(&mut self, scale: f64) {
        let _ = self.set_scale_checked(scale);
    }

    /// Set scale after validating the configured range.
    pub fn set_scale_checked(&mut self, scale: f64) -> Result<()> {
        self.validate()?;
        anyhow::ensure!(
            scale.is_finite() && scale > 0.0,
            "viewport scale must be finite and positive"
        );
        self.scale = scale.clamp(self.min_scale, self.max_scale);
        Ok(())
    }

    /// Convert screen coordinates to world coordinates.
    pub fn screen_to_world(&self, screen_x: f64, screen_y: f64) -> (f64, f64) {
        self.screen_to_world_checked(screen_x, screen_y)
            .unwrap_or((0.0, 0.0))
    }

    /// Convert screen coordinates after validating the viewport and inputs.
    pub fn screen_to_world_checked(&self, screen_x: f64, screen_y: f64) -> Result<(f64, f64)> {
        self.validate()?;
        anyhow::ensure!(
            screen_x.is_finite() && screen_y.is_finite(),
            "screen coordinates must be finite"
        );
        let result = (
            (screen_x - self.offset_x) / self.scale,
            (screen_y - self.offset_y) / self.scale,
        );
        anyhow::ensure!(
            result.0.is_finite() && result.1.is_finite(),
            "world coordinates overflowed"
        );
        Ok(result)
    }

    /// Convert world coordinates to screen coordinates.
    pub fn world_to_screen(&self, world_x: f64, world_y: f64) -> (f64, f64) {
        self.world_to_screen_checked(world_x, world_y)
            .unwrap_or((0.0, 0.0))
    }

    /// Convert world coordinates after validating the viewport and inputs.
    pub fn world_to_screen_checked(&self, world_x: f64, world_y: f64) -> Result<(f64, f64)> {
        self.validate()?;
        anyhow::ensure!(
            world_x.is_finite() && world_y.is_finite(),
            "world coordinates must be finite"
        );
        let result = (
            world_x * self.scale + self.offset_x,
            world_y * self.scale + self.offset_y,
        );
        anyhow::ensure!(
            result.0.is_finite() && result.1.is_finite(),
            "screen coordinates overflowed"
        );
        Ok(result)
    }

    /// Reset to identity (scale=1, no offset).
    pub fn reset(&mut self) {
        self.scale = 1.0;
        self.offset_x = 0.0;
        self.offset_y = 0.0;
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.scale.is_finite() && self.scale > 0.0,
            "viewport scale is invalid"
        );
        anyhow::ensure!(
            self.offset_x.is_finite() && self.offset_y.is_finite(),
            "viewport offset is invalid"
        );
        anyhow::ensure!(
            self.min_scale.is_finite()
                && self.max_scale.is_finite()
                && self.min_scale > 0.0
                && self.min_scale <= self.max_scale,
            "viewport scale limits are invalid"
        );
        Ok(())
    }
}

impl Default for ViewportTransform {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Scene Graph
// ---------------------------------------------------------------------------

/// Unique identifier for a scene node.
pub type SceneNodeId = u64;

/// A node in the scene graph representing a visual element on the canvas.
pub struct SceneNode {
    /// Unique identifier.
    pub id: SceneNodeId,
    /// Axis-aligned bounding box in world space.
    pub bounds: SceneRect<f64>,
    /// Whether this node should be rendered.
    pub visible: bool,
    /// Whether this node is locked from editing.
    pub locked: bool,
    /// Human-readable name for the node.
    pub name: String,
    /// Ordered list of child node identifiers.
    pub children: Vec<SceneNodeId>,
    /// Parent node, if any.
    pub parent: Option<SceneNodeId>,
}

/// A hierarchical scene graph for canvas/creative applications.
pub struct SceneGraph {
    nodes: HashMap<SceneNodeId, SceneNode>,
    roots: Vec<SceneNodeId>,
    next_id: SceneNodeId,
}

impl SceneGraph {
    /// Create an empty scene graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            roots: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a node with the given name and bounds, optionally parented to another node.
    pub fn add_node(
        &mut self,
        name: impl Into<String>,
        bounds: SceneRect<f64>,
        parent: Option<SceneNodeId>,
    ) -> SceneNodeId {
        let parent = parent.filter(|parent| self.nodes.contains_key(parent));
        self.add_node_checked(name, bounds, parent).unwrap_or(0)
    }

    /// Add a validated node while enforcing graph size, depth, and identifier bounds.
    pub fn add_node_checked(
        &mut self,
        name: impl Into<String>,
        bounds: SceneRect<f64>,
        parent: Option<SceneNodeId>,
    ) -> Result<SceneNodeId> {
        anyhow::ensure!(
            self.nodes.len() < MAX_SCENE_NODES,
            "scene graph cannot exceed {MAX_SCENE_NODES} nodes"
        );
        anyhow::ensure!(bounds.is_valid(), "scene node bounds are invalid");
        let name = name.into();
        anyhow::ensure!(!name.trim().is_empty(), "scene node name cannot be empty");
        anyhow::ensure!(
            name.len() <= MAX_SCENE_NAME_BYTES,
            "scene node name cannot exceed {MAX_SCENE_NAME_BYTES} bytes"
        );
        anyhow::ensure!(
            !name.chars().any(char::is_control),
            "scene node name cannot contain control characters"
        );
        if let Some(parent_id) = parent {
            anyhow::ensure!(
                self.nodes.contains_key(&parent_id),
                "scene parent does not exist"
            );
            anyhow::ensure!(
                self.ancestor_depth(parent_id)? < MAX_SCENE_DEPTH,
                "scene graph cannot exceed depth {MAX_SCENE_DEPTH}"
            );
        }
        let id = self.allocate_id()?;

        let node = SceneNode {
            id,
            bounds,
            visible: true,
            locked: false,
            name,
            children: Vec::new(),
            parent,
        };

        self.nodes.insert(id, node);

        if let Some(parent_id) = parent {
            self.nodes
                .get_mut(&parent_id)
                .expect("validated scene parent disappeared")
                .children
                .push(id);
        } else {
            self.roots.push(id);
        }

        Ok(id)
    }

    /// Remove a node and all its descendants. Returns the removed node (without children removed).
    pub fn remove_node(&mut self, id: SceneNodeId) -> Option<SceneNode> {
        let node = self.nodes.remove(&id)?;
        let mut removed_ids = HashSet::from([id]);
        let mut pending = node.children.clone();
        while let Some(child_id) = pending.pop() {
            if !removed_ids.insert(child_id) {
                continue;
            }
            if let Some(child) = self.nodes.remove(&child_id) {
                pending.extend(child.children);
            }
        }
        for remaining in self.nodes.values_mut() {
            remaining
                .children
                .retain(|child| !removed_ids.contains(child));
        }
        self.roots.retain(|root| !removed_ids.contains(root));

        Some(node)
    }

    /// Get a reference to a node by id.
    pub fn get(&self, id: SceneNodeId) -> Option<&SceneNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable reference to a node by id.
    pub fn get_mut(&mut self, id: SceneNodeId) -> Option<&mut SceneNode> {
        self.nodes.get_mut(&id)
    }

    /// Return the list of root node identifiers.
    pub fn roots(&self) -> &[SceneNodeId] {
        &self.roots
    }

    /// Total number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Find all visible nodes whose bounds contain the given point, topmost first.
    pub fn hit_test(&self, x: f64, y: f64) -> Vec<SceneNodeId> {
        if !x.is_finite() || !y.is_finite() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        let mut visited = HashSet::new();
        let mut pending = self
            .roots
            .iter()
            .copied()
            .map(|root| (root, false))
            .collect::<Vec<_>>();
        while let Some((node_id, children_visited)) = pending.pop() {
            let Some(node) = self.nodes.get(&node_id) else {
                continue;
            };
            if children_visited {
                if node.visible && node.bounds.contains_point(x, y) {
                    hits.push(node_id);
                }
                continue;
            }
            if !node.visible || !visited.insert(node_id) {
                continue;
            }
            pending.push((node_id, true));
            pending.extend(node.children.iter().copied().map(|child| (child, false)));
        }
        hits
    }

    /// Move a node by the given delta, updating its bounds.
    pub fn move_node(&mut self, id: SceneNodeId, dx: f64, dy: f64) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| anyhow!("node {} not found", id))?;
        anyhow::ensure!(!node.locked, "locked scene node cannot be moved");
        anyhow::ensure!(
            dx.is_finite() && dy.is_finite(),
            "scene movement must be finite"
        );
        let x = node.bounds.x + dx;
        let y = node.bounds.y + dy;
        anyhow::ensure!(x.is_finite() && y.is_finite(), "scene movement overflowed");
        node.bounds.x = x;
        node.bounds.y = y;
        Ok(())
    }

    /// Move a node to a new parent (or to root if `new_parent` is `None`).
    pub fn reparent(&mut self, id: SceneNodeId, new_parent: Option<SceneNodeId>) -> Result<()> {
        if !self.nodes.contains_key(&id) {
            return Err(anyhow!("node {} not found", id));
        }
        if let Some(np) = new_parent {
            if !self.nodes.contains_key(&np) {
                return Err(anyhow!("new parent {} not found", np));
            }
            if np == id {
                return Err(anyhow!("cannot parent a node to itself"));
            }
            let mut ancestor = Some(np);
            let mut visited = HashSet::new();
            while let Some(ancestor_id) = ancestor {
                anyhow::ensure!(
                    visited.insert(ancestor_id),
                    "scene graph contains a parent cycle"
                );
                anyhow::ensure!(
                    ancestor_id != id,
                    "cannot parent a node beneath its descendant"
                );
                ancestor = self.nodes.get(&ancestor_id).and_then(|node| node.parent);
            }
            let parent_depth = self.ancestor_depth(np)?;
            let subtree_height = self.subtree_height(id)?;
            anyhow::ensure!(
                parent_depth.saturating_add(subtree_height) <= MAX_SCENE_DEPTH,
                "scene graph cannot exceed depth {MAX_SCENE_DEPTH}"
            );
        }

        anyhow::ensure!(
            !self.nodes[&id].locked,
            "locked scene node cannot be reparented"
        );

        let old_parent = self.nodes.get(&id).and_then(|n| n.parent);

        if let Some(old_parent_id) = old_parent {
            if let Some(parent_node) = self.nodes.get_mut(&old_parent_id) {
                parent_node.children.retain(|c| *c != id);
            }
        } else {
            self.roots.retain(|r| *r != id);
        }

        if let Some(new_parent_id) = new_parent {
            if let Some(parent_node) = self.nodes.get_mut(&new_parent_id) {
                parent_node.children.push(id);
            }
        } else {
            self.roots.push(id);
        }

        if let Some(node) = self.nodes.get_mut(&id) {
            node.parent = new_parent;
        }

        Ok(())
    }

    fn allocate_id(&mut self) -> Result<SceneNodeId> {
        for _ in 0..=self.nodes.len() {
            let id = self.next_id.max(1);
            self.next_id = id.wrapping_add(1).max(1);
            if !self.nodes.contains_key(&id) {
                return Ok(id);
            }
        }
        anyhow::bail!("scene node identifier space is exhausted")
    }

    fn ancestor_depth(&self, id: SceneNodeId) -> Result<usize> {
        let mut depth = 0;
        let mut current = Some(id);
        let mut visited = HashSet::new();
        while let Some(node_id) = current {
            anyhow::ensure!(
                visited.insert(node_id),
                "scene graph contains a parent cycle"
            );
            depth += 1;
            anyhow::ensure!(
                depth <= MAX_SCENE_DEPTH,
                "scene graph cannot exceed depth {MAX_SCENE_DEPTH}"
            );
            current = self.nodes.get(&node_id).and_then(|node| node.parent);
        }
        Ok(depth)
    }

    fn subtree_height(&self, id: SceneNodeId) -> Result<usize> {
        let mut max_depth = 0;
        let mut pending = vec![(id, 1_usize)];
        let mut visited = HashSet::new();
        while let Some((node_id, depth)) = pending.pop() {
            anyhow::ensure!(
                visited.insert(node_id),
                "scene graph contains a child cycle"
            );
            max_depth = max_depth.max(depth);
            anyhow::ensure!(
                max_depth <= MAX_SCENE_DEPTH,
                "scene graph cannot exceed depth {MAX_SCENE_DEPTH}"
            );
            if let Some(node) = self.nodes.get(&node_id) {
                pending.extend(
                    node.children
                        .iter()
                        .copied()
                        .map(|child| (child, depth + 1)),
                );
            }
        }
        Ok(max_depth)
    }
}

impl Default for SceneGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Transform Handles
// ---------------------------------------------------------------------------

/// Position of a transform handle on a bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlePosition {
    /// Top-left corner.
    TopLeft,
    /// Top center.
    Top,
    /// Top-right corner.
    TopRight,
    /// Right center.
    Right,
    /// Bottom-right corner.
    BottomRight,
    /// Bottom center.
    Bottom,
    /// Bottom-left corner.
    BottomLeft,
    /// Left center.
    Left,
}

/// A transform handle at a specific position on a bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformHandle {
    /// Which edge/corner this handle occupies.
    pub position: HandlePosition,
    /// X coordinate of the handle in world space.
    pub x: f64,
    /// Y coordinate of the handle in world space.
    pub y: f64,
}

/// Compute the eight transform handles for the given bounding rectangle.
pub fn compute_handles(bounds: &SceneRect<f64>) -> Vec<TransformHandle> {
    compute_handles_checked(bounds).unwrap_or_default()
}

/// Compute transform handles after validating the bounding rectangle.
pub fn compute_handles_checked(bounds: &SceneRect<f64>) -> Result<Vec<TransformHandle>> {
    anyhow::ensure!(bounds.is_valid(), "transform handle bounds are invalid");
    let x = bounds.x;
    let y = bounds.y;
    let mx = x + bounds.width / 2.0;
    let my = y + bounds.height / 2.0;
    let rx = x + bounds.width;
    let by = y + bounds.height;

    Ok(vec![
        TransformHandle {
            position: HandlePosition::TopLeft,
            x,
            y,
        },
        TransformHandle {
            position: HandlePosition::Top,
            x: mx,
            y,
        },
        TransformHandle {
            position: HandlePosition::TopRight,
            x: rx,
            y,
        },
        TransformHandle {
            position: HandlePosition::Right,
            x: rx,
            y: my,
        },
        TransformHandle {
            position: HandlePosition::BottomRight,
            x: rx,
            y: by,
        },
        TransformHandle {
            position: HandlePosition::Bottom,
            x: mx,
            y: by,
        },
        TransformHandle {
            position: HandlePosition::BottomLeft,
            x,
            y: by,
        },
        TransformHandle {
            position: HandlePosition::Left,
            x,
            y: my,
        },
    ])
}

// ---------------------------------------------------------------------------
// Snapping
// ---------------------------------------------------------------------------

/// The axis along which a snap guide runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapAxis {
    /// A horizontal guide (constant Y value).
    Horizontal,
    /// A vertical guide (constant X value).
    Vertical,
}

/// A visual guide line produced by snapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapGuide {
    /// Which axis this guide runs along.
    pub axis: SnapAxis,
    /// Position of the guide in world units.
    pub position: f64,
}

/// The result of a snap operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapResult {
    /// Snapped X coordinate.
    pub snapped_x: f64,
    /// Snapped Y coordinate.
    pub snapped_y: f64,
    /// Active snap guides.
    pub guides: Vec<SnapGuide>,
}

/// Snap a point to nearby edges/centers of the given rectangles.
///
/// The `threshold` specifies the maximum snap distance in world units.
/// Returns the snapped position and any guides that were activated.
pub fn snap_to_guides(x: f64, y: f64, all_bounds: &[SceneRect<f64>], threshold: f64) -> SnapResult {
    snap_to_guides_checked(x, y, all_bounds, threshold).unwrap_or(SnapResult {
        snapped_x: if x.is_finite() { x } else { 0.0 },
        snapped_y: if y.is_finite() { y } else { 0.0 },
        guides: Vec::new(),
    })
}

/// Snap a point after validating coordinates, threshold, and guide geometry.
pub fn snap_to_guides_checked(
    x: f64,
    y: f64,
    all_bounds: &[SceneRect<f64>],
    threshold: f64,
) -> Result<SnapResult> {
    anyhow::ensure!(x.is_finite() && y.is_finite(), "snap point must be finite");
    anyhow::ensure!(
        threshold.is_finite() && threshold >= 0.0,
        "snap threshold must be finite and non-negative"
    );
    anyhow::ensure!(
        all_bounds.iter().all(SceneRect::is_valid),
        "snap guide bounds are invalid"
    );
    let mut best_dx = f64::MAX;
    let mut best_dy = f64::MAX;
    let mut snapped_x = x;
    let mut snapped_y = y;
    let mut guides = Vec::new();

    for bounds in all_bounds {
        let snap_xs = [
            bounds.x,
            bounds.x + bounds.width / 2.0,
            bounds.x + bounds.width,
        ];
        let snap_ys = [
            bounds.y,
            bounds.y + bounds.height / 2.0,
            bounds.y + bounds.height,
        ];

        for sx in &snap_xs {
            let dist = (x - sx).abs();
            if dist < threshold && dist < best_dx {
                best_dx = dist;
                snapped_x = *sx;
            }
        }

        for sy in &snap_ys {
            let dist = (y - sy).abs();
            if dist < threshold && dist < best_dy {
                best_dy = dist;
                snapped_y = *sy;
            }
        }
    }

    if best_dx < f64::MAX {
        guides.push(SnapGuide {
            axis: SnapAxis::Vertical,
            position: snapped_x,
        });
    }
    if best_dy < f64::MAX {
        guides.push(SnapGuide {
            axis: SnapAxis::Horizontal,
            position: snapped_y,
        });
    }

    Ok(SnapResult {
        snapped_x,
        snapped_y,
        guides,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> SceneRect<f64> {
        SceneRect::new(x, y, w, h)
    }

    #[test]
    fn scene_rect_contains_point() {
        let r = rect(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains_point(10.0, 20.0));
        assert!(r.contains_point(60.0, 45.0));
        assert!(r.contains_point(110.0, 70.0));
        assert!(!r.contains_point(9.9, 20.0));
        assert!(!r.contains_point(60.0, 71.0));
    }

    #[test]
    fn scene_rect_intersects() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(5.0, 5.0, 10.0, 10.0);
        let c = rect(20.0, 20.0, 5.0, 5.0);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn frame_budget_basic() {
        let mut fb = FrameBudget::new(60.0, 10);
        assert!((fb.budget_ms() - 16.6666).abs() < 0.01);
        assert_eq!(fb.average_ms(), 0.0);

        fb.record_frame(Duration::from_millis(8));
        fb.record_frame(Duration::from_millis(12));
        assert!((fb.average_ms() - 10.0).abs() < 0.01);
        assert_eq!(fb.over_budget_count(), 0);
    }

    #[test]
    fn frame_budget_over_budget() {
        let mut fb = FrameBudget::new(60.0, 5);
        fb.record_frame(Duration::from_millis(20));
        fb.record_frame(Duration::from_millis(10));
        fb.record_frame(Duration::from_millis(25));
        assert_eq!(fb.over_budget_count(), 2);
    }

    #[test]
    fn frame_budget_max_samples() {
        let mut fb = FrameBudget::new(60.0, 3);
        for i in 0..5 {
            fb.record_frame(Duration::from_millis(10 + i));
        }
        assert_eq!(fb.frame_times.len(), 3);
    }

    #[test]
    fn frame_budget_utilization() {
        let mut fb = FrameBudget::new(60.0, 10);
        let budget_dur = Duration::from_secs_f64(1.0 / 60.0);
        fb.record_frame(budget_dur);
        assert!((fb.utilization() - 100.0).abs() < 0.1);
    }

    #[test]
    fn render_stats_lifecycle() {
        let mut stats = RenderStats::new();
        assert_eq!(stats.layout_count, 0);

        stats.record_layout();
        stats.record_layout();
        stats.record_paint();
        stats.set_scene_nodes(42);
        stats.add_gpu_upload(1024);
        stats.add_gpu_upload(2048);
        stats.set_atlas_bytes(4096);
        stats.set_overdraw(1.5);

        assert_eq!(stats.layout_count, 2);
        assert_eq!(stats.paint_count, 1);
        assert_eq!(stats.scene_node_count, 42);
        assert_eq!(stats.gpu_upload_bytes, 3072);
        assert_eq!(stats.texture_atlas_bytes, 4096);
        assert!((stats.overdraw_ratio - 1.5).abs() < f64::EPSILON);

        stats.reset();
        assert_eq!(stats.layout_count, 0);
        assert_eq!(stats.gpu_upload_bytes, 0);
    }

    #[test]
    fn spatial_index_point_query() {
        let mut idx = SpatialIndex::new();
        idx.insert(rect(0.0, 0.0, 10.0, 10.0), "a");
        idx.insert(rect(5.0, 5.0, 10.0, 10.0), "b");
        idx.insert(rect(20.0, 20.0, 5.0, 5.0), "c");

        let hits = idx.query(7.0, 7.0);
        assert_eq!(hits.len(), 2);
        assert!(hits.contains(&&"a"));
        assert!(hits.contains(&&"b"));

        let hits = idx.query(22.0, 22.0);
        assert_eq!(hits.len(), 1);
        assert!(hits.contains(&&"c"));

        let hits = idx.query(100.0, 100.0);
        assert!(hits.is_empty());
    }

    #[test]
    fn spatial_index_rect_query() {
        let mut idx = SpatialIndex::new();
        idx.insert(rect(0.0, 0.0, 10.0, 10.0), 1);
        idx.insert(rect(20.0, 20.0, 10.0, 10.0), 2);

        let query = rect(5.0, 5.0, 20.0, 20.0);
        let hits = idx.query_rect(&query);
        assert_eq!(hits.len(), 2);

        let query = rect(50.0, 50.0, 5.0, 5.0);
        let hits = idx.query_rect(&query);
        assert!(hits.is_empty());
    }

    #[test]
    fn spatial_index_remove_and_clear() {
        let mut idx = SpatialIndex::new();
        idx.insert(rect(0.0, 0.0, 10.0, 10.0), "a");
        idx.insert(rect(10.0, 10.0, 10.0, 10.0), "b");
        assert_eq!(idx.len(), 2);

        let removed = idx.remove_at(0).unwrap();
        assert_eq!(removed.data, "a");
        assert_eq!(idx.len(), 1);

        assert!(idx.remove_at(99).is_none());

        idx.clear();
        assert!(idx.is_empty());
    }

    #[test]
    fn viewport_identity() {
        let vt = ViewportTransform::new();
        assert_eq!(vt.screen_to_world(100.0, 200.0), (100.0, 200.0));
        assert_eq!(vt.world_to_screen(100.0, 200.0), (100.0, 200.0));
    }

    #[test]
    fn viewport_pan() {
        let mut vt = ViewportTransform::new();
        vt.pan(50.0, -30.0);
        let (wx, wy) = vt.screen_to_world(50.0, -30.0);
        assert!((wx - 0.0).abs() < 1e-10);
        assert!((wy - 0.0).abs() < 1e-10);
    }

    #[test]
    fn viewport_zoom() {
        let mut vt = ViewportTransform::new();
        vt.zoom(2.0, 0.0, 0.0);
        assert!((vt.scale - 2.0).abs() < 1e-10);

        let (sx, sy) = vt.world_to_screen(10.0, 10.0);
        assert!((sx - 20.0).abs() < 1e-10);
        assert!((sy - 20.0).abs() < 1e-10);
    }

    #[test]
    fn viewport_zoom_clamp() {
        let mut vt = ViewportTransform::new();
        vt.min_scale = 0.5;
        vt.max_scale = 4.0;
        vt.zoom(0.1, 0.0, 0.0);
        assert!((vt.scale - 0.5).abs() < 1e-10);

        vt.scale = 1.0;
        vt.zoom(100.0, 0.0, 0.0);
        assert!((vt.scale - 4.0).abs() < 1e-10);
    }

    #[test]
    fn viewport_screen_world_roundtrip() {
        let mut vt = ViewportTransform::new();
        vt.pan(100.0, 200.0);
        vt.zoom(2.5, 0.0, 0.0);

        let (wx, wy) = vt.screen_to_world(300.0, 400.0);
        let (sx, sy) = vt.world_to_screen(wx, wy);
        assert!((sx - 300.0).abs() < 1e-8);
        assert!((sy - 400.0).abs() < 1e-8);
    }

    #[test]
    fn viewport_reset() {
        let mut vt = ViewportTransform::new();
        vt.pan(50.0, 50.0);
        vt.zoom(3.0, 0.0, 0.0);
        vt.reset();
        assert!((vt.scale - 1.0).abs() < 1e-10);
        assert!((vt.offset_x).abs() < 1e-10);
        assert!((vt.offset_y).abs() < 1e-10);
    }

    #[test]
    fn scene_graph_add_and_get() {
        let mut sg = SceneGraph::new();
        let id = sg.add_node("root", rect(0.0, 0.0, 100.0, 100.0), None);
        assert_eq!(sg.node_count(), 1);
        assert_eq!(sg.roots().len(), 1);

        let node = sg.get(id).unwrap();
        assert_eq!(node.name, "root");
        assert!(node.visible);
        assert!(!node.locked);
    }

    #[test]
    fn scene_graph_parent_child() {
        let mut sg = SceneGraph::new();
        let parent = sg.add_node("parent", rect(0.0, 0.0, 100.0, 100.0), None);
        let child = sg.add_node("child", rect(10.0, 10.0, 50.0, 50.0), Some(parent));

        assert_eq!(sg.node_count(), 2);
        assert_eq!(sg.roots().len(), 1);

        let parent_node = sg.get(parent).unwrap();
        assert_eq!(parent_node.children.len(), 1);
        assert_eq!(parent_node.children[0], child);

        let child_node = sg.get(child).unwrap();
        assert_eq!(child_node.parent, Some(parent));
    }

    #[test]
    fn scene_graph_remove_node_with_children() {
        let mut sg = SceneGraph::new();
        let parent = sg.add_node("parent", rect(0.0, 0.0, 100.0, 100.0), None);
        let _child1 = sg.add_node("child1", rect(10.0, 10.0, 30.0, 30.0), Some(parent));
        let _child2 = sg.add_node("child2", rect(50.0, 50.0, 30.0, 30.0), Some(parent));

        assert_eq!(sg.node_count(), 3);
        sg.remove_node(parent);
        assert_eq!(sg.node_count(), 0);
        assert!(sg.roots().is_empty());
    }

    #[test]
    fn scene_graph_hit_test() {
        let mut sg = SceneGraph::new();
        let a = sg.add_node("a", rect(0.0, 0.0, 100.0, 100.0), None);
        let b = sg.add_node("b", rect(50.0, 50.0, 100.0, 100.0), None);

        let hits = sg.hit_test(75.0, 75.0);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0], b);
        assert_eq!(hits[1], a);

        let hits = sg.hit_test(25.0, 25.0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], a);
    }

    #[test]
    fn scene_graph_hit_test_hidden() {
        let mut sg = SceneGraph::new();
        let a = sg.add_node("a", rect(0.0, 0.0, 100.0, 100.0), None);
        sg.get_mut(a).unwrap().visible = false;

        let hits = sg.hit_test(50.0, 50.0);
        assert!(hits.is_empty());
    }

    #[test]
    fn scene_graph_move_node() {
        let mut sg = SceneGraph::new();
        let id = sg.add_node("box", rect(10.0, 20.0, 30.0, 40.0), None);
        sg.move_node(id, 5.0, -3.0).unwrap();

        let node = sg.get(id).unwrap();
        assert!((node.bounds.x - 15.0).abs() < 1e-10);
        assert!((node.bounds.y - 17.0).abs() < 1e-10);
    }

    #[test]
    fn scene_graph_move_node_not_found() {
        let mut sg = SceneGraph::new();
        assert!(sg.move_node(999, 1.0, 1.0).is_err());
    }

    #[test]
    fn scene_graph_reparent() {
        let mut sg = SceneGraph::new();
        let a = sg.add_node("a", rect(0.0, 0.0, 10.0, 10.0), None);
        let b = sg.add_node("b", rect(20.0, 20.0, 10.0, 10.0), None);

        assert_eq!(sg.roots().len(), 2);

        sg.reparent(b, Some(a)).unwrap();
        assert_eq!(sg.roots().len(), 1);
        assert_eq!(sg.get(a).unwrap().children.len(), 1);
        assert_eq!(sg.get(b).unwrap().parent, Some(a));
    }

    #[test]
    fn scene_graph_reparent_to_root() {
        let mut sg = SceneGraph::new();
        let a = sg.add_node("a", rect(0.0, 0.0, 10.0, 10.0), None);
        let b = sg.add_node("b", rect(0.0, 0.0, 5.0, 5.0), Some(a));

        sg.reparent(b, None).unwrap();
        assert_eq!(sg.roots().len(), 2);
        assert!(sg.get(a).unwrap().children.is_empty());
        assert_eq!(sg.get(b).unwrap().parent, None);
    }

    #[test]
    fn scene_graph_reparent_errors() {
        let mut sg = SceneGraph::new();
        let a = sg.add_node("a", rect(0.0, 0.0, 10.0, 10.0), None);

        assert!(sg.reparent(999, Some(a)).is_err());
        assert!(sg.reparent(a, Some(999)).is_err());
        assert!(sg.reparent(a, Some(a)).is_err());
    }

    #[test]
    fn compute_handles_produces_eight() {
        let bounds = rect(10.0, 20.0, 100.0, 50.0);
        let handles = compute_handles(&bounds);
        assert_eq!(handles.len(), 8);

        let tl = handles
            .iter()
            .find(|h| h.position == HandlePosition::TopLeft)
            .unwrap();
        assert!((tl.x - 10.0).abs() < 1e-10);
        assert!((tl.y - 20.0).abs() < 1e-10);

        let br = handles
            .iter()
            .find(|h| h.position == HandlePosition::BottomRight)
            .unwrap();
        assert!((br.x - 110.0).abs() < 1e-10);
        assert!((br.y - 70.0).abs() < 1e-10);

        let center_top = handles
            .iter()
            .find(|h| h.position == HandlePosition::Top)
            .unwrap();
        assert!((center_top.x - 60.0).abs() < 1e-10);
        assert!((center_top.y - 20.0).abs() < 1e-10);
    }

    #[test]
    fn snap_to_guides_within_threshold() {
        let bounds = vec![rect(100.0, 200.0, 50.0, 30.0)];
        let result = snap_to_guides(102.0, 198.0, &bounds, 5.0);
        assert!((result.snapped_x - 100.0).abs() < 1e-10);
        assert!((result.snapped_y - 200.0).abs() < 1e-10);
        assert!(!result.guides.is_empty());
    }

    #[test]
    fn snap_to_guides_outside_threshold() {
        let bounds = vec![rect(100.0, 200.0, 50.0, 30.0)];
        let result = snap_to_guides(50.0, 50.0, &bounds, 5.0);
        assert!((result.snapped_x - 50.0).abs() < 1e-10);
        assert!((result.snapped_y - 50.0).abs() < 1e-10);
        assert!(result.guides.is_empty());
    }

    #[test]
    fn snap_to_guides_center_snap() {
        let bounds = vec![rect(100.0, 100.0, 60.0, 40.0)];
        let result = snap_to_guides(131.0, 119.0, &bounds, 2.0);
        assert!((result.snapped_x - 130.0).abs() < 1e-10);
        assert!((result.snapped_y - 120.0).abs() < 1e-10);
    }

    #[test]
    fn snap_to_guides_multiple_bounds() {
        let bounds = vec![rect(0.0, 0.0, 50.0, 50.0), rect(100.0, 100.0, 50.0, 50.0)];
        let result = snap_to_guides(51.0, 99.0, &bounds, 3.0);
        assert!((result.snapped_x - 50.0).abs() < 1e-10);
        assert!((result.snapped_y - 100.0).abs() < 1e-10);
    }

    #[test]
    fn scene_graph_add_node_with_invalid_parent() {
        let mut sg = SceneGraph::new();
        let id = sg.add_node("orphan", rect(0.0, 0.0, 10.0, 10.0), Some(999));
        assert!(sg.roots().contains(&id));
        assert_eq!(sg.get(id).unwrap().parent, None);
    }

    #[test]
    fn spatial_index_default() {
        let idx: SpatialIndex<i32> = SpatialIndex::default();
        assert!(idx.is_empty());
    }

    #[test]
    fn render_stats_default() {
        let stats = RenderStats::default();
        assert_eq!(stats.layout_count, 0);
    }

    #[test]
    fn viewport_set_scale_clamped() {
        let mut vt = ViewportTransform::new();
        vt.min_scale = 0.5;
        vt.max_scale = 5.0;
        vt.set_scale(0.1);
        assert!((vt.scale - 0.5).abs() < 1e-10);
        vt.set_scale(10.0);
        assert!((vt.scale - 5.0).abs() < 1e-10);
    }

    #[test]
    fn invalid_geometry_and_metrics_fail_closed() {
        assert!(!rect(f64::NAN, 0.0, 1.0, 1.0).is_valid());
        assert!(!rect(0.0, 0.0, -1.0, 1.0).contains_point(0.0, 0.0));
        assert!(!rect(f64::MAX, 0.0, f64::MAX, 1.0).is_valid());

        assert!(FrameBudget::new_checked(f64::NAN, 1).is_err());
        assert!(FrameBudget::new_checked(60.0, 0).is_err());
        assert!(FrameBudget::new_checked(60.0, MAX_FRAME_BUDGET_SAMPLES + 1).is_err());
        let mut budget = FrameBudget::new(60.0, usize::MAX);
        budget.record_frame(Duration::MAX);
        budget.record_frame(Duration::MAX);
        assert!(budget.average_ms().is_finite());

        let mut stats = RenderStats::new();
        stats.layout_count = u64::MAX;
        stats.paint_count = u64::MAX;
        stats.gpu_upload_bytes = u64::MAX;
        stats.record_layout();
        stats.record_paint();
        stats.add_gpu_upload(1);
        stats.set_overdraw(f64::NAN);
        assert_eq!(stats.layout_count, u64::MAX);
        assert_eq!(stats.paint_count, u64::MAX);
        assert_eq!(stats.gpu_upload_bytes, u64::MAX);
        assert_eq!(stats.overdraw_ratio, 0.0);
    }

    #[test]
    fn spatial_and_viewport_checked_paths_reject_invalid_values() {
        let mut index = SpatialIndex::new();
        assert!(index.insert_checked(rect(0.0, 0.0, -1.0, 1.0), 1).is_err());
        assert!(index.is_empty());

        let mut viewport = ViewportTransform::new();
        assert!(viewport.pan_checked(f64::NAN, 0.0).is_err());
        assert!(viewport.zoom_checked(0.0, 0.0, 0.0).is_err());
        assert!(viewport.set_scale_checked(f64::INFINITY).is_err());
        viewport.scale = 0.0;
        assert!(viewport.screen_to_world_checked(1.0, 1.0).is_err());
        assert_eq!(viewport.screen_to_world(1.0, 1.0), (0.0, 0.0));

        assert!(compute_handles_checked(&rect(0.0, 0.0, -1.0, 1.0)).is_err());
        assert!(snap_to_guides_checked(0.0, 0.0, &[], f64::NAN).is_err());
    }

    #[test]
    fn scene_graph_rejects_cycles_locked_edits_and_identifier_wrap() {
        let mut graph = SceneGraph::new();
        let root = graph
            .add_node_checked("root", rect(0.0, 0.0, 10.0, 10.0), None)
            .unwrap();
        let child = graph
            .add_node_checked("child", rect(0.0, 0.0, 5.0, 5.0), Some(root))
            .unwrap();
        assert!(graph.reparent(root, Some(child)).is_err());

        graph.get_mut(child).unwrap().locked = true;
        assert!(graph.move_node(child, 1.0, 0.0).is_err());
        assert!(graph.reparent(child, None).is_err());

        graph.next_id = u64::MAX;
        let near_wrap = graph
            .add_node_checked("near wrap", rect(0.0, 0.0, 1.0, 1.0), None)
            .unwrap();
        let after_wrap = graph
            .add_node_checked("after wrap", rect(0.0, 0.0, 1.0, 1.0), None)
            .unwrap();
        assert_ne!(near_wrap, after_wrap);
        assert_ne!(after_wrap, root);
    }

    #[test]
    fn scene_graph_traversal_contains_corrupt_child_cycles() {
        let mut graph = SceneGraph::new();
        let root = graph.add_node("root", rect(0.0, 0.0, 10.0, 10.0), None);
        let child = graph.add_node("child", rect(0.0, 0.0, 5.0, 5.0), Some(root));
        graph.get_mut(child).unwrap().children.push(root);

        let hits = graph.hit_test(1.0, 1.0);
        assert_eq!(hits.len(), 2);
        assert!(graph.remove_node(root).is_some());
        assert_eq!(graph.node_count(), 0);
    }
}
