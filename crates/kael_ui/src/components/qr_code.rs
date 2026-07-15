//! QR code generator rendering as colored quads.

use kael::{prelude::FluentBuilder as _, *};
use qrcode::types::EcLevel;
use qrcode::QrCode;

use crate::theme::Theme;

const QUIET_ZONE_MODULES: usize = 4;

#[derive(Clone)]
struct QRPaintData {
    modules: Vec<Vec<bool>>,
    fg_color: Hsla,
    bg_color: Hsla,
}

#[derive(IntoElement)]
pub struct QRCodeComponent {
    data: SharedString,
    label: SharedString,
    size: Pixels,
    fg_color: Option<Hsla>,
    bg_color: Option<Hsla>,
    error_correction: EcLevel,
    style: StyleRefinement,
}

impl QRCodeComponent {
    pub fn new(data: impl Into<SharedString>) -> Self {
        Self {
            data: data.into(),
            label: "QR code".into(),
            size: px(200.0),
            fg_color: None,
            bg_color: None,
            error_correction: EcLevel::M,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        let value = size / px(1.0);
        self.size = px(if value.is_finite() {
            value.max(1.0)
        } else {
            200.0
        });
        self
    }

    /// Set meaningful alternative text for the encoded destination or action.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        if !label.trim().is_empty() {
            self.label = label;
        }
        self
    }

    pub fn fg_color(mut self, color: Hsla) -> Self {
        self.fg_color = Some(color);
        self
    }

    pub fn bg_color(mut self, color: Hsla) -> Self {
        self.bg_color = Some(color);
        self
    }

    pub fn error_correction(mut self, level: EcLevel) -> Self {
        self.error_correction = level;
        self
    }
}

impl Styled for QRCodeComponent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn generate_modules(data: &str, ec_level: EcLevel) -> Option<Vec<Vec<bool>>> {
    QrCode::with_error_correction_level(data, ec_level)
        .ok()
        .map(|code| {
            let width = code.width();
            let colors = code.into_colors();
            let mut grid = Vec::with_capacity(width);
            for row in 0..width {
                let mut row_vec = Vec::with_capacity(width);
                for col in 0..width {
                    let idx = row * width + col;
                    row_vec.push(colors[idx] == qrcode::Color::Dark);
                }
                grid.push(row_vec);
            }
            grid
        })
}

fn module_layout(width: f32, height: f32, module_count: usize) -> Option<(f32, f32, f32)> {
    let total_modules = module_count.checked_add(QUIET_ZONE_MODULES * 2)?;
    let module_size = (width / total_modules as f32)
        .min(height / total_modules as f32)
        .floor();
    if !module_size.is_finite() || module_size < 1.0 {
        return None;
    }

    let painted_size = module_size * total_modules as f32;
    Some((
        module_size,
        (width - painted_size) * 0.5,
        (height - painted_size) * 0.5,
    ))
}

impl RenderOnce for QRCodeComponent {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let foreground = tokens.foreground;
        let background = tokens.background;
        let user_style = self.style;

        let fg = self.fg_color.unwrap_or(foreground);
        let bg = self.bg_color.unwrap_or(background);

        let modules = generate_modules(&self.data, self.error_correction);
        let has_error = modules.is_none();
        let qr_size = self.size;
        let accessibility = if has_error {
            AccessibilityAttributes::new(AccessibilityRole::Alert)
                .label("Unable to generate QR code")
        } else {
            AccessibilityAttributes::new(AccessibilityRole::Image).label(self.label.to_string())
        };

        div()
            .w(qr_size)
            .h(qr_size)
            .accessibility(accessibility)
            .when_some(modules, |this, modules| {
                let paint_data = QRPaintData {
                    modules,
                    fg_color: fg,
                    bg_color: bg,
                };
                this.child(
                    canvas_with_prepaint(
                        move |_, _, _| paint_data,
                        move |bounds, data, window, _cx| {
                            window.paint_quad(fill(bounds, data.bg_color));

                            let module_count = data.modules.len();
                            let width = bounds.size.width / px(1.0);
                            let height = bounds.size.height / px(1.0);
                            let Some((module_size, offset_x, offset_y)) =
                                module_layout(width, height, module_count)
                            else {
                                return;
                            };
                            let quiet_offset = QUIET_ZONE_MODULES as f32 * module_size;

                            for (row_idx, row) in data.modules.iter().enumerate() {
                                for (col_idx, &is_dark) in row.iter().enumerate() {
                                    if is_dark {
                                        let x = bounds.left()
                                            + px(offset_x
                                                + quiet_offset
                                                + col_idx as f32 * module_size);
                                        let y = bounds.top()
                                            + px(offset_y
                                                + quiet_offset
                                                + row_idx as f32 * module_size);
                                        let cell_bounds = Bounds::new(
                                            point(x, y),
                                            size(px(module_size), px(module_size)),
                                        );
                                        window.paint_quad(fill(cell_bounds, data.fg_color));
                                    }
                                }
                            }
                        },
                    )
                    .size_full(),
                )
            })
            .when(has_error, |this| {
                this.flex()
                    .items_center()
                    .justify_center()
                    .p(px(12.0))
                    .bg(bg)
                    .text_center()
                    .text_size(px(12.0))
                    .text_color(tokens.destructive)
                    .child("Unable to generate QR code")
            })
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{generate_modules, module_layout, QRCodeComponent};
    use kael::px;
    use qrcode::types::EcLevel;

    #[test]
    fn generated_grid_matches_qr_width() {
        let modules = generate_modules("https://kael.dev", EcLevel::M).unwrap();
        assert!(!modules.is_empty());
        assert!(modules.iter().all(|row| row.len() == modules.len()));
    }

    #[test]
    fn oversized_payload_reports_failure() {
        assert!(generate_modules(&"x".repeat(10_000), EcLevel::H).is_none());
    }

    #[test]
    fn layout_reserves_four_module_quiet_zone() {
        let (module_size, offset_x, offset_y) = module_layout(290.0, 300.0, 21).unwrap();
        assert_eq!(module_size, 10.0);
        assert_eq!(offset_x, 0.0);
        assert_eq!(offset_y, 5.0);
    }

    #[test]
    fn invalid_size_and_empty_label_keep_safe_defaults() {
        let qr = QRCodeComponent::new("kael://launch")
            .size(px(f32::NAN))
            .label("   ");
        assert_eq!(qr.size, px(200.0));
        assert_eq!(qr.label.as_ref(), "QR code");
    }
}
