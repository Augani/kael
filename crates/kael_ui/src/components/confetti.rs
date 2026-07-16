//! Pre-configured celebration particle burst.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

const DEFAULT_CONFETTI_COLORS: [u32; 6] =
    [0xFF6B6B, 0x4ECDC4, 0x45B7D1, 0xFFA07A, 0x98D8C8, 0xF7DC6F];
const MAX_PARTICLES: usize = 1_000;
const SIMULATION_REFERENCE_SIZE: f32 = 500.0;

#[derive(Clone)]
pub struct ConfettiParticle {
    pub position: Point<f32>,
    pub velocity: Point<f32>,
    pub rotation_speed: f32,
    pub size: f32,
    pub color: Hsla,
    pub age: f32,
    pub lifetime: f32,
}

pub struct ConfettiState {
    is_active: bool,
    particles: Vec<ConfettiParticle>,
    particle_count: usize,
    colors: Vec<Hsla>,
    gravity: f32,
    origin: Point<f32>,
    spread: f32,
}

impl ConfettiState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            is_active: false,
            particles: Vec::new(),
            particle_count: 80,
            colors: DEFAULT_CONFETTI_COLORS
                .iter()
                .map(|&c| {
                    let color: Rgba = rgb(c);
                    Hsla::from(color)
                })
                .collect(),
            gravity: 120.0,
            origin: Point { x: 0.5, y: 0.5 },
            spread: 300.0,
        }
    }

    pub fn set_particle_count(&mut self, count: usize) {
        self.particle_count = count.min(MAX_PARTICLES);
    }

    pub fn set_colors(&mut self, colors: Vec<Hsla>) {
        if !colors.is_empty() {
            self.colors = colors;
        }
    }

    pub fn set_gravity(&mut self, gravity: f32) {
        if gravity.is_finite() {
            self.gravity = gravity;
        }
    }

    pub fn set_origin(&mut self, origin: Point<f32>) {
        if origin.x.is_finite() && origin.y.is_finite() {
            self.origin = Point {
                x: origin.x.clamp(0.0, 1.0),
                y: origin.y.clamp(0.0, 1.0),
            };
        }
    }

    pub fn set_spread(&mut self, spread: f32) {
        if spread.is_finite() && spread >= 0.0 {
            self.spread = spread;
        }
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn burst(&mut self, cx: &mut Context<Self>) {
        self.particles.clear();

        if cx.reduce_motion() || self.particle_count == 0 {
            self.is_active = false;
            cx.notify();
            return;
        }

        self.is_active = true;

        let count = self.particle_count;
        let color_count = self.colors.len().max(1);

        for i in 0..count {
            let seed = i as u32;
            let angle = pseudo_random_f32(seed) * std::f32::consts::TAU;
            let speed = self.spread * (0.3 + pseudo_random_f32(seed + 3) * 0.7);

            let vx = angle.cos() * speed;
            let vy = angle.sin() * speed - self.spread * 0.5;

            let color_idx =
                (pseudo_random_f32(seed + 7) * color_count as f32) as usize % color_count;
            let particle_size = 4.0 + pseudo_random_f32(seed + 11) * 6.0;
            let rotation_spd = (pseudo_random_f32(seed + 17) - 0.5) * 10.0;
            let lifetime = 1.5 + pseudo_random_f32(seed + 23) * 1.5;

            self.particles.push(ConfettiParticle {
                position: Point {
                    x: self.origin.x,
                    y: self.origin.y,
                },
                velocity: Point { x: vx, y: vy },
                rotation_speed: rotation_spd,
                size: particle_size,
                color: self.colors[color_idx],
                age: 0.0,
                lifetime,
            });
        }

        self.schedule_tick(cx);
        cx.notify();
    }

    fn update_particles(&mut self, dt: f32) {
        let gravity = self.gravity;

        for particle in &mut self.particles {
            particle.age += dt;
            // Positions are normalized to the canvas. Keep the physics values in
            // familiar pixel-like units while converting displacement once here.
            particle.velocity.y += gravity * dt;
            particle.velocity.x *= 0.99;
            particle.position.x += particle.velocity.x * dt / SIMULATION_REFERENCE_SIZE;
            particle.position.y += particle.velocity.y * dt / SIMULATION_REFERENCE_SIZE;
        }

        self.particles.retain(|p| p.age < p.lifetime);

        if self.particles.is_empty() {
            self.is_active = false;
        }
    }

    pub fn particles(&self) -> &[ConfettiParticle] {
        &self.particles
    }

    fn schedule_tick(&self, cx: &mut Context<Self>) {
        if !self.is_active {
            return;
        }

        cx.spawn(async |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;

            _ = this.update(cx, |state, cx| {
                if !state.is_active || cx.reduce_motion() {
                    state.is_active = false;
                    state.particles.clear();
                    cx.notify();
                    return;
                }

                let dt = 1.0 / 60.0;
                state.update_particles(dt);

                if state.is_active {
                    state.schedule_tick(cx);
                }

                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ConfettiState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct ConfettiPaintData {
    particles: Vec<ConfettiParticle>,
}

#[derive(IntoElement)]
pub struct Confetti {
    id: ElementId,
    state: Entity<ConfettiState>,
    style: StyleRefinement,
}

impl Confetti {
    pub fn new(id: impl Into<ElementId>, state: Entity<ConfettiState>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
        }
    }

    pub fn particle_count(self, count: usize, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.set_particle_count(count));
        self
    }

    pub fn colors(self, colors: Vec<Hsla>, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.set_colors(colors));
        self
    }

    pub fn gravity(self, gravity: f32, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.set_gravity(gravity));
        self
    }

    pub fn spread(self, spread: f32, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.set_spread(spread));
        self
    }
}

impl Styled for Confetti {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Confetti {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let paint_data = ConfettiPaintData {
            particles: state.particles().to_vec(),
        };

        div()
            .id(self.id)
            .relative()
            .size_full()
            .child(
                canvas_with_prepaint(
                    move |_bounds, _window, _cx| paint_data,
                    move |bounds, data, window, _cx| {
                        paint_confetti(bounds, &data, window);
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

fn paint_confetti(bounds: Bounds<Pixels>, data: &ConfettiPaintData, window: &mut Window) {
    if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
        return;
    }

    let bw = bounds.size.width / px(1.0);
    let bh = bounds.size.height / px(1.0);

    for particle in &data.particles {
        let x = bounds.left() + px(particle.position.x * bw);
        let y = bounds.top() + px(particle.position.y * bh);
        let half = particle.size * 0.5;

        if x + px(half) < bounds.left()
            || x - px(half) > bounds.right()
            || y + px(half) < bounds.top()
            || y - px(half) > bounds.bottom()
        {
            continue;
        }

        let fade = 1.0 - (particle.age / particle.lifetime).clamp(0.0, 1.0);
        let alpha = particle.color.a * fade;

        let wobble = (particle.age * particle.rotation_speed).sin().abs();
        let w = particle.size * (0.5 + wobble * 0.5);
        let h = particle.size;

        window.paint_quad(PaintQuad {
            bounds: Bounds {
                origin: point(x - px(w * 0.5), y - px(h * 0.5)),
                size: kael::size(px(w), px(h)),
            },
            corner_radii: Corners::all(px(1.0)),
            background: hsla(particle.color.h, particle.color.s, particle.color.l, alpha).into(),
            border_widths: Edges::default(),
            border_color: (transparent_black()).into(),
            border_style: BorderStyle::default(),
            continuous_corners: false,
            transform: Default::default(),
            blend_mode: Default::default(),
        });
    }
}

fn pseudo_random_f32(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9E3779B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x45D9F3B);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn particle_motion_stays_in_normalized_canvas_coordinates() {
        let mut cx = TestAppContext::single();
        let state = cx.new(ConfettiState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_particle_count(1);
                state.burst(cx);
                state.update_particles(1.0 / 60.0);

                let particle = &state.particles()[0];
                assert!((particle.position.x - 0.5).abs() < 0.02);
                assert!((particle.position.y - 0.5).abs() < 0.02);
            });
        });
    }

    #[::core::prelude::v1::test]
    fn reduced_motion_suppresses_the_particle_burst() {
        let mut cx = TestAppContext::single();
        cx.set_reduce_motion(true);
        let state = cx.new(ConfettiState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| state.burst(cx));
            assert!(!state.read(cx).is_active());
            assert!(state.read(cx).particles().is_empty());
        });
    }

    #[::core::prelude::v1::test]
    fn invalid_configuration_keeps_safe_values() {
        let mut cx = TestAppContext::single();
        let state = cx.new(ConfettiState::new);

        cx.update(|cx| {
            state.update(cx, |state, _| {
                state.set_particle_count(usize::MAX);
                state.set_gravity(f32::NAN);
                state.set_origin(Point {
                    x: f32::INFINITY,
                    y: 0.0,
                });
                state.set_spread(-1.0);

                assert_eq!(state.particle_count, MAX_PARTICLES);
                assert_eq!(state.gravity, 120.0);
                assert_eq!(state.origin, Point { x: 0.5, y: 0.5 });
                assert_eq!(state.spread, 300.0);
            });
        });
    }
}
