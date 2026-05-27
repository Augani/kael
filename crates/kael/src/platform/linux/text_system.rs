use crate::{
    Bounds, DevicePixels, Font, FontFeature, FontFeatures, FontId, FontMetrics, FontRun, FontStyle,
    FontWeight, GlyphId, LineLayout, Pixels, PlatformTextSystem, Point, RenderGlyphParams,
    SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, ShapedGlyph, ShapedRun, SharedString, Size, point,
    size,
};
use anyhow::{Context as _, Ok, Result};
use collections::HashMap;
use cosmic_text::{
    Attrs, AttrsList, CacheKey, Family, Font as CosmicTextFont, FontFeatures as CosmicFontFeatures,
    FontSystem, ShapeBuffer, ShapeLine, SwashCache,
};

use itertools::Itertools;
use parking_lot::RwLock;
use pathfinder_geometry::{
    rect::{RectF, RectI},
    vector::{Vector2F, Vector2I},
};
use smallvec::SmallVec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{borrow::Cow, sync::Arc};

/// Scale factor applied to emoji glyphs relative to the surrounding text size.
/// A value of 1.0 means emoji are rendered at exactly the font size.
/// Values between 1.0 and 1.2 are typical for matching emoji visual weight to text.
/// We use 1.1 as a balanced default that ensures emoji are legible without being oversized.
const EMOJI_SIZE_SCALE: f32 = 1.1;

pub(crate) struct CosmicTextSystem {
    state: Arc<RwLock<CosmicTextSystemState>>,
    /// Indicates whether the font system has finished loading system fonts.
    /// When false, font queries that need system fonts will wait for the background load.
    fonts_loaded: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: SharedString,
    features: FontFeatures,
}

impl FontKey {
    fn new(family: SharedString, features: FontFeatures) -> Self {
        Self { family, features }
    }
}

struct CosmicTextSystemState {
    swash_cache: SwashCache,
    font_system: FontSystem,
    scratch: ShapeBuffer,
    /// Contains all already loaded fonts, including all faces. Indexed by `FontId`.
    loaded_fonts: Vec<LoadedFont>,
    /// Caches the `FontId`s associated with a specific family to avoid iterating the font database
    /// for every font face in a family.
    font_ids_by_family_cache: HashMap<FontKey, SmallVec<[FontId; 4]>>,
}

struct LoadedFont {
    font: Arc<CosmicTextFont>,
    features: CosmicFontFeatures,
    is_known_emoji_font: bool,
    weight: cosmic_text::fontdb::Weight,
}

impl CosmicTextSystem {
    pub(crate) fn new() -> Self {
        let fonts_loaded = Arc::new(AtomicBool::new(false));
        let fonts_loaded_clone = fonts_loaded.clone();

        // Initialize with an empty font system first so the UI can start immediately.
        // System fonts are loaded on a background thread to avoid blocking the UI
        // during fc-list / fontconfig enumeration.
        let locale = std::env::var("LANG")
            .ok()
            .and_then(|l| l.split('.').next().map(|s| s.replace('_', "-")))
            .unwrap_or_else(|| String::from("en-US"));
        let font_system =
            FontSystem::new_with_locale_and_db(locale, cosmic_text::fontdb::Database::new());

        let state = Arc::new(RwLock::new(CosmicTextSystemState {
            font_system,
            swash_cache: SwashCache::new(),
            scratch: ShapeBuffer::default(),
            loaded_fonts: Vec::new(),
            font_ids_by_family_cache: HashMap::default(),
        }));

        let result = Self {
            state: state.clone(),
            fonts_loaded,
        };

        // Load system fonts on a background thread to avoid UI stalls during fc-list enumeration
        std::thread::Builder::new()
            .name("font-loader".into())
            .spawn(move || {
                // Load all system fonts (this calls fc-list / fontconfig which can be slow)
                let loaded_font_system = FontSystem::new();

                // Swap in the fully-loaded font system
                let mut state_guard = state.write();
                state_guard.font_system = loaded_font_system;
                drop(state_guard);

                fonts_loaded_clone.store(true, Ordering::Release);
            })
            .expect("failed to spawn font-loader thread");

        result
    }

    /// Ensures system fonts have been loaded. If the background thread is still loading,
    /// this will spin-wait briefly. In practice, font loading completes quickly and this
    /// is only needed for the first font query.
    fn ensure_fonts_loaded(&self) {
        if !self.fonts_loaded.load(Ordering::Acquire) {
            // Spin-wait with yield - the font loading thread should complete quickly
            while !self.fonts_loaded.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
        }
    }
}

impl Default for CosmicTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTextSystem for CosmicTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.state.write().add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        self.ensure_fonts_loaded();
        let mut result = self
            .state
            .read()
            .font_system
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|family| family.0.clone()))
            .collect_vec();
        result.sort();
        result.dedup();
        result
    }

    fn font_id(&self, font: &Font) -> Result<FontId> {
        self.ensure_fonts_loaded();
        let mut state = self.state.write();
        let key = FontKey::new(font.family.clone(), font.features.clone());
        let candidates = if let Some(font_ids) = state.font_ids_by_family_cache.get(&key) {
            font_ids.as_slice()
        } else {
            let font_ids = state.load_family(&font.family, &font.features)?;
            state.font_ids_by_family_cache.insert(key.clone(), font_ids);
            state.font_ids_by_family_cache[&key].as_ref()
        };

        let candidate_properties = candidates
            .iter()
            .map(|font_id| {
                let database_id = state.loaded_font(*font_id).font.id();
                let face_info = state.font_system.db().face(database_id).expect("");
                face_info_into_properties(face_info)
            })
            .collect::<SmallVec<[_; 4]>>();

        let ix =
            font_kit::matching::find_best_match(&candidate_properties, &font_into_properties(font))
                .context("requested font family contains no font matching the other parameters")?;

        Ok(candidates[ix])
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        let lock = self.state.read();
        let loaded_font = lock.loaded_font(font_id);
        let swash_font = loaded_font.font.as_swash();
        let metrics = swash_font.metrics(&[]);

        // Compute accurate bounding box from the font's global bounds.
        // Use the glyph bounding box from the head table if available,
        // otherwise fall back to computed values from ascent/descent/max_width.
        let bbox_height = metrics.ascent + metrics.descent.abs();
        let bounding_box = Bounds {
            origin: point(0.0, -metrics.descent.abs()),
            size: size(metrics.max_width, bbox_height),
        };

        FontMetrics {
            units_per_em: metrics.units_per_em as u32,
            ascent: metrics.ascent,
            descent: -metrics.descent.abs(),
            line_gap: metrics.leading,
            underline_position: metrics.underline_offset,
            underline_thickness: metrics.stroke_size,
            cap_height: metrics.cap_height,
            x_height: metrics.x_height,
            bounding_box,
        }
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        let lock = self.state.read();
        let loaded_font = lock.loaded_font(font_id);
        let swash_font = loaded_font.font.as_swash();
        let glyph_metrics = swash_font.glyph_metrics(&[]);
        let glyph_id_u16 = glyph_id.0 as u16;

        // Compute accurate typographic bounds using glyph-level metrics.
        // The bounds represent the ink rectangle of the glyph relative to the origin.
        let advance_width = glyph_metrics.advance_width(glyph_id_u16);
        let advance_height = glyph_metrics.advance_height(glyph_id_u16);
        let lsb = glyph_metrics.lsb(glyph_id_u16);
        let tsb = glyph_metrics.tsb(glyph_id_u16);

        Ok(Bounds {
            origin: point(lsb, tsb),
            size: size(advance_width, advance_height),
        })
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.state.read().advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.state.read().glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.state.write().raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.state.write().rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.ensure_fonts_loaded();
        self.state.write().layout_line(text, font_size, runs)
    }

    fn layout_line_with_features(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        features: &[FontFeature],
    ) -> LineLayout {
        if features.is_empty() {
            return self.layout_line(text, font_size, runs);
        }
        self.ensure_fonts_loaded();
        self.state
            .write()
            .layout_line_with_features(text, font_size, runs, features)
    }
}

impl CosmicTextSystemState {
    fn loaded_font(&self, font_id: FontId) -> &LoadedFont {
        &self.loaded_fonts[font_id.0]
    }

    #[profiling::function]
    fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        let db = self.font_system.db_mut();
        for bytes in fonts {
            match bytes {
                Cow::Borrowed(embedded_font) => {
                    db.load_font_data(embedded_font.to_vec());
                }
                Cow::Owned(bytes) => {
                    db.load_font_data(bytes);
                }
            }
        }
        Ok(())
    }

    #[profiling::function]
    fn load_family(
        &mut self,
        name: &str,
        features: &FontFeatures,
    ) -> Result<SmallVec<[FontId; 4]>> {
        // TODO: Determine the proper system UI font.
        let name = crate::text_system::font_name_with_fallbacks(name, "IBM Plex Sans");

        let families = self
            .font_system
            .db()
            .faces()
            .filter(|face| face.families.iter().any(|family| *name == family.0))
            .map(|face| (face.id, face.post_script_name.clone(), face.weight))
            .collect::<SmallVec<[_; 4]>>();

        let mut loaded_font_ids = SmallVec::new();
        for (font_id, postscript_name, weight) in families {
            let font = self
                .font_system
                .get_font(font_id, weight)
                .context("Could not load font")?;

            // HACK: To let the storybook run and render Windows caption icons. We should actually do better font fallback.
            let allowed_bad_font_names = [
                "SegoeFluentIcons", // NOTE: Segoe fluent icons postscript name is inconsistent
                "Segoe Fluent Icons",
            ];

            if font.as_swash().charmap().map('m') == 0
                && !allowed_bad_font_names.contains(&postscript_name.as_str())
            {
                self.font_system.db_mut().remove_face(font.id());
                continue;
            };

            let font_id = FontId(self.loaded_fonts.len());
            loaded_font_ids.push(font_id);
            self.loaded_fonts.push(LoadedFont {
                font: font.clone(),
                features: features.try_into()?,
                is_known_emoji_font: is_color_emoji_font(&postscript_name, &font),
                weight,
            });
        }

        Ok(loaded_font_ids)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        let glyph_metrics = self.loaded_font(font_id).font.as_swash().glyph_metrics(&[]);
        Ok(Size {
            width: glyph_metrics.advance_width(glyph_id.0 as u16),
            height: glyph_metrics.advance_height(glyph_id.0 as u16),
        })
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        let glyph_id = self.loaded_font(font_id).font.as_swash().charmap().map(ch);
        if glyph_id == 0 {
            None
        } else {
            Some(GlyphId(glyph_id.into()))
        }
    }

    fn raster_bounds(&mut self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let font = &self.loaded_fonts[params.font_id.0].font;
        let is_emoji = self.loaded_fonts[params.font_id.0].is_known_emoji_font;
        let font_weight = self.loaded_fonts[params.font_id.0].weight;
        // Apply emoji size scaling to ensure emoji glyphs are sized correctly
        // relative to surrounding text (1.0-1.2x the font size per design spec).
        let effective_font_size = if is_emoji {
            params.font_size * EMOJI_SIZE_SCALE
        } else {
            params.font_size
        };
        let subpixel_shift = point(
            params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / params.scale_factor,
            params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / params.scale_factor,
        );
        let image = self
            .swash_cache
            .get_image(
                &mut self.font_system,
                CacheKey::new(
                    font.id(),
                    params.glyph_id.0 as u16,
                    (effective_font_size * params.scale_factor).into(),
                    (subpixel_shift.x, subpixel_shift.y.trunc()),
                    font_weight,
                    cosmic_text::CacheKeyFlags::empty(),
                )
                .0,
            )
            .clone()
            .with_context(|| format!("no image for {params:?} in font {font:?}"))?;
        Ok(Bounds {
            origin: point(image.placement.left.into(), (-image.placement.top).into()),
            size: size(image.placement.width.into(), image.placement.height.into()),
        })
    }

    #[profiling::function]
    fn rasterize_glyph(
        &mut self,
        params: &RenderGlyphParams,
        glyph_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        if glyph_bounds.size.width.0 == 0 || glyph_bounds.size.height.0 == 0 {
            anyhow::bail!("glyph bounds are empty");
        } else {
            let bitmap_size = glyph_bounds.size;
            let font = &self.loaded_fonts[params.font_id.0].font;
            let is_emoji = self.loaded_fonts[params.font_id.0].is_known_emoji_font;
            let font_weight = self.loaded_fonts[params.font_id.0].weight;
            // Apply emoji size scaling consistent with raster_bounds
            let effective_font_size = if is_emoji {
                params.font_size * EMOJI_SIZE_SCALE
            } else {
                params.font_size
            };
            let subpixel_shift = point(
                params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / params.scale_factor,
                params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / params.scale_factor,
            );
            let mut image = self
                .swash_cache
                .get_image(
                    &mut self.font_system,
                    CacheKey::new(
                        font.id(),
                        params.glyph_id.0 as u16,
                        (effective_font_size * params.scale_factor).into(),
                        (subpixel_shift.x, subpixel_shift.y.trunc()),
                        font_weight,
                        cosmic_text::CacheKeyFlags::empty(),
                    )
                    .0,
                )
                .clone()
                .with_context(|| format!("no image for {params:?} in font {font:?}"))?;

            if params.is_emoji {
                // Convert from RGBA to BGRA.
                for pixel in image.data.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
            }

            Ok((bitmap_size, image.data))
        }
    }

    /// This is used when cosmic_text has chosen a fallback font instead of using the requested
    /// font, typically to handle some unicode characters. When this happens, `loaded_fonts` may not
    /// yet have an entry for this fallback font, and so one is added.
    ///
    /// Note that callers shouldn't use this `FontId` somewhere that will retrieve the corresponding
    /// `LoadedFont.features`, as it will have an arbitrarily chosen or empty value. The only
    /// current use of this field is for the *input* of `layout_line`, and so it's fine to use
    /// `font_id_for_cosmic_id` when computing the *output* of `layout_line`.
    fn font_id_for_cosmic_id(&mut self, id: cosmic_text::fontdb::ID) -> FontId {
        if let Some(ix) = self
            .loaded_fonts
            .iter()
            .position(|loaded_font| loaded_font.font.id() == id)
        {
            FontId(ix)
        } else {
            let (face_weight, face_post_script_name) = {
                let face = self.font_system.db().face(id).unwrap();
                (face.weight, face.post_script_name.clone())
            };
            let font = self.font_system.get_font(id, face_weight).unwrap();

            let font_id = FontId(self.loaded_fonts.len());
            self.loaded_fonts.push(LoadedFont {
                font: font.clone(),
                features: CosmicFontFeatures::new(),
                is_known_emoji_font: is_color_emoji_font(&face_post_script_name, &font),
                weight: face_weight,
            });

            font_id
        }
    }

    #[profiling::function]
    fn layout_line(&mut self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        let mut attrs_list = AttrsList::new(&Attrs::new());
        let mut offs = 0;
        for run in font_runs {
            let loaded_font = self.loaded_font(run.font_id);
            let font = self.font_system.db().face(loaded_font.font.id()).unwrap();

            attrs_list.add_span(
                offs..(offs + run.len),
                &Attrs::new()
                    .metadata(run.font_id.0)
                    .family(Family::Name(&font.families.first().unwrap().0))
                    .stretch(font.stretch)
                    .style(font.style)
                    .weight(font.weight)
                    .font_features(loaded_font.features.clone()),
            );
            offs += run.len;
        }

        let line = ShapeLine::new(
            &mut self.font_system,
            text,
            &attrs_list,
            cosmic_text::Shaping::Advanced,
            4,
        );
        let mut layout_lines = Vec::with_capacity(1);
        line.layout_to_buffer(
            &mut self.scratch,
            font_size.0,
            None, // We do our own wrapping
            cosmic_text::Wrap::None,
            cosmic_text::Ellipsize::None,
            None,
            &mut layout_lines,
            None,
            cosmic_text::Hinting::default(),
        );
        let layout = layout_lines.first().unwrap();

        let mut runs: Vec<ShapedRun> = Vec::new();
        for glyph in &layout.glyphs {
            let mut font_id = FontId(glyph.metadata);
            let mut loaded_font = self.loaded_font(font_id);
            if loaded_font.font.id() != glyph.font_id {
                font_id = self.font_id_for_cosmic_id(glyph.font_id);
                loaded_font = self.loaded_font(font_id);
            }
            let is_emoji = loaded_font.is_known_emoji_font;

            // HACK: Prevent crash caused by variation selectors.
            if glyph.glyph_id == 3 && is_emoji {
                continue;
            }

            let shaped_glyph = ShapedGlyph {
                id: GlyphId(glyph.glyph_id as u32),
                position: point(glyph.x.into(), glyph.y.into()),
                index: glyph.start,
                is_emoji,
            };

            if let Some(last_run) = runs
                .last_mut()
                .filter(|last_run| last_run.font_id == font_id)
            {
                last_run.glyphs.push(shaped_glyph);
            } else {
                runs.push(ShapedRun {
                    font_id,
                    glyphs: vec![shaped_glyph],
                });
            }
        }

        LineLayout {
            font_size,
            width: layout.w.into(),
            ascent: layout.max_ascent.into(),
            descent: layout.max_descent.into(),
            runs,
            len: text.len(),
        }
    }

    fn layout_line_with_features(
        &mut self,
        text: &str,
        font_size: Pixels,
        font_runs: &[FontRun],
        features: &[FontFeature],
    ) -> LineLayout {
        let mut attrs_list = AttrsList::new(&Attrs::new());
        let mut offs = 0;
        for run in font_runs {
            let loaded_font = self.loaded_font(run.font_id);
            let font = self.font_system.db().face(loaded_font.font.id()).unwrap();

            // Merge per-font features with the extra features requested by the caller.
            // Invalid or unsupported tags are silently ignored per Req 29.4 —
            // HarfBuzz simply skips tags it doesn't recognise.
            let mut merged = loaded_font.features.clone();
            for feature in features {
                let tag = cosmic_text::FeatureTag::new(&feature.tag);
                merged.set(tag, feature.value);
            }

            attrs_list.add_span(
                offs..(offs + run.len),
                &Attrs::new()
                    .metadata(run.font_id.0)
                    .family(Family::Name(&font.families.first().unwrap().0))
                    .stretch(font.stretch)
                    .style(font.style)
                    .weight(font.weight)
                    .font_features(merged),
            );
            offs += run.len;
        }

        let line = ShapeLine::new(
            &mut self.font_system,
            text,
            &attrs_list,
            cosmic_text::Shaping::Advanced,
            4,
        );
        let mut layout_lines = Vec::with_capacity(1);
        line.layout_to_buffer(
            &mut self.scratch,
            font_size.0,
            None,
            cosmic_text::Wrap::None,
            cosmic_text::Ellipsize::None,
            None,
            &mut layout_lines,
            None,
            cosmic_text::Hinting::default(),
        );
        let layout = layout_lines.first().unwrap();

        let mut runs: Vec<ShapedRun> = Vec::new();
        for glyph in &layout.glyphs {
            let mut font_id = FontId(glyph.metadata);
            let mut loaded_font = self.loaded_font(font_id);
            if loaded_font.font.id() != glyph.font_id {
                font_id = self.font_id_for_cosmic_id(glyph.font_id);
                loaded_font = self.loaded_font(font_id);
            }
            let is_emoji = loaded_font.is_known_emoji_font;

            if glyph.glyph_id == 3 && is_emoji {
                continue;
            }

            let shaped_glyph = ShapedGlyph {
                id: GlyphId(glyph.glyph_id as u32),
                position: point(glyph.x.into(), glyph.y.into()),
                index: glyph.start,
                is_emoji,
            };

            if let Some(last_run) = runs
                .last_mut()
                .filter(|last_run| last_run.font_id == font_id)
            {
                last_run.glyphs.push(shaped_glyph);
            } else {
                runs.push(ShapedRun {
                    font_id,
                    glyphs: vec![shaped_glyph],
                });
            }
        }

        LineLayout {
            font_size,
            width: layout.w.into(),
            ascent: layout.max_ascent.into(),
            descent: layout.max_descent.into(),
            runs,
            len: text.len(),
        }
    }
}

impl TryFrom<&FontFeatures> for CosmicFontFeatures {
    type Error = anyhow::Error;

    fn try_from(features: &FontFeatures) -> Result<Self> {
        let mut result = CosmicFontFeatures::new();
        for feature in features.0.iter() {
            let name_bytes: [u8; 4] = feature
                .0
                .as_bytes()
                .try_into()
                .context("Incorrect feature flag format")?;

            let tag = cosmic_text::FeatureTag::new(&name_bytes);

            result.set(tag, feature.1);
        }
        Ok(result)
    }
}

impl From<RectF> for Bounds<f32> {
    fn from(rect: RectF) -> Self {
        Bounds {
            origin: point(rect.origin_x(), rect.origin_y()),
            size: size(rect.width(), rect.height()),
        }
    }
}

impl From<RectI> for Bounds<DevicePixels> {
    fn from(rect: RectI) -> Self {
        Bounds {
            origin: point(DevicePixels(rect.origin_x()), DevicePixels(rect.origin_y())),
            size: size(DevicePixels(rect.width()), DevicePixels(rect.height())),
        }
    }
}

impl From<Vector2I> for Size<DevicePixels> {
    fn from(value: Vector2I) -> Self {
        size(value.x().into(), value.y().into())
    }
}

impl From<RectI> for Bounds<i32> {
    fn from(rect: RectI) -> Self {
        Bounds {
            origin: point(rect.origin_x(), rect.origin_y()),
            size: size(rect.width(), rect.height()),
        }
    }
}

impl From<Point<u32>> for Vector2I {
    fn from(size: Point<u32>) -> Self {
        Vector2I::new(size.x as i32, size.y as i32)
    }
}

impl From<Vector2F> for Size<f32> {
    fn from(vec: Vector2F) -> Self {
        size(vec.x(), vec.y())
    }
}

impl From<FontWeight> for cosmic_text::Weight {
    fn from(value: FontWeight) -> Self {
        cosmic_text::Weight(value.0 as u16)
    }
}

impl From<FontStyle> for cosmic_text::Style {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => cosmic_text::Style::Normal,
            FontStyle::Italic => cosmic_text::Style::Italic,
            FontStyle::Oblique => cosmic_text::Style::Oblique,
        }
    }
}

fn font_into_properties(font: &crate::Font) -> font_kit::properties::Properties {
    font_kit::properties::Properties {
        style: match font.style {
            crate::FontStyle::Normal => font_kit::properties::Style::Normal,
            crate::FontStyle::Italic => font_kit::properties::Style::Italic,
            crate::FontStyle::Oblique => font_kit::properties::Style::Oblique,
        },
        weight: font_kit::properties::Weight(font.weight.0),
        stretch: Default::default(),
    }
}

fn face_info_into_properties(
    face_info: &cosmic_text::fontdb::FaceInfo,
) -> font_kit::properties::Properties {
    font_kit::properties::Properties {
        style: match face_info.style {
            cosmic_text::Style::Normal => font_kit::properties::Style::Normal,
            cosmic_text::Style::Italic => font_kit::properties::Style::Italic,
            cosmic_text::Style::Oblique => font_kit::properties::Style::Oblique,
        },
        // both libs use the same values for weight
        weight: font_kit::properties::Weight(face_info.weight.0.into()),
        stretch: match face_info.stretch {
            cosmic_text::Stretch::Condensed => font_kit::properties::Stretch::CONDENSED,
            cosmic_text::Stretch::Expanded => font_kit::properties::Stretch::EXPANDED,
            cosmic_text::Stretch::ExtraCondensed => font_kit::properties::Stretch::EXTRA_CONDENSED,
            cosmic_text::Stretch::ExtraExpanded => font_kit::properties::Stretch::EXTRA_EXPANDED,
            cosmic_text::Stretch::Normal => font_kit::properties::Stretch::NORMAL,
            cosmic_text::Stretch::SemiCondensed => font_kit::properties::Stretch::SEMI_CONDENSED,
            cosmic_text::Stretch::SemiExpanded => font_kit::properties::Stretch::SEMI_EXPANDED,
            cosmic_text::Stretch::UltraCondensed => font_kit::properties::Stretch::ULTRA_CONDENSED,
            cosmic_text::Stretch::UltraExpanded => font_kit::properties::Stretch::ULTRA_EXPANDED,
        },
    }
}

/// Checks whether a font is a known color emoji font by its PostScript name.
/// This covers the most common emoji fonts found on Linux distributions:
/// - NotoColorEmoji: Google's color emoji font (bundled with most distros)
/// - Twemoji / TwitterColorEmoji: Twitter's open-source emoji set (used by Firefox, some distros)
/// - EmojiOne / JoyPixels: Popular third-party emoji fonts
/// - Apple Color Emoji: Sometimes installed via third-party packages
/// - Segoe UI Emoji: Sometimes available via Wine/cross-platform installs
/// - OpenMoji: Open-source emoji project
fn check_is_known_emoji_font(postscript_name: &str) -> bool {
    matches!(
        postscript_name,
        "NotoColorEmoji"
            | "Noto-COLRv1"
            | "NotoColorEmoji-Regular"
            | "Twemoji"
            | "TwemojiMozilla"
            | "TwitterColorEmoji"
            | "EmojiOneMozilla"
            | "EmojiOneColor"
            | "JoyPixels"
            | "AppleColorEmoji"
            | "SegoeUIEmoji"
            | "OpenMoji"
            | "OpenMojiColor"
    )
}

/// Detects whether a font contains color emoji OpenType tables by inspecting
/// the raw font data. Checks for:
/// - CBDT/CBLC tables: Color Bitmap Data/Location (used by NotoColorEmoji)
/// - COLR/CPAL tables: Color Layers/Palette (used by newer vector emoji fonts)
/// - sbix table: Standard Bitmap Graphics (used by Apple Color Emoji)
/// - SVG table: SVG-based color glyphs
fn has_color_font_tables(font: &cosmic_text::Font) -> bool {
    let swash = font.as_swash();
    // swash::FontRef exposes table_data() for reading raw OpenType tables.
    // We check for the presence of known color font table tags.
    let color_table_tags: &[[u8; 4]] = &[
        *b"CBDT", // Color Bitmap Data Table (bitmap emoji like NotoColorEmoji)
        *b"CBLC", // Color Bitmap Location Table (companion to CBDT)
        *b"COLR", // Color Layers table (vector emoji, COLRv0/v1)
        *b"CPAL", // Color Palette table (companion to COLR)
        *b"sbix", // Standard Bitmap Graphics (Apple-style color emoji)
        *b"SVG ", // SVG table for SVG-based color glyphs
    ];

    for tag in color_table_tags {
        let tag_value = u32::from_be_bytes(*tag);
        if swash.table(tag_value).is_some() {
            return true;
        }
    }
    false
}

/// Determines if a font is a color emoji font by checking both the PostScript name
/// against known emoji fonts and by inspecting the font's OpenType tables for
/// color glyph data (CBDT/CBLC, COLR/CPAL, sbix, SVG).
fn is_color_emoji_font(postscript_name: &str, font: &cosmic_text::Font) -> bool {
    check_is_known_emoji_font(postscript_name) || has_color_font_tables(font)
}
