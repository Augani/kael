// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, DevicePixels, Edges, Hsla,
    Pixels, Point, Radians, ScaledPixels, Size, bounds_tree::BoundsTree, point,
};
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
    sync::Arc,
};

#[allow(non_camel_case_types, unused)]
pub(crate) type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

pub(crate) type DrawOrder = u32;

#[derive(Default)]
pub(crate) struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    pub(crate) shadows: Vec<Shadow>,
    pub(crate) blur_rects: Vec<BlurRect>,
    pub(crate) quads: Vec<Quad>,
    pub(crate) paths: Vec<Path<ScaledPixels>>,
    pub(crate) underlines: Vec<Underline>,
    pub(crate) monochrome_sprites: Vec<MonochromeSprite>,
    pub(crate) polychrome_sprites: Vec<PolychromeSprite>,
    pub(crate) surfaces: Vec<PaintSurface>,
    pub(crate) cached_surface_snapshots: Vec<CachedSurfaceSnapshot>,
}

impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.blur_rects.clear();
        self.quads.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
        self.cached_surface_snapshots.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub(crate) fn structural_checksum(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut mix = |value: u64| {
            hash ^= value;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        mix(self.shadows.len() as u64);
        mix(self.blur_rects.len() as u64);
        mix(self.quads.len() as u64);
        mix(self.paths.len() as u64);
        mix(self.underlines.len() as u64);
        mix(self.monochrome_sprites.len() as u64);
        mix(self.polychrome_sprites.len() as u64);
        mix(self.surfaces.len() as u64);
        for quad in &self.quads {
            mix(quad.bounds.origin.x.0.to_bits() as u64);
            mix(quad.bounds.origin.y.0.to_bits() as u64);
            mix(quad.bounds.size.width.0.to_bits() as u64);
            mix(quad.bounds.size.height.0.to_bits() as u64);
            mix(quad.background.solid.h.to_bits() as u64);
            mix(quad.background.solid.s.to_bits() as u64);
            mix(quad.background.solid.l.to_bits() as u64);
            mix(quad.background.solid.a.to_bits() as u64);
        }
        for sprite in &self.monochrome_sprites {
            mix(sprite.color.h.to_bits() as u64);
            mix(sprite.color.s.to_bits() as u64);
            mix(sprite.color.l.to_bits() as u64);
            mix(sprite.color.a.to_bits() as u64);
        }
        hash
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let primitive = primitive.into();
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        let (kind, index) = match primitive {
            Primitive::Shadow(mut shadow) => {
                shadow.order = order;
                let idx = self.shadows.len();
                self.shadows.push(shadow);
                (PrimitiveKind::Shadow, idx)
            }
            Primitive::BlurRect(mut blur_rect) => {
                blur_rect.order = order;
                let idx = self.blur_rects.len();
                self.blur_rects.push(blur_rect);
                (PrimitiveKind::BlurRect, idx)
            }
            Primitive::Quad(mut quad) => {
                quad.order = order;
                let idx = self.quads.len();
                self.quads.push(quad);
                (PrimitiveKind::Quad, idx)
            }
            Primitive::Path(mut path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                let idx = self.paths.len();
                self.paths.push(path);
                (PrimitiveKind::Path, idx)
            }
            Primitive::Underline(mut underline) => {
                underline.order = order;
                let idx = self.underlines.len();
                self.underlines.push(underline);
                (PrimitiveKind::Underline, idx)
            }
            Primitive::MonochromeSprite(mut sprite) => {
                sprite.order = order;
                let idx = self.monochrome_sprites.len();
                self.monochrome_sprites.push(sprite);
                (PrimitiveKind::MonochromeSprite, idx)
            }
            Primitive::PolychromeSprite(mut sprite) => {
                sprite.order = order;
                let idx = self.polychrome_sprites.len();
                self.polychrome_sprites.push(sprite);
                (PrimitiveKind::PolychromeSprite, idx)
            }
            Primitive::Surface(mut surface) => {
                surface.order = order;
                let idx = self.surfaces.len();
                self.surfaces.push(surface);
                (PrimitiveKind::Surface, idx)
            }
        };
        self.paint_operations
            .push(PaintOperation::Primitive(kind, index));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(kind, index) => {
                    let primitive = match kind {
                        PrimitiveKind::Shadow => {
                            Primitive::Shadow(prev_scene.shadows[*index].clone())
                        }
                        PrimitiveKind::BlurRect => {
                            Primitive::BlurRect(prev_scene.blur_rects[*index].clone())
                        }
                        PrimitiveKind::Quad => Primitive::Quad(prev_scene.quads[*index].clone()),
                        PrimitiveKind::Path => Primitive::Path(prev_scene.paths[*index].clone()),
                        PrimitiveKind::Underline => {
                            Primitive::Underline(prev_scene.underlines[*index].clone())
                        }
                        PrimitiveKind::MonochromeSprite => Primitive::MonochromeSprite(
                            prev_scene.monochrome_sprites[*index].clone(),
                        ),
                        PrimitiveKind::PolychromeSprite => Primitive::PolychromeSprite(
                            prev_scene.polychrome_sprites[*index].clone(),
                        ),
                        PrimitiveKind::Surface => {
                            Primitive::Surface(prev_scene.surfaces[*index].clone())
                        }
                    };
                    self.insert_primitive(primitive);
                }
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
            }
        }
    }

    pub fn finish(&mut self) {
        // Primitives are typically inserted in draw order during painting.
        // Skip the O(n log n) sort when data is already sorted (common case).
        if !self.shadows.is_sorted_by_key(|s| s.order) {
            self.shadows.sort_unstable_by_key(|s| s.order);
        }
        if !self.blur_rects.is_sorted_by_key(|b| b.order) {
            self.blur_rects.sort_unstable_by_key(|b| b.order);
        }
        if !self.quads.is_sorted_by_key(|q| q.order) {
            self.quads.sort_unstable_by_key(|q| q.order);
        }
        if !self.paths.is_sorted_by_key(|p| p.order) {
            self.paths.sort_unstable_by_key(|p| p.order);
        }
        if !self.underlines.is_sorted_by_key(|u| u.order) {
            self.underlines.sort_unstable_by_key(|u| u.order);
        }
        if !self
            .monochrome_sprites
            .is_sorted_by_key(|s| (s.order, s.tile.tile_id))
        {
            self.monochrome_sprites
                .sort_unstable_by_key(|s| (s.order, s.tile.tile_id));
        }
        if !self
            .polychrome_sprites
            .is_sorted_by_key(|s| (s.order, s.tile.tile_id))
        {
            self.polychrome_sprites
                .sort_unstable_by_key(|s| (s.order, s.tile.tile_id));
        }
        if !self.surfaces.is_sorted_by_key(|s| s.order) {
            self.surfaces.sort_unstable_by_key(|s| s.order);
        }
    }

    pub(crate) fn request_cached_surface_snapshot(&mut self, snapshot: CachedSurfaceSnapshot) {
        self.cached_surface_snapshots.push(snapshot);
    }

    pub(crate) fn snapshot_subscene(&self, paint_operations: Range<usize>) -> Scene {
        let mut scene = Scene::default();
        scene.replay(paint_operations, self);
        scene.finish();
        scene
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub(crate) fn batches(&self) -> impl Iterator<Item = PrimitiveBatch<'_>> {
        BatchIterator {
            shadows: &self.shadows,
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            blur_rects: &self.blur_rects,
            blur_rects_start: 0,
            blur_rects_iter: self.blur_rects.iter().peekable(),
            quads: &self.quads,
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            paths: &self.paths,
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines: &self.underlines,
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites: &self.monochrome_sprites,
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            polychrome_sprites: &self.polychrome_sprites,
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces: &self.surfaces,
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    Shadow,
    BlurRect,
    #[default]
    Quad,
    Path,
    Underline,
    MonochromeSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(PrimitiveKind, usize),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
}

#[derive(Clone)]
pub(crate) enum Primitive {
    Shadow(Shadow),
    BlurRect(BlurRect),
    Quad(Quad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

impl Primitive {
    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::BlurRect(blur_rect) => &blur_rect.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::BlurRect(blur_rect) => &blur_rect.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    shadows: &'a [Shadow],
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    blur_rects: &'a [BlurRect],
    blur_rects_start: usize,
    blur_rects_iter: Peekable<slice::Iter<'a, BlurRect>>,
    quads: &'a [Quad],
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    paths: &'a [Path<ScaledPixels>],
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines: &'a [Underline],
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites: &'a [MonochromeSprite],
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    polychrome_sprites: &'a [PolychromeSprite],
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces: &'a [PaintSurface],
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (
                self.blur_rects_iter.peek().map(|b| b.order),
                PrimitiveKind::BlurRect,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(
                    &self.shadows[shadows_start..shadows_end],
                ))
            }
            PrimitiveKind::BlurRect => {
                let blur_rects_start = self.blur_rects_start;
                let mut blur_rects_end = blur_rects_start + 1;
                self.blur_rects_iter.next();
                while self
                    .blur_rects_iter
                    .next_if(|blur_rect| (blur_rect.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    blur_rects_end += 1;
                }
                self.blur_rects_start = blur_rects_end;
                Some(PrimitiveBatch::BlurRects(
                    &self.blur_rects[blur_rects_start..blur_rects_end],
                ))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(&self.quads[quads_start..quads_end]))
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(&self.paths[paths_start..paths_end]))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(
                    &self.underlines[underlines_start..underlines_end],
                ))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    sprites: &self.monochrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = self.polychrome_sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    sprites: &self.polychrome_sprites[sprites_start..sprites_end],
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(
                    &self.surfaces[surfaces_start..surfaces_end],
                ))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveBatch<'a> {
    Shadows(&'a [Shadow]),
    BlurRects(&'a [BlurRect]),
    Quads(&'a [Quad]),
    Paths(&'a [Path<ScaledPixels>]),
    Underlines(&'a [Underline]),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [MonochromeSprite],
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        sprites: &'a [PolychromeSprite],
    },
    Surfaces(&'a [PaintSurface]),
}

#[derive(Default, Debug, Clone)]
#[repr(C)]
pub(crate) struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Background,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
    pub continuous_corners: u32,
    pub pad: u32,
    pub transform: TransformationMatrix,
    pub blend_mode: u32,
    pub pad2: u32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
    pub color_filter: ColorFilter,
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: u32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
    pub color_filter: ColorFilter,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub inset: u32,
    pub pad: u32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
    pub color_filter: ColorFilter,
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub(crate) struct BlurRect {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tint: Hsla,
    pub saturation: f32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
}

impl From<BlurRect> for Primitive {
    fn from(blur_rect: BlurRect) -> Self {
        Primitive::BlurRect(blur_rect)
    }
}

impl BlurRect {
    pub(crate) fn capture_bounds(&self, viewport_size: Size<DevicePixels>) -> Bounds<ScaledPixels> {
        let margin = ScaledPixels((self.blur_radius.0 * 3.0).ceil());
        let viewport_bounds = Bounds {
            origin: point(ScaledPixels::default(), ScaledPixels::default()),
            size: Size {
                width: ScaledPixels::from(viewport_size.width),
                height: ScaledPixels::from(viewport_size.height),
            },
        };

        self.bounds
            .dilate(margin)
            .intersect(&viewport_bounds)
            .map_origin(|origin| origin.floor())
            .map_size(|size| size.ceil())
    }
}

/// A color filter applied to an element's painted output, composing across a subtree.
///
/// The identity filter leaves color untouched: `grayscale: 0.0`, `saturate: 1.0`,
/// `brightness: 1.0`, `contrast: 1.0`. Filters are applied in the fragment shader in
/// the order contrast, brightness, saturation, grayscale.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub struct ColorFilter {
    /// Fraction of the color to desaturate toward luminance, in the range 0.0 to 1.0.
    pub grayscale: f32,
    /// Saturation multiplier; 1.0 leaves saturation unchanged, 0.0 produces grayscale.
    pub saturate: f32,
    /// Brightness multiplier applied to the rgb channels; 1.0 leaves brightness unchanged.
    pub brightness: f32,
    /// Contrast multiplier around mid-gray; 1.0 leaves contrast unchanged.
    pub contrast: f32,
}

impl ColorFilter {
    /// The identity color filter, which leaves color untouched.
    pub const fn identity() -> Self {
        Self {
            grayscale: 0.0,
            saturate: 1.0,
            brightness: 1.0,
            contrast: 1.0,
        }
    }

    /// Compose this filter with another, producing a filter equivalent to applying
    /// `self` and then `other`. Multiplicative factors multiply; grayscale combines so
    /// that the result is fully gray when either input is.
    pub fn compose(self, other: ColorFilter) -> ColorFilter {
        ColorFilter {
            grayscale: 1.0 - (1.0 - self.grayscale) * (1.0 - other.grayscale),
            saturate: self.saturate * other.saturate,
            brightness: self.brightness * other.brightness,
            contrast: self.contrast * other.contrast,
        }
    }

    /// Whether this filter is the identity filter and can be skipped.
    pub fn is_identity(&self) -> bool {
        *self == ColorFilter::identity()
    }
}

impl Default for ColorFilter {
    fn default() -> Self {
        ColorFilter::identity()
    }
}

/// The blend mode to apply when rendering a quad.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BlendMode {
    /// Standard alpha blending (source over destination).
    #[default]
    Normal = 0,
    /// Darkens by multiplying source color with itself.
    Multiply = 1,
    /// Lightens by applying the screen formula to the source color.
    Screen = 2,
    /// Combines multiply and screen based on source luminance.
    Overlay = 3,
    /// A softer version of overlay that produces gentler contrast.
    SoftLight = 4,
    /// Subtracts the darker color from the lighter color.
    Difference = 5,
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Skew along each axis by the given angles in radians, around the origin
    pub fn skew(self, x_radians: f32, y_radians: f32) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, x_radians.tan()], [y_radians.tan(), 1.0]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
    pub color_filter: ColorFilter,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

pub(crate) const POLYCHROME_SPRITE_KIND_COLOR: u32 = 0;
pub(crate) const POLYCHROME_SPRITE_KIND_SUBPIXEL_TEXT: u32 = 1;
pub(crate) const POLYCHROME_SPRITE_KIND_PREMULTIPLIED: u32 = 2;
pub(crate) const POLYCHROME_SPRITE_KIND_CONTENT_BLURRED: u32 = 3;
pub(crate) const POLYCHROME_SPRITE_KIND_CONTENT_SHADOW: u32 = 4;

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub grayscale: bool,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
    pub sprite_kind: u32,
    pub color: Hsla,
    pub pad3: u32,
    pub rounded_clip_bounds: Bounds<ScaledPixels>,
    pub rounded_clip_radii: Corners<ScaledPixels>,
    pub color_filter: ColorFilter,
    pub transformation: TransformationMatrix,
    pub blur_radius: f32,
    pub pad2: u32,
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(target_os = "macos")]
    pub image_buffer: core_video::pixel_buffer::CVPixelBuffer,
}

#[derive(Clone, Debug)]
pub(crate) struct CachedSurfaceSnapshot {
    pub paint_operations: Range<usize>,
    pub source_bounds: Bounds<DevicePixels>,
    pub target: AtlasTile,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PathId(pub(crate) usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub(crate) id: PathId,
    pub(crate) order: DrawOrder,
    pub(crate) bounds: Bounds<P>,
    pub(crate) content_mask: ContentMask<P>,
    pub(crate) vertices: Vec<PathVertex<P>>,
    pub(crate) color: Background,
    pub(crate) source_path: Option<Arc<lyon::path::Path>>,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

const CURVE_FLATTEN_SEGMENT_PX: f32 = 2.0;

fn point_distance(a: Point<Pixels>, b: Point<Pixels>) -> f32 {
    let dx = a.x.0 - b.x.0;
    let dy = a.y.0 - b.y.0;
    (dx * dx + dy * dy).sqrt()
}

fn cubic_bezier_point(
    p0: Point<Pixels>,
    c1: Point<Pixels>,
    c2: Point<Pixels>,
    p3: Point<Pixels>,
    t: f32,
) -> Point<Pixels> {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    point(
        crate::px(a * p0.x.0 + b * c1.x.0 + c * c2.x.0 + d * p3.x.0),
        crate::px(a * p0.y.0 + b * c1.y.0 + c * c2.y.0 + d * p3.y.0),
    )
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            source_path: None,
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale_and_snap_conservative(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
            source_path: self.source_path.clone(),
        }
    }

    pub(crate) fn with_source_path(mut self, source_path: Arc<lyon::path::Path>) -> Self {
        self.source_path = Some(source_path);
        self
    }

    pub(crate) fn source_path(&self) -> Option<&lyon::path::Path> {
        self.source_path.as_deref()
    }

    /// Returns whether `point` (in this path's own coordinate space) lies inside the
    /// filled region, using the nonzero winding rule. Mirrors the web canvas
    /// `isPointInPath`. Returns `false` for paths built without a retained source
    /// outline (e.g. those assembled directly from vertex buffers).
    pub fn contains(&self, point: Point<Pixels>) -> bool {
        let Some(source) = self.source_path.as_ref() else {
            return false;
        };
        let position = lyon::math::point(point.x.0, point.y.0);
        lyon::algorithms::hit_test::hit_test_path(
            &position,
            source.iter(),
            lyon::path::FillRule::NonZero,
            0.1,
        )
    }

    pub(crate) fn transformed(&self, transform: TransformationMatrix) -> Self {
        if transform == TransformationMatrix::unit() {
            return self.clone();
        }

        let mut transformed = self.clone();
        let mut bounds = Bounds::default();
        for vertex in &mut transformed.vertices {
            vertex.xy_position = transform.apply(vertex.xy_position);
            bounds = bounds.union(&Bounds {
                origin: vertex.xy_position,
                size: Default::default(),
            });
        }
        transformed.start = transform.apply(self.start);
        transformed.current = transform.apply(self.current);
        transformed.bounds = bounds;
        transformed
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Close the current subpath with a straight line from the current point back to the
    /// subpath's start, mirroring the web canvas `closePath`. No-op if already at start.
    pub fn close_path(&mut self) {
        if self.current != self.start {
            self.line_to(self.start);
        }
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Draw a cubic Bézier curve from the current point to `to`, using `ctrl1` and
    /// `ctrl2` as control points. Mirrors the web canvas `bezierCurveTo`; the curve is
    /// flattened into line segments that fill with the existing winding rule.
    pub fn cubic_to(&mut self, to: Point<Pixels>, ctrl1: Point<Pixels>, ctrl2: Point<Pixels>) {
        let from = self.current;
        let hull =
            point_distance(from, ctrl1) + point_distance(ctrl1, ctrl2) + point_distance(ctrl2, to);
        let segments = ((hull / CURVE_FLATTEN_SEGMENT_PX).ceil() as usize).clamp(8, 256);
        for step in 1..=segments {
            let t = step as f32 / segments as f32;
            self.line_to(cubic_bezier_point(from, ctrl1, ctrl2, to, t));
        }
    }

    /// Append a circular arc centered at `center` with the given `radius`, sweeping from
    /// `start_angle` to `end_angle` in radians. Mirrors the web canvas `arc`; like the
    /// canvas it connects from the current point with a straight line to the arc start.
    pub fn arc(&mut self, center: Point<Pixels>, radius: Pixels, start_angle: f32, end_angle: f32) {
        let radius = radius.0.max(0.0);
        let span = end_angle - start_angle;
        let arc_len = span.abs() * radius;
        let segments = ((arc_len / CURVE_FLATTEN_SEGMENT_PX).ceil() as usize).clamp(2, 512);
        for step in 0..=segments {
            let t = step as f32 / segments as f32;
            let angle = start_angle + span * t;
            self.line_to(point(
                crate::px(center.x.0 + radius * angle.cos()),
                crate::px(center.y.0 + radius * angle.sin()),
            ));
        }
    }

    /// Append an elliptical arc centered at `center` with radii `radius_x`/`radius_y`,
    /// the ellipse rotated by `rotation` radians, sweeping from `start_angle` to
    /// `end_angle`. Mirrors the web canvas `ellipse`; flattened to line segments so it
    /// needs no shader work.
    pub fn ellipse(
        &mut self,
        center: Point<Pixels>,
        radius_x: Pixels,
        radius_y: Pixels,
        rotation: f32,
        start_angle: f32,
        end_angle: f32,
    ) {
        let radius_x = radius_x.0.max(0.0);
        let radius_y = radius_y.0.max(0.0);
        let span = end_angle - start_angle;
        let arc_len = span.abs() * radius_x.max(radius_y);
        let segments = ((arc_len / CURVE_FLATTEN_SEGMENT_PX).ceil() as usize).clamp(2, 512);
        let (cos_r, sin_r) = (rotation.cos(), rotation.sin());
        for step in 0..=segments {
            let t = step as f32 / segments as f32;
            let angle = start_angle + span * t;
            let ex = radius_x * angle.cos();
            let ey = radius_y * angle.sin();
            self.line_to(point(
                crate::px(center.x.0 + ex * cos_r - ey * sin_r),
                crate::px(center.y.0 + ex * sin_r + ey * cos_r),
            ));
        }
    }

    /// Append a closed rectangle subpath, as the web canvas `rect()` does.
    pub fn rect(&mut self, bounds: Bounds<Pixels>) {
        let x = bounds.origin.x.0;
        let y = bounds.origin.y.0;
        let w = bounds.size.width.0;
        let h = bounds.size.height.0;
        self.move_to(point(crate::px(x), crate::px(y)));
        self.line_to(point(crate::px(x + w), crate::px(y)));
        self.line_to(point(crate::px(x + w), crate::px(y + h)));
        self.line_to(point(crate::px(x), crate::px(y + h)));
        self.close_path();
    }

    /// Append a closed rounded-rectangle subpath with a uniform corner `radius`, as the
    /// web canvas `roundRect()` (single-radius form). The radius is clamped to half the
    /// smaller side; a zero radius falls back to a plain rectangle.
    pub fn round_rect(&mut self, bounds: Bounds<Pixels>, radius: Pixels) {
        let x = bounds.origin.x.0;
        let y = bounds.origin.y.0;
        let w = bounds.size.width.0;
        let h = bounds.size.height.0;
        let r = radius.0.max(0.0).min(w * 0.5).min(h * 0.5);
        if r <= 0.0 {
            self.rect(bounds);
            return;
        }
        use std::f32::consts::{FRAC_PI_2, PI};
        self.move_to(point(crate::px(x + r), crate::px(y)));
        self.arc(
            point(crate::px(x + w - r), crate::px(y + r)),
            crate::px(r),
            -FRAC_PI_2,
            0.0,
        );
        self.arc(
            point(crate::px(x + w - r), crate::px(y + h - r)),
            crate::px(r),
            0.0,
            FRAC_PI_2,
        );
        self.arc(
            point(crate::px(x + r), crate::px(y + h - r)),
            crate::px(r),
            FRAC_PI_2,
            PI,
        );
        self.arc(
            point(crate::px(x + r), crate::px(y + r)),
            crate::px(r),
            PI,
            PI + FRAC_PI_2,
        );
        self.close_path();
    }

    /// Append an arc tangent to the lines (current point → `ctrl`) and (`ctrl` → `to`)
    /// with the given `radius`, as the web canvas `arcTo` does — the primitive behind
    /// rounded paths. Degenerate or collinear inputs fall back to a straight line.
    pub fn arc_to(&mut self, ctrl: Point<Pixels>, to: Point<Pixels>, radius: Pixels) {
        let radius = radius.0.max(0.0);
        let from = self.current;
        let (v1x, v1y) = (from.x.0 - ctrl.x.0, from.y.0 - ctrl.y.0);
        let (v2x, v2y) = (to.x.0 - ctrl.x.0, to.y.0 - ctrl.y.0);
        let len1 = (v1x * v1x + v1y * v1y).sqrt();
        let len2 = (v2x * v2x + v2y * v2y).sqrt();
        if radius <= 0.0 || len1 < f32::EPSILON || len2 < f32::EPSILON {
            self.line_to(ctrl);
            return;
        }
        let (u1x, u1y) = (v1x / len1, v1y / len1);
        let (u2x, u2y) = (v2x / len2, v2y / len2);
        let cos_angle = (u1x * u2x + u1y * u2y).clamp(-1.0, 1.0);
        let angle = cos_angle.acos();
        if angle < 1e-4 || (std::f32::consts::PI - angle) < 1e-4 {
            self.line_to(ctrl);
            return;
        }
        let half = angle / 2.0;
        let tan_dist = radius / half.tan();
        let (bix, biy) = (u1x + u2x, u1y + u2y);
        let bilen = (bix * bix + biy * biy).sqrt();
        if bilen < f32::EPSILON {
            self.line_to(ctrl);
            return;
        }
        let center_dist = radius / half.sin();
        let cx = ctrl.x.0 + (bix / bilen) * center_dist;
        let cy = ctrl.y.0 + (biy / bilen) * center_dist;
        let t1x = ctrl.x.0 + u1x * tan_dist;
        let t1y = ctrl.y.0 + u1y * tan_dist;
        let t2x = ctrl.x.0 + u2x * tan_dist;
        let t2y = ctrl.y.0 + u2y * tan_dist;
        let a1 = (t1y - cy).atan2(t1x - cx);
        let a2 = (t2y - cy).atan2(t2x - cx);
        let pi = std::f32::consts::PI;
        let two_pi = 2.0 * pi;
        let mut span = a2 - a1;
        while span > pi {
            span -= two_pi;
        }
        while span < -pi {
            span += two_pi;
        }
        self.arc(
            point(crate::px(cx), crate::px(cy)),
            crate::px(radius),
            a1,
            a1 + span,
        );
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    pub(crate) fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub(crate) xy_position: Point<P>,
    pub(crate) st_position: Point<f32>,
    pub(crate) content_mask: ContentMask<P>,
}

impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_contains_hit_tests_filled_region() {
        let mut builder = crate::PathBuilder::fill();
        builder.move_to(crate::point(crate::px(0.), crate::px(0.)));
        builder.line_to(crate::point(crate::px(100.), crate::px(0.)));
        builder.line_to(crate::point(crate::px(100.), crate::px(100.)));
        builder.close();
        let path = builder.build().expect("path builds");

        assert!(path.contains(crate::point(crate::px(80.), crate::px(20.))));
        assert!(!path.contains(crate::point(crate::px(20.), crate::px(80.))));
    }

    #[test]
    fn cubic_to_flattens_to_endpoint_and_bulges() {
        let mut path = Path::new(crate::point(crate::px(0.), crate::px(0.)));
        path.cubic_to(
            crate::point(crate::px(100.), crate::px(0.)),
            crate::point(crate::px(0.), crate::px(100.)),
            crate::point(crate::px(100.), crate::px(100.)),
        );
        assert!((path.current.x.0 - 100.).abs() < 0.5);
        assert!((path.current.y.0 - 0.).abs() < 0.5);
        assert!(!path.vertices.is_empty());
        assert!(path.bounds.size.height.0 > 10.);
    }

    #[test]
    fn arc_full_circle_spans_diameter() {
        let center = crate::point(crate::px(50.), crate::px(50.));
        let radius = 25.;
        let mut path = Path::new(crate::point(crate::px(75.), crate::px(50.)));
        path.arc(center, crate::px(radius), 0., std::f32::consts::TAU);
        assert!((path.bounds.size.width.0 - 2. * radius).abs() < 1.0);
        assert!((path.bounds.size.height.0 - 2. * radius).abs() < 1.0);
    }

    #[test]
    fn ellipse_respects_radii_and_rotation() {
        let center = crate::point(crate::px(50.), crate::px(50.));

        let mut axis_aligned = Path::new(crate::point(crate::px(80.), crate::px(50.)));
        axis_aligned.ellipse(
            center,
            crate::px(30.),
            crate::px(10.),
            0.0,
            0.0,
            std::f32::consts::TAU,
        );
        assert!((axis_aligned.bounds.size.width.0 - 60.).abs() < 1.0);
        assert!((axis_aligned.bounds.size.height.0 - 20.).abs() < 1.0);

        let mut rotated = Path::new(center);
        rotated.ellipse(
            center,
            crate::px(30.),
            crate::px(10.),
            std::f32::consts::FRAC_PI_2,
            0.0,
            std::f32::consts::TAU,
        );
        assert!((rotated.bounds.size.width.0 - 20.).abs() < 1.0);
        assert!((rotated.bounds.size.height.0 - 60.).abs() < 1.0);
    }

    #[test]
    fn rect_and_round_rect_span_their_bounds() {
        let bounds = Bounds::new(
            crate::point(crate::px(10.), crate::px(20.)),
            crate::size(crate::px(100.), crate::px(60.)),
        );

        let mut rect = Path::new(crate::point(crate::px(10.), crate::px(20.)));
        rect.rect(bounds);
        assert!((rect.bounds.size.width.0 - 100.).abs() < 0.5);
        assert!((rect.bounds.size.height.0 - 60.).abs() < 0.5);
        assert!((rect.current.x.0 - 10.).abs() < 0.5);
        assert!((rect.current.y.0 - 20.).abs() < 0.5);

        let mut rounded = Path::new(crate::point(crate::px(10.), crate::px(20.)));
        rounded.round_rect(bounds, crate::px(12.));
        assert!((rounded.bounds.size.width.0 - 100.).abs() < 1.0);
        assert!((rounded.bounds.size.height.0 - 60.).abs() < 1.0);
    }

    #[test]
    fn arc_to_rounds_corner_to_second_tangent() {
        let mut path = Path::new(crate::point(crate::px(0.), crate::px(0.)));
        path.arc_to(
            crate::point(crate::px(100.), crate::px(0.)),
            crate::point(crate::px(100.), crate::px(100.)),
            crate::px(20.),
        );
        assert!((path.current.x.0 - 100.).abs() < 0.5);
        assert!((path.current.y.0 - 20.).abs() < 0.5);
    }

    #[test]
    fn arc_to_collinear_falls_back_to_line() {
        let mut path = Path::new(crate::point(crate::px(0.), crate::px(0.)));
        let ctrl = crate::point(crate::px(50.), crate::px(0.));
        path.arc_to(
            ctrl,
            crate::point(crate::px(100.), crate::px(0.)),
            crate::px(10.),
        );
        assert!((path.current.x.0 - 50.).abs() < 0.5);
        assert!((path.current.y.0 - 0.).abs() < 0.5);
    }

    #[test]
    fn close_path_returns_current_to_subpath_start() {
        let start = crate::point(crate::px(0.), crate::px(0.));
        let mut path = Path::new(start);
        path.line_to(crate::point(crate::px(100.), crate::px(0.)));
        path.line_to(crate::point(crate::px(100.), crate::px(100.)));
        path.close_path();
        assert!((path.current.x.0 - start.x.0).abs() < 0.001);
        assert!((path.current.y.0 - start.y.0).abs() < 0.001);
    }

    fn wgsl_struct_span(module: &naga::Module, struct_name: &str) -> usize {
        let (_, ty) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some(struct_name))
            .unwrap_or_else(|| panic!("struct '{struct_name}' not found in shaders.wgsl"));
        match ty.inner {
            naga::TypeInner::Struct { span, .. } => span as usize,
            _ => panic!("type '{struct_name}' is not a struct in shaders.wgsl"),
        }
    }

    #[test]
    fn gpu_primitive_structs_match_wgsl_layout() {
        let source = include_str!("platform/blade/shaders.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("shaders.wgsl failed to parse: {err:?}"));

        assert_eq!(
            std::mem::size_of::<Quad>(),
            wgsl_struct_span(&module, "Quad"),
            "Quad layout diverges from shaders.wgsl"
        );
        assert_eq!(
            std::mem::size_of::<Shadow>(),
            wgsl_struct_span(&module, "Shadow"),
            "Shadow layout diverges from shaders.wgsl"
        );
        assert_eq!(
            std::mem::size_of::<Underline>(),
            wgsl_struct_span(&module, "Underline"),
            "Underline layout diverges from shaders.wgsl"
        );
        assert_eq!(
            std::mem::size_of::<MonochromeSprite>(),
            wgsl_struct_span(&module, "MonochromeSprite"),
            "MonochromeSprite layout diverges from shaders.wgsl"
        );
        assert_eq!(
            std::mem::size_of::<PolychromeSprite>(),
            wgsl_struct_span(&module, "PolychromeSprite"),
            "PolychromeSprite layout diverges from shaders.wgsl"
        );
        assert_eq!(
            std::mem::size_of::<Background>(),
            wgsl_struct_span(&module, "Background"),
            "Background layout diverges from shaders.wgsl"
        );

        assert_eq!(std::mem::size_of::<Quad>() % 8, 0);
        assert_eq!(std::mem::size_of::<Shadow>() % 8, 0);
        assert_eq!(std::mem::size_of::<Underline>() % 8, 0);
        assert_eq!(std::mem::size_of::<MonochromeSprite>() % 8, 0);
        assert_eq!(std::mem::size_of::<PolychromeSprite>() % 8, 0);
    }

    #[test]
    fn color_filter_identity_is_default() {
        assert_eq!(ColorFilter::default(), ColorFilter::identity());
        assert!(ColorFilter::identity().is_identity());
        assert_eq!(
            ColorFilter::identity(),
            ColorFilter {
                grayscale: 0.0,
                saturate: 1.0,
                brightness: 1.0,
                contrast: 1.0,
            }
        );
    }

    #[test]
    fn color_filter_compose_with_identity_is_unchanged() {
        let filter = ColorFilter {
            grayscale: 0.4,
            saturate: 0.5,
            brightness: 1.2,
            contrast: 0.8,
        };
        for composed in [
            filter.compose(ColorFilter::identity()),
            ColorFilter::identity().compose(filter),
        ] {
            assert!((composed.grayscale - filter.grayscale).abs() < 1e-6);
            assert!((composed.saturate - filter.saturate).abs() < 1e-6);
            assert!((composed.brightness - filter.brightness).abs() < 1e-6);
            assert!((composed.contrast - filter.contrast).abs() < 1e-6);
        }
    }

    #[test]
    fn color_filter_compose_multiplies_and_saturates_grayscale() {
        let a = ColorFilter {
            grayscale: 0.5,
            saturate: 0.5,
            brightness: 2.0,
            contrast: 0.5,
        };
        let b = ColorFilter {
            grayscale: 0.5,
            saturate: 0.4,
            brightness: 1.5,
            contrast: 4.0,
        };
        let composed = a.compose(b);
        assert!((composed.grayscale - 0.75).abs() < 1e-6);
        assert!((composed.saturate - 0.2).abs() < 1e-6);
        assert!((composed.brightness - 3.0).abs() < 1e-6);
        assert!((composed.contrast - 2.0).abs() < 1e-6);
    }

    #[test]
    fn color_filter_compose_full_grayscale_stays_full() {
        let full = ColorFilter {
            grayscale: 1.0,
            ..ColorFilter::identity()
        };
        let composed = full.compose(ColorFilter::identity());
        assert!((composed.grayscale - 1.0).abs() < 1e-6);
    }

    fn test_blur_rect(bounds: Bounds<ScaledPixels>) -> BlurRect {
        BlurRect {
            order: 0,
            rounded_clip_bounds: Bounds::default(),
            rounded_clip_radii: Corners::default(),
            blur_radius: ScaledPixels(6.0),
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::all(ScaledPixels(4.0)),
            tint: Hsla::transparent_black(),
            saturation: 1.25,
        }
    }

    #[test]
    fn blur_rects_batch_together_within_the_same_layer() {
        let mut scene = Scene::default();
        let layer_bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: Size {
                width: ScaledPixels(120.0),
                height: ScaledPixels(120.0),
            },
        };
        scene.push_layer(layer_bounds);
        scene.insert_primitive(test_blur_rect(Bounds {
            origin: point(ScaledPixels(4.0), ScaledPixels(6.0)),
            size: Size {
                width: ScaledPixels(20.0),
                height: ScaledPixels(18.0),
            },
        }));
        scene.insert_primitive(test_blur_rect(Bounds {
            origin: point(ScaledPixels(40.0), ScaledPixels(12.0)),
            size: Size {
                width: ScaledPixels(24.0),
                height: ScaledPixels(16.0),
            },
        }));
        scene.pop_layer();
        scene.finish();

        let mut batches = scene.batches();
        match batches.next() {
            Some(PrimitiveBatch::BlurRects(blur_rects)) => {
                assert_eq!(blur_rects.len(), 2);
                assert!(
                    blur_rects
                        .iter()
                        .all(|blur_rect| blur_rect.order == scene.blur_rects[0].order)
                );
            }
            other => panic!("expected blur batch, got {other:?}"),
        }
        assert!(batches.next().is_none());
    }

    #[test]
    fn snapshot_subscene_replays_blur_rects() {
        let mut scene = Scene::default();
        let layer_bounds = Bounds {
            origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
            size: Size {
                width: ScaledPixels(80.0),
                height: ScaledPixels(80.0),
            },
        };
        let original = test_blur_rect(Bounds {
            origin: point(ScaledPixels(8.0), ScaledPixels(10.0)),
            size: Size {
                width: ScaledPixels(30.0),
                height: ScaledPixels(22.0),
            },
        });

        scene.push_layer(layer_bounds);
        scene.insert_primitive(original.clone());
        scene.pop_layer();
        scene.finish();

        let snapshot = scene.snapshot_subscene(0..scene.len());

        assert_eq!(snapshot.blur_rects.len(), 1);
        let replayed = &snapshot.blur_rects[0];
        assert_eq!(replayed.blur_radius, original.blur_radius);
        assert_eq!(replayed.bounds, original.bounds);
        assert_eq!(replayed.content_mask.bounds, original.content_mask.bounds);
        assert_eq!(replayed.corner_radii, original.corner_radii);
        assert_eq!(replayed.tint, original.tint);
        assert_eq!(replayed.saturation, original.saturation);

        match snapshot.batches().next() {
            Some(PrimitiveBatch::BlurRects(blur_rects)) => assert_eq!(blur_rects.len(), 1),
            other => panic!("expected replayed blur batch, got {other:?}"),
        }
    }

    #[test]
    fn structural_checksum_detects_quad_color_changes() {
        fn scene_with_quad_color(color: crate::Hsla) -> Scene {
            let bounds = Bounds {
                origin: point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size: Size {
                    width: ScaledPixels(10.0),
                    height: ScaledPixels(10.0),
                },
            };
            let mut scene = Scene::default();
            scene.insert_primitive(Quad {
                bounds,
                content_mask: ContentMask { bounds },
                background: Background::from(color),
                ..Default::default()
            });
            scene.finish();
            scene
        }

        let red = scene_with_quad_color(crate::hsla(0.0, 1.0, 0.5, 1.0));
        let red_again = scene_with_quad_color(crate::hsla(0.0, 1.0, 0.5, 1.0));
        let blue = scene_with_quad_color(crate::hsla(0.66, 1.0, 0.5, 1.0));

        assert_eq!(
            red.structural_checksum(),
            red_again.structural_checksum(),
            "identical scenes must hash equally"
        );
        assert_ne!(
            red.structural_checksum(),
            blue.structural_checksum(),
            "a color-only change must change the frame checksum"
        );
    }
}
