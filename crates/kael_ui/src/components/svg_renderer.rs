//! Parses SVG path data and renders via Kael PathBuilder.

use kael::{prelude::FluentBuilder as _, *};
use svgtypes::{PathParser, PathSegment};

use crate::theme::Theme;

#[derive(Clone, Debug, PartialEq)]
enum SvgCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    QuadTo(f32, f32, f32, f32),
    ArcTo {
        rx: f32,
        ry: f32,
        rotation: f32,
        large_arc: bool,
        sweep: bool,
        x: f32,
        y: f32,
    },
    Close,
}

fn parse_svg_path(data: &str) -> Vec<SvgCommand> {
    let mut commands = Vec::new();
    let mut current = (0.0_f32, 0.0_f32);
    let mut subpath_start = current;
    let mut cubic_control = None;
    let mut quadratic_control = None;

    let value = |number: f64| {
        let number = number as f32;
        number.is_finite().then_some(number)
    };
    let absolute = |abs: bool, x: f32, y: f32, current: (f32, f32)| {
        if abs {
            (x, y)
        } else {
            (current.0 + x, current.1 + y)
        }
    };

    for segment in PathParser::from(data) {
        let Ok(segment) = segment else {
            return Vec::new();
        };
        match segment {
            PathSegment::MoveTo { abs, x, y } => {
                let (Some(x), Some(y)) = (value(x), value(y)) else {
                    return Vec::new();
                };
                current = absolute(abs, x, y, current);
                subpath_start = current;
                commands.push(SvgCommand::MoveTo(current.0, current.1));
                cubic_control = None;
                quadratic_control = None;
            }
            PathSegment::LineTo { abs, x, y } => {
                let (Some(x), Some(y)) = (value(x), value(y)) else {
                    return Vec::new();
                };
                current = absolute(abs, x, y, current);
                commands.push(SvgCommand::LineTo(current.0, current.1));
                cubic_control = None;
                quadratic_control = None;
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                let Some(x) = value(x) else {
                    return Vec::new();
                };
                current.0 = if abs { x } else { current.0 + x };
                commands.push(SvgCommand::LineTo(current.0, current.1));
                cubic_control = None;
                quadratic_control = None;
            }
            PathSegment::VerticalLineTo { abs, y } => {
                let Some(y) = value(y) else {
                    return Vec::new();
                };
                current.1 = if abs { y } else { current.1 + y };
                commands.push(SvgCommand::LineTo(current.0, current.1));
                cubic_control = None;
                quadratic_control = None;
            }
            PathSegment::CurveTo {
                abs,
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x), Some(y)) = (
                    value(x1),
                    value(y1),
                    value(x2),
                    value(y2),
                    value(x),
                    value(y),
                ) else {
                    return Vec::new();
                };
                let control_a = absolute(abs, x1, y1, current);
                let control_b = absolute(abs, x2, y2, current);
                let end = absolute(abs, x, y, current);
                commands.push(SvgCommand::CurveTo(
                    control_a.0,
                    control_a.1,
                    control_b.0,
                    control_b.1,
                    end.0,
                    end.1,
                ));
                current = end;
                cubic_control = Some(control_b);
                quadratic_control = None;
            }
            PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                let (Some(x2), Some(y2), Some(x), Some(y)) =
                    (value(x2), value(y2), value(x), value(y))
                else {
                    return Vec::new();
                };
                let control_a = cubic_control
                    .map(|control| (2.0 * current.0 - control.0, 2.0 * current.1 - control.1))
                    .unwrap_or(current);
                let control_b = absolute(abs, x2, y2, current);
                let end = absolute(abs, x, y, current);
                commands.push(SvgCommand::CurveTo(
                    control_a.0,
                    control_a.1,
                    control_b.0,
                    control_b.1,
                    end.0,
                    end.1,
                ));
                current = end;
                cubic_control = Some(control_b);
                quadratic_control = None;
            }
            PathSegment::Quadratic { abs, x1, y1, x, y } => {
                let (Some(x1), Some(y1), Some(x), Some(y)) =
                    (value(x1), value(y1), value(x), value(y))
                else {
                    return Vec::new();
                };
                let control = absolute(abs, x1, y1, current);
                let end = absolute(abs, x, y, current);
                commands.push(SvgCommand::QuadTo(control.0, control.1, end.0, end.1));
                current = end;
                cubic_control = None;
                quadratic_control = Some(control);
            }
            PathSegment::SmoothQuadratic { abs, x, y } => {
                let (Some(x), Some(y)) = (value(x), value(y)) else {
                    return Vec::new();
                };
                let control = quadratic_control
                    .map(|control| (2.0 * current.0 - control.0, 2.0 * current.1 - control.1))
                    .unwrap_or(current);
                let end = absolute(abs, x, y, current);
                commands.push(SvgCommand::QuadTo(control.0, control.1, end.0, end.1));
                current = end;
                cubic_control = None;
                quadratic_control = Some(control);
            }
            PathSegment::EllipticalArc {
                abs,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
            } => {
                let (Some(rx), Some(ry), Some(rotation), Some(x), Some(y)) = (
                    value(rx),
                    value(ry),
                    value(x_axis_rotation),
                    value(x),
                    value(y),
                ) else {
                    return Vec::new();
                };
                let end = absolute(abs, x, y, current);
                commands.push(SvgCommand::ArcTo {
                    rx: rx.abs(),
                    ry: ry.abs(),
                    rotation,
                    large_arc,
                    sweep,
                    x: end.0,
                    y: end.1,
                });
                current = end;
                cubic_control = None;
                quadratic_control = None;
            }
            PathSegment::ClosePath { .. } => {
                commands.push(SvgCommand::Close);
                current = subpath_start;
                cubic_control = None;
                quadratic_control = None;
            }
        }
    }

    commands
}

#[derive(Clone)]
struct SvgPaintData {
    commands: Vec<SvgCommand>,
    view_box: Bounds<f32>,
    fill_color: Option<Hsla>,
    stroke_color: Option<Hsla>,
    stroke_width: f32,
}

#[derive(IntoElement)]
pub struct SVGRenderer {
    path_data: SharedString,
    view_box: Bounds<f32>,
    fill_color: Option<Hsla>,
    fill_enabled: bool,
    stroke_color: Option<Hsla>,
    stroke_width: f32,
    style: StyleRefinement,
}

impl Default for SVGRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl SVGRenderer {
    pub fn new() -> Self {
        Self {
            path_data: SharedString::default(),
            view_box: Bounds::new(point(0.0_f32, 0.0_f32), size(100.0_f32, 100.0_f32)),
            fill_color: None,
            fill_enabled: true,
            stroke_color: None,
            stroke_width: 1.0,
            style: StyleRefinement::default(),
        }
    }

    pub fn path_data(mut self, data: impl Into<SharedString>) -> Self {
        self.path_data = data.into();
        self
    }

    pub fn view_box(mut self, x: f32, y: f32, w: f32, h: f32) -> Self {
        if [x, y, w, h].into_iter().all(f32::is_finite) && w > 0.0 && h > 0.0 {
            self.view_box = Bounds::new(point(x, y), size(w, h));
        }
        self
    }

    pub fn fill(mut self, color: Hsla) -> Self {
        self.fill_color = Some(color);
        self.fill_enabled = true;
        self
    }

    /// Renders only the stroke, matching SVG paths declared with `fill="none"`.
    pub fn no_fill(mut self) -> Self {
        self.fill_enabled = false;
        self
    }

    pub fn stroke(mut self, color: Hsla) -> Self {
        self.stroke_color = Some(color);
        self
    }

    pub fn stroke_width(mut self, width: f32) -> Self {
        if width.is_finite() && width >= 0.0 {
            self.stroke_width = width;
        }
        self
    }
}

impl Styled for SVGRenderer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn transform_point(
    vx: f32,
    vy: f32,
    view_box: &Bounds<f32>,
    bounds: &Bounds<Pixels>,
) -> Point<Pixels> {
    let scale_x = bounds.size.width / px(1.0) / view_box.size.width;
    let scale_y = bounds.size.height / px(1.0) / view_box.size.height;
    let scale = scale_x.min(scale_y);

    let offset_x = (bounds.size.width / px(1.0) - view_box.size.width * scale) * 0.5;
    let offset_y = (bounds.size.height / px(1.0) - view_box.size.height * scale) * 0.5;

    point(
        bounds.left() + px(offset_x + (vx - view_box.origin.x) * scale),
        bounds.top() + px(offset_y + (vy - view_box.origin.y) * scale),
    )
}

impl RenderOnce for SVGRenderer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let foreground = Theme::of(cx).tokens.foreground;
        let user_style = self.style;

        let fill_color = self
            .fill_enabled
            .then(|| self.fill_color.unwrap_or(foreground));
        let commands = parse_svg_path(&self.path_data);

        let paint_data = SvgPaintData {
            commands,
            view_box: self.view_box,
            fill_color,
            stroke_color: self.stroke_color,
            stroke_width: self.stroke_width,
        };

        div()
            .size_full()
            .child(
                canvas_with_prepaint(
                    move |_, _, _| paint_data,
                    move |bounds, data, window, _cx| {
                        if data.commands.is_empty() {
                            return;
                        }

                        if let Some(stroke_color) = data.stroke_color {
                            let mut builder = PathBuilder::stroke(px(data.stroke_width));
                            let mut started = false;

                            for cmd in &data.commands {
                                match cmd {
                                    SvgCommand::MoveTo(x, y) => {
                                        let pt = transform_point(*x, *y, &data.view_box, &bounds);
                                        if started {
                                            if let Ok(path) = builder.build() {
                                                window.paint_path(path, stroke_color);
                                            }
                                            builder = PathBuilder::stroke(px(data.stroke_width));
                                        }
                                        builder.move_to(pt);
                                        started = true;
                                    }
                                    SvgCommand::LineTo(x, y) => {
                                        let pt = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.line_to(pt);
                                    }
                                    SvgCommand::CurveTo(x1, y1, x2, y2, x, y) => {
                                        let cp1 =
                                            transform_point(*x1, *y1, &data.view_box, &bounds);
                                        let cp2 =
                                            transform_point(*x2, *y2, &data.view_box, &bounds);
                                        let end = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.cubic_bezier_to(end, cp1, cp2);
                                    }
                                    SvgCommand::QuadTo(cx1, cy1, x, y) => {
                                        let cp =
                                            transform_point(*cx1, *cy1, &data.view_box, &bounds);
                                        let end = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.curve_to(end, cp);
                                    }
                                    SvgCommand::ArcTo {
                                        rx,
                                        ry,
                                        rotation,
                                        large_arc,
                                        sweep,
                                        x,
                                        y,
                                    } => {
                                        let scale_x =
                                            bounds.size.width / px(1.0) / data.view_box.size.width;
                                        let scale_y = bounds.size.height
                                            / px(1.0)
                                            / data.view_box.size.height;
                                        let scale = scale_x.min(scale_y);
                                        builder.arc_to(
                                            point(px(*rx * scale), px(*ry * scale)),
                                            px(*rotation),
                                            *large_arc,
                                            *sweep,
                                            transform_point(*x, *y, &data.view_box, &bounds),
                                        );
                                    }
                                    SvgCommand::Close => {
                                        builder.close();
                                    }
                                }
                            }

                            if started && let Ok(path) = builder.build() {
                                window.paint_path(path, stroke_color);
                            }
                        }

                        if let Some(fill_color) = data.fill_color {
                            let mut builder = PathBuilder::fill();
                            let mut started = false;

                            for cmd in &data.commands {
                                match cmd {
                                    SvgCommand::MoveTo(x, y) => {
                                        if started {
                                            builder.close();
                                            if let Ok(path) = builder.build() {
                                                window.paint_path(path, fill_color);
                                            }
                                            builder = PathBuilder::fill();
                                        }
                                        let pt = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.move_to(pt);
                                        started = true;
                                    }
                                    SvgCommand::LineTo(x, y) => {
                                        let pt = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.line_to(pt);
                                    }
                                    SvgCommand::CurveTo(x1, y1, x2, y2, x, y) => {
                                        let cp1 =
                                            transform_point(*x1, *y1, &data.view_box, &bounds);
                                        let cp2 =
                                            transform_point(*x2, *y2, &data.view_box, &bounds);
                                        let end = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.cubic_bezier_to(end, cp1, cp2);
                                    }
                                    SvgCommand::QuadTo(cx1, cy1, x, y) => {
                                        let cp =
                                            transform_point(*cx1, *cy1, &data.view_box, &bounds);
                                        let end = transform_point(*x, *y, &data.view_box, &bounds);
                                        builder.curve_to(end, cp);
                                    }
                                    SvgCommand::ArcTo {
                                        rx,
                                        ry,
                                        rotation,
                                        large_arc,
                                        sweep,
                                        x,
                                        y,
                                    } => {
                                        let scale_x =
                                            bounds.size.width / px(1.0) / data.view_box.size.width;
                                        let scale_y = bounds.size.height
                                            / px(1.0)
                                            / data.view_box.size.height;
                                        let scale = scale_x.min(scale_y);
                                        builder.arc_to(
                                            point(px(*rx * scale), px(*ry * scale)),
                                            px(*rotation),
                                            *large_arc,
                                            *sweep,
                                            transform_point(*x, *y, &data.view_box, &bounds),
                                        );
                                    }
                                    SvgCommand::Close => {
                                        builder.close();
                                    }
                                }
                            }

                            if started {
                                builder.close();
                                if let Ok(path) = builder.build() {
                                    window.paint_path(path, fill_color);
                                }
                            }
                        }
                    },
                )
                .size_full(),
            )
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn parser_preserves_cubic_and_quadratic_control_points() {
        assert_eq!(
            parse_svg_path("M 0 0 C 10 20 30 40 50 60 Q 70 80 90 100 Z"),
            vec![
                SvgCommand::MoveTo(0.0, 0.0),
                SvgCommand::CurveTo(10.0, 20.0, 30.0, 40.0, 50.0, 60.0),
                SvgCommand::QuadTo(70.0, 80.0, 90.0, 100.0),
                SvgCommand::Close,
            ]
        );
    }

    #[::core::prelude::v1::test]
    fn invalid_geometry_keeps_renderable_defaults() {
        let renderer = SVGRenderer::new()
            .view_box(0.0, 0.0, 0.0, f32::NAN)
            .stroke_width(f32::INFINITY);
        assert_eq!(renderer.view_box.size, size(100.0, 100.0));
        assert_eq!(renderer.stroke_width, 1.0);
    }

    #[::core::prelude::v1::test]
    fn parser_normalizes_relative_smooth_arc_and_exponent_commands() {
        let commands = parse_svg_path(
            "M1e1 10 l5-2 h3 v4 c1 2 3 4 5 6 s7 8 9 10 q2 3 4 5 t6 7 a2 3 45 0 1 8 9 z",
        );

        assert_eq!(commands[0], SvgCommand::MoveTo(10.0, 10.0));
        assert_eq!(commands[1], SvgCommand::LineTo(15.0, 8.0));
        assert_eq!(commands[2], SvgCommand::LineTo(18.0, 8.0));
        assert_eq!(commands[3], SvgCommand::LineTo(18.0, 12.0));
        assert!(matches!(
            commands[8],
            SvgCommand::ArcTo {
                rx: 2.0,
                ry: 3.0,
                rotation: 45.0,
                large_arc: false,
                sweep: true,
                x: 50.0,
                y: 49.0,
            }
        ));
        assert_eq!(commands.last(), Some(&SvgCommand::Close));
    }

    #[::core::prelude::v1::test]
    fn renderer_can_explicitly_disable_fill() {
        let renderer = SVGRenderer::new().fill(white()).no_fill();
        assert!(!renderer.fill_enabled);
    }
}
