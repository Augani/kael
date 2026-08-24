//! Lightweight particle system using canvas painting.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::animations::DisplayFrameClock;

const MAX_PARTICLES: usize = 10_000;

#[derive(Clone)]
pub struct Particle {
    pub position: Point<f32>,
    pub velocity: Point<f32>,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    pub color: Hsla,
}

#[derive(Clone)]
pub struct ParticleEmitterConfig {
    pub spawn_rate: f32,
    pub lifetime: Duration,
    pub velocity_range: (f32, f32),
    pub size_range: (f32, f32),
    pub color_start: Hsla,
    pub color_end: Hsla,
    pub gravity: f32,
    pub spread_angle: f32,
    pub max_particles: usize,
    pub origin: Point<f32>,
}

impl Default for ParticleEmitterConfig {
    fn default() -> Self {
        Self {
            spawn_rate: 10.0,
            lifetime: Duration::from_millis(1500),
            velocity_range: (50.0, 150.0),
            size_range: (2.0, 6.0),
            color_start: hsla(0.55, 0.8, 0.6, 1.0),
            color_end: hsla(0.55, 0.8, 0.6, 0.0),
            gravity: 80.0,
            spread_angle: std::f32::consts::PI,
            max_particles: 200,
            origin: Point { x: 0.0, y: 0.0 },
        }
    }
}

pub struct ParticleEmitterState {
    particles: Vec<Particle>,
    config: ParticleEmitterConfig,
    accumulator: f32,
    running: bool,
    frame_clock: DisplayFrameClock,
}

impl ParticleEmitterState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            particles: Vec::with_capacity(200),
            config: ParticleEmitterConfig::default(),
            accumulator: 0.0,
            running: false,
            frame_clock: DisplayFrameClock::default(),
        }
    }

    pub fn with_config(config: ParticleEmitterConfig, _cx: &mut Context<Self>) -> Self {
        let config = sanitized_config(config);
        let cap = config.max_particles;
        Self {
            particles: Vec::with_capacity(cap),
            config,
            accumulator: 0.0,
            running: false,
            frame_clock: DisplayFrameClock::default(),
        }
    }

    pub fn set_config(&mut self, config: ParticleEmitterConfig, cx: &mut Context<Self>) {
        self.config = sanitized_config(config);
        self.particles.truncate(self.config.max_particles);
        cx.notify();
    }

    pub fn set_origin(&mut self, origin: Point<f32>, cx: &mut Context<Self>) {
        if origin.x.is_finite() && origin.y.is_finite() {
            self.config.origin = origin;
        }
        cx.notify();
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.running || cx.reduce_motion() {
            return;
        }
        self.running = true;
        self.frame_clock.restart();
        cx.notify();
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        self.frame_clock.stop();
        cx.notify();
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.particles.clear();
        cx.notify();
    }

    pub fn emit(&mut self, count: usize) {
        let config = &self.config;
        let lifetime_secs = config.lifetime.as_secs_f32();

        for _ in 0..count {
            if self.particles.len() >= config.max_particles {
                break;
            }

            let angle_offset =
                (pseudo_random_f32(self.particles.len() as u32) - 0.5) * config.spread_angle;
            let speed = config.velocity_range.0
                + pseudo_random_f32(self.particles.len() as u32 + 7)
                    * (config.velocity_range.1 - config.velocity_range.0);
            let particle_size = config.size_range.0
                + pseudo_random_f32(self.particles.len() as u32 + 13)
                    * (config.size_range.1 - config.size_range.0);

            let vx = angle_offset.sin() * speed;
            let vy = -angle_offset.cos() * speed;

            self.particles.push(Particle {
                position: config.origin,
                velocity: Point { x: vx, y: vy },
                age: 0.0,
                lifetime: lifetime_secs,
                size: particle_size,
                color: config.color_start,
            });
        }
    }

    pub fn update(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let gravity = self.config.gravity;
        let color_start = self.config.color_start;
        let color_end = self.config.color_end;

        for particle in &mut self.particles {
            particle.age += dt;
            particle.velocity.y += gravity * dt;
            particle.position.x += particle.velocity.x * dt;
            particle.position.y += particle.velocity.y * dt;

            let t = (particle.age / particle.lifetime).clamp(0.0, 1.0);
            particle.color = lerp_hsla(color_start, color_end, t);
        }

        self.particles.retain(|p| p.age < p.lifetime);
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }
}

fn schedule_emitter_frame(state: &Entity<ParticleEmitterState>, window: &Window, cx: &mut App) {
    let generation = state.update(cx, |state, _| {
        state.running.then(|| state.frame_clock.try_arm()).flatten()
    });
    let Some(generation) = generation else {
        return;
    };

    let state = state.downgrade();
    window.on_next_frame(move |window, cx| {
        _ = state.update(cx, |state, cx| {
            let Some(dt) = state.frame_clock.sample(generation) else {
                return;
            };
            if !state.running || !window.animations_enabled() {
                state.running = false;
                state.particles.clear();
                state.frame_clock.stop();
                cx.notify();
                return;
            }

            state.accumulator += state.config.spawn_rate * dt;
            let to_spawn = state.accumulator as usize;
            state.accumulator -= to_spawn as f32;
            state.emit(to_spawn);
            state.update(dt);
            cx.notify();
        });
    });
}

impl Render for ParticleEmitterState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct EmitterPaintData {
    particles: Vec<Particle>,
}

#[derive(IntoElement)]
pub struct ParticleEmitter {
    id: ElementId,
    state: Entity<ParticleEmitterState>,
    style: StyleRefinement,
}

impl ParticleEmitter {
    pub fn new(id: impl Into<ElementId>, state: Entity<ParticleEmitterState>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
        }
    }

    pub fn spawn_rate(self, rate: f32, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.spawn_rate = rate;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn lifetime(self, lifetime: Duration, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.lifetime = lifetime;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn velocity_range(self, range: (f32, f32), cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.velocity_range = range;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn size_range(self, range: (f32, f32), cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.size_range = range;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn color_start(self, color: Hsla, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.config.color_start = color);
        self
    }

    pub fn color_end(self, color: Hsla, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.config.color_end = color);
        self
    }

    pub fn gravity(self, gravity: f32, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.gravity = gravity;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn spread_angle(self, angle: f32, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            let mut config = s.config.clone();
            config.spread_angle = angle;
            s.config = sanitized_config(config);
        });
        self
    }

    pub fn max_particles(self, max: usize, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            s.config.max_particles = max.min(MAX_PARTICLES);
            s.particles.truncate(s.config.max_particles);
        });
        self
    }

    pub fn origin(self, origin: Point<f32>, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| {
            if origin.x.is_finite() && origin.y.is_finite() {
                s.config.origin = origin;
            }
        });
        self
    }
}

impl Styled for ParticleEmitter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ParticleEmitter {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        schedule_emitter_frame(&self.state, window, cx);
        let state = self.state.read(cx);
        let paint_data = EmitterPaintData {
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
                        paint_particles(bounds, &data, window);
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

fn paint_particles(bounds: Bounds<Pixels>, data: &EmitterPaintData, window: &mut Window) {
    if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
        return;
    }

    for particle in &data.particles {
        let x = bounds.left() + px(particle.position.x);
        let y = bounds.top() + px(particle.position.y);
        let half = particle.size * 0.5;

        if x + px(half) < bounds.left()
            || x - px(half) > bounds.right()
            || y + px(half) < bounds.top()
            || y - px(half) > bounds.bottom()
        {
            continue;
        }

        window.paint_quad(PaintQuad {
            bounds: Bounds {
                origin: point(x - px(half), y - px(half)),
                size: kael::size(px(particle.size), px(particle.size)),
            },
            corner_radii: Corners::all(px(half)),
            background: particle.color.into(),
            border_widths: Edges::default(),
            border_color: (transparent_black()).into(),
            border_style: BorderStyle::default(),
            continuous_corners: false,
            transform: Default::default(),
            blend_mode: Default::default(),
        });
    }
}

fn lerp_hsla(a: Hsla, b: Hsla, t: f32) -> Hsla {
    hsla(
        a.h + (b.h - a.h) * t,
        a.s + (b.s - a.s) * t,
        a.l + (b.l - a.l) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn pseudo_random_f32(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9E3779B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x45D9F3B);
    x ^= x >> 16;
    (x & 0xFFFF) as f32 / 65535.0
}

fn sanitized_config(mut config: ParticleEmitterConfig) -> ParticleEmitterConfig {
    let defaults = ParticleEmitterConfig::default();

    if !config.spawn_rate.is_finite() || config.spawn_rate < 0.0 {
        config.spawn_rate = defaults.spawn_rate;
    }
    if config.lifetime.is_zero() {
        config.lifetime = defaults.lifetime;
    }
    config.velocity_range = sanitized_range(config.velocity_range, defaults.velocity_range, false);
    config.size_range = sanitized_range(config.size_range, defaults.size_range, true);
    if !config.gravity.is_finite() {
        config.gravity = defaults.gravity;
    }
    if !config.spread_angle.is_finite() || config.spread_angle < 0.0 {
        config.spread_angle = defaults.spread_angle;
    }
    if !config.origin.x.is_finite() || !config.origin.y.is_finite() {
        config.origin = defaults.origin;
    }
    config.max_particles = config.max_particles.min(MAX_PARTICLES);

    config
}

fn sanitized_range(range: (f32, f32), fallback: (f32, f32), positive: bool) -> (f32, f32) {
    let (mut start, mut end) = range;
    if !start.is_finite() || !end.is_finite() || (positive && (start <= 0.0 || end <= 0.0)) {
        return fallback;
    }
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn configuration_is_sanitized_before_simulation() {
        let mut cx = TestAppContext::single();
        let state = cx.new(|cx| {
            ParticleEmitterState::with_config(
                ParticleEmitterConfig {
                    spawn_rate: f32::NAN,
                    lifetime: Duration::ZERO,
                    velocity_range: (200.0, 50.0),
                    size_range: (-2.0, 4.0),
                    gravity: f32::INFINITY,
                    spread_angle: -1.0,
                    origin: Point {
                        x: f32::NAN,
                        y: 0.0,
                    },
                    ..ParticleEmitterConfig::default()
                },
                cx,
            )
        });

        cx.update(|cx| {
            let state = state.read(cx);
            assert_eq!(state.config.spawn_rate, 10.0);
            assert_eq!(state.config.lifetime, Duration::from_millis(1500));
            assert_eq!(state.config.velocity_range, (50.0, 200.0));
            assert_eq!(state.config.size_range, (2.0, 6.0));
            assert_eq!(state.config.gravity, 80.0);
            assert_eq!(state.config.spread_angle, std::f32::consts::PI);
            assert_eq!(state.config.origin, Point { x: 0.0, y: 0.0 });
        });
    }

    #[::core::prelude::v1::test]
    fn reduced_motion_does_not_start_the_emitter() {
        let mut cx = TestAppContext::single();
        cx.set_reduce_motion(true);
        let state = cx.new(ParticleEmitterState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| state.start(cx));
            assert!(!state.read(cx).is_running());
        });
    }
}
