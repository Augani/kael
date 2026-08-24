//! GTK4/GSK retained-scene bridge for the GTK-owned native Wayland backend.
//!
//! GSK render nodes are immutable and cached by GTK. Building the node tree from
//! Kael's already-batched scene keeps the full-frame output in GTK's GPU render
//! graph, where it can be clipped and composited with WebKitGTK widgets without
//! copying a completed frame through the CPU.

use crate::{
    AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Background, BackgroundTag, BlendMode,
    BlurRect, BorderStyle, Bounds, ColorFilter, ContentMask, Corners, DevicePixels, Hsla,
    MonochromeSprite, POLYCHROME_SPRITE_KIND_CONTENT_BLURRED,
    POLYCHROME_SPRITE_KIND_CONTENT_SHADOW, POLYCHROME_SPRITE_KIND_PREMULTIPLIED,
    POLYCHROME_SPRITE_KIND_SUBPIXEL_TEXT, Path, PlatformAtlas, PolychromeSprite, PrimitiveBatch,
    Quad, Rgba, ScaledPixels, Scene, Shadow, Size, TileId, TransformationMatrix, Underline,
    linear_color_stop, linear_gradient, point, size, validate_atlas_payload,
};
use anyhow::{Context as _, Result};
use collections::{FxHashMap, FxHashSet};
use gtk4::{
    Snapshot,
    gdk::{self, Paintable},
    graphene, gsk,
    prelude::*,
};
use parking_lot::Mutex;
use std::{borrow::Cow, sync::Arc};

const PROOF_WIDTH: f32 = 340.0;
const PROOF_HEIGHT: f32 = 603.0;

/// CPU-owned immutable texture data for one GTK/GSK atlas entry.
///
/// GTK render nodes retain their `GdkTexture`, so removing an entry from the
/// atlas cannot invalidate a paintable that is already being presented.
#[derive(Clone)]
struct Gtk4AtlasEntry {
    size: Size<DevicePixels>,
    kind: AtlasTextureKind,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
struct Gtk4AtlasState {
    next_texture_index: u32,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    keys_by_texture: FxHashMap<AtlasTextureId, AtlasKey>,
    entries: FxHashMap<AtlasTextureId, Gtk4AtlasEntry>,
    last_used: FxHashMap<AtlasTextureId, u64>,
    access_clock: u64,
    total_bytes: u64,
    byte_budget: Option<u64>,
}

/// Atlas used by the GTK-owned native-Wayland renderer.
///
/// Each immutable tile is its own GDK texture. This deliberately avoids
/// copying or re-uploading a packed atlas page when one glyph changes. GSK
/// caches the immutable render nodes and textures across frames.
pub(crate) struct Gtk4Atlas(Mutex<Gtk4AtlasState>);

impl Default for Gtk4Atlas {
    fn default() -> Self {
        Self(Mutex::new(Gtk4AtlasState::default()))
    }
}

impl Gtk4Atlas {
    fn insert_entry(
        state: &mut Gtk4AtlasState,
        kind: AtlasTextureKind,
        tile_size: Size<DevicePixels>,
        bytes: Arc<[u8]>,
    ) -> Result<AtlasTile> {
        validate_atlas_payload(tile_size, kind, bytes.len())?;
        let index = state.next_texture_index;
        state.next_texture_index = index
            .checked_add(1)
            .context("GTK4 atlas texture id space is exhausted")?;
        let texture_id = AtlasTextureId { index, kind };
        let tile = AtlasTile {
            texture_id,
            tile_id: TileId(index),
            padding: 0,
            bounds: Bounds {
                origin: point(DevicePixels(0), DevicePixels(0)),
                size: tile_size,
            },
        };
        let byte_len = bytes.len() as u64;
        state.entries.insert(
            texture_id,
            Gtk4AtlasEntry {
                size: tile_size,
                kind,
                bytes,
            },
        );
        state.total_bytes = state.total_bytes.saturating_add(byte_len);
        Self::touch(state, texture_id);
        Ok(tile)
    }

    fn touch(state: &mut Gtk4AtlasState, texture_id: AtlasTextureId) {
        state.access_clock = state.access_clock.saturating_add(1);
        state.last_used.insert(texture_id, state.access_clock);
    }

    fn remove_texture(state: &mut Gtk4AtlasState, texture_id: AtlasTextureId) {
        if let Some(entry) = state.entries.remove(&texture_id) {
            state.total_bytes = state.total_bytes.saturating_sub(entry.bytes.len() as u64);
        }
        if let Some(key) = state.keys_by_texture.remove(&texture_id) {
            state.tiles_by_key.remove(&key);
        }
        state.last_used.remove(&texture_id);
    }

    fn entry(&self, id: AtlasTextureId) -> Option<Gtk4AtlasEntry> {
        self.0.lock().entries.get(&id).cloned()
    }

    fn live_texture_ids(&self) -> FxHashSet<AtlasTextureId> {
        self.0.lock().entries.keys().copied().collect()
    }

    pub(crate) fn set_byte_budget(&self, budget: Option<u64>) {
        self.0.lock().byte_budget = budget;
    }

    fn evict_to_budget_keeping(&self, keep: &FxHashSet<AtlasTextureId>) -> usize {
        let mut state = self.0.lock();
        let Some(budget) = state.byte_budget else {
            return 0;
        };
        if state.total_bytes <= budget {
            return 0;
        }

        let mut candidates: Vec<_> = state
            .entries
            .keys()
            .copied()
            .filter(|texture_id| !keep.contains(texture_id))
            .map(|texture_id| {
                (
                    texture_id,
                    state.last_used.get(&texture_id).copied().unwrap_or(0),
                )
            })
            .collect();
        candidates.sort_by_key(|(_, last_used)| *last_used);

        let mut evicted = 0;
        for (texture_id, _) in candidates {
            if state.total_bytes <= budget {
                break;
            }
            Self::remove_texture(&mut state, texture_id);
            evicted += 1;
        }
        evicted
    }

    #[cfg(test)]
    fn entry_count(&self) -> usize {
        self.0.lock().entries.len()
    }
}

impl PlatformAtlas for Gtk4Atlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        let mut state = self.0.lock();
        if let Some(tile) = state.tiles_by_key.get(key).cloned() {
            Self::touch(&mut state, tile.texture_id);
            return Ok(Some(tile));
        }
        let Some((tile_size, bytes)) = build()? else {
            return Ok(None);
        };
        let tile = Self::insert_entry(
            &mut state,
            key.texture_kind(),
            tile_size,
            Arc::from(bytes.into_owned()),
        )?;
        state.tiles_by_key.insert(key.clone(), tile.clone());
        state.keys_by_texture.insert(tile.texture_id, key.clone());
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.0.lock();
        if let Some(tile) = state.tiles_by_key.remove(key) {
            Self::remove_texture(&mut state, tile.texture_id);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TextureVariant {
    MonochromeMask,
    PolychromeStraight,
    PolychromePremultiplied,
    AlphaMask,
    SubpixelText(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextureCacheKey {
    id: AtlasTextureId,
    variant: TextureVariant,
}

/// Persistent GTK4/GSK retained-scene renderer.
pub(crate) struct Gtk4SceneRenderer {
    atlas: Arc<Gtk4Atlas>,
    textures: FxHashMap<TextureCacheKey, gdk::Texture>,
    used_textures: FxHashSet<TextureCacheKey>,
}

impl Default for Gtk4SceneRenderer {
    fn default() -> Self {
        Self::new(Arc::new(Gtk4Atlas::default()))
    }
}

impl Gtk4SceneRenderer {
    pub(crate) fn new(atlas: Arc<Gtk4Atlas>) -> Self {
        Self {
            atlas,
            textures: FxHashMap::default(),
            used_textures: FxHashSet::default(),
        }
    }

    pub(crate) fn atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    pub(crate) fn set_atlas_byte_budget(&self, budget: Option<u64>) {
        self.atlas.set_byte_budget(budget);
    }

    pub(crate) fn paintable(
        &mut self,
        scene: &Scene,
        viewport: Size<ScaledPixels>,
    ) -> Result<Paintable> {
        self.paintable_at_scale(scene, viewport, 1.0)
    }

    /// Build a GTK-logical paintable from Kael's device-scaled scene.
    ///
    /// Kael rasterizes glyphs and records primitives at the active display
    /// scale. GTK/GSK coordinates are logical, so the retained render node is
    /// scaled back exactly once before GTK applies the monitor scale while
    /// presenting it. This keeps text metrics stable when a window crosses
    /// monitors with different integer or fractional scale factors.
    pub(crate) fn paintable_at_scale(
        &mut self,
        scene: &Scene,
        viewport: Size<ScaledPixels>,
        scale_factor: f32,
    ) -> Result<Paintable> {
        self.used_textures.clear();
        let scene_snapshot = self.scene_snapshot(scene)?;
        let snapshot = Snapshot::new();
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        snapshot.scale(1.0 / scale_factor, 1.0 / scale_factor);
        if let Some(node) = scene_snapshot.to_node() {
            snapshot.append_node(node);
        }
        let used_ids = self
            .used_textures
            .iter()
            .map(|key| key.id)
            .collect::<FxHashSet<_>>();
        self.atlas.evict_to_budget_keeping(&used_ids);
        let live_ids = self.atlas.live_texture_ids();
        self.textures
            .retain(|key, _| live_ids.contains(&key.id) && self.used_textures.contains(key));
        snapshot
            .to_paintable(Some(&graphene::Size::new(
                finite(viewport.width.0).max(0.0),
                finite(viewport.height.0).max(0.0),
            )))
            .context("GTK4 did not produce a paintable for the Kael scene")
    }

    /// Render the retained scene into a PNG using the same GSK renderer family
    /// as the visible GTK surface. The scene is already device-scaled, so the
    /// capture viewport is the logical window size multiplied by the active
    /// monitor scale and is not inverse-scaled like the on-screen paintable.
    pub(crate) fn render_png(
        &mut self,
        scene: &Scene,
        viewport: Size<ScaledPixels>,
        scale_factor: f32,
        surface: &gdk::Surface,
    ) -> Result<crate::Image> {
        self.used_textures.clear();
        let scene_snapshot = self.scene_snapshot(scene)?;
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let width = (finite(viewport.width.0).max(0.0) * scale_factor)
            .ceil()
            .max(1.0);
        let height = (finite(viewport.height.0).max(0.0) * scale_factor)
            .ceil()
            .max(1.0);
        let root = scene_snapshot.to_node().unwrap_or_else(|| {
            gsk::ColorNode::new(
                &gdk::RGBA::new(0.0, 0.0, 0.0, 0.0),
                &graphene::Rect::new(0.0, 0.0, width, height),
            )
            .upcast()
        });
        let renderer = gsk::Renderer::for_surface(surface)
            .context("GTK4 could not create a GSK renderer for scene capture")?;
        let texture =
            renderer.render_texture(&root, Some(&graphene::Rect::new(0.0, 0.0, width, height)));
        let png = texture.save_to_png_bytes();
        // `Renderer::for_surface` returns an already-realized renderer. GSK
        // requires an explicit unrealize before the final reference is
        // released; dropping it while realized is a fatal assertion in debug
        // builds (and leaks backend resources in release builds).
        renderer.unrealize();
        let used_ids = self
            .used_textures
            .iter()
            .map(|key| key.id)
            .collect::<FxHashSet<_>>();
        self.atlas.evict_to_budget_keeping(&used_ids);
        let live_ids = self.atlas.live_texture_ids();
        self.textures
            .retain(|key, _| live_ids.contains(&key.id) && self.used_textures.contains(key));
        Ok(crate::Image::from_bytes(
            crate::ImageFormat::Png,
            png.as_ref().to_vec(),
        ))
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn rect(bounds: Bounds<ScaledPixels>) -> graphene::Rect {
    graphene::Rect::new(
        finite(bounds.origin.x.0),
        finite(bounds.origin.y.0),
        finite(bounds.size.width.0).max(0.0),
        finite(bounds.size.height.0).max(0.0),
    )
}

fn rgba(color: Hsla) -> gdk::RGBA {
    let color = Rgba::from(color);
    gdk::RGBA::new(color.r, color.g, color.b, color.a)
}

fn rounded_rect(bounds: Bounds<ScaledPixels>, corners: &Corners<ScaledPixels>) -> gsk::RoundedRect {
    let corner = |radius: ScaledPixels| {
        let radius = finite(radius.0).max(0.0);
        graphene::Size::new(radius, radius)
    };
    gsk::RoundedRect::new(
        rect(bounds),
        corner(corners.top_left),
        corner(corners.top_right),
        corner(corners.bottom_right),
        corner(corners.bottom_left),
    )
}

fn gradient_stops(background: &Background) -> Vec<gsk::ColorStop> {
    let count = if background.stop_count == 0 {
        2
    } else {
        background.stop_count.min(background.colors.len() as u32) as usize
    };
    background.colors[..count]
        .iter()
        .map(|stop| gsk::ColorStop::new(stop.percentage.clamp(0.0, 1.0), rgba(stop.color)))
        .collect()
}

fn append_slash_pattern(
    snapshot: &Snapshot,
    background: &Background,
    bounds: Bounds<ScaledPixels>,
) {
    let encoded = finite(background.gradient_angle_or_pattern_height).max(0.0);
    let stripe_width = ((encoded / 65_535.0) / 255.0).max(0.5);
    let stripe_interval = ((encoded % 65_535.0) / 255.0).max(0.0);
    let spacing = (stripe_width + stripe_interval).max(1.0);
    let bounds = rect(bounds);
    let span = bounds.width() + bounds.height();
    let count = ((span / spacing).ceil() as usize).clamp(1, 16_384);
    let builder = gsk::PathBuilder::new();
    let start = bounds.x() - bounds.height();
    for index in 0..=count {
        let x = start + index as f32 * spacing;
        builder.move_to(x, bounds.y() + bounds.height());
        builder.line_to(x + bounds.height(), bounds.y());
    }
    snapshot.push_clip(&bounds);
    snapshot.append_stroke(
        &builder.to_path(),
        &gsk::Stroke::new(stripe_width),
        &rgba(background.solid),
    );
    snapshot.pop();
}

fn append_background(snapshot: &Snapshot, background: &Background, bounds: Bounds<ScaledPixels>) {
    let bounds_rect = rect(bounds);
    match background.tag {
        BackgroundTag::Solid => {
            snapshot.append_color(&rgba(background.solid), &bounds_rect);
        }
        BackgroundTag::PatternSlash => append_slash_pattern(snapshot, background, bounds),
        BackgroundTag::LinearGradient => {
            let angle = background.gradient_angle_or_pattern_height.to_radians();
            let direction = (angle.sin(), -angle.cos());
            let center_x = bounds_rect.x() + bounds_rect.width() * 0.5;
            let center_y = bounds_rect.y() + bounds_rect.height() * 0.5;
            let half = (bounds_rect.width().abs() * direction.0.abs()
                + bounds_rect.height().abs() * direction.1.abs())
                * 0.5;
            let start =
                graphene::Point::new(center_x - direction.0 * half, center_y - direction.1 * half);
            let end =
                graphene::Point::new(center_x + direction.0 * half, center_y + direction.1 * half);
            snapshot.append_linear_gradient(
                &bounds_rect,
                &start,
                &end,
                &gradient_stops(background),
            );
        }
        BackgroundTag::RadialGradient => {
            let center = graphene::Point::new(
                bounds_rect.x() + bounds_rect.width() * background.center[0],
                bounds_rect.y() + bounds_rect.height() * background.center[1],
            );
            snapshot.append_radial_gradient(
                &bounds_rect,
                &center,
                bounds_rect.width() * background.radius[0],
                bounds_rect.height() * background.radius[1],
                0.0,
                1.0,
                &gradient_stops(background),
            );
        }
        BackgroundTag::ConicGradient => {
            let center = graphene::Point::new(
                bounds_rect.x() + bounds_rect.width() * background.center[0],
                bounds_rect.y() + bounds_rect.height() * background.center[1],
            );
            snapshot.append_conic_gradient(
                &bounds_rect,
                &center,
                background.gradient_angle_or_pattern_height,
                &gradient_stops(background),
            );
        }
    }
}

fn push_content_mask(snapshot: &Snapshot, content_mask: &ContentMask<ScaledPixels>) -> bool {
    if content_mask.bounds.is_empty() {
        return false;
    }
    snapshot.push_clip(&rect(content_mask.bounds));
    true
}

fn push_rounded_clip(
    snapshot: &Snapshot,
    bounds: Bounds<ScaledPixels>,
    corners: &Corners<ScaledPixels>,
) -> bool {
    if bounds.is_empty() {
        return false;
    }
    snapshot.push_rounded_clip(&rounded_rect(bounds, corners));
    true
}

fn push_transform(snapshot: &Snapshot, transform: TransformationMatrix) -> bool {
    if transform == TransformationMatrix::unit() {
        return false;
    }
    snapshot.save();
    snapshot.transform_matrix(&graphene::Matrix::from_2d(
        transform.rotation_scale[0][0] as f64,
        transform.rotation_scale[1][0] as f64,
        transform.rotation_scale[0][1] as f64,
        transform.rotation_scale[1][1] as f64,
        transform.translation[0] as f64,
        transform.translation[1] as f64,
    ));
    true
}

fn pop_if(snapshot: &Snapshot, pushed: bool) {
    if pushed {
        snapshot.pop();
    }
}

fn restore_if(snapshot: &Snapshot, saved: bool) {
    if saved {
        snapshot.restore();
    }
}

fn blend_mode(raw: u32) -> Option<gsk::BlendMode> {
    match raw {
        value if value == BlendMode::Multiply as u32 => Some(gsk::BlendMode::Multiply),
        value if value == BlendMode::Screen as u32 => Some(gsk::BlendMode::Screen),
        value if value == BlendMode::Overlay as u32 => Some(gsk::BlendMode::Overlay),
        value if value == BlendMode::SoftLight as u32 => Some(gsk::BlendMode::SoftLight),
        value if value == BlendMode::Difference as u32 => Some(gsk::BlendMode::Difference),
        _ => None,
    }
}

fn push_color_filter(snapshot: &Snapshot, filter: ColorFilter) -> bool {
    if filter == ColorFilter::identity() {
        return false;
    }

    // Exact contrast/brightness are affine. Saturation and grayscale are
    // combined into one luminance matrix, matching Kael's shader order.
    let saturation = filter.saturate * (1.0 - filter.grayscale);
    let inv = 1.0 - saturation;
    let lum = [0.2126_f32, 0.7152, 0.0722];
    let contrast = filter.contrast;
    let brightness = filter.brightness;
    let scale = contrast * brightness;
    let matrix = graphene::Matrix::from_float([
        scale * (saturation + inv * lum[0]),
        scale * (inv * lum[0]),
        scale * (inv * lum[0]),
        0.0,
        scale * (inv * lum[1]),
        scale * (saturation + inv * lum[1]),
        scale * (inv * lum[1]),
        0.0,
        scale * (inv * lum[2]),
        scale * (inv * lum[2]),
        scale * (saturation + inv * lum[2]),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    let offset = 0.5 * (1.0 - contrast) * brightness;
    snapshot.push_color_matrix(&matrix, &graphene::Vec4::new(offset, offset, offset, 0.0));
    true
}

fn inset_border_geometry(quad: &Quad) -> (Bounds<ScaledPixels>, Corners<ScaledPixels>) {
    let left = quad.border_widths.left.0.max(0.0);
    let right = quad.border_widths.right.0.max(0.0);
    let top = quad.border_widths.top.0.max(0.0);
    let bottom = quad.border_widths.bottom.0.max(0.0);
    let bounds = Bounds {
        origin: point(
            ScaledPixels(quad.bounds.origin.x.0 + left),
            ScaledPixels(quad.bounds.origin.y.0 + top),
        ),
        size: size(
            ScaledPixels((quad.bounds.size.width.0 - left - right).max(0.0)),
            ScaledPixels((quad.bounds.size.height.0 - top - bottom).max(0.0)),
        ),
    };
    let corners = Corners {
        top_left: ScaledPixels((quad.corner_radii.top_left.0 - left.max(top)).max(0.0)),
        top_right: ScaledPixels((quad.corner_radii.top_right.0 - right.max(top)).max(0.0)),
        bottom_right: ScaledPixels((quad.corner_radii.bottom_right.0 - right.max(bottom)).max(0.0)),
        bottom_left: ScaledPixels((quad.corner_radii.bottom_left.0 - left.max(bottom)).max(0.0)),
    };
    (bounds, corners)
}

fn append_dashed_border(snapshot: &Snapshot, quad: &Quad) {
    let widths = [
        quad.border_widths.top.0.max(0.0),
        quad.border_widths.right.0.max(0.0),
        quad.border_widths.bottom.0.max(0.0),
        quad.border_widths.left.0.max(0.0),
    ];
    let first = widths[0];
    let uniform = first > 0.0 && widths.iter().all(|width| (*width - first).abs() <= 0.01);
    if uniform {
        let half = first * 0.5;
        let stroke_bounds = Bounds {
            origin: point(
                ScaledPixels(quad.bounds.origin.x.0 + half),
                ScaledPixels(quad.bounds.origin.y.0 + half),
            ),
            size: size(
                ScaledPixels((quad.bounds.size.width.0 - first).max(0.0)),
                ScaledPixels((quad.bounds.size.height.0 - first).max(0.0)),
            ),
        };
        let radii = Corners {
            top_left: ScaledPixels((quad.corner_radii.top_left.0 - half).max(0.0)),
            top_right: ScaledPixels((quad.corner_radii.top_right.0 - half).max(0.0)),
            bottom_right: ScaledPixels((quad.corner_radii.bottom_right.0 - half).max(0.0)),
            bottom_left: ScaledPixels((quad.corner_radii.bottom_left.0 - half).max(0.0)),
        };
        let builder = gsk::PathBuilder::new();
        builder.add_rounded_rect(&rounded_rect(stroke_bounds, &radii));
        let stroke = gsk::Stroke::new(first);
        stroke.set_dash(&[first * 2.0, first]);
        snapshot.push_stroke(&builder.to_path(), &stroke);
        append_background(snapshot, &quad.border_color, quad.bounds);
        snapshot.pop();
        return;
    }

    let x = quad.bounds.origin.x.0;
    let y = quad.bounds.origin.y.0;
    let right = x + quad.bounds.size.width.0;
    let bottom = y + quad.bounds.size.height.0;
    for (width, start, end) in [
        (
            widths[0],
            (x, y + widths[0] * 0.5),
            (right, y + widths[0] * 0.5),
        ),
        (
            widths[1],
            (right - widths[1] * 0.5, y),
            (right - widths[1] * 0.5, bottom),
        ),
        (
            widths[2],
            (right, bottom - widths[2] * 0.5),
            (x, bottom - widths[2] * 0.5),
        ),
        (
            widths[3],
            (x + widths[3] * 0.5, bottom),
            (x + widths[3] * 0.5, y),
        ),
    ] {
        if width <= 0.0 {
            continue;
        }
        let builder = gsk::PathBuilder::new();
        builder.move_to(start.0, start.1);
        builder.line_to(end.0, end.1);
        let stroke = gsk::Stroke::new(width);
        stroke.set_dash(&[width * 2.0, width]);
        snapshot.push_stroke(&builder.to_path(), &stroke);
        append_background(snapshot, &quad.border_color, quad.bounds);
        snapshot.pop();
    }
}

fn append_quad_border(snapshot: &Snapshot, quad: &Quad, outline: &gsk::RoundedRect) {
    let widths = [
        quad.border_widths.top.0.max(0.0),
        quad.border_widths.right.0.max(0.0),
        quad.border_widths.bottom.0.max(0.0),
        quad.border_widths.left.0.max(0.0),
    ];
    if widths.iter().all(|width| *width <= 0.0) {
        return;
    }
    if quad.border_style == BorderStyle::Dashed {
        append_dashed_border(snapshot, quad);
    } else if quad.border_color.tag == BackgroundTag::Solid {
        snapshot.append_border(outline, &widths, &[rgba(quad.border_color.solid); 4]);
    } else {
        let (inner_bounds, inner_corners) = inset_border_geometry(quad);
        let builder = gsk::PathBuilder::new();
        builder.add_rounded_rect(outline);
        if !inner_bounds.is_empty() {
            builder.add_rounded_rect(&rounded_rect(inner_bounds, &inner_corners));
        }
        snapshot.push_fill(&builder.to_path(), gsk::FillRule::EvenOdd);
        append_background(snapshot, &quad.border_color, quad.bounds);
        snapshot.pop();
    }
}

fn append_quad(snapshot: &Snapshot, quad: &Quad) {
    let masked = push_content_mask(snapshot, &quad.content_mask);
    let rounded_clip =
        push_rounded_clip(snapshot, quad.rounded_clip_bounds, &quad.rounded_clip_radii);
    let transformed = push_transform(snapshot, quad.transform);
    let filtered = push_color_filter(snapshot, quad.color_filter);
    let blended = blend_mode(quad.blend_mode).is_some_and(|mode| {
        snapshot.push_blend(mode);
        true
    });

    let outline = rounded_rect(quad.bounds, &quad.corner_radii);
    snapshot.push_rounded_clip(&outline);
    append_background(snapshot, &quad.background, quad.bounds);
    snapshot.pop();

    append_quad_border(snapshot, quad, &outline);

    pop_if(snapshot, blended);
    pop_if(snapshot, filtered);
    restore_if(snapshot, transformed);
    pop_if(snapshot, rounded_clip);
    pop_if(snapshot, masked);
}

fn append_shadow(snapshot: &Snapshot, shadow: &Shadow) {
    let masked = push_content_mask(snapshot, &shadow.content_mask);
    let rounded_clip = push_rounded_clip(
        snapshot,
        shadow.rounded_clip_bounds,
        &shadow.rounded_clip_radii,
    );
    let filtered = push_color_filter(snapshot, shadow.color_filter);
    let outline = rounded_rect(shadow.bounds, &shadow.corner_radii);
    if shadow.inset == 0 {
        snapshot.append_outset_shadow(
            &outline,
            &rgba(shadow.color),
            0.0,
            0.0,
            0.0,
            shadow.blur_radius.0.max(0.0),
        );
    } else {
        snapshot.append_inset_shadow(
            &outline,
            &rgba(shadow.color),
            0.0,
            0.0,
            0.0,
            shadow.blur_radius.0.max(0.0),
        );
    }
    pop_if(snapshot, filtered);
    pop_if(snapshot, rounded_clip);
    pop_if(snapshot, masked);
}

fn append_backdrop_blur(snapshot: Snapshot, blur: &BlurRect) -> Snapshot {
    let Some(base) = snapshot.to_node() else {
        let snapshot = Snapshot::new();
        let masked = push_content_mask(&snapshot, &blur.content_mask);
        let rounded_clip = push_rounded_clip(
            &snapshot,
            blur.rounded_clip_bounds,
            &blur.rounded_clip_radii,
        );
        snapshot.push_rounded_clip(&rounded_rect(blur.bounds, &blur.corner_radii));
        snapshot.append_color(&rgba(blur.tint), &rect(blur.bounds));
        snapshot.pop();
        pop_if(&snapshot, rounded_clip);
        pop_if(&snapshot, masked);
        return snapshot;
    };

    let snapshot = Snapshot::new();
    snapshot.append_node(&base);
    let masked = push_content_mask(&snapshot, &blur.content_mask);
    let rounded_clip = push_rounded_clip(
        &snapshot,
        blur.rounded_clip_bounds,
        &blur.rounded_clip_radii,
    );
    snapshot.push_rounded_clip(&rounded_rect(blur.bounds, &blur.corner_radii));
    let should_blur = blur.blur_radius.0 > 0.0;
    if should_blur {
        snapshot.push_blur(finite(blur.blur_radius.0).max(0.0) as f64);
    }
    let saturated = push_color_filter(
        &snapshot,
        ColorFilter {
            saturate: finite(blur.saturation).max(0.0),
            ..ColorFilter::identity()
        },
    );
    snapshot.append_node(&base);
    pop_if(&snapshot, saturated);
    pop_if(&snapshot, should_blur);
    snapshot.append_color(&rgba(blur.tint), &rect(blur.bounds));
    snapshot.pop();
    pop_if(&snapshot, rounded_clip);
    pop_if(&snapshot, masked);
    snapshot
}

fn append_underline(snapshot: &Snapshot, underline: &Underline) {
    let masked = push_content_mask(snapshot, &underline.content_mask);
    let rounded_clip = push_rounded_clip(
        snapshot,
        underline.rounded_clip_bounds,
        &underline.rounded_clip_radii,
    );
    let filtered = push_color_filter(snapshot, underline.color_filter);
    if underline.wavy == 0 {
        snapshot.append_color(&rgba(underline.color), &rect(underline.bounds));
    } else {
        let width = finite(underline.bounds.size.width.0).max(0.0);
        let thickness = finite(underline.thickness.0).max(0.5);
        let amplitude = thickness * 0.8;
        let wavelength = (thickness * 4.0).max(2.0);
        let segment_count = ((width / (wavelength / 8.0)).ceil() as usize).clamp(2, 8_192);
        let x0 = finite(underline.bounds.origin.x.0);
        let center_y =
            finite(underline.bounds.origin.y.0) + finite(underline.bounds.size.height.0) * 0.5;
        let builder = gsk::PathBuilder::new();
        for segment in 0..=segment_count {
            let progress = segment as f32 / segment_count as f32;
            let x = x0 + width * progress;
            let y = center_y
                + (progress * width / wavelength * std::f32::consts::TAU).sin() * amplitude;
            if segment == 0 {
                builder.move_to(x, y);
            } else {
                builder.line_to(x, y);
            }
        }
        snapshot.append_stroke(
            &builder.to_path(),
            &gsk::Stroke::new(thickness),
            &rgba(underline.color),
        );
    }
    pop_if(snapshot, filtered);
    pop_if(snapshot, rounded_clip);
    pop_if(snapshot, masked);
}

fn is_quadratic_curve_triangle(vertices: &[crate::PathVertex<ScaledPixels>]) -> bool {
    let expected = [(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)];
    vertices.iter().zip(expected).all(|(vertex, expected)| {
        (vertex.st_position.x - expected.0).abs() <= f32::EPSILON
            && (vertex.st_position.y - expected.1).abs() <= f32::EPSILON
    })
}

fn append_path(snapshot: &Snapshot, path: &Path<ScaledPixels>) -> Result<()> {
    anyhow::ensure!(
        path.vertices.len().is_multiple_of(3),
        "GTK4 path vertex count is not divisible by three"
    );
    if path.vertices.is_empty() {
        return Ok(());
    }

    let builder = gsk::PathBuilder::new();
    for triangle in path.vertices.chunks_exact(3) {
        let points = [
            (
                finite(triangle[0].xy_position.x.0),
                finite(triangle[0].xy_position.y.0),
            ),
            (
                finite(triangle[1].xy_position.x.0),
                finite(triangle[1].xy_position.y.0),
            ),
            (
                finite(triangle[2].xy_position.x.0),
                finite(triangle[2].xy_position.y.0),
            ),
        ];
        builder.move_to(points[0].0, points[0].1);
        if is_quadratic_curve_triangle(triangle) {
            builder.quad_to(points[1].0, points[1].1, points[2].0, points[2].1);
        } else {
            builder.line_to(points[1].0, points[1].1);
            builder.line_to(points[2].0, points[2].1);
        }
        builder.close();
    }

    let masked = push_content_mask(snapshot, &path.content_mask);
    snapshot.push_fill(&builder.to_path(), gsk::FillRule::Winding);
    append_background(snapshot, &path.color, path.bounds);
    snapshot.pop();
    pop_if(snapshot, masked);
    Ok(())
}

fn rgba_key(color: Hsla) -> u32 {
    u32::from(Rgba::from(color))
}

fn unpack_rgba(key: u32) -> [f32; 4] {
    [
        ((key >> 24) & 0xff) as f32 / 255.0,
        ((key >> 16) & 0xff) as f32 / 255.0,
        ((key >> 8) & 0xff) as f32 / 255.0,
        (key & 0xff) as f32 / 255.0,
    ]
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn texture_bytes(
    entry: &Gtk4AtlasEntry,
    variant: TextureVariant,
) -> Result<(gdk::MemoryFormat, Arc<[u8]>)> {
    match variant {
        TextureVariant::MonochromeMask => {
            anyhow::ensure!(
                entry.kind == AtlasTextureKind::Monochrome,
                "GTK4 monochrome sprite referenced a polychrome atlas entry"
            );
            let expanded = entry
                .bytes
                .iter()
                .flat_map(|alpha| [*alpha; 4])
                .collect::<Vec<_>>();
            Ok((
                gdk::MemoryFormat::R8g8b8a8Premultiplied,
                Arc::from(expanded),
            ))
        }
        TextureVariant::PolychromeStraight => {
            anyhow::ensure!(
                entry.kind == AtlasTextureKind::Polychrome,
                "GTK4 polychrome sprite referenced a monochrome atlas entry"
            );
            Ok((gdk::MemoryFormat::B8g8r8a8, entry.bytes.clone()))
        }
        TextureVariant::PolychromePremultiplied => {
            anyhow::ensure!(
                entry.kind == AtlasTextureKind::Polychrome,
                "GTK4 premultiplied sprite referenced a monochrome atlas entry"
            );
            Ok((
                gdk::MemoryFormat::B8g8r8a8Premultiplied,
                entry.bytes.clone(),
            ))
        }
        TextureVariant::AlphaMask => {
            anyhow::ensure!(
                entry.kind == AtlasTextureKind::Polychrome,
                "GTK4 alpha mask referenced a monochrome atlas entry"
            );
            let expanded = entry
                .bytes
                .chunks_exact(4)
                .flat_map(|pixel| [pixel[3]; 4])
                .collect::<Vec<_>>();
            Ok((
                gdk::MemoryFormat::R8g8b8a8Premultiplied,
                Arc::from(expanded),
            ))
        }
        TextureVariant::SubpixelText(tint) => {
            anyhow::ensure!(
                entry.kind == AtlasTextureKind::Polychrome,
                "GTK4 subpixel glyph referenced a monochrome atlas entry"
            );
            let [tint_r, tint_g, tint_b, tint_a] = unpack_rgba(tint);
            let mut output = Vec::with_capacity(entry.bytes.len());
            for pixel in entry.bytes.chunks_exact(4) {
                // Kael's polychrome atlas contract is BGRA. A subpixel glyph's
                // RGB channels are independent coverages; converting them once
                // to a premultiplied tinted texture preserves component detail
                // without a deprecated GtkGLShader in the GTK render graph.
                let coverage_b = pixel[0] as f32 / 255.0;
                let coverage_g = pixel[1] as f32 / 255.0;
                let coverage_r = pixel[2] as f32 / 255.0;
                let coverage_a = coverage_r.max(coverage_g).max(coverage_b);
                output.extend_from_slice(&[
                    byte(tint_b * coverage_b * tint_a),
                    byte(tint_g * coverage_g * tint_a),
                    byte(tint_r * coverage_r * tint_a),
                    byte(coverage_a * tint_a),
                ]);
            }
            Ok((gdk::MemoryFormat::B8g8r8a8Premultiplied, Arc::from(output)))
        }
    }
}

impl Gtk4SceneRenderer {
    fn texture(&mut self, id: AtlasTextureId, variant: TextureVariant) -> Result<gdk::Texture> {
        let key = TextureCacheKey { id, variant };
        self.used_textures.insert(key);
        if let Some(texture) = self.textures.get(&key) {
            return Ok(texture.clone());
        }
        let entry = self
            .atlas
            .entry(id)
            .with_context(|| format!("GTK4 atlas texture {id:?} is unavailable"))?;
        let (format, bytes) = texture_bytes(&entry, variant)?;
        let width = entry.size.width.0;
        let height = entry.size.height.0;
        let stride = usize::try_from(width)
            .context("GTK4 atlas width is negative")?
            .checked_mul(4)
            .context("GTK4 atlas row stride overflow")?;
        let bytes = gtk4::glib::Bytes::from_owned(bytes);
        let texture: gdk::Texture =
            gdk::MemoryTexture::new(width, height, format, &bytes, stride).upcast();
        self.textures.insert(key, texture.clone());
        Ok(texture)
    }

    fn append_monochrome_sprite(
        &mut self,
        snapshot: &Snapshot,
        sprite: &MonochromeSprite,
    ) -> Result<()> {
        let texture = self.texture(sprite.tile.texture_id, TextureVariant::MonochromeMask)?;
        let masked = push_content_mask(snapshot, &sprite.content_mask);
        let rounded_clip = push_rounded_clip(
            snapshot,
            sprite.rounded_clip_bounds,
            &sprite.rounded_clip_radii,
        );
        let transformed = push_transform(snapshot, sprite.transformation);
        let filtered = push_color_filter(snapshot, sprite.color_filter);
        let bounds = rect(sprite.bounds);
        let source = gsk::ColorNode::new(&rgba(sprite.color), &bounds);
        let mask = gsk::TextureNode::new(&texture, &bounds);
        snapshot.append_node(&gsk::MaskNode::new(source, mask, gsk::MaskMode::Alpha));
        pop_if(snapshot, filtered);
        restore_if(snapshot, transformed);
        pop_if(snapshot, rounded_clip);
        pop_if(snapshot, masked);
        Ok(())
    }

    fn append_polychrome_sprite(
        &mut self,
        snapshot: &Snapshot,
        sprite: &PolychromeSprite,
    ) -> Result<()> {
        let variant = match sprite.sprite_kind {
            POLYCHROME_SPRITE_KIND_PREMULTIPLIED => TextureVariant::PolychromePremultiplied,
            POLYCHROME_SPRITE_KIND_SUBPIXEL_TEXT => {
                TextureVariant::SubpixelText(rgba_key(sprite.color))
            }
            POLYCHROME_SPRITE_KIND_CONTENT_SHADOW => TextureVariant::AlphaMask,
            _ => TextureVariant::PolychromeStraight,
        };
        let texture = self.texture(sprite.tile.texture_id, variant)?;
        let masked = push_content_mask(snapshot, &sprite.content_mask);
        let rounded_clip = push_rounded_clip(
            snapshot,
            sprite.rounded_clip_bounds,
            &sprite.rounded_clip_radii,
        );
        let transformed = push_transform(snapshot, sprite.transformation);
        let shape_clip = push_rounded_clip(snapshot, sprite.bounds, &sprite.corner_radii);
        let filtered = push_color_filter(
            snapshot,
            if sprite.grayscale {
                sprite.color_filter.compose(ColorFilter {
                    grayscale: 1.0,
                    ..ColorFilter::identity()
                })
            } else {
                sprite.color_filter
            },
        );
        let opacity = finite(sprite.opacity).clamp(0.0, 1.0);
        let translucent = opacity < 1.0;
        if translucent {
            snapshot.push_opacity(opacity as f64);
        }
        let blurred = matches!(
            sprite.sprite_kind,
            POLYCHROME_SPRITE_KIND_CONTENT_BLURRED | POLYCHROME_SPRITE_KIND_CONTENT_SHADOW
        ) && sprite.blur_radius > 0.0;
        if blurred {
            snapshot.push_blur(finite(sprite.blur_radius).max(0.0) as f64);
        }
        let bounds = rect(sprite.bounds);
        if sprite.sprite_kind == POLYCHROME_SPRITE_KIND_CONTENT_SHADOW {
            let source = gsk::ColorNode::new(&rgba(sprite.color), &bounds);
            let mask = gsk::TextureNode::new(&texture, &bounds);
            snapshot.append_node(&gsk::MaskNode::new(source, mask, gsk::MaskMode::Alpha));
        } else {
            snapshot.append_texture(&texture, &bounds);
        }
        pop_if(snapshot, blurred);
        pop_if(snapshot, translucent);
        pop_if(snapshot, filtered);
        pop_if(snapshot, shape_clip);
        restore_if(snapshot, transformed);
        pop_if(snapshot, rounded_clip);
        pop_if(snapshot, masked);
        Ok(())
    }

    fn scene_snapshot(&mut self, scene: &Scene) -> Result<Snapshot> {
        let mut snapshot = Snapshot::new();
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(shadows) => {
                    shadows
                        .iter()
                        .for_each(|shadow| append_shadow(&snapshot, shadow));
                }
                PrimitiveBatch::BlurRects(blurs) => {
                    for blur in blurs {
                        snapshot = append_backdrop_blur(snapshot, blur);
                    }
                }
                PrimitiveBatch::Quads(quads) => {
                    quads.iter().for_each(|quad| append_quad(&snapshot, quad));
                }
                PrimitiveBatch::Paths(paths) => {
                    for path in paths {
                        append_path(&snapshot, path)?;
                    }
                }
                PrimitiveBatch::Underlines(underlines) => {
                    underlines
                        .iter()
                        .for_each(|underline| append_underline(&snapshot, underline));
                }
                PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites,
                } => {
                    for sprite in sprites {
                        anyhow::ensure!(
                            sprite.tile.texture_id == texture_id,
                            "GTK4 monochrome sprite batch spans atlas textures"
                        );
                        self.append_monochrome_sprite(&snapshot, sprite)?;
                    }
                }
                PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites,
                } => {
                    for sprite in sprites {
                        anyhow::ensure!(
                            sprite.tile.texture_id == texture_id,
                            "GTK4 polychrome sprite batch spans atlas textures"
                        );
                        self.append_polychrome_sprite(&snapshot, sprite)?;
                    }
                }
                PrimitiveBatch::Surfaces(surfaces) => {
                    anyhow::bail!(
                        "GTK4 retained-scene bridge cannot import {} native video surface(s) yet",
                        surfaces.len()
                    );
                }
            }
        }
        Ok(snapshot)
    }
}

fn proof_tiles(atlas: &Gtk4Atlas) -> Result<(AtlasTile, AtlasTile)> {
    let mono_size = size(DevicePixels(220), DevicePixels(28));
    let mut mono = vec![0_u8; 220 * 28];
    for y in 0..28 {
        for x in 0..220 {
            let baseline = (12..=15).contains(&y);
            let signal = ((x / 11) % 3 == 0 && (5..=22).contains(&y))
                || ((x + y * 3) % 47 < 5 && (7..=20).contains(&y));
            if baseline || signal {
                mono[y * 220 + x] = 235;
            }
        }
    }

    let poly_size = size(DevicePixels(58), DevicePixels(58));
    let mut poly = Vec::with_capacity(58 * 58 * 4);
    for y in 0..58 {
        for x in 0..58 {
            let dx = x as f32 - 28.5;
            let dy = y as f32 - 28.5;
            let distance = (dx * dx + dy * dy).sqrt();
            let alpha = (1.0 - ((distance - 22.0) / 6.0).clamp(0.0, 1.0)).max(0.0);
            poly.extend_from_slice(&[
                byte((0.92 - y as f32 / 160.0) * alpha),
                byte((0.35 + x as f32 / 120.0) * alpha),
                byte((0.18 + y as f32 / 120.0) * alpha),
                byte(alpha),
            ]);
        }
    }

    let mut state = atlas.0.lock();
    let mono = Gtk4Atlas::insert_entry(
        &mut state,
        AtlasTextureKind::Monochrome,
        mono_size,
        Arc::from(mono),
    )?;
    let poly = Gtk4Atlas::insert_entry(
        &mut state,
        AtlasTextureKind::Polychrome,
        poly_size,
        Arc::from(poly),
    )?;
    Ok((mono, poly))
}

fn proof_scene(atlas: &Gtk4Atlas) -> Result<Scene> {
    let viewport = Bounds {
        origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size: size(ScaledPixels(PROOF_WIDTH), ScaledPixels(PROOF_HEIGHT)),
    };
    let mask = ContentMask { bounds: viewport };
    let mut scene = Scene::default();

    scene.insert_primitive(Quad {
        bounds: viewport,
        content_mask: mask.clone(),
        background: Hsla::from(crate::rgba(0x08101fff)).into(),
        ..Default::default()
    });

    let card = Bounds {
        origin: point(ScaledPixels(24.0), ScaledPixels(105.0)),
        size: size(ScaledPixels(292.0), ScaledPixels(252.0)),
    };
    scene.insert_primitive(Shadow {
        order: 0,
        bounds: card,
        corner_radii: Corners::all(ScaledPixels(22.0)),
        content_mask: mask.clone(),
        color: Hsla::from(crate::rgba(0x00000099)),
        blur_radius: ScaledPixels(24.0),
        inset: 0,
        pad: 0,
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        color_filter: ColorFilter::identity(),
    });
    scene.insert_primitive(Quad {
        bounds: card,
        content_mask: mask.clone(),
        background: linear_gradient(
            135.0,
            linear_color_stop(crate::rgba(0x10294fff), 0.0),
            linear_color_stop(crate::rgba(0x091426ff), 1.0),
        ),
        border_color: Hsla::from(crate::rgba(0x53c8ff99)).into(),
        border_widths: crate::Edges::all(ScaledPixels(1.0)),
        corner_radii: Corners::all(ScaledPixels(22.0)),
        ..Default::default()
    });

    for (y, width, color) in [
        (137.0, 174.0, 0x8be9fdff),
        (173.0, 238.0, 0x324968ff),
        (199.0, 206.0, 0x243a5aff),
        (247.0, 118.0, 0x50fa7bff),
    ] {
        scene.insert_primitive(Quad {
            bounds: Bounds {
                origin: point(ScaledPixels(50.0), ScaledPixels(y)),
                size: size(ScaledPixels(width), ScaledPixels(12.0)),
            },
            content_mask: mask.clone(),
            background: Hsla::from(crate::rgba(color)).into(),
            corner_radii: Corners::all(ScaledPixels(6.0)),
            ..Default::default()
        });
    }

    scene.insert_primitive(Quad {
        bounds: Bounds {
            origin: point(ScaledPixels(50.0), ScaledPixels(287.0)),
            size: size(ScaledPixels(238.0), ScaledPixels(42.0)),
        },
        content_mask: mask.clone(),
        background: Hsla::from(crate::rgba(0x0d7c66ff)).into(),
        corner_radii: Corners::all(ScaledPixels(12.0)),
        ..Default::default()
    });

    let (mono_tile, poly_tile) = proof_tiles(atlas)?;
    scene.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: Bounds {
            origin: point(ScaledPixels(50.0), ScaledPixels(375.0)),
            size: size(ScaledPixels(220.0), ScaledPixels(28.0)),
        },
        content_mask: mask.clone(),
        color: Hsla::from(crate::rgba(0xc3f4ffff)),
        tile: mono_tile,
        transformation: TransformationMatrix::unit(),
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        color_filter: ColorFilter::identity(),
    });
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false,
        opacity: 1.0,
        bounds: Bounds {
            origin: point(ScaledPixels(50.0), ScaledPixels(405.0)),
            size: size(ScaledPixels(58.0), ScaledPixels(58.0)),
        },
        content_mask: mask.clone(),
        corner_radii: Corners::all(ScaledPixels(29.0)),
        tile: poly_tile,
        sprite_kind: crate::POLYCHROME_SPRITE_KIND_COLOR,
        color: Hsla::transparent_black(),
        pad3: 0,
        rounded_clip_bounds: Bounds::default(),
        rounded_clip_radii: Corners::default(),
        color_filter: ColorFilter::identity(),
        transformation: TransformationMatrix::unit(),
        blur_radius: 0.0,
        pad2: 0,
    });

    let mut path = Path::new(point(crate::px(142.0), crate::px(470.0)));
    path.push_triangle(
        (
            point(crate::px(142.0), crate::px(470.0)),
            point(crate::px(202.0), crate::px(405.0)),
            point(crate::px(274.0), crate::px(470.0)),
        ),
        (point(0.0, 1.0), point(0.0, 1.0), point(0.0, 1.0)),
    );
    let mut path = path.scale(1.0);
    path.content_mask = mask.clone();
    path.color = linear_gradient(
        90.0,
        linear_color_stop(crate::rgba(0x50fa7bff), 0.0),
        linear_color_stop(crate::rgba(0x8be9fdff), 1.0),
    );
    scene.insert_primitive(path);

    scene.finish();
    Ok(scene)
}

/// Build the retained GSK paintable used by the native-Wayland release proof.
///
/// This is deliberately hidden from Kael's stable API. It exists so the Linux
/// integration smoke can prove that GTK is compositing real Kael scene
/// primitives—not a placeholder widget—beside WebKitGTK.
#[doc(hidden)]
pub fn gtk4_wayland_scene_proof_paintable() -> anyhow::Result<Paintable> {
    let mut renderer = Gtk4SceneRenderer::default();
    let scene = proof_scene(&renderer.atlas).context("building the GTK4 Kael proof scene")?;
    renderer.paintable(
        &scene,
        size(ScaledPixels(PROOF_WIDTH), ScaledPixels(PROOF_HEIGHT)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn proof_scene_uses_ordered_kael_primitives() {
        let atlas = Gtk4Atlas::default();
        let scene = proof_scene(&atlas).unwrap();
        assert_eq!(scene.shadows.len(), 1);
        assert_eq!(scene.quads.len(), 7);
        assert_eq!(scene.paths.len(), 1);
        assert_eq!(scene.monochrome_sprites.len(), 1);
        assert_eq!(scene.polychrome_sprites.len(), 1);
        assert_eq!(scene.len(), 11);
        assert_eq!(atlas.entry_count(), 2);
    }

    #[test]
    fn gtk4_atlas_deduplicates_validates_and_removes_tiles() {
        let atlas = Gtk4Atlas::default();
        let key = AtlasKey::IconAtlas(crate::RenderIconAtlasParams {
            edge: DevicePixels(2),
        });
        let builds = Cell::new(0);
        let mut build = || {
            builds.set(builds.get() + 1);
            Ok(Some((
                size(DevicePixels(2), DevicePixels(2)),
                Cow::Owned(vec![1, 2, 3, 4]),
            )))
        };
        let first = atlas.get_or_insert_with(&key, &mut build).unwrap().unwrap();
        let second = atlas.get_or_insert_with(&key, &mut build).unwrap().unwrap();
        assert_eq!(first, second);
        assert_eq!(builds.get(), 1);
        assert_eq!(atlas.entry_count(), 1);

        atlas.remove(&key);
        assert_eq!(atlas.entry_count(), 0);

        let mut invalid = || {
            Ok(Some((
                size(DevicePixels(2), DevicePixels(2)),
                Cow::Owned(vec![1, 2, 3]),
            )))
        };
        assert!(atlas.get_or_insert_with(&key, &mut invalid).is_err());
        assert_eq!(atlas.entry_count(), 0);
    }

    #[test]
    fn gtk4_atlas_budget_evicts_lru_without_touching_current_scene() {
        let atlas = Gtk4Atlas::default();
        let first_key = AtlasKey::IconAtlas(crate::RenderIconAtlasParams {
            edge: DevicePixels(2),
        });
        let second_key = AtlasKey::IconAtlas(crate::RenderIconAtlasParams {
            edge: DevicePixels(3),
        });
        let mut first_build = || {
            Ok(Some((
                size(DevicePixels(2), DevicePixels(2)),
                Cow::Owned(vec![1, 2, 3, 4]),
            )))
        };
        let mut second_build = || {
            Ok(Some((
                size(DevicePixels(2), DevicePixels(2)),
                Cow::Owned(vec![5, 6, 7, 8]),
            )))
        };
        let first = atlas
            .get_or_insert_with(&first_key, &mut first_build)
            .unwrap()
            .unwrap();
        let second = atlas
            .get_or_insert_with(&second_key, &mut second_build)
            .unwrap()
            .unwrap();

        atlas.set_byte_budget(Some(1));
        let keep = FxHashSet::from_iter([second.texture_id]);
        assert_eq!(atlas.evict_to_budget_keeping(&keep), 1);
        assert!(atlas.entry(first.texture_id).is_none());
        assert!(atlas.entry(second.texture_id).is_some());
        assert_eq!(atlas.entry_count(), 1);
    }

    #[test]
    fn texture_conversion_preserves_masks_and_subpixel_channels() {
        let mono = Gtk4AtlasEntry {
            size: size(DevicePixels(2), DevicePixels(1)),
            kind: AtlasTextureKind::Monochrome,
            bytes: Arc::from(vec![0_u8, 255]),
        };
        let (_, mask) = texture_bytes(&mono, TextureVariant::MonochromeMask).unwrap();
        assert_eq!(&*mask, &[0, 0, 0, 0, 255, 255, 255, 255]);

        let subpixel = Gtk4AtlasEntry {
            size: size(DevicePixels(1), DevicePixels(1)),
            kind: AtlasTextureKind::Polychrome,
            bytes: Arc::from(vec![64_u8, 128, 255, 0]),
        };
        let (_, tinted) =
            texture_bytes(&subpixel, TextureVariant::SubpixelText(0xff0000ff)).unwrap();
        assert_eq!(&*tinted, &[0, 0, 255, 255]);
    }
}
