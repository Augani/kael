use crate::{
    Bounds, DevicePixels, Font, FontFallbacks, FontFeatures, FontId, FontMetrics, FontRun,
    FontStyle, FontWeight, GlyphId, GlyphRasterMode, LineLayout, Pixels, PlatformTextSystem, Point,
    RenderGlyphParams, Result, SUBPIXEL_VARIANTS_X, ShapedGlyph, ShapedRun, SharedString, Size,
    point, px, size, swap_rgba_pa_to_bgra,
};
use anyhow::anyhow;
use collections::HashMap;
use core_foundation::{
    attributed_string::CFMutableAttributedString,
    base::{CFRange, TCFType},
    number::CFNumber,
    string::CFString,
};
use core_graphics::base::CGFloat;
use core_graphics::{
    base::{
        CGGlyph, kCGBitmapByteOrder32Little, kCGImageAlphaPremultipliedFirst,
        kCGImageAlphaPremultipliedLast,
    },
    color_space::CGColorSpace,
    context::{CGContext, CGTextDrawingMode},
    display::CGPoint,
};
use core_text::{
    font::{CTFont, CTFontRef},
    font_descriptor::{
        kCTFontSlantTrait, kCTFontSymbolicTrait, kCTFontWeightTrait, kCTFontWidthTrait,
    },
    line::CTLine,
    string_attributes::kCTFontAttributeName,
};
use font_kit::{
    font::Font as FontKitFont,
    handle::Handle,
    hinting::HintingOptions,
    metrics::Metrics,
    properties::{Style as FontkitStyle, Weight as FontkitWeight},
    source::SystemSource,
    sources::mem::MemSource,
};
use objc2::rc::Retained;
use objc2_app_kit::NSFont;
use parking_lot::{RwLock, RwLockUpgradableReadGuard};
use pathfinder_geometry::{
    rect::{RectF, RectI},
    transform2d::Transform2F,
    vector::{Vector2F, Vector2I},
};
use smallvec::SmallVec;
use std::{borrow::Cow, char, convert::TryFrom, sync::Arc};

use super::open_type::apply_features_and_fallbacks;

#[allow(non_upper_case_globals)]
const kCGImageAlphaOnly: u32 = 7;

/// SF Pro switches from the "Text" to the "Display" optical variant at 20pt.
/// AppKit/SwiftUI handle this automatically via `+[NSFont systemFontOfSize:weight:]`;
/// Kael must mirror this to match native rendering at headline sizes.
const SYSTEM_FONT_DISPLAY_OPTICAL_THRESHOLD: f32 = 20.0;

/// Representative size used to materialize the SF Pro Display optical variant.
/// The opsz axis travels with the font; cloning to any size >= 20pt keeps the
/// glyph proportions in the Display range, which is what AppKit produces.
const SYSTEM_DISPLAY_REPRESENTATIVE_SIZE: f64 = 24.0;

/// Representative size used to materialize the SF Pro Text optical variant
/// so it matches what AppKit ships at body sizes (11-19pt).
const SYSTEM_TEXT_REPRESENTATIVE_SIZE: f64 = 14.0;

pub(crate) struct MacTextSystem(RwLock<MacTextSystemState>);

#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    font_family: SharedString,
    font_features: FontFeatures,
    font_fallbacks: Option<FontFallbacks>,
}

struct MacTextSystemState {
    memory_source: MemSource,
    system_source: SystemSource,
    fonts: Vec<FontKitFont>,
    font_selections: HashMap<Font, FontId>,
    font_ids_by_postscript_name: HashMap<String, FontId>,
    font_ids_by_font_key: HashMap<FontKey, SmallVec<[FontId; 4]>>,
    postscript_names_by_font_id: HashMap<FontId, String>,
    zwnjs_scratch_space: Vec<(usize, usize)>,
    /// Maps a system-UI FontId rendered with the SF Pro Text optical variant
    /// to the matching SF Pro Display variant, swapped in at >=20pt to match
    /// AppKit/SwiftUI's automatic optical sizing.
    system_display_variants: HashMap<FontId, FontId>,
}

impl MacTextSystem {
    pub(crate) fn new() -> Self {
        Self(RwLock::new(MacTextSystemState {
            memory_source: MemSource::empty(),
            system_source: SystemSource::new(),
            fonts: Vec::new(),
            font_selections: HashMap::default(),
            font_ids_by_postscript_name: HashMap::default(),
            font_ids_by_font_key: HashMap::default(),
            postscript_names_by_font_id: HashMap::default(),
            zwnjs_scratch_space: Vec::new(),
            system_display_variants: HashMap::default(),
        }))
    }
}

impl Default for MacTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTextSystem for MacTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.0.write().add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let collection = core_text::font_collection::create_for_all_families();
        let Some(descriptors) = collection.get_descriptors() else {
            return names;
        };
        for descriptor in descriptors.into_iter() {
            names.extend(lenient_font_attributes::family_name(&descriptor));
        }
        if let Ok(fonts_in_memory) = self.0.read().memory_source.all_families() {
            names.extend(fonts_in_memory);
        }
        names
    }

    fn font_id(&self, font: &Font) -> Result<FontId> {
        let lock = self.0.upgradable_read();
        if let Some(font_id) = lock.font_selections.get(font) {
            Ok(*font_id)
        } else {
            let mut lock = RwLockUpgradableReadGuard::upgrade(lock);
            if font.family.as_ref() == ".SystemUIFont"
                && font.style == FontStyle::Normal
                && font.features.tag_value_list().is_empty()
                && font.fallbacks.is_none()
            {
                let font_id = lock.id_for_system_ui_font(font.weight)?;
                lock.font_selections.insert(font.clone(), font_id);
                return Ok(font_id);
            }

            let font_key = FontKey {
                font_family: font.family.clone(),
                font_features: font.features.clone(),
                font_fallbacks: font.fallbacks.clone(),
            };
            let candidates = if let Some(font_ids) = lock.font_ids_by_font_key.get(&font_key) {
                font_ids.as_slice()
            } else {
                let font_ids =
                    lock.load_family(&font.family, &font.features, font.fallbacks.as_ref())?;
                lock.font_ids_by_font_key.insert(font_key.clone(), font_ids);
                lock.font_ids_by_font_key[&font_key].as_ref()
            };

            let candidate_properties = candidates
                .iter()
                .map(|font_id| lock.fonts[font_id.0].properties())
                .collect::<SmallVec<[_; 4]>>();

            let ix = font_kit::matching::find_best_match(
                &candidate_properties,
                &font_kit::properties::Properties {
                    style: font.style.into(),
                    weight: font.weight.into(),
                    stretch: Default::default(),
                },
            )?;

            let font_id = candidates[ix];
            lock.font_selections.insert(font.clone(), font_id);
            Ok(font_id)
        }
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        self.0.read().fonts[font_id.0].metrics().into()
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        Ok(self.0.read().fonts[font_id.0]
            .typographic_bounds(glyph_id.0)?
            .into())
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.0.read().advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.read().glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.0.read().raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        glyph_id: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.read().rasterize_glyph(glyph_id, raster_bounds)
    }

    fn supports_subpixel_glyphs(&self) -> bool {
        true
    }

    fn layout_line(&self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        self.0.write().layout_line(text, font_size, font_runs)
    }
}

impl MacTextSystemState {
    fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        let fonts = fonts
            .into_iter()
            .map(|bytes| match bytes {
                Cow::Borrowed(embedded_font) => {
                    let data_provider = unsafe {
                        core_graphics::data_provider::CGDataProvider::from_slice(embedded_font)
                    };
                    let font = core_graphics::font::CGFont::from_data_provider(data_provider)
                        .map_err(|()| anyhow!("Could not load an embedded font."))?;
                    let font = font_kit::loaders::core_text::Font::from_core_graphics_font(font);
                    Ok(Handle::from_native(&font))
                }
                Cow::Owned(bytes) => Ok(Handle::from_memory(Arc::new(bytes), 0)),
            })
            .collect::<Result<Vec<_>>>()?;
        self.memory_source.add_fonts(fonts.into_iter())?;
        Ok(())
    }

    fn load_family(
        &mut self,
        name: &str,
        features: &FontFeatures,
        fallbacks: Option<&FontFallbacks>,
    ) -> Result<SmallVec<[FontId; 4]>> {
        let name = crate::text_system::font_name_with_fallbacks(name, ".AppleSystemUIFont");

        let mut font_ids = SmallVec::new();
        let family = self
            .memory_source
            .select_family_by_name(name)
            .or_else(|_| self.system_source.select_family_by_name(name))?;
        for font in family.fonts() {
            let mut font = font.load()?;

            apply_features_and_fallbacks(&mut font, features, fallbacks)?;
            // This block contains a precautionary fix to guard against loading fonts
            // that might cause panics due to `.unwrap()`s up the chain.
            {
                // We use the 'm' character for text measurements in various spots
                // (e.g., the editor). However, at time of writing some of those usages
                // will panic if the font has no 'm' glyph.
                //
                // Therefore, we check up front that the font has the necessary glyph.
                let has_m_glyph = font.glyph_for_char('m').is_some();

                // HACK: The 'Segoe Fluent Icons' font does not have an 'm' glyph,
                // but we need to be able to load it for rendering Windows icons in
                // the Storybook (on macOS).
                let is_segoe_fluent_icons = font.full_name() == "Segoe Fluent Icons";

                if !has_m_glyph && !is_segoe_fluent_icons {
                    // I spent far too long trying to track down why a font missing the 'm'
                    // character wasn't loading. This log statement will hopefully save
                    // someone else from suffering the same fate.
                    log::warn!(
                        "font '{}' has no 'm' character and was not loaded",
                        font.full_name()
                    );
                    continue;
                }
            }

            // We've seen a number of panics in production caused by calling font.properties()
            // which unwraps a downcast to CFNumber. This is an attempt to avoid the panic,
            // and to try and identify the incalcitrant font.
            let traits = font.native_font().all_traits();
            if unsafe {
                !(traits
                    .get(kCTFontSymbolicTrait)
                    .downcast::<CFNumber>()
                    .is_some()
                    && traits
                        .get(kCTFontWidthTrait)
                        .downcast::<CFNumber>()
                        .is_some()
                    && traits
                        .get(kCTFontWeightTrait)
                        .downcast::<CFNumber>()
                        .is_some()
                    && traits
                        .get(kCTFontSlantTrait)
                        .downcast::<CFNumber>()
                        .is_some())
            } {
                log::error!(
                    "Failed to read traits for font {:?}",
                    font.postscript_name().unwrap()
                );
                continue;
            }

            let font_id = FontId(self.fonts.len());
            font_ids.push(font_id);
            let postscript_name = font.postscript_name().unwrap();
            self.font_ids_by_postscript_name
                .insert(postscript_name.clone(), font_id);
            self.postscript_names_by_font_id
                .insert(font_id, postscript_name);
            self.fonts.push(font);
        }
        Ok(font_ids)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        Ok(self.fonts[font_id.0].advance(glyph_id.0)?.into())
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.fonts[font_id.0].glyph_for_char(ch).map(GlyphId)
    }

    fn id_for_native_font(&mut self, requested_font: CTFont) -> FontId {
        let postscript_name = requested_font.postscript_name();
        if let Some(font_id) = self.font_ids_by_postscript_name.get(&postscript_name) {
            *font_id
        } else {
            let font_id = FontId(self.fonts.len());
            self.font_ids_by_postscript_name
                .insert(postscript_name.clone(), font_id);
            self.postscript_names_by_font_id
                .insert(font_id, postscript_name);
            self.fonts
                .push(unsafe { font_kit::font::Font::from_native_font(&requested_font) });
            font_id
        }
    }

    fn id_for_system_ui_font(&mut self, weight: FontWeight) -> Result<FontId> {
        let postscript_name = system_ui_font_postscript_name(weight);

        let text_id = if let Some(font_id) = self
            .font_ids_by_postscript_name
            .get(postscript_name)
            .copied()
        {
            font_id
        } else if let Some(ct_font) =
            create_system_ui_ct_font(SYSTEM_TEXT_REPRESENTATIVE_SIZE, weight)
        {
            // Preferred path: ask AppKit for the system font at a Text-range size
            // so the opsz axis is set correctly and the CTFont matches what
            // SwiftUI's `Font.system(size:weight:)` produces below 20pt.
            self.id_for_native_font(ct_font)
        } else {
            // Fallback: legacy path via the private PostScript name. This loses
            // the opsz axis but is used only if the NSFont API is unavailable.
            let cg_font = core_graphics::font::CGFont::from_name(&CFString::from_static_string(
                postscript_name,
            ))
            .map_err(|()| anyhow!("failed to load system UI font '{postscript_name}'"))?;
            self.id_for_native_font(core_text::font::new_from_CGFont(&cg_font, 0.))
        };

        if !self.system_display_variants.contains_key(&text_id) {
            if let Some(display_font) =
                create_system_ui_ct_font(SYSTEM_DISPLAY_REPRESENTATIVE_SIZE, weight)
            {
                let display_id = self.id_for_native_font(display_font);
                if display_id != text_id {
                    self.system_display_variants.insert(text_id, display_id);
                }
            }
        }

        Ok(text_id)
    }

    fn is_emoji(&self, font_id: FontId) -> bool {
        self.postscript_names_by_font_id
            .get(&font_id)
            .is_some_and(|postscript_name| {
                postscript_name == "AppleColorEmoji" || postscript_name == ".AppleColorEmojiUI"
            })
    }

    fn raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let font = &self.fonts[params.font_id.0];
        let scale = Transform2F::from_scale(params.scale_factor);
        Ok(font
            .raster_bounds(
                params.glyph_id.0,
                params.font_size.into(),
                scale,
                HintingOptions::None,
                font_kit::canvas::RasterizationOptions::GrayscaleAa,
            )?
            .into())
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        glyph_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        if glyph_bounds.size.width.0 == 0 || glyph_bounds.size.height.0 == 0 {
            anyhow::bail!("glyph bounds are empty");
        } else {
            // Add an extra pixel when the subpixel variant isn't zero to make room for anti-aliasing.
            let mut bitmap_size = glyph_bounds.size;
            if params.subpixel_variant.x > 0 {
                bitmap_size.width += DevicePixels(1);
            }
            if params.subpixel_variant.y > 0 {
                bitmap_size.height += DevicePixels(1);
            }
            let bitmap_size = bitmap_size;

            let mut bytes;
            let cx;
            if params.is_emoji {
                bytes = vec![0; bitmap_size.width.0 as usize * 4 * bitmap_size.height.0 as usize];
                cx = CGContext::create_bitmap_context(
                    Some(bytes.as_mut_ptr() as *mut _),
                    bitmap_size.width.0 as usize,
                    bitmap_size.height.0 as usize,
                    8,
                    bitmap_size.width.0 as usize * 4,
                    &CGColorSpace::create_device_rgb(),
                    kCGImageAlphaPremultipliedLast,
                );
            } else if params.raster_mode == GlyphRasterMode::Subpixel {
                bytes = vec![0; bitmap_size.width.0 as usize * 4 * bitmap_size.height.0 as usize];
                cx = CGContext::create_bitmap_context(
                    Some(bytes.as_mut_ptr() as *mut _),
                    bitmap_size.width.0 as usize,
                    bitmap_size.height.0 as usize,
                    8,
                    bitmap_size.width.0 as usize * 4,
                    &CGColorSpace::create_device_rgb(),
                    kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little,
                );
            } else {
                bytes = vec![0; bitmap_size.width.0 as usize * bitmap_size.height.0 as usize];
                cx = CGContext::create_bitmap_context(
                    Some(bytes.as_mut_ptr() as *mut _),
                    bitmap_size.width.0 as usize,
                    bitmap_size.height.0 as usize,
                    8,
                    bitmap_size.width.0 as usize,
                    &CGColorSpace::create_device_gray(),
                    kCGImageAlphaOnly,
                );
            }

            // Move the origin to bottom left and account for scaling, this
            // makes drawing text consistent with the font-kit's raster_bounds.
            cx.translate(
                -glyph_bounds.origin.x.0 as CGFloat,
                (glyph_bounds.origin.y.0 + glyph_bounds.size.height.0) as CGFloat,
            );
            cx.scale(
                params.scale_factor as CGFloat,
                params.scale_factor as CGFloat,
            );

            let subpixel_shift = params
                .subpixel_variant
                .map(|v| v as f32 / SUBPIXEL_VARIANTS_X as f32);
            cx.set_text_drawing_mode(CGTextDrawingMode::CGTextFill);
            // Modern AppKit/SwiftUI render text with `AppleFontSmoothing = 0`
            // (CG font smoothing OFF) — this is the macOS default since 10.14.
            // The legacy smoothing path thickens glyphs noticeably on light
            // backgrounds; leaving it on makes Kael's body text and section
            // headers look bolder than the same string drawn by SwiftUI.
            if params.raster_mode == GlyphRasterMode::Subpixel {
                cx.set_should_smooth_fonts(false);
                cx.set_rgb_fill_color(1.0, 1.0, 1.0, 1.0);
            } else {
                cx.set_should_smooth_fonts(false);
                cx.set_gray_fill_color(0.0, 1.0);
            }
            cx.set_allows_antialiasing(true);
            cx.set_should_antialias(true);
            cx.set_allows_font_subpixel_positioning(true);
            cx.set_should_subpixel_position_fonts(true);
            cx.set_allows_font_subpixel_quantization(false);
            cx.set_should_subpixel_quantize_fonts(false);
            self.fonts[params.font_id.0]
                .native_font()
                .clone_with_font_size(f32::from(params.font_size) as CGFloat)
                .draw_glyphs(
                    &[params.glyph_id.0 as CGGlyph],
                    &[CGPoint::new(
                        (subpixel_shift.x / params.scale_factor) as CGFloat,
                        (subpixel_shift.y / params.scale_factor) as CGFloat,
                    )],
                    cx,
                );

            if params.is_emoji {
                // Convert from RGBA with premultiplied alpha to BGRA with straight alpha.
                for pixel in bytes.chunks_exact_mut(4) {
                    swap_rgba_pa_to_bgra(pixel);
                }
            }

            Ok((bitmap_size, bytes))
        }
    }

    fn layout_line(&mut self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        const ZWNJ: char = '\u{200C}';
        const ZWNJ_STR: &str = "\u{200C}";
        const ZWNJ_SIZE_16: usize = ZWNJ.len_utf16();

        // SF Pro switches optical variants at 20pt. AppKit/SwiftUI swap the font
        // automatically; do the same here so headlines pick up SF Pro Display
        // instead of staying on the lighter SF Pro Text glyphs.
        let display_substituted: Option<SmallVec<[FontRun; 4]>> = if f32::from(font_size)
            >= SYSTEM_FONT_DISPLAY_OPTICAL_THRESHOLD
            && !self.system_display_variants.is_empty()
        {
            let mut any = false;
            let swapped = font_runs
                .iter()
                .map(|run| {
                    let mapped = self
                        .system_display_variants
                        .get(&run.font_id)
                        .copied()
                        .unwrap_or(run.font_id);
                    if mapped != run.font_id {
                        any = true;
                    }
                    FontRun {
                        font_id: mapped,
                        len: run.len,
                    }
                })
                .collect::<SmallVec<[FontRun; 4]>>();
            if any { Some(swapped) } else { None }
        } else {
            None
        };
        let font_runs: &[FontRun] = display_substituted.as_deref().unwrap_or(font_runs);

        self.zwnjs_scratch_space.clear();
        // Construct the attributed string, converting UTF8 ranges to UTF16 ranges.
        let mut string = CFMutableAttributedString::new();
        let mut max_ascent = 0.0f32;
        let mut max_descent = 0.0f32;

        {
            let mut ix_converter = StringIndexConverter::new(&text);
            let mut last_font_run = None;
            for run in font_runs {
                let text = &text[ix_converter.utf8_ix..][..run.len];
                // if the fonts are the same, we need to disconnect the text with a ZWNJ
                // to prevent core text from forming ligatures between them
                let needs_zwnj = last_font_run.replace(run.font_id) == Some(run.font_id);

                let n_zwnjs = self.zwnjs_scratch_space.len();
                let utf16_start = ix_converter.utf16_ix + n_zwnjs * ZWNJ_SIZE_16;
                ix_converter.advance_to_utf8_ix(ix_converter.utf8_ix + run.len);

                string.replace_str(&CFString::new(text), CFRange::init(utf16_start as isize, 0));
                if needs_zwnj {
                    let zwnjs_pos = string.char_len();
                    self.zwnjs_scratch_space.push((n_zwnjs, zwnjs_pos as usize));
                    string.replace_str(
                        &CFString::from_static_string(ZWNJ_STR),
                        CFRange::init(zwnjs_pos, 0),
                    );
                }
                let utf16_end = string.char_len() as usize;

                let cf_range =
                    CFRange::init(utf16_start as isize, (utf16_end - utf16_start) as isize);
                let font = &self.fonts[run.font_id.0];

                let font_metrics = font.metrics();
                let font_scale = font_size.0 / font_metrics.units_per_em as f32;
                max_ascent = max_ascent.max(font_metrics.ascent * font_scale);
                max_descent = max_descent.max(-font_metrics.descent * font_scale);

                unsafe {
                    string.set_attribute(
                        cf_range,
                        kCTFontAttributeName,
                        &font.native_font().clone_with_font_size(font_size.into()),
                    );
                }
            }
        }
        // Retrieve the glyphs from the shaped line, converting UTF16 offsets to UTF8 offsets.
        let line = CTLine::new_with_attributed_string(string.as_concrete_TypeRef());
        let glyph_runs = line.glyph_runs();
        let mut runs = <Vec<ShapedRun>>::with_capacity(glyph_runs.len() as usize);
        let mut ix_converter = StringIndexConverter::new(text);
        for run in glyph_runs.into_iter() {
            let attributes = run.attributes().unwrap();
            let font = unsafe {
                attributes
                    .get(kCTFontAttributeName)
                    .downcast::<CTFont>()
                    .unwrap()
            };
            let font_id = self.id_for_native_font(font);

            let mut glyphs = match runs.last_mut() {
                Some(run) if run.font_id == font_id => &mut run.glyphs,
                _ => {
                    runs.push(ShapedRun {
                        font_id,
                        glyphs: Vec::with_capacity(run.glyph_count().try_into().unwrap_or(0)),
                    });
                    &mut runs.last_mut().unwrap().glyphs
                }
            };
            for ((&glyph_id, position), &glyph_utf16_ix) in run
                .glyphs()
                .iter()
                .zip(run.positions().iter())
                .zip(run.string_indices().iter())
            {
                let mut glyph_utf16_ix = usize::try_from(glyph_utf16_ix).unwrap();
                let r = self
                    .zwnjs_scratch_space
                    .binary_search_by(|&(_, it)| it.cmp(&glyph_utf16_ix));
                match r {
                    // this glyph is a ZWNJ, skip it
                    Ok(_) => continue,
                    // adjust the index to account for the ZWNJs we've inserted
                    Err(idx) => glyph_utf16_ix -= idx * ZWNJ_SIZE_16,
                }
                if ix_converter.utf16_ix > glyph_utf16_ix {
                    // We cannot reuse current index converter, as it can only seek forward. Restart the search.
                    ix_converter = StringIndexConverter::new(text);
                }
                ix_converter.advance_to_utf16_ix(glyph_utf16_ix);
                glyphs.push(ShapedGlyph {
                    id: GlyphId(glyph_id as u32),
                    position: point(position.x as f32, position.y as f32).map(px),
                    index: ix_converter.utf8_ix,
                    is_emoji: self.is_emoji(font_id),
                });
            }
        }
        let typographic_bounds = line.get_typographic_bounds();
        LineLayout {
            runs,
            font_size,
            width: typographic_bounds.width.into(),
            ascent: max_ascent.into(),
            descent: max_descent.into(),
            len: text.len(),
        }
    }
}

fn system_ui_font_postscript_name(weight: FontWeight) -> &'static str {
    match weight.0 {
        value if value <= 150.0 => ".AppleSystemUIFontUltraLight",
        value if value <= 250.0 => ".AppleSystemUIFontThin",
        value if value <= 350.0 => ".AppleSystemUIFontLight",
        value if value <= 450.0 => ".AppleSystemUIFont",
        value if value <= 550.0 => ".AppleSystemUIFontMedium",
        value if value <= 650.0 => ".AppleSystemUIFontDemi",
        value if value <= 750.0 => ".AppleSystemUIFontBold",
        value if value <= 850.0 => ".AppleSystemUIFontHeavy",
        _ => ".AppleSystemUIFontBlack",
    }
}

/// Numeric NSFontWeight that AppKit uses for `+[NSFont systemFontOfSize:weight:]`.
/// Apple's "Ultra Light" (-0.8) is the lightest weight and maps to the CSS-style
/// Thin (100), matching the existing `system_ui_font_postscript_name` table.
fn ns_font_weight(weight: FontWeight) -> CGFloat {
    match weight.0 {
        value if value <= 150.0 => -0.8,
        value if value <= 250.0 => -0.6,
        value if value <= 350.0 => -0.4,
        value if value <= 450.0 => 0.0,
        value if value <= 550.0 => 0.23,
        value if value <= 650.0 => 0.3,
        value if value <= 750.0 => 0.4,
        value if value <= 850.0 => 0.56,
        _ => 0.62,
    }
}

/// Materializes the system UI font via `+[NSFont systemFontOfSize:weight:]`, the
/// same path SwiftUI uses. The returned CTFont carries the opsz axis set to
/// `size`, so the Text/Display optical variant matches AppKit's defaults.
fn create_system_ui_ct_font(size: f64, weight: FontWeight) -> Option<CTFont> {
    let ns_weight = ns_font_weight(weight);
    let ns_font = NSFont::systemFontOfSize_weight(size as CGFloat, ns_weight);
    let ct_font_ref = Retained::as_ptr(&ns_font) as CTFontRef;
    Some(unsafe { CTFont::wrap_under_get_rule(ct_font_ref) })
}

#[derive(Clone)]
struct StringIndexConverter<'a> {
    text: &'a str,
    utf8_ix: usize,
    utf16_ix: usize,
}

impl<'a> StringIndexConverter<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            utf8_ix: 0,
            utf16_ix: 0,
        }
    }

    fn advance_to_utf8_ix(&mut self, utf8_target: usize) {
        for (ix, c) in self.text[self.utf8_ix..].char_indices() {
            if self.utf8_ix + ix >= utf8_target {
                self.utf8_ix += ix;
                return;
            }
            self.utf16_ix += c.len_utf16();
        }
        self.utf8_ix = self.text.len();
    }

    fn advance_to_utf16_ix(&mut self, utf16_target: usize) {
        for (ix, c) in self.text[self.utf8_ix..].char_indices() {
            if self.utf16_ix >= utf16_target {
                self.utf8_ix += ix;
                return;
            }
            self.utf16_ix += c.len_utf16();
        }
        self.utf8_ix = self.text.len();
    }
}

impl From<Metrics> for FontMetrics {
    fn from(metrics: Metrics) -> Self {
        FontMetrics {
            units_per_em: metrics.units_per_em,
            ascent: metrics.ascent,
            descent: metrics.descent,
            line_gap: metrics.line_gap,
            underline_position: metrics.underline_position,
            underline_thickness: metrics.underline_thickness,
            cap_height: metrics.cap_height,
            x_height: metrics.x_height,
            bounding_box: metrics.bounding_box.into(),
        }
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

impl From<FontWeight> for FontkitWeight {
    fn from(value: FontWeight) -> Self {
        FontkitWeight(value.0)
    }
}

impl From<FontStyle> for FontkitStyle {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => FontkitStyle::Normal,
            FontStyle::Italic => FontkitStyle::Italic,
            FontStyle::Oblique => FontkitStyle::Oblique,
        }
    }
}

// Some fonts may have no attributes despite `core_text` requiring them (and panicking).
// This is the same version as `core_text` has without `expect` calls.
mod lenient_font_attributes {
    use core_foundation::{
        base::{CFRetain, CFType, TCFType},
        string::{CFString, CFStringRef},
    };
    use core_text::font_descriptor::{
        CTFontDescriptor, CTFontDescriptorCopyAttribute, kCTFontFamilyNameAttribute,
    };

    pub fn family_name(descriptor: &CTFontDescriptor) -> Option<String> {
        unsafe { get_string_attribute(descriptor, kCTFontFamilyNameAttribute) }
    }

    fn get_string_attribute(
        descriptor: &CTFontDescriptor,
        attribute: CFStringRef,
    ) -> Option<String> {
        unsafe {
            let value = CTFontDescriptorCopyAttribute(descriptor.as_concrete_TypeRef(), attribute);
            if value.is_null() {
                return None;
            }

            let value = CFType::wrap_under_create_rule(value);
            assert!(value.instance_of::<CFString>());
            let s = wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef);
            Some(s.to_string())
        }
    }

    unsafe fn wrap_under_get_rule(reference: CFStringRef) -> CFString {
        unsafe {
            assert!(!reference.is_null(), "Attempted to create a NULL object.");
            let reference = CFRetain(reference as *const ::std::os::raw::c_void) as CFStringRef;
            TCFType::wrap_under_create_rule(reference)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{FontRun, GlyphId, MacTextSystem, PlatformTextSystem, font, px};

    #[test]
    fn test_layout_line_bom_char() {
        let fonts = MacTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();
        let line = "\u{feff}";
        let mut style = FontRun {
            font_id,
            len: line.len(),
        };

        let layout = fonts.layout_line(line, px(16.), &[style]);
        assert_eq!(layout.len, line.len());
        assert!(layout.runs.is_empty());

        let line = "a\u{feff}b";
        style.len = line.len();
        let layout = fonts.layout_line(line, px(16.), &[style]);
        assert_eq!(layout.len, line.len());
        assert_eq!(layout.runs.len(), 1);
        assert_eq!(layout.runs[0].glyphs.len(), 2);
        assert_eq!(layout.runs[0].glyphs[0].id, GlyphId(68u32)); // a
        // There's no glyph for \u{feff}
        assert_eq!(layout.runs[0].glyphs[1].id, GlyphId(69u32)); // b
    }

    #[test]
    fn test_layout_line_zwnj_insertion() {
        let fonts = MacTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();

        let text = "hello world";
        let font_runs = &[
            FontRun { font_id, len: 5 }, // "hello"
            FontRun { font_id, len: 6 }, // " world"
        ];

        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        for run in &layout.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }

        // Test with different font runs - should not insert ZWNJ
        let font_id2 = fonts.font_id(&font("Times")).unwrap_or(font_id);
        let font_runs_different = &[
            FontRun { font_id, len: 5 }, // "hello"
            // " world"
            FontRun {
                font_id: font_id2,
                len: 6,
            },
        ];

        let layout2 = fonts.layout_line(text, px(16.), font_runs_different);
        assert_eq!(layout2.len, text.len());

        for run in &layout2.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }
    }

    #[test]
    fn test_layout_line_zwnj_edge_cases() {
        let fonts = MacTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();

        let text = "hello";
        let font_runs = &[FontRun { font_id, len: 5 }];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        let text = "abc";
        let font_runs = &[
            FontRun { font_id, len: 1 }, // "a"
            FontRun { font_id, len: 1 }, // "b"
            FontRun { font_id, len: 1 }, // "c"
        ];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        for run in &layout.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }

        // Test with empty text
        let text = "";
        let font_runs = &[];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, 0);
        assert!(layout.runs.is_empty());
    }
}
