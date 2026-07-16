//! Skeleton loader - renders shimmer placeholders when loading, transitions to content when ready.

use kael::prelude::FluentBuilder as _;
use kael::*;
use std::time::Duration;

use crate::theme::Theme;

fn sanitize_line_height(height: Pixels) -> Pixels {
    let height = f32::from(height);
    px(if height.is_finite() && height > 0.0 {
        height
    } else {
        16.0
    })
}

fn sanitize_line_gap(gap: Pixels) -> Pixels {
    let gap = f32::from(gap);
    px(if gap.is_finite() && gap >= 0.0 {
        gap
    } else {
        12.0
    })
}

fn sanitize_shimmer_duration(duration: Duration) -> Duration {
    if duration.is_zero() {
        Duration::from_millis(1500)
    } else {
        duration
    }
}

pub struct SkeletonLoaderState {
    is_loading: bool,
    transition_version: usize,
}

impl Default for SkeletonLoaderState {
    fn default() -> Self {
        Self::new()
    }
}

impl SkeletonLoaderState {
    pub fn new() -> Self {
        Self {
            is_loading: true,
            transition_version: 0,
        }
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        if self.is_loading != loading {
            self.is_loading = loading;
            self.transition_version = self.transition_version.wrapping_add(1);
            cx.notify();
        }
    }

    pub fn is_loading(&self) -> bool {
        self.is_loading
    }
}

#[derive(IntoElement)]
pub struct SkeletonLoader {
    id: ElementId,
    state: Entity<SkeletonLoaderState>,
    lines: usize,
    line_height: Pixels,
    line_gap: Pixels,
    shimmer_duration: Duration,
    shimmer_repeat: Repeat,
    accessibility_label: SharedString,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl SkeletonLoader {
    pub fn new(id: impl Into<ElementId>, state: Entity<SkeletonLoaderState>) -> Self {
        Self {
            id: id.into(),
            state,
            lines: 3,
            line_height: px(16.0),
            line_gap: px(12.0),
            shimmer_duration: Duration::from_millis(1500),
            shimmer_repeat: Repeat::Forever,
            accessibility_label: "Loading content".into(),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn lines(mut self, lines: usize) -> Self {
        self.lines = lines.max(1);
        self
    }

    pub fn line_height(mut self, height: Pixels) -> Self {
        self.line_height = sanitize_line_height(height);
        self
    }

    pub fn line_gap(mut self, gap: Pixels) -> Self {
        self.line_gap = sanitize_line_gap(gap);
        self
    }

    pub fn shimmer_duration(mut self, duration: Duration) -> Self {
        self.shimmer_duration = sanitize_shimmer_duration(duration);
        self
    }

    /// Set the status announced while placeholders are visible.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = if label.trim().is_empty() {
            "Loading content".into()
        } else {
            label
        };
        self
    }

    /// Limit loading shimmer to a finite number of sweeps.
    pub fn shimmer_cycles(mut self, cycles: u32) -> Self {
        self.shimmer_repeat = Repeat::Count(cycles.max(1));
        self
    }
}

impl Styled for SkeletonLoader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for SkeletonLoader {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

fn skeleton_line_width_pct(index: usize) -> f32 {
    match index % 5 {
        0 => 1.0,
        1 => 0.85,
        2 => 0.92,
        3 => 0.6,
        4 => 0.78,
        _ => 1.0,
    }
}

impl RenderOnce for SkeletonLoader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let loader_state = self.state.read(cx);
        let is_loading = loader_state.is_loading;
        let version = loader_state.transition_version;
        let shimmer_color = theme.tokens.muted;
        let highlight_color = hsla(0.0, 0.0, 1.0, 0.15);
        let animations_enabled = window.animations_enabled();
        let shimmer_repeat = self.shimmer_repeat;

        let mut wrapper = div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .when(is_loading, |this| {
                this.accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::ProgressBar)
                        .label(self.accessibility_label.clone())
                        .value(AccessibilityValue::Text("Loading".into()))
                        .busy(true),
                )
            });

        if is_loading {
            let mut skeleton_container = div().flex().flex_col().gap(self.line_gap).w_full();

            for line_idx in 0..self.lines {
                let width_pct = skeleton_line_width_pct(line_idx);
                let shimmer_dur = self.shimmer_duration;
                let anim_id =
                    ElementId::Name(format!("skeleton-shimmer-{}-{line_idx}", self.id).into());

                skeleton_container = skeleton_container.child(
                    div()
                        .w(relative(width_pct))
                        .h(self.line_height)
                        .rounded(theme.tokens.radius_md)
                        .bg(shimmer_color)
                        .overflow_hidden()
                        .relative()
                        .child(if animations_enabled {
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .w(px(200.0))
                                .bg(kael::linear_gradient(
                                    90.0,
                                    kael::linear_color_stop(transparent_black(), 0.0),
                                    kael::linear_color_stop(highlight_color, 1.0),
                                ))
                                .with_animation(
                                    anim_id,
                                    Animation::new(shimmer_dur)
                                        .repeat(shimmer_repeat)
                                        .with_easing(kael::linear),
                                    move |this, delta| {
                                        let start = px(-200.0);
                                        let end = px(600.0);
                                        let current = start + (end - start) * delta;
                                        this.left(current)
                                    },
                                )
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                );
            }

            wrapper = wrapper.child(skeleton_container);
        } else {
            let fade_duration = if animations_enabled {
                Duration::from_millis(300)
            } else {
                Duration::ZERO
            };
            let content = div().w_full().children(self.children).with_animation(
                ElementId::Name(format!("skeleton-fade-{version}").into()),
                Animation::new(fade_duration).with_easing(kael::ease_in_out),
                move |el, delta| el.opacity(delta),
            );

            wrapper = wrapper.child(content);
        }

        wrapper.map(|this| {
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
    fn invalid_geometry_and_duration_use_safe_defaults() {
        assert_eq!(sanitize_line_height(px(f32::NAN)), px(16.0));
        assert_eq!(sanitize_line_height(px(-1.0)), px(16.0));
        assert_eq!(sanitize_line_gap(px(f32::NEG_INFINITY)), px(12.0));
        assert_eq!(sanitize_line_gap(px(-1.0)), px(12.0));
        assert_eq!(
            sanitize_shimmer_duration(Duration::ZERO),
            Duration::from_millis(1500)
        );
    }
}
