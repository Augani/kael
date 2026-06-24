use std::sync::Arc;

use lyon::tessellation::StrokeOptions;
pub use lyon::tessellation::{LineCap, LineJoin};
use refineable::Refineable as _;

use crate::{
    point, px, quad, size, transparent_black, App, Background, Bounds, ContentMask, Corners,
    Element, ElementId, GlobalElementId, Hsla, InspectorElementId, IntoElement, Path, PathBuilder,
    PathStyle, Pixels, Point, Radians, RenderImage, ShapedLine, Size, Style, StyleRefinement,
    Styled, TransformationMatrix, Window,
};

use super::canvas::CanvasConstructor;

#[derive(Clone)]
struct DrawState {
    transform: TransformationMatrix,
    opacity: f32,
    content_mask: ContentMask<Pixels>,
}

enum DrawCommand {
    Path {
        path: Path<Pixels>,
        fill: Background,
        state: DrawState,
    },
    Quad {
        quad: crate::PaintQuad,
        state: DrawState,
    },
    Text {
        text: ShapedLine,
        origin: Point<Pixels>,
        color: Hsla,
        state: DrawState,
    },
    Image {
        data: Arc<RenderImage>,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        frame_index: usize,
        grayscale: bool,
        state: DrawState,
    },
}

/// Stroke dash settings for canvas stroke operations.
#[derive(Clone, Debug)]
pub struct StrokeDash {
    /// The on/off dash segment lengths in pixels.
    pub segments: Vec<Pixels>,
    /// The dash pattern offset along the stroked outline.
    pub offset: Pixels,
}

/// Stroke styling for immediate-mode canvas drawing.
#[derive(Clone, Debug)]
pub struct Stroke {
    /// Stroke width in logical pixels.
    pub width: Pixels,
    /// Stroke color.
    pub color: Hsla,
    /// Optional dash pattern.
    pub dash: Option<StrokeDash>,
    /// Line cap style.
    pub cap: LineCap,
    /// Line join style.
    pub join: LineJoin,
}

/// Construct a stroke with the default cap and join settings.
pub fn stroke(width: Pixels, color: impl Into<Hsla>) -> Stroke {
    Stroke {
        width,
        color: color.into(),
        dash: None,
        cap: LineCap::Butt,
        join: LineJoin::Miter,
    }
}

/// Immediate-mode drawing context used by `canvas(size, draw)`.
pub struct DrawContext {
    bounds: Bounds<Pixels>,
    canvas_origin: Point<Pixels>,
    current_state: DrawState,
    state_stack: Vec<DrawState>,
    commands: Vec<DrawCommand>,
}

impl DrawContext {
    pub(crate) fn new(bounds: Bounds<Pixels>, content_mask: ContentMask<Pixels>) -> Self {
        Self {
            bounds: Bounds::new(Point::default(), bounds.size),
            canvas_origin: bounds.origin,
            current_state: DrawState {
                transform: TransformationMatrix::unit(),
                opacity: 1.0,
                content_mask,
            },
            state_stack: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// Returns the canvas-local bounds for the current draw pass.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Returns the canvas-local size for the current draw pass.
    pub fn size(&self) -> Size<Pixels> {
        self.bounds.size
    }

    /// Fill an existing path with the given background.
    pub fn fill_path(&mut self, path: &Path<Pixels>, fill: impl Into<Background>) {
        self.commands.push(DrawCommand::Path {
            path: path.clone(),
            fill: fill.into(),
            state: self.current_state.clone(),
        });
    }

    /// Stroke a path using the retained source outline stored on the path.
    pub fn stroke_path(&mut self, path: &Path<Pixels>, stroke: Stroke) {
        if let Some(path) = stroke_existing_path(path, &stroke) {
            self.commands.push(DrawCommand::Path {
                path,
                fill: stroke.color.into(),
                state: self.current_state.clone(),
            });
        }
    }

    /// Stroke a straight line segment between two points.
    pub fn stroke_line(&mut self, from: Point<Pixels>, to: Point<Pixels>, stroke: Stroke) {
        if let Some(path) = build_stroked_path(&stroke, |builder| {
            builder.move_to(from);
            builder.line_to(to);
        }) {
            self.commands.push(DrawCommand::Path {
                path,
                fill: stroke.color.into(),
                state: self.current_state.clone(),
            });
        }
    }

    /// Fill an axis-aligned rectangle.
    pub fn fill_rect(&mut self, bounds: Bounds<Pixels>, fill: impl Into<Background>) {
        self.fill_rounded_rect(bounds, px(0.), fill);
    }

    /// Fill a rounded rectangle.
    pub fn fill_rounded_rect(
        &mut self,
        bounds: Bounds<Pixels>,
        radii: impl Into<Corners<Pixels>>,
        fill: impl Into<Background>,
    ) {
        self.commands.push(DrawCommand::Quad {
            quad: quad(
                bounds,
                radii,
                fill,
                px(0.),
                transparent_black(),
                crate::BorderStyle::Solid,
            ),
            state: self.current_state.clone(),
        });
    }

    /// Fill a circle.
    pub fn fill_circle(
        &mut self,
        center: Point<Pixels>,
        radius: Pixels,
        fill: impl Into<Background>,
    ) {
        if let Some(path) = build_circle_path(PathBuilder::fill(), center, radius) {
            self.commands.push(DrawCommand::Path {
                path,
                fill: fill.into(),
                state: self.current_state.clone(),
            });
        }
    }

    /// Stroke a circle.
    pub fn stroke_circle(&mut self, center: Point<Pixels>, radius: Pixels, stroke: Stroke) {
        if let Some(path) = build_circle_path(configure_stroke_builder(&stroke), center, radius) {
            self.commands.push(DrawCommand::Path {
                path,
                fill: stroke.color.into(),
                state: self.current_state.clone(),
            });
        }
    }

    /// Apply an affine transform to commands issued within the callback.
    pub fn with_transform<R>(
        &mut self,
        matrix: TransformationMatrix,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.current_state.clone();
        self.current_state.transform = self.current_state.transform.compose(matrix);
        let result = f(self);
        self.current_state = previous;
        result
    }

    /// Multiply the current drawing opacity for commands issued within the callback.
    pub fn with_opacity<R>(&mut self, opacity: f32, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.current_state.clone();
        self.current_state.opacity *= opacity.clamp(0.0, 1.0);
        let result = f(self);
        self.current_state = previous;
        result
    }

    /// Restrict drawing to an additional clip rectangle.
    pub fn with_clip<R>(&mut self, bounds: Bounds<Pixels>, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.current_state.clone();
        let clip_bounds = transform_bounds(
            bounds,
            full_path_transform(self.canvas_origin, self.current_state.transform),
        );
        self.current_state.content_mask = self.current_state.content_mask.intersect(&ContentMask {
            bounds: clip_bounds,
        });
        let result = f(self);
        self.current_state = previous;
        result
    }

    /// Save the current drawing state (transform, opacity, clip) onto the state stack.
    ///
    /// Mirrors `CanvasRenderingContext2D.save()`. Pair every `save` with a matching
    /// [`DrawContext::restore`]. This is the flat alternative to the scoped
    /// [`DrawContext::with_transform`] / [`DrawContext::with_opacity`] / [`DrawContext::with_clip`]
    /// helpers, for loops and recursion where closures are awkward.
    pub fn save(&mut self) {
        self.state_stack.push(self.current_state.clone());
    }

    /// Restore the most recently saved drawing state. No-op if the stack is empty.
    ///
    /// Mirrors `CanvasRenderingContext2D.restore()`.
    pub fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.current_state = state;
        }
    }

    /// Translate the current transform by the given offset, in canvas-local pixels.
    pub fn translate(&mut self, x: Pixels, y: Pixels) {
        self.current_state.transform = self
            .current_state
            .transform
            .compose(translation_matrix(point(x, y)));
    }

    /// Rotate the current transform clockwise by the given angle in radians.
    pub fn rotate(&mut self, radians: f32) {
        self.current_state.transform = self.current_state.transform.rotate(Radians(radians));
    }

    /// Scale the current transform by independent x and y factors.
    pub fn scale(&mut self, x: f32, y: f32) {
        self.current_state.transform = self.current_state.transform.scale(size(x, y));
    }

    /// Set the global drawing opacity applied to subsequent commands (clamped to `0.0..=1.0`).
    ///
    /// Mirrors `CanvasRenderingContext2D.globalAlpha`. Saved and restored by
    /// [`DrawContext::save`] / [`DrawContext::restore`].
    pub fn set_global_alpha(&mut self, alpha: f32) {
        self.current_state.opacity = alpha.clamp(0.0, 1.0);
    }

    /// Restrict subsequent drawing to the given rectangle, intersected with any
    /// existing clip. The flat counterpart to [`DrawContext::with_clip`].
    pub fn clip_rect(&mut self, bounds: Bounds<Pixels>) {
        let clip_bounds = transform_bounds(
            bounds,
            full_path_transform(self.canvas_origin, self.current_state.transform),
        );
        self.current_state.content_mask = self.current_state.content_mask.intersect(&ContentMask {
            bounds: clip_bounds,
        });
    }

    /// Draw a shaped line of text at the given local origin.
    pub fn draw_text(&mut self, text: &ShapedLine, origin: Point<Pixels>, color: Hsla) {
        self.commands.push(DrawCommand::Text {
            text: text.clone(),
            origin,
            color,
            state: self.current_state.clone(),
        });
    }

    /// Draw an image filling the given rectangle, respecting the current transform,
    /// clip, and opacity. Mirrors `CanvasRenderingContext2D.drawImage`.
    pub fn draw_image(&mut self, image: Arc<RenderImage>, bounds: Bounds<Pixels>) {
        self.draw_image_rounded(image, bounds, Corners::default());
    }

    /// Draw an image filling the given rectangle with rounded corners.
    pub fn draw_image_rounded(
        &mut self,
        image: Arc<RenderImage>,
        bounds: Bounds<Pixels>,
        corner_radii: impl Into<Corners<Pixels>>,
    ) {
        self.commands.push(DrawCommand::Image {
            data: image,
            bounds,
            corner_radii: corner_radii.into(),
            frame_index: 0,
            grayscale: false,
            state: self.current_state.clone(),
        });
    }

    /// Draw a single frame of a multi-frame image (e.g. an animated GIF), optionally desaturated.
    pub fn draw_image_frame(
        &mut self,
        image: Arc<RenderImage>,
        bounds: Bounds<Pixels>,
        frame_index: usize,
        grayscale: bool,
    ) {
        self.commands.push(DrawCommand::Image {
            data: image,
            bounds,
            corner_radii: Corners::default(),
            frame_index,
            grayscale,
            state: self.current_state.clone(),
        });
    }

    /// Flush any queued draw commands into the window.
    ///
    /// Call this when mixing `DrawContext` drawing with direct `Window` painting and you need
    /// exact interleaving order.
    pub fn flush(&mut self, window: &mut Window, _cx: &mut App) {
        if self.commands.is_empty() {
            return;
        }

        let commands = std::mem::take(&mut self.commands);
        for command in commands {
            self.replay(command, window);
        }
    }

    fn replay(&self, command: DrawCommand, window: &mut Window) {
        match command {
            DrawCommand::Path { path, fill, state } => {
                let path =
                    path.transformed(full_path_transform(self.canvas_origin, state.transform));
                window.with_content_mask(Some(state.content_mask), |window| {
                    window.with_element_opacity(Some(state.opacity), |window| {
                        window.paint_path(path, fill);
                    })
                });
            }
            DrawCommand::Quad { mut quad, state } => {
                quad.bounds = offset_bounds(quad.bounds, self.canvas_origin);
                quad.transform = resolve_quad_transform(self.canvas_origin, state.transform)
                    .compose(quad.transform);
                window.with_content_mask(Some(state.content_mask), |window| {
                    window.with_element_opacity(Some(state.opacity), |window| {
                        window.paint_quad(quad);
                    })
                });
            }
            DrawCommand::Text {
                text,
                origin,
                color,
                state,
            } => {
                window.with_content_mask(Some(state.content_mask), |window| {
                    window.with_element_opacity(Some(state.opacity), |window| {
                        paint_text_line(
                            window,
                            &text,
                            origin,
                            color,
                            self.canvas_origin,
                            state.transform,
                        );
                    })
                });
            }
            DrawCommand::Image {
                data,
                bounds,
                corner_radii,
                frame_index,
                grayscale,
                state,
            } => {
                let local_bounds = transform_bounds(bounds, state.transform);
                let window_bounds = offset_bounds(local_bounds, self.canvas_origin);
                window.with_content_mask(Some(state.content_mask), |window| {
                    window.with_element_opacity(Some(state.opacity), |window| {
                        let _ = window.paint_image(
                            window_bounds,
                            corner_radii,
                            data,
                            frame_index,
                            grayscale,
                        );
                    })
                });
            }
        }
    }
}

/// A canvas element backed by an immediate-mode [`DrawContext`].
pub struct CanvasDraw {
    draw: Option<Box<dyn for<'a, 'b, 'c> FnOnce(&'a mut DrawContext, &'b mut Window, &'c mut App)>>,
    style: StyleRefinement,
}

impl CanvasDraw {
    fn new(
        size: Size<Pixels>,
        draw: impl 'static + for<'a, 'b, 'c> FnOnce(&'a mut DrawContext, &'b mut Window, &'c mut App),
    ) -> Self {
        Self {
            draw: Some(Box::new(draw)),
            style: StyleRefinement::default(),
        }
        .w(size.width)
        .h(size.height)
    }
}

impl<FDraw> CanvasConstructor<FDraw> for Size<Pixels>
where
    FDraw: 'static + for<'a, 'b, 'c> FnOnce(&'a mut DrawContext, &'b mut Window, &'c mut App),
{
    type Output = CanvasDraw;

    fn into_canvas(self, draw: FDraw) -> Self::Output {
        CanvasDraw::new(self, draw)
    }
}

impl IntoElement for CanvasDraw {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CanvasDraw {
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
        style.paint(bounds, window, cx, |window, cx| {
            let mut draw_context = DrawContext::new(bounds, window.content_mask());
            (self.draw.take().unwrap())(&mut draw_context, window, cx);
            draw_context.flush(window, cx);
        });
    }
}

impl Styled for CanvasDraw {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn build_circle_path(
    mut builder: PathBuilder,
    center: Point<Pixels>,
    radius: Pixels,
) -> Option<Path<Pixels>> {
    if radius <= px(0.) {
        return None;
    }

    let right = point(center.x + radius, center.y);
    let left = point(center.x - radius, center.y);
    builder.move_to(right);
    builder.arc_to(point(radius, radius), px(0.), true, true, left);
    builder.arc_to(point(radius, radius), px(0.), true, true, right);
    builder.close();
    builder.build().ok()
}

fn configure_stroke_builder(stroke: &Stroke) -> PathBuilder {
    let mut builder = PathBuilder::fill().with_style(PathStyle::Stroke(stroke_options(stroke)));
    if let Some(dash) = &stroke.dash {
        builder = builder.dash_array(&dash.segments).dash_offset(dash.offset);
    }
    builder
}

fn build_stroked_path(
    stroke: &Stroke,
    draw: impl FnOnce(&mut PathBuilder),
) -> Option<Path<Pixels>> {
    if stroke.width <= px(0.) {
        return None;
    }

    let mut builder = configure_stroke_builder(stroke);
    draw(&mut builder);
    builder.build().ok()
}

fn stroke_existing_path(path: &Path<Pixels>, stroke: &Stroke) -> Option<Path<Pixels>> {
    if stroke.width <= px(0.) {
        return None;
    }

    let source_path = path.source_path()?;
    let dash_array = stroke.dash.as_ref().map(|dash| dash.segments.clone());
    let dash_offset = stroke.dash.as_ref().map_or(px(0.), |dash| dash.offset);
    PathBuilder::stroke_source_path(source_path, stroke_options(stroke), dash_array, dash_offset)
        .ok()
}

fn stroke_options(stroke: &Stroke) -> StrokeOptions {
    StrokeOptions::default()
        .with_line_width(stroke.width.0)
        .with_line_cap(stroke.cap)
        .with_line_join(stroke.join)
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

fn paint_text_line(
    window: &mut Window,
    text: &ShapedLine,
    origin: Point<Pixels>,
    color: Hsla,
    canvas_origin: Point<Pixels>,
    transform: TransformationMatrix,
) {
    let baseline_offset = point(px(0.), text.layout.ascent);
    let mut glyph_origin = origin;
    let mut previous_glyph_position = Point::default();
    let absolute_transform = resolve_quad_transform(canvas_origin, transform);
    let absolute_point_transform = full_path_transform(canvas_origin, transform);
    let translation_only = transform.rotation_scale == [[1.0, 0.0], [0.0, 1.0]];

    for run in &text.layout.runs {
        for glyph in &run.glyphs {
            glyph_origin.x += glyph.position.x - previous_glyph_position.x;
            previous_glyph_position = glyph.position;

            let glyph_origin = glyph_origin + baseline_offset;
            if glyph.is_emoji {
                let glyph_origin = if translation_only {
                    absolute_point_transform.apply(glyph_origin)
                } else {
                    absolute_point_transform.apply(glyph_origin)
                };
                let _ =
                    window.paint_emoji(glyph_origin, run.font_id, glyph.id, text.layout.font_size);
            } else {
                let _ = window.paint_glyph_with_transformation(
                    glyph_origin + canvas_origin,
                    run.font_id,
                    glyph.id,
                    text.layout.font_size,
                    color,
                    absolute_transform,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{stroke, transform_bounds, StrokeDash};
    use crate::{point, px, size, Bounds, PathBuilder};

    #[test]
    fn save_restore_round_trips_transform_and_alpha() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.)));
        let mask = crate::ContentMask { bounds };
        let mut cx = super::DrawContext::new(bounds, mask);

        cx.translate(px(10.), px(20.));
        cx.set_global_alpha(0.5);
        cx.save();
        cx.translate(px(5.), px(5.));
        cx.set_global_alpha(0.25);
        assert_eq!(cx.current_state.transform.translation, [15.0, 25.0]);
        assert_eq!(cx.current_state.opacity, 0.25);

        cx.restore();
        assert_eq!(cx.current_state.transform.translation, [10.0, 20.0]);
        assert_eq!(cx.current_state.opacity, 0.5);

        cx.restore();
        assert_eq!(cx.current_state.transform.translation, [10.0, 20.0]);
    }

    #[test]
    fn draw_image_records_command_with_state() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(10.), px(10.)));
        let mask = crate::ContentMask { bounds };
        let mut cx = super::DrawContext::new(bounds, mask);

        let buffer = image::ImageBuffer::from_pixel(1, 1, image::Rgba([10u8, 20, 30, 255]));
        let img = std::sync::Arc::new(crate::RenderImage::new(vec![image::Frame::new(buffer)]));

        cx.translate(px(4.), px(6.));
        cx.draw_image(
            img,
            Bounds::new(point(px(0.), px(0.)), size(px(8.), px(8.))),
        );

        assert_eq!(cx.commands.len(), 1);
        match &cx.commands[0] {
            super::DrawCommand::Image {
                state, bounds: b, ..
            } => {
                assert_eq!(state.transform.translation, [4.0, 6.0]);
                assert_eq!(b.size.width.0, 8.0);
            }
            _ => panic!("expected an image command"),
        }
    }

    #[test]
    fn transformed_bounds_cover_all_corners() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(10.), px(20.)));
        let rotated =
            crate::TransformationMatrix::unit().rotate(crate::Radians(std::f32::consts::FRAC_PI_2));
        let transformed = transform_bounds(bounds, rotated);
        assert!(transformed.size.width > px(0.));
        assert!(transformed.size.height > px(0.));
    }

    #[test]
    fn stroke_path_reuses_retained_outline() {
        let mut builder = PathBuilder::fill();
        builder.move_to(point(px(0.), px(0.)));
        builder.line_to(point(px(20.), px(0.)));
        builder.line_to(point(px(20.), px(20.)));
        builder.close();
        let path = builder.build().expect("path should build");
        let mut stroke = stroke(px(2.), crate::black());
        stroke.dash = Some(StrokeDash {
            segments: vec![px(4.), px(2.)],
            offset: px(1.),
        });

        let stroked =
            super::stroke_existing_path(&path, &stroke).expect("stroke should tessellate");
        assert!(!stroked.vertices.is_empty());
    }
}
