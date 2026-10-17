use tiny_skia::Pixmap;
#[cfg(feature = "text")]
use tiny_skia::{LineCap as TinyLineCap, LineJoin as TinyLineJoin, Transform as PixmapTransform};
#[cfg(feature = "text")]
use unicode_segmentation::UnicodeSegmentation;

#[cfg(feature = "text")]
use super::drawing::render_shape_items;
#[cfg(feature = "text")]
use super::renderer::{BrushStyle, FillStyle, Rgba8, ShapeRenderState, ShapeStyles, StrokeStyle};
use super::renderer::{RenderTransform, Renderer, ShapeCaches};
use crate::{Animation, Layer, RasterlottieError};
#[cfg(feature = "text")]
use crate::{Font, TextDocument};

#[cfg(feature = "text")]
impl Renderer {
    pub(super) fn render_text_layer(
        animation: &Animation,
        layer: &Layer,
        frame: f32,
        pixmap: &mut Pixmap,
        inherited_transform: RenderTransform,
        shape_caches: ShapeCaches<'_>,
    ) -> Result<(), RasterlottieError> {
        span_enter!(
            tracing::Level::TRACE,
            "render_text_layer",
            frame = frame,
            layer = layer.name.as_str()
        );
        let text = layer
            .text
            .as_ref()
            .ok_or_else(|| RasterlottieError::InvalidTextLayer {
                layer: layer.name.clone(),
                detail: "text layer data is missing".to_string(),
            })?;
        if text.has_animators() {
            return Err(RasterlottieError::InvalidTextLayer {
                layer: layer.name.clone(),
                detail: "text animators are not supported yet".to_string(),
            });
        }
        if text.has_path() {
            return Err(RasterlottieError::InvalidTextLayer {
                layer: layer.name.clone(),
                detail: "text path data is not supported yet".to_string(),
            });
        }

        let document =
            text.document_at(frame)
                .ok_or_else(|| RasterlottieError::InvalidTextLayer {
                    layer: layer.name.clone(),
                    detail: "text layer has no document keyframes".to_string(),
                })?;
        let font = animation.lookup_font(&document.font).ok_or_else(|| {
            RasterlottieError::InvalidTextLayer {
                layer: layer.name.clone(),
                detail: format!("missing font `{}`", document.font),
            }
        })?;
        if document.box_size.is_some() {
            return Err(RasterlottieError::InvalidTextLayer {
                layer: layer.name.clone(),
                detail: "boxed text is not supported yet".to_string(),
            });
        }

        let styles = text_styles(document);
        let tracking_offset = (document.tracking / 1000.0) * document.size;
        let line_height = document.effective_line_height();
        let ascent = (font.ascent * document.size) / 100.0;
        let position = document.position.unwrap_or([0.0, 0.0]);

        for (line_index, line) in text_lines(document.text.as_str()).iter().enumerate() {
            let line_width = line_width(animation, font, line, document.size, tracking_offset)
                .ok_or_else(|| RasterlottieError::InvalidTextLayer {
                    layer: layer.name.clone(),
                    detail: "failed to measure text line".to_string(),
                })?;
            let justify_offset = justify_offset(document, line_width);
            let baseline_y = line_height.mul_add(
                line_index as f32,
                position[1] + ascent - document.baseline_shift,
            );
            let mut x = 0.0;

            for grapheme in line {
                let glyph = animation.lookup_glyph(grapheme, font).ok_or_else(|| {
                    RasterlottieError::InvalidTextLayer {
                        layer: layer.name.clone(),
                        detail: format!("missing glyph data for `{grapheme}`"),
                    }
                })?;
                let advance = glyph.advance_for_size(document.size).ok_or_else(|| {
                    RasterlottieError::InvalidTextLayer {
                        layer: layer.name.clone(),
                        detail: format!("glyph `{grapheme}` has non-positive size"),
                    }
                })?;
                let glyph_transform = inherited_transform.concat(RenderTransform {
                    matrix: PixmapTransform::identity()
                        .pre_translate(position[0] + justify_offset + x, baseline_y)
                        .pre_scale(document.size / glyph.size, document.size / glyph.size),
                    opacity: 1.0,
                });
                render_shape_items(
                    &glyph.data.shapes,
                    frame,
                    pixmap,
                    glyph_transform,
                    ShapeRenderState {
                        styles: &styles,
                        trim: None,
                        static_path_cache: shape_caches.static_paths,
                        shape_plan_cache: shape_caches.plans,
                        timeline_sample_cache: shape_caches.timeline_samples,
                    },
                )?;
                x += advance + tracking_offset;
            }
        }

        Ok(())
    }
}

#[cfg(not(feature = "text"))]
impl Renderer {
    pub(super) fn render_text_layer(
        _animation: &Animation,
        layer: &Layer,
        _frame: f32,
        _pixmap: &mut Pixmap,
        _inherited_transform: RenderTransform,
        _shape_caches: ShapeCaches<'_>,
    ) -> Result<(), RasterlottieError> {
        Err(RasterlottieError::InvalidTextLayer {
            layer: layer.name.clone(),
            detail: "text support is disabled because the `text` feature is not enabled"
                .to_string(),
        })
    }
}

#[cfg(feature = "text")]
fn text_styles(document: &TextDocument) -> ShapeStyles {
    ShapeStyles {
        fill: document.fill_color_rgba().map(|color| FillStyle {
            brush: BrushStyle::Solid(Rgba8::new(color[0], color[1], color[2], color[3])),
            opacity: 1.0,
        }),
        stroke: document.stroke_color_rgba().map(|color| {
            StrokeStyle::new(
                BrushStyle::Solid(Rgba8::new(color[0], color[1], color[2], color[3])),
                1.0,
                document.stroke_width,
                TinyLineCap::Butt,
                TinyLineJoin::Miter,
                4.0,
                None,
            )
        }),
    }
}

#[cfg(feature = "text")]
fn text_lines(text: &str) -> Vec<Vec<String>> {
    let normalized = text.replace("\r\n", "\n").replace(['\r', '\u{0003}'], "\n");
    normalized
        .split('\n')
        .map(|line| line.graphemes(true).map(str::to_owned).collect())
        .collect()
}

#[cfg(feature = "text")]
fn line_width(
    animation: &Animation,
    font: &Font,
    line: &[String],
    size: f32,
    tracking_offset: f32,
) -> Option<f32> {
    if line.is_empty() {
        return Some(0.0);
    }

    let mut width = 0.0;
    for grapheme in line {
        let glyph = animation.lookup_glyph(grapheme, font)?;
        width += glyph.advance_for_size(size)?;
        width += tracking_offset;
    }
    Some((width - tracking_offset).max(0.0))
}

#[cfg(feature = "text")]
fn justify_offset(document: &TextDocument, line_width: f32) -> f32 {
    match document.justify {
        1 => -line_width,
        2 => -line_width * 0.5,
        _ => 0.0,
    }
}
