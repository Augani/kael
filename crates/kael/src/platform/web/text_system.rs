use crate::{
    Bounds, DevicePixels, Font, FontFeature, FontFeatures, FontId, FontMetrics, FontRun, FontStyle,
    FontWeight, GlyphId, LineLayout, Pixels, PlatformTextSystem, RenderGlyphParams,
    SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, ShapedGlyph, ShapedRun, Size, point, px, size,
};
use anyhow::{Context as _, Result, anyhow};
use cosmic_text::{
    Attrs, AttrsList, CacheKey, CacheKeyFlags, Family, Font as CosmicFont,
    FontFeatures as CosmicFontFeatures, FontSystem, ShapeBuffer, ShapeLine, SwashCache,
    SwashContent,
};
use parking_lot::RwLock;
use std::{borrow::Cow, sync::Arc};
use wasm_bindgen::{JsCast as _, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

/// Browser text backend.
///
/// Bundled fonts are shaped with HarfRust and rasterized with Swash through
/// cosmic-text. Fonts that only exist in the browser/operating system retain a
/// Canvas 2D fallback because browser APIs do not expose their OpenType bytes.
pub(crate) struct WebTextSystem {
    state: RwLock<WebTextState>,
}

struct WebTextState {
    fonts: Vec<Font>,
    registered_names: Vec<String>,
    registered_faces: Vec<RegisteredFace>,
    cosmic_fonts: Vec<Option<LoadedCosmicFont>>,
    canvas_fallbacks: Vec<Option<FontId>>,
    force_canvas: Vec<bool>,
    font_system: FontSystem,
    swash_cache: SwashCache,
    scratch: ShapeBuffer,
}

#[derive(Clone)]
struct RegisteredFace {
    family: String,
    weight: f32,
    style: FontStyle,
    metrics: FontMetrics,
    cosmic_id: cosmic_text::fontdb::ID,
    cosmic_weight: cosmic_text::fontdb::Weight,
}

#[derive(Clone)]
struct LoadedCosmicFont {
    font: Arc<CosmicFont>,
    weight: cosmic_text::fontdb::Weight,
}

#[derive(Clone)]
struct WebFontRun {
    start: usize,
    end: usize,
    font_id: FontId,
    font: Font,
    cosmic: bool,
}

struct CosmicSegment {
    width: f32,
    ascent: f32,
    descent: f32,
    runs: Vec<ShapedRun>,
}

impl WebTextSystem {
    pub(crate) fn new() -> Self {
        let locale = web_sys::window()
            .and_then(|window| window.navigator().language())
            .unwrap_or_else(|| "en-US".into());
        let font_system =
            FontSystem::new_with_locale_and_db(locale, cosmic_text::fontdb::Database::new());
        Self {
            state: RwLock::new(WebTextState {
                fonts: Vec::new(),
                registered_names: Vec::new(),
                registered_faces: Vec::new(),
                cosmic_fonts: Vec::new(),
                canvas_fallbacks: Vec::new(),
                force_canvas: Vec::new(),
                font_system,
                swash_cache: SwashCache::new(),
                scratch: ShapeBuffer::default(),
            }),
        }
    }

    fn canvas() -> Result<(HtmlCanvasElement, CanvasRenderingContext2d)> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser document is unavailable")?;
        let canvas = document
            .create_element("canvas")
            .map_err(js_error)?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|_| anyhow!("created browser element is not a canvas"))?;
        let context = canvas
            .get_context("2d")
            .map_err(js_error)?
            .context("Canvas 2D is unavailable")?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| anyhow!("2D canvas context has an unexpected type"))?;
        Ok((canvas, context))
    }

    fn font(&self, id: FontId) -> Result<Font> {
        self.state
            .read()
            .fonts
            .get(id.0)
            .cloned()
            .ok_or_else(|| anyhow!("unknown browser font id {}", id.0))
    }

    fn css_family(family: &str) -> String {
        match family {
            ".SystemUIFont" => "system-ui, sans-serif".into(),
            ".KaelSans" => "\".KaelSans\", sans-serif".into(),
            ".KaelMono" => "\".KaelMono\", monospace".into(),
            "sans-serif" | "serif" | "monospace" | "system-ui" => family.into(),
            _ => format!("\"{}\", system-ui, sans-serif", family.replace('"', "\\\"")),
        }
    }

    fn css_font(font: &Font, pixels: f32) -> String {
        let style = match font.style {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        };
        format!(
            "{style} {} {}px {}",
            font.weight.0.clamp(100.0, 900.0),
            pixels.max(1.0),
            Self::css_family(font.family.as_ref())
        )
    }

    fn measure_font(font: &Font, text: &str, pixels: f32) -> Result<web_sys::TextMetrics> {
        let (_, context) = Self::canvas()?;
        context.set_font(&Self::css_font(font, pixels));
        context.measure_text(text).map_err(js_error)
    }

    fn canvas_font_metrics(font: &Font) -> Result<FontMetrics> {
        const UNITS_PER_EM: f32 = 1_000.0;
        let (_, context) = Self::canvas()?;
        context.set_font(&Self::css_font(font, UNITS_PER_EM));
        let em = context.measure_text("Hg").map_err(js_error)?;
        let cap = context.measure_text("H").map_err(js_error)?;
        let x = context.measure_text("x").map_err(js_error)?;
        let ascent = em.font_bounding_box_ascent() as f32;
        let descent = em.font_bounding_box_descent() as f32;
        let left = em.actual_bounding_box_left() as f32;
        let right = em.actual_bounding_box_right() as f32;
        Ok(FontMetrics {
            units_per_em: UNITS_PER_EM as u32,
            ascent,
            descent: -descent,
            line_gap: 0.0,
            underline_position: -UNITS_PER_EM * 0.1,
            underline_thickness: UNITS_PER_EM * 0.05,
            cap_height: cap.actual_bounding_box_ascent() as f32,
            x_height: x.actual_bounding_box_ascent() as f32,
            bounding_box: Bounds {
                origin: point(-left, -descent),
                size: size((left + right).max(em.width() as f32), ascent + descent),
            },
        })
    }

    fn registered_font_metrics(&self, font: &Font) -> Option<FontMetrics> {
        self.state
            .read()
            .selected_registered_face(font)
            .map(|face| face.metrics)
    }

    fn normalized_runs(&self, text: &str, runs: &[FontRun]) -> Vec<WebFontRun> {
        let state = self.state.read();
        let fallback_id = runs.first().map_or(FontId(0), |run| run.font_id);
        let run_count = runs.len().max(1);
        let mut result = Vec::with_capacity(run_count);
        let mut start = 0usize;

        for run_index in 0..run_count {
            let font_id = runs.get(run_index).map_or(fallback_id, |run| run.font_id);
            let requested_end = runs
                .get(run_index)
                .map_or(text.len(), |run| start.saturating_add(run.len));
            let end = requested_end.min(text.len());
            if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                log::error!("browser text run did not end on a UTF-8 boundary");
                break;
            }
            let Some(font) = state.fonts.get(font_id.0).cloned() else {
                log::error!("ignoring unknown browser font id {}", font_id.0);
                start = end;
                continue;
            };
            result.push(WebFontRun {
                start,
                end,
                font_id,
                font,
                cosmic: state
                    .cosmic_fonts
                    .get(font_id.0)
                    .is_some_and(Option::is_some),
            });
            start = end;
        }

        if start < text.len() {
            log::warn!("browser text runs covered fewer bytes than the source line");
        }
        result
    }

    fn append_canvas_run(
        context: &CanvasRenderingContext2d,
        run: &WebFontRun,
        text: &str,
        font_size: Pixels,
        origin_x: f32,
        shaped_runs: &mut Vec<ShapedRun>,
    ) -> f32 {
        let Some(run_text) = text.get(run.start..run.end) else {
            return 0.0;
        };
        context.set_font(&Self::css_font(&run.font, font_size.0));
        for (relative_byte_index, ch) in run_text.char_indices() {
            // Browser-owned fonts do not expose font bytes, so Canvas prefix
            // measurement remains the best available kerning-aware fallback.
            let prefix_width = if relative_byte_index == 0 {
                0.0
            } else {
                context
                    .measure_text(&run_text[..relative_byte_index])
                    .map(|metrics| metrics.width() as f32)
                    .unwrap_or(relative_byte_index as f32 * font_size.0 * 0.6)
            };
            let glyph = ShapedGlyph {
                id: GlyphId(ch as u32),
                position: point(px(origin_x + prefix_width), px(0.0)),
                index: run.start + relative_byte_index,
                is_emoji: false,
            };
            append_shaped_glyph(shaped_runs, run.font_id, glyph);
        }
        context
            .measure_text(run_text)
            .map(|metrics| metrics.width() as f32)
            .unwrap_or(run_text.chars().count() as f32 * font_size.0 * 0.6)
    }

    fn layout_line_impl(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        extra_features: &[FontFeature],
    ) -> LineLayout {
        if text.is_empty() {
            return LineLayout {
                font_size,
                len: 0,
                ..Default::default()
            };
        }

        let runs = self.normalized_runs(text, runs);
        let mut shaped_runs = Vec::new();
        let mut x = 0.0f32;
        let mut ascent = 0.0f32;
        let mut descent = 0.0f32;
        let mut canvas_context = None;
        let mut index = 0usize;

        while index < runs.len() {
            if runs[index].cosmic {
                let start = index;
                while index < runs.len() && runs[index].cosmic {
                    index += 1;
                }
                match self.state.write().shape_segment(
                    text,
                    font_size,
                    &runs[start..index],
                    extra_features,
                    x,
                ) {
                    Ok(segment) => {
                        x += segment.width;
                        ascent = ascent.max(segment.ascent);
                        descent = descent.max(segment.descent);
                        for run in segment.runs {
                            for glyph in run.glyphs {
                                append_shaped_glyph(&mut shaped_runs, run.font_id, glyph);
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("failed to shape bundled browser font: {error:#}");
                        let fallback_runs =
                            match self.state.write().canvas_fallback_runs(&runs[start..index]) {
                                Ok(runs) => runs,
                                Err(error) => {
                                    log::error!(
                                        "failed to prepare visible Canvas text fallback: {error:#}"
                                    );
                                    continue;
                                }
                            };
                        let context = match Self::canvas() {
                            Ok((_, context)) => context,
                            Err(error) => {
                                log::error!(
                                    "failed to create bundled-font fallback canvas: {error:#}"
                                );
                                continue;
                            }
                        };
                        for run in &fallback_runs {
                            let run_width = Self::append_canvas_run(
                                &context,
                                run,
                                text,
                                font_size,
                                x,
                                &mut shaped_runs,
                            );
                            x += run_width;
                            let metrics = self.font_metrics(run.font_id);
                            let scale = font_size.0 / metrics.units_per_em as f32;
                            ascent = ascent.max(metrics.ascent * scale);
                            descent = descent.max((-metrics.descent * scale).max(0.0));
                        }
                    }
                }
            } else {
                let run = &runs[index];
                let context = match canvas_context.as_ref() {
                    Some(context) => context,
                    None => match Self::canvas() {
                        Ok((_, context)) => {
                            canvas_context = Some(context);
                            canvas_context.as_ref().unwrap()
                        }
                        Err(error) => {
                            log::error!("failed to create browser text canvas: {error:#}");
                            index += 1;
                            continue;
                        }
                    },
                };
                let run_width =
                    Self::append_canvas_run(context, run, text, font_size, x, &mut shaped_runs);
                x += run_width;
                let metrics = self.font_metrics(run.font_id);
                let scale = font_size.0 / metrics.units_per_em as f32;
                ascent = ascent.max(metrics.ascent * scale);
                descent = descent.max((-metrics.descent * scale).max(0.0));
                index += 1;
            }
        }

        LineLayout {
            font_size,
            width: px(x),
            ascent: px(ascent),
            descent: px(descent),
            runs: shaped_runs,
            len: text.len(),
        }
    }

    fn update_text_probe(&self, document: &web_sys::Document) {
        let Some(root) = document.document_element() else {
            return;
        };
        if root.get_attribute("data-kael-text-probe").as_deref() != Some("requested") {
            return;
        }

        match self.run_text_probe() {
            Ok(probe) => {
                let _ = root.set_attribute("data-kael-text-ffi", &probe.ffi);
                let _ = root.set_attribute("data-kael-text-dlig", &probe.dlig);
                let _ = root.set_attribute("data-kael-text-rtl", &probe.rtl);
                let _ = root.set_attribute("data-kael-text-kerning", "verified");
                let _ = root.set_attribute("data-kael-text-probe", "passed");
            }
            Err(error) => {
                log::error!("browser text shaping probe failed: {error:#}");
                let _ = root.set_attribute("data-kael-text-probe", "failed");
                let _ = root.set_attribute("data-kael-text-probe-error", &error.to_string());
            }
        }
    }

    fn run_text_probe(&self) -> Result<TextProbe> {
        let font_id = self.font_id(&crate::font("Inter"))?;
        let run = |text: &str| {
            self.layout_line(
                text,
                px(32.0),
                &[FontRun {
                    len: text.len(),
                    font_id,
                }],
            )
        };
        let glyphs = |layout: &LineLayout| -> Vec<ShapedGlyph> {
            layout
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .cloned()
                .collect::<Vec<_>>()
        };

        // This bundled Inter build has no standard `ffi` GSUB ligature. Its
        // correct advanced-shaping result is three retained input clusters.
        let ffi_layout = run("ffi");
        let ffi_indices = glyphs(&ffi_layout)
            .into_iter()
            .map(|glyph| glyph.index)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            ffi_indices == [0, 1, 2],
            "unexpected Inter ffi clusters {ffi_indices:?}"
        );

        // Inter does contain a discretionary arrow ligature, which also
        // verifies that caller-provided OpenType features reach HarfRust.
        let arrow = self.layout_line_with_features(
            "->",
            px(32.0),
            &[FontRun { len: 2, font_id }],
            &[FontFeature::new(*b"dlig", 1)],
        );
        let arrow_indices = glyphs(&arrow)
            .into_iter()
            .map(|glyph| glyph.index)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            arrow_indices == [0],
            "unexpected Inter discretionary ligature clusters {arrow_indices:?}"
        );

        let av = run("AV");
        let a = run("A");
        let v = run("V");
        anyhow::ensure!(av.width < a.width + v.width, "Inter AV kerning was lost");

        // RLO gives the Latin-only bundled fixture a deterministic RTL case.
        // Cluster indices stay tied to the UTF-8 input while their visual x
        // positions descend in logical a/b/c order.
        let rtl_layout = run("\u{202e}abc\u{202c}");
        let rtl_glyphs = glyphs(&rtl_layout);
        let position = |index| {
            rtl_glyphs
                .iter()
                .find(|glyph| glyph.index == index)
                .map(|glyph| glyph.position.x)
                .with_context(|| format!("missing RTL cluster at byte {index}"))
        };
        let a_x = position(3)?;
        let b_x = position(4)?;
        let c_x = position(5)?;
        anyhow::ensure!(
            a_x > b_x && b_x > c_x,
            "unexpected RTL cluster positions {a_x:?}, {b_x:?}, {c_x:?}"
        );

        Ok(TextProbe {
            ffi: format!("{}:{}", ffi_indices.len(), join_indices(&ffi_indices)),
            dlig: format!("{}:{}", arrow_indices.len(), join_indices(&arrow_indices)),
            rtl: format!("{:.3}>{:.3}>{:.3}", a_x.0, b_x.0, c_x.0),
        })
    }
}

struct TextProbe {
    ffi: String,
    dlig: String,
    rtl: String,
}

impl WebTextState {
    fn selected_registered_face(&self, font: &Font) -> Option<RegisteredFace> {
        self.registered_faces
            .iter()
            .filter(|face| face.family.eq_ignore_ascii_case(font.family.as_ref()))
            .min_by(|left, right| {
                registered_face_score(left, font).total_cmp(&registered_face_score(right, font))
            })
            .cloned()
    }

    fn bind_font(&mut self, font_id: FontId) -> Result<bool> {
        if self.force_canvas.get(font_id.0).copied().unwrap_or(false) {
            return Ok(false);
        }
        let Some(font) = self.fonts.get(font_id.0).cloned() else {
            return Ok(false);
        };
        let Some(face) = self.selected_registered_face(&font) else {
            return Ok(false);
        };
        let cosmic_font = self
            .font_system
            .get_font(face.cosmic_id, face.cosmic_weight)
            .context("cosmic-text could not load a registered browser face")?;
        if self.cosmic_fonts.len() <= font_id.0 {
            self.cosmic_fonts.resize(font_id.0 + 1, None);
        }
        self.cosmic_fonts[font_id.0] = Some(LoadedCosmicFont {
            font: cosmic_font,
            weight: face.cosmic_weight,
        });
        Ok(true)
    }

    fn refresh_font_bindings(&mut self) {
        for index in 0..self.fonts.len() {
            if let Err(error) = self.bind_font(FontId(index)) {
                log::error!("failed to bind bundled browser font: {error:#}");
            }
        }
    }

    fn canvas_fallback_runs(&mut self, runs: &[WebFontRun]) -> Result<Vec<WebFontRun>> {
        runs.iter()
            .map(|run| {
                let font_id = self.canvas_fallback_font(run.font_id)?;
                Ok(WebFontRun {
                    start: run.start,
                    end: run.end,
                    font_id,
                    font: run.font.clone(),
                    cosmic: false,
                })
            })
            .collect()
    }

    fn canvas_fallback_font(&mut self, source_id: FontId) -> Result<FontId> {
        if let Some(font_id) = self
            .canvas_fallbacks
            .get(source_id.0)
            .and_then(|font_id| *font_id)
        {
            return Ok(font_id);
        }
        let font = self
            .fonts
            .get(source_id.0)
            .cloned()
            .context("bundled Canvas fallback font disappeared")?;
        let font_id = FontId(self.fonts.len());
        self.fonts.push(font);
        self.cosmic_fonts.push(None);
        self.canvas_fallbacks.push(None);
        self.force_canvas.push(true);
        if self.canvas_fallbacks.len() <= source_id.0 {
            self.canvas_fallbacks.resize(source_id.0 + 1, None);
        }
        self.canvas_fallbacks[source_id.0] = Some(font_id);
        Ok(font_id)
    }

    fn font_id_for_cosmic_id(
        &mut self,
        cosmic_id: cosmic_text::fontdb::ID,
        requested_id: FontId,
        weight: cosmic_text::fontdb::Weight,
    ) -> Result<FontId> {
        if self
            .cosmic_fonts
            .get(requested_id.0)
            .and_then(Option::as_ref)
            .is_some_and(|font| font.font.id() == cosmic_id)
        {
            return Ok(requested_id);
        }
        if let Some(index) = self.cosmic_fonts.iter().position(|font| {
            font.as_ref()
                .is_some_and(|font| font.font.id() == cosmic_id && font.weight == weight)
        }) {
            return Ok(FontId(index));
        }

        let face = self
            .font_system
            .db()
            .face(cosmic_id)
            .cloned()
            .context("cosmic-text fallback face disappeared")?;
        let cosmic_font = self
            .font_system
            .get_font(cosmic_id, weight)
            .context("cosmic-text could not load a browser fallback face")?;
        let requested_features = self
            .fonts
            .get(requested_id.0)
            .map(|font| font.features.clone())
            .unwrap_or_default();
        let descriptor = Font {
            family: face
                .families
                .first()
                .map_or_else(|| face.post_script_name.clone(), |family| family.0.clone())
                .into(),
            features: requested_features,
            fallbacks: None,
            weight: FontWeight(f32::from(weight.0)),
            style: fontdb_style(face.style),
        };
        let font_id = FontId(self.fonts.len());
        self.fonts.push(descriptor);
        self.cosmic_fonts.push(Some(LoadedCosmicFont {
            font: cosmic_font,
            weight,
        }));
        self.canvas_fallbacks.push(None);
        self.force_canvas.push(false);
        Ok(font_id)
    }

    fn shape_segment(
        &mut self,
        text: &str,
        font_size: Pixels,
        runs: &[WebFontRun],
        extra_features: &[FontFeature],
        origin_x: f32,
    ) -> Result<CosmicSegment> {
        let first = runs.first().context("bundled shape segment is empty")?;
        let last = runs.last().context("bundled shape segment is empty")?;
        let segment_text = text
            .get(first.start..last.end)
            .context("bundled shape segment is not valid UTF-8")?;
        let mut attrs_list = AttrsList::new(&Attrs::new());

        for run in runs {
            let loaded = self
                .cosmic_fonts
                .get(run.font_id.0)
                .and_then(Option::as_ref)
                .context("bundled browser font binding disappeared")?;
            let face = self
                .font_system
                .db()
                .face(loaded.font.id())
                .context("bundled browser font face disappeared")?;
            let family = face
                .families
                .first()
                .map(|family| family.0.as_str())
                .context("bundled browser font has no family")?;
            attrs_list.add_span(
                run.start - first.start..run.end - first.start,
                &Attrs::new()
                    .metadata(run.font_id.0)
                    .family(Family::Name(family))
                    .stretch(face.stretch)
                    .style(face.style)
                    .weight(loaded.weight)
                    .font_features(cosmic_features(&run.font.features, extra_features)),
            );
        }

        let line = ShapeLine::new(
            &mut self.font_system,
            segment_text,
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
        let Some(layout) = layout_lines.first() else {
            return Ok(CosmicSegment {
                width: 0.0,
                ascent: 0.0,
                descent: 0.0,
                runs: Vec::new(),
            });
        };

        let mut shaped_runs = Vec::new();
        for glyph in &layout.glyphs {
            let requested_id = FontId(glyph.metadata);
            let font_id =
                self.font_id_for_cosmic_id(glyph.font_id, requested_id, glyph.font_weight)?;
            append_shaped_glyph(
                &mut shaped_runs,
                font_id,
                ShapedGlyph {
                    id: GlyphId(u32::from(glyph.glyph_id)),
                    position: point(px(origin_x + glyph.x), px(glyph.y)),
                    index: first.start + glyph.start,
                    is_emoji: false,
                },
            );
        }

        Ok(CosmicSegment {
            width: layout.w,
            ascent: layout.max_ascent,
            descent: layout.max_descent,
            runs: shaped_runs,
        })
    }

    fn cache_key(&self, params: &RenderGlyphParams) -> Result<CacheKey> {
        let loaded = self
            .cosmic_fonts
            .get(params.font_id.0)
            .and_then(Option::as_ref)
            .context("unknown bundled browser font")?;
        let glyph_id = u16::try_from(params.glyph_id.0).context("bundled glyph id is too large")?;
        let subpixel_shift = point(
            params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / params.scale_factor,
            params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / params.scale_factor,
        );
        Ok(CacheKey::new(
            loaded.font.id(),
            glyph_id,
            (params.font_size * params.scale_factor).into(),
            (subpixel_shift.x, subpixel_shift.y),
            loaded.weight,
            CacheKeyFlags::empty(),
        )
        .0)
    }
}

impl PlatformTextSystem for WebTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser document is unavailable")?;
        let font_set = document.fonts();
        let loads = js_sys::Array::new();

        for bytes in fonts {
            let parsed = ttf_parser::Face::parse(bytes.as_ref(), 0)
                .map_err(|_| anyhow!("failed to parse a bundled browser font"))?;
            let family = parsed
                .names()
                .into_iter()
                .find(|name| name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                .and_then(|name| name.to_string())
                .or_else(|| {
                    parsed
                        .names()
                        .into_iter()
                        .find(|name| name.name_id == ttf_parser::name_id::FAMILY)
                        .and_then(|name| name.to_string())
                })
                .context("bundled browser font has no Unicode family name")?;
            let lower_family = family.to_ascii_lowercase();
            let mut aliases = vec![family.clone()];
            if lower_family.contains("inter") {
                aliases.push(".KaelSans".into());
            }
            if lower_family.contains("jetbrains") || lower_family.contains("mono") {
                aliases.push(".KaelMono".into());
            }

            let style = if parsed.is_italic() {
                FontStyle::Italic
            } else if parsed.is_oblique() {
                FontStyle::Oblique
            } else {
                FontStyle::Normal
            };
            let weight = parsed.weight().to_number() as f32;
            let bounding_box = parsed.global_bounding_box();
            let underline = parsed.underline_metrics();
            let metrics = FontMetrics {
                units_per_em: u32::from(parsed.units_per_em()),
                ascent: f32::from(parsed.ascender()),
                descent: f32::from(parsed.descender()),
                line_gap: f32::from(parsed.line_gap()),
                underline_position: underline.map_or(0.0, |metrics| f32::from(metrics.position)),
                underline_thickness: underline.map_or(0.0, |metrics| f32::from(metrics.thickness)),
                cap_height: f32::from(parsed.capital_height().unwrap_or(parsed.ascender())),
                x_height: f32::from(
                    parsed
                        .x_height()
                        .unwrap_or_else(|| parsed.ascender().saturating_mul(2) / 3),
                ),
                bounding_box: Bounds {
                    origin: point(f32::from(bounding_box.x_min), f32::from(bounding_box.y_min)),
                    size: size(
                        f32::from(bounding_box.x_max) - f32::from(bounding_box.x_min),
                        f32::from(bounding_box.y_max) - f32::from(bounding_box.y_min),
                    ),
                },
            };

            {
                let mut state = self.state.write();
                let ids = state.font_system.db_mut().load_font_source(
                    cosmic_text::fontdb::Source::Binary(Arc::new(bytes.as_ref().to_vec())),
                );
                let cosmic_id = ids
                    .first()
                    .copied()
                    .context("cosmic-text rejected a bundled browser font")?;
                let cosmic_weight = state
                    .font_system
                    .db()
                    .face(cosmic_id)
                    .map(|face| face.weight)
                    .context("cosmic-text lost a newly loaded browser face")?;
                if lower_family.contains("inter") {
                    state
                        .font_system
                        .db_mut()
                        .set_sans_serif_family(family.clone());
                }
                if lower_family.contains("jetbrains") || lower_family.contains("mono") {
                    state
                        .font_system
                        .db_mut()
                        .set_monospace_family(family.clone());
                }
                for alias in &aliases {
                    state.registered_names.push(alias.clone());
                    state.registered_faces.push(RegisteredFace {
                        family: alias.clone(),
                        weight,
                        style,
                        metrics,
                        cosmic_id,
                        cosmic_weight,
                    });
                }
                state.registered_names.sort();
                state.registered_names.dedup();
                state.refresh_font_bindings();
            }

            for alias in aliases {
                let face = web_sys::FontFace::new_with_u8_array(&alias, bytes.as_ref())
                    .map_err(js_error)?;
                face.set_weight(&parsed.weight().to_number().to_string());
                face.set_style(if parsed.is_italic() {
                    "italic"
                } else if parsed.is_oblique() {
                    "oblique"
                } else {
                    "normal"
                });
                font_set.add(&face).map_err(js_error)?;
                let load = face.load().map_err(js_error)?;
                loads.push(load.as_ref());
            }
        }

        if let Some(root) = document.document_element() {
            let _ = root.set_attribute("data-kael-text-shaper", "cosmic-text");
        }
        self.update_text_probe(&document);
        if loads.length() > 0 {
            let ready = js_sys::Promise::all(&loads);
            wasm_bindgen_futures::spawn_local(async move {
                match wasm_bindgen_futures::JsFuture::from(ready).await {
                    Ok(_) => {
                        if let Ok(canvases) =
                            document.query_selector_all("canvas[data-kael-window-surface-id]")
                        {
                            for index in 0..canvases.length() {
                                let Some(canvas) = canvases.item(index) else {
                                    continue;
                                };
                                if let Ok(event) = web_sys::Event::new("kael-fonts-loaded") {
                                    let _ = canvas.dispatch_event(&event);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("failed to load browser fonts: {}", js_error(error));
                    }
                }
            });
        }
        Ok(())
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut names = vec![
            ".SystemUIFont".into(),
            ".KaelSans".into(),
            ".KaelMono".into(),
            "system-ui".into(),
            "sans-serif".into(),
            "serif".into(),
            "monospace".into(),
        ];
        names.extend(self.state.read().registered_names.iter().cloned());
        names.sort();
        names.dedup();
        names
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        let mut state = self.state.write();
        if let Some(index) = state.fonts.iter().position(|font| font == descriptor) {
            return Ok(FontId(index));
        }
        let id = FontId(state.fonts.len());
        state.fonts.push(descriptor.clone());
        state.cosmic_fonts.push(None);
        state.canvas_fallbacks.push(None);
        state.force_canvas.push(false);
        if let Err(error) = state.bind_font(id) {
            log::error!(
                "failed to bind browser font '{}': {error:#}",
                descriptor.family
            );
        }
        Ok(id)
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        let font = self.font(font_id);
        font.as_ref()
            .ok()
            .and_then(|font| self.registered_font_metrics(font))
            .or_else(|| {
                font.as_ref()
                    .ok()
                    .and_then(|font| Self::canvas_font_metrics(font).ok())
            })
            .unwrap_or(FontMetrics {
                units_per_em: 1_000,
                ascent: 800.0,
                descent: -200.0,
                line_gap: 0.0,
                underline_position: -100.0,
                underline_thickness: 50.0,
                cap_height: 700.0,
                x_height: 500.0,
                bounding_box: Bounds {
                    origin: point(0.0, -200.0),
                    size: size(1_000.0, 1_000.0),
                },
            })
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        if let Some(loaded) = self
            .state
            .read()
            .cosmic_fonts
            .get(font_id.0)
            .and_then(Option::as_ref)
            .cloned()
        {
            let glyph_id = u16::try_from(glyph_id.0).context("bundled glyph id is too large")?;
            let metrics = loaded.font.as_swash().glyph_metrics(&[]);
            return Ok(Bounds {
                origin: point(metrics.lsb(glyph_id), metrics.tsb(glyph_id)),
                size: size(
                    metrics.advance_width(glyph_id),
                    metrics.advance_height(glyph_id),
                ),
            });
        }

        let ch = char::from_u32(glyph_id.0).context("invalid browser glyph id")?;
        let metrics = Self::measure_font(&self.font(font_id)?, &ch.to_string(), 1_000.0)?;
        let left = metrics.actual_bounding_box_left() as f32;
        let right = metrics.actual_bounding_box_right() as f32;
        let ascent = metrics.actual_bounding_box_ascent() as f32;
        let descent = metrics.actual_bounding_box_descent() as f32;
        Ok(Bounds {
            origin: point(-left, -ascent),
            size: size((left + right).max(metrics.width() as f32), ascent + descent),
        })
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        if let Some(loaded) = self
            .state
            .read()
            .cosmic_fonts
            .get(font_id.0)
            .and_then(Option::as_ref)
            .cloned()
        {
            let glyph_id = u16::try_from(glyph_id.0).context("bundled glyph id is too large")?;
            let metrics = loaded.font.as_swash().glyph_metrics(&[]);
            return Ok(size(
                metrics.advance_width(glyph_id),
                metrics.advance_height(glyph_id),
            ));
        }

        let ch = char::from_u32(glyph_id.0).context("invalid browser glyph id")?;
        Ok(size(
            Self::measure_font(&self.font(font_id)?, &ch.to_string(), 1_000.0)?.width() as f32,
            0.0,
        ))
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        if let Some(loaded) = self
            .state
            .read()
            .cosmic_fonts
            .get(font_id.0)
            .and_then(Option::as_ref)
            .cloned()
        {
            let glyph_id = loaded.font.as_swash().charmap().map(ch);
            return (glyph_id != 0).then(|| GlyphId(u32::from(glyph_id)));
        }
        Some(GlyphId(ch as u32))
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        anyhow::ensure!(
            params.scale_factor.is_finite() && params.scale_factor > 0.0,
            "glyph scale factor must be finite and positive"
        );
        let mut state = self.state.write();
        if state
            .cosmic_fonts
            .get(params.font_id.0)
            .is_some_and(Option::is_some)
        {
            let cache_key = state.cache_key(params)?;
            let WebTextState {
                font_system,
                swash_cache,
                ..
            } = &mut *state;
            let image = swash_cache
                .get_image(font_system, cache_key)
                .as_ref()
                .with_context(|| format!("no bundled browser glyph image for {params:?}"))?;
            return Ok(Bounds {
                origin: point(
                    DevicePixels(image.placement.left),
                    DevicePixels(-image.placement.top),
                ),
                size: size(
                    DevicePixels(i32::try_from(image.placement.width)?),
                    DevicePixels(i32::try_from(image.placement.height)?),
                ),
            });
        }
        drop(state);

        let ch = char::from_u32(params.glyph_id.0).context("invalid browser glyph id")?;
        let font = self.font(params.font_id)?;
        let metrics = Self::measure_font(
            &font,
            &ch.to_string(),
            params.font_size.0 * params.scale_factor,
        )?;
        let left = metrics.actual_bounding_box_left().ceil() as i32;
        let right = metrics.actual_bounding_box_right().ceil() as i32;
        let ascent = metrics.actual_bounding_box_ascent().ceil() as i32;
        let descent = metrics.actual_bounding_box_descent().ceil() as i32;
        Ok(Bounds {
            origin: point(DevicePixels(-left - 1), DevicePixels(-ascent - 1)),
            size: size(
                DevicePixels((left + right + 2).max(1)),
                DevicePixels((ascent + descent + 2).max(1)),
            ),
        })
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let mut state = self.state.write();
        if state
            .cosmic_fonts
            .get(params.font_id.0)
            .is_some_and(Option::is_some)
        {
            let cache_key = state.cache_key(params)?;
            let WebTextState {
                font_system,
                swash_cache,
                ..
            } = &mut *state;
            let image = swash_cache
                .get_image(font_system, cache_key)
                .clone()
                .with_context(|| format!("no bundled browser glyph image for {params:?}"))?;
            // The WebGL atlas owns the resulting bitmap, so do not retain a
            // second permanent copy in Swash's image cache.
            swash_cache.image_cache.remove(&cache_key);
            let alpha = match image.content {
                SwashContent::Mask => image.data,
                SwashContent::Color => image.data.chunks_exact(4).map(|pixel| pixel[3]).collect(),
                SwashContent::SubpixelMask => image
                    .data
                    .chunks_exact(4)
                    .map(|pixel| pixel[..3].iter().copied().max().unwrap_or(0))
                    .collect(),
            };
            return Ok((raster_bounds.size, alpha));
        }
        drop(state);

        let width =
            u32::try_from(raster_bounds.size.width.0).context("invalid browser glyph width")?;
        let height =
            u32::try_from(raster_bounds.size.height.0).context("invalid browser glyph height")?;
        anyhow::ensure!(width > 0 && height > 0, "browser glyph bounds are empty");
        let (canvas, context) = Self::canvas()?;
        canvas.set_width(width);
        canvas.set_height(height);
        context.set_font(&Self::css_font(
            &self.font(params.font_id)?,
            params.font_size.0 * params.scale_factor,
        ));
        context.set_text_baseline("alphabetic");
        context.set_fill_style_str("white");
        let ch = char::from_u32(params.glyph_id.0).context("invalid browser glyph id")?;
        context
            .fill_text(
                &ch.to_string(),
                f64::from(-raster_bounds.origin.x.0),
                f64::from(-raster_bounds.origin.y.0),
            )
            .map_err(js_error)?;
        let rgba = context
            .get_image_data(0.0, 0.0, width.into(), height.into())
            .map_err(js_error)?
            .data()
            .0;
        let alpha = rgba.chunks_exact(4).map(|pixel| pixel[3]).collect();
        Ok((raster_bounds.size, alpha))
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.layout_line_impl(text, font_size, runs, &[])
    }

    fn layout_line_with_features(
        &self,
        text: &str,
        font_size: Pixels,
        runs: &[FontRun],
        features: &[FontFeature],
    ) -> LineLayout {
        self.layout_line_impl(text, font_size, runs, features)
    }
}

fn registered_face_score(face: &RegisteredFace, font: &Font) -> f32 {
    (face.weight - font.weight.0).abs()
        + if face.style == font.style {
            0.0
        } else {
            1_000.0
        }
}

fn cosmic_features(font: &FontFeatures, extra: &[FontFeature]) -> CosmicFontFeatures {
    let mut result = CosmicFontFeatures::new();
    for (tag, value) in font.tag_value_list() {
        let Ok(tag) = <&[u8; 4]>::try_from(tag.as_bytes()) else {
            log::error!("ignoring invalid browser font feature tag {tag:?}");
            continue;
        };
        result.set(cosmic_text::FeatureTag::new(tag), *value);
    }
    for feature in extra {
        result.set(cosmic_text::FeatureTag::new(&feature.tag), feature.value);
    }
    result
}

fn fontdb_style(style: cosmic_text::fontdb::Style) -> FontStyle {
    match style {
        cosmic_text::fontdb::Style::Normal => FontStyle::Normal,
        cosmic_text::fontdb::Style::Italic => FontStyle::Italic,
        cosmic_text::fontdb::Style::Oblique => FontStyle::Oblique,
    }
}

fn append_shaped_glyph(runs: &mut Vec<ShapedRun>, font_id: FontId, glyph: ShapedGlyph) {
    if let Some(run) = runs.last_mut().filter(|run| run.font_id == font_id) {
        run.glyphs.push(glyph);
    } else {
        runs.push(ShapedRun {
            font_id,
            glyphs: vec![glyph],
        });
    }
}

fn js_error(value: JsValue) -> anyhow::Error {
    anyhow!(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}

fn join_indices(indices: &[usize]) -> String {
    indices
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
