//! Aurora background - animated overlapping blobs creating organic color-shifting effect.

use kael::prelude::FluentBuilder as _;
use kael::*;
use std::{panic::Location, time::Duration};

use crate::theme::Theme;

/// Low-frequency animation clock for [`Aurora`]. Keeping the clock in an
/// entity avoids a full display-rate render loop for a decorative background.
pub struct AuroraState {
    version: usize,
    paused: bool,
}

impl AuroraState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let result = this.update(cx, |state, cx| {
                    if !state.paused && !cx.reduce_motion() {
                        state.version = state.version.wrapping_add(1);
                        cx.notify();
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            version: 0,
            paused: false,
        }
    }

    pub fn new_paused(cx: &mut Context<Self>) -> Self {
        let mut state = Self::new(cx);
        state.paused = true;
        state
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        if !self.paused {
            self.paused = true;
            cx.notify();
        }
    }

    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if self.paused && !cx.reduce_motion() {
            self.paused = false;
            cx.notify();
        }
    }
}

#[derive(IntoElement)]
pub struct Aurora {
    id: ElementId,
    state: Option<Entity<AuroraState>>,
    colors: Vec<Hsla>,
    speed: f32,
    blur_amount: Pixels,
    animated: bool,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Aurora {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "aurora:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state: None,
            colors: Vec::new(),
            speed: 1.0,
            blur_amount: px(80.0),
            animated: true,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn colors(mut self, colors: Vec<Hsla>) -> Self {
        self.colors = colors;
        self
    }

    /// Attach the low-frequency clock used for animated rendering. Without a
    /// state, Aurora renders a stable frame and consumes no idle CPU.
    pub fn state(mut self, state: Entity<AuroraState>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn speed(mut self, speed: f32) -> Self {
        if speed.is_finite() {
            self.speed = speed.max(0.1);
        }
        self
    }

    pub fn blur_amount(mut self, blur: Pixels) -> Self {
        if (blur / px(1.0)).is_finite() && blur >= px(0.0) {
            self.blur_amount = blur;
        }
        self
    }

    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }
}

impl Default for Aurora {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Aurora {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Aurora {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

struct BlobConfig {
    x_pct: f32,
    y_pct: f32,
    w_pct: f32,
    h_pct: f32,
}

const BLOB_CONFIGS: [BlobConfig; 5] = [
    BlobConfig {
        x_pct: -0.1,
        y_pct: -0.2,
        w_pct: 0.7,
        h_pct: 0.6,
    },
    BlobConfig {
        x_pct: 0.3,
        y_pct: 0.1,
        w_pct: 0.6,
        h_pct: 0.7,
    },
    BlobConfig {
        x_pct: 0.5,
        y_pct: -0.1,
        w_pct: 0.5,
        h_pct: 0.5,
    },
    BlobConfig {
        x_pct: -0.05,
        y_pct: 0.4,
        w_pct: 0.65,
        h_pct: 0.55,
    },
    BlobConfig {
        x_pct: 0.4,
        y_pct: 0.3,
        w_pct: 0.55,
        h_pct: 0.65,
    },
];

impl RenderOnce for Aurora {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        let default_colors = vec![
            Hsla {
                a: 0.3,
                ..theme.tokens.primary
            },
            Hsla {
                a: 0.25,
                ..theme.tokens.accent
            },
            hsla(280.0 / 360.0, 0.7, 0.6, 0.2),
            hsla(200.0 / 360.0, 0.8, 0.5, 0.25),
            hsla(160.0 / 360.0, 0.6, 0.5, 0.2),
        ];

        let colors = if self.colors.is_empty() {
            default_colors
        } else {
            self.colors
        };

        let speed = self.speed;
        let blur = self.blur_amount;
        let blur_f32 = blur / px(1.0);
        let animated = self.animated && !cx.reduce_motion();
        let version = self
            .state
            .as_ref()
            .map(|state| state.read(cx).version)
            .unwrap_or(0);

        let mut blob_layer = div()
            .absolute()
            .left(relative(-0.03))
            .top(relative(-0.03))
            .w(relative(1.06))
            .h(relative(1.06));

        for (idx, config) in BLOB_CONFIGS.iter().enumerate() {
            let color = colors[idx % colors.len()];
            blob_layer = blob_layer.child(
                div()
                    .absolute()
                    .left(relative(config.x_pct))
                    .top(relative(config.y_pct))
                    .w(relative(config.w_pct))
                    .h(relative(config.h_pct))
                    .rounded(px(blur_f32 * 2.0))
                    .bg(color),
            );
        }

        let phase = if animated && self.state.is_some() {
            let cycle_ticks = (100.0 / speed).max(5.0);
            let delta = (version as f32 % cycle_ticks) / cycle_ticks;
            if delta < 0.5 {
                delta * 2.0
            } else {
                2.0 - delta * 2.0
            }
        } else {
            0.0
        };
        let blob_layer = blob_layer
            .left(relative(-0.03 + 0.06 * phase))
            .top(relative(-0.01 - 0.04 * phase))
            .opacity(0.82 + 0.18 * phase);

        div()
            .id(self.id)
            .relative()
            .w_full()
            .h_full()
            .overflow_hidden()
            .child(blob_layer)
            .child(div().relative().size_full().children(self.children))
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
    fn invalid_motion_configuration_keeps_safe_defaults() {
        let aurora = Aurora::new()
            .speed(f32::NAN)
            .blur_amount(px(-10.0))
            .animated(false);

        assert_eq!(aurora.speed, 1.0);
        assert_eq!(aurora.blur_amount, px(80.0));
        assert!(!aurora.animated);
    }

    #[::core::prelude::v1::test]
    fn paused_state_can_resume_when_motion_is_allowed() {
        let mut cx = TestAppContext::single();
        let state = cx.new(AuroraState::new_paused);

        cx.update(|cx| {
            assert!(state.read(cx).is_paused());
            state.update(cx, |state, cx| state.resume(cx));
            assert!(!state.read(cx).is_paused());
        });
    }
}
