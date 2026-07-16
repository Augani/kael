//! macOS Dock-style magnification row.

use kael::{prelude::FluentBuilder as _, *};

use crate::theme::Theme;

pub struct DockState {
    hovered_index: Option<usize>,
}

impl DockState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            hovered_index: None,
        }
    }
}

impl Render for DockState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[allow(dead_code)]
struct DockItemInfo {
    center_x: f32,
    scale: f32,
}

fn compute_scales(
    item_count: usize,
    item_size_f: f32,
    gap: f32,
    cursor_rel_x: Option<f32>,
    max_scale: f32,
    sigma: f32,
) -> Vec<DockItemInfo> {
    let mut items = Vec::with_capacity(item_count);
    let total_width = item_count as f32 * item_size_f + (item_count.saturating_sub(1)) as f32 * gap;
    let start_x = -total_width * 0.5;

    for i in 0..item_count {
        let center = start_x + i as f32 * (item_size_f + gap) + item_size_f * 0.5;
        let scale = if let Some(cx_pos) = cursor_rel_x {
            let dist = (center - cx_pos).abs();
            1.0 + max_scale * (-dist * dist / (2.0 * sigma * sigma)).exp()
        } else {
            1.0
        };
        items.push(DockItemInfo {
            center_x: center,
            scale,
        });
    }
    items
}

#[derive(IntoElement)]
pub struct Dock {
    id: ElementId,
    state: Entity<DockState>,
    max_scale: f32,
    item_size: Pixels,
    gap: Pixels,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl Dock {
    pub fn new(id: impl Into<ElementId>, state: Entity<DockState>) -> Self {
        Self {
            id: id.into(),
            state,
            max_scale: 0.5,
            item_size: px(48.0),
            gap: px(4.0),
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }

    pub fn max_scale(mut self, scale: f32) -> Self {
        self.max_scale = if scale.is_finite() {
            scale.clamp(0.0, 2.0)
        } else {
            0.5
        };
        self
    }

    pub fn item_size(mut self, size: Pixels) -> Self {
        if f32::from(size).is_finite() {
            self.item_size = size.max(px(1.0));
        }
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        if f32::from(gap).is_finite() {
            self.gap = gap.max(px(0.0));
        }
        self
    }
}

impl Styled for Dock {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Dock {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dock {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state = self.state.read(cx);
        let item_count = self.children.len();
        let item_size_f = self.item_size / px(1.0);
        let gap_f = self.gap / px(1.0);
        let sigma = (item_size_f * 1.5).max(1.0);

        let total_width =
            item_count as f32 * item_size_f + item_count.saturating_sub(1) as f32 * gap_f;
        let start_x = -total_width * 0.5;
        let animations_enabled = !cx.reduce_motion();
        let cursor_rel = if animations_enabled {
            state
                .hovered_index
                .map(|index| start_x + index as f32 * (item_size_f + gap_f) + item_size_f * 0.5)
        } else {
            None
        };

        let scales = compute_scales(
            item_count,
            item_size_f,
            gap_f,
            cursor_rel,
            self.max_scale,
            sigma,
        );

        let state_hover = self.state.clone();
        let dock_id = self.id.clone();

        let mut row = div()
            .id(dock_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Application dock"),
            )
            .flex()
            .flex_row()
            .items_end()
            .gap(self.gap)
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(12.0))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .on_hover(move |hovered: &bool, _window, cx| {
                if animations_enabled && !*hovered {
                    state_hover.update(cx, |s, cx| {
                        s.hovered_index = None;
                        cx.notify();
                    });
                }
            })
            .map(|this: Stateful<Div>| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            });

        for (index, (child, info)) in self.children.into_iter().zip(scales.iter()).enumerate() {
            let scaled_size = px(item_size_f * info.scale);
            let item_state = self.state.clone();
            row = row.child(
                div()
                    .id(ElementId::NamedChild(
                        Box::new(dock_id.clone()),
                        format!("item-{index}").into(),
                    ))
                    .flex_shrink_0()
                    .w(scaled_size)
                    .h(scaled_size)
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_hover(move |hovered: &bool, _, cx| {
                        if !animations_enabled {
                            return;
                        }
                        item_state.update(cx, |state, cx| {
                            let next = hovered.then_some(index);
                            if state.hovered_index != next {
                                state.hovered_index = next;
                                cx.notify();
                            }
                        });
                    })
                    .child(child),
            );
        }

        row
    }
}

#[cfg(test)]
mod tests {
    use super::compute_scales;

    #[test]
    fn hovered_item_receives_peak_magnification() {
        let item_size = 48.0;
        let gap = 4.0;
        let count = 5;
        let total_width = count as f32 * item_size + (count - 1) as f32 * gap;
        let start = -total_width * 0.5;
        let hovered_index = 2;
        let cursor = start + hovered_index as f32 * (item_size + gap) + item_size * 0.5;
        let scales = compute_scales(count, item_size, gap, Some(cursor), 0.5, 72.0);

        assert_eq!(scales.len(), count);
        assert!((scales[hovered_index].scale - 1.5).abs() < f32::EPSILON);
        assert!(scales[hovered_index].scale > scales[0].scale);
        assert!(scales[hovered_index].scale > scales[4].scale);
    }

    #[test]
    fn idle_dock_keeps_every_item_at_base_scale() {
        let scales = compute_scales(4, 48.0, 4.0, None, 0.5, 72.0);
        assert!(scales.iter().all(|item| item.scale == 1.0));
    }
}
