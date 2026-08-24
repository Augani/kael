use std::time::Duration;
use web_time::Instant;

const MIN_DT: f32 = 0.001;
const MAX_DT: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SpringPreset {
    Gentle,
    Wobbly,
    Stiff,
    Slow,
    #[default]
    Snappy,
}

impl SpringPreset {
    pub fn spring(self) -> Spring {
        match self {
            SpringPreset::Gentle => Spring::gentle(),
            SpringPreset::Wobbly => Spring::wobbly(),
            SpringPreset::Stiff => Spring::stiff(),
            SpringPreset::Slow => Spring::slow(),
            SpringPreset::Snappy => Spring::snappy(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Spring {
    pub position: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
    mass: f32,
    rest_threshold: f32,
}

impl Default for Spring {
    fn default() -> Self {
        Self::gentle()
    }
}

impl Spring {
    pub fn new(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            target: 0.0,
            stiffness: stiffness.max(0.1),
            damping: damping.max(0.0),
            mass: mass.max(0.01),
            rest_threshold: 0.001,
        }
    }

    pub fn gentle() -> Self {
        Self::new(120.0, 14.0, 1.0)
    }

    pub fn wobbly() -> Self {
        Self::new(180.0, 12.0, 1.0)
    }

    pub fn stiff() -> Self {
        Self::new(210.0, 20.0, 1.0)
    }

    pub fn slow() -> Self {
        Self::new(280.0, 60.0, 1.0)
    }

    pub fn snappy() -> Self {
        Self::new(400.0, 30.0, 1.0)
    }

    pub fn with_position(mut self, position: f32) -> Self {
        self.position = position;
        self
    }

    pub fn with_target(mut self, target: f32) -> Self {
        self.target = target;
        self
    }

    pub fn with_velocity(mut self, velocity: f32) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn with_rest_threshold(mut self, threshold: f32) -> Self {
        self.rest_threshold = threshold.max(0.0001);
        self
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn set_position(&mut self, position: f32) {
        self.position = position;
    }

    pub fn impulse(&mut self, velocity: f32) {
        self.velocity += velocity;
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = dt.min(0.064);

        let displacement = self.position - self.target;
        let spring_force = -self.stiffness * displacement;
        let damping_force = -self.damping * self.velocity;
        let acceleration = (spring_force + damping_force) / self.mass;

        self.velocity += acceleration * dt;
        self.position += self.velocity * dt;

        let is_moving = self.velocity.abs() > self.rest_threshold
            || (self.position - self.target).abs() > self.rest_threshold;

        if !is_moving {
            self.position = self.target;
            self.velocity = 0.0;
        }

        is_moving
    }

    pub fn tick_duration(&mut self, duration: Duration) -> bool {
        self.tick(duration.as_secs_f32())
    }

    pub fn is_at_rest(&self) -> bool {
        self.velocity.abs() <= self.rest_threshold
            && (self.position - self.target).abs() <= self.rest_threshold
    }

    pub fn progress(&self) -> f32 {
        if (self.target - self.position).abs() < self.rest_threshold {
            return 1.0;
        }
        let start = 0.0_f32;
        let total = self.target - start;
        if total.abs() < f32::EPSILON {
            return 1.0;
        }
        ((self.position - start) / total).clamp(0.0, 1.5)
    }

    pub fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
    }

    pub fn snap_to_target(&mut self) {
        self.position = self.target;
        self.velocity = 0.0;
    }
}

#[derive(Clone, Debug)]
pub struct SpringValue {
    spring: Spring,
    last_tick: Option<Instant>,
    settled: bool,
}

impl SpringValue {
    pub fn new(value: f32) -> Self {
        Self {
            spring: Spring::default().with_position(value).with_target(value),
            last_tick: None,
            settled: true,
        }
    }

    pub fn with_preset(value: f32, preset: SpringPreset) -> Self {
        Self {
            spring: preset.spring().with_position(value).with_target(value),
            last_tick: None,
            settled: true,
        }
    }

    pub fn with_spring(value: f32, spring: Spring) -> Self {
        Self {
            spring: spring.with_position(value).with_target(value),
            last_tick: None,
            settled: true,
        }
    }

    pub fn set_preset(&mut self, preset: SpringPreset) {
        let position = self.spring.position;
        let target = self.spring.target;
        let velocity = self.spring.velocity;
        self.spring = preset
            .spring()
            .with_position(position)
            .with_target(target)
            .with_velocity(velocity);
    }

    pub fn value(&self) -> f32 {
        self.spring.position
    }

    pub fn velocity(&self) -> f32 {
        self.spring.velocity
    }

    pub fn target(&self) -> f32 {
        self.spring.target
    }

    pub fn set_target(&mut self, target: f32) {
        if (self.spring.target - target).abs() > f32::EPSILON {
            self.spring.set_target(target);
            self.wake();
        }
    }

    pub fn set_value(&mut self, value: f32) {
        self.spring.set_position(value);
        self.spring.velocity = 0.0;
        self.spring.set_target(value);
        self.settled = true;
        self.last_tick = None;
    }

    pub fn impulse(&mut self, velocity: f32) {
        self.spring.impulse(velocity);
        self.wake();
    }

    pub fn is_settled(&self) -> bool {
        self.settled
    }

    pub fn stop(&mut self) {
        self.spring.velocity = 0.0;
        self.spring.set_target(self.spring.position);
        self.settled = true;
        self.last_tick = None;
    }

    pub fn tick_with_real_dt(&mut self) -> bool {
        let now = Instant::now();
        let dt = match self.last_tick {
            Some(previous) => now.duration_since(previous).as_secs_f32(),
            None => MIN_DT,
        };
        self.last_tick = Some(now);
        self.tick(dt.clamp(MIN_DT, MAX_DT))
    }

    pub fn advance(&mut self) -> bool {
        self.tick_with_real_dt()
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if self.settled {
            return false;
        }
        let moving = self.spring.tick(dt.clamp(MIN_DT, MAX_DT));
        self.settled = !moving;
        if self.settled {
            self.last_tick = None;
        }
        moving
    }

    fn wake(&mut self) {
        self.settled = false;
        self.last_tick = None;
    }
}

#[derive(Clone, Debug)]
pub struct SpringPoint {
    x: SpringValue,
    y: SpringValue,
}

impl SpringPoint {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: SpringValue::new(x),
            y: SpringValue::new(y),
        }
    }

    pub fn with_preset(x: f32, y: f32, preset: SpringPreset) -> Self {
        Self {
            x: SpringValue::with_preset(x, preset),
            y: SpringValue::with_preset(y, preset),
        }
    }

    pub fn set_preset(&mut self, preset: SpringPreset) {
        self.x.set_preset(preset);
        self.y.set_preset(preset);
    }

    pub fn value(&self) -> (f32, f32) {
        (self.x.value(), self.y.value())
    }

    pub fn velocity(&self) -> (f32, f32) {
        (self.x.velocity(), self.y.velocity())
    }

    pub fn target(&self) -> (f32, f32) {
        (self.x.target(), self.y.target())
    }

    pub fn set_target(&mut self, x: f32, y: f32) {
        self.x.set_target(x);
        self.y.set_target(y);
    }

    pub fn set_value(&mut self, x: f32, y: f32) {
        self.x.set_value(x);
        self.y.set_value(y);
    }

    pub fn impulse(&mut self, vx: f32, vy: f32) {
        self.x.impulse(vx);
        self.y.impulse(vy);
    }

    pub fn is_settled(&self) -> bool {
        self.x.is_settled() && self.y.is_settled()
    }

    pub fn stop(&mut self) {
        self.x.stop();
        self.y.stop();
    }

    pub fn tick_with_real_dt(&mut self) -> bool {
        let moving_x = self.x.tick_with_real_dt();
        let moving_y = self.y.tick_with_real_dt();
        moving_x || moving_y
    }

    pub fn advance(&mut self) -> bool {
        self.tick_with_real_dt()
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let moving_x = self.x.tick(dt);
        let moving_y = self.y.tick(dt);
        moving_x || moving_y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(value: &mut SpringValue, dt: f32, max_steps: usize) -> usize {
        let mut steps = 0;
        while value.tick(dt) && steps < max_steps {
            steps += 1;
        }
        steps
    }

    #[test]
    fn spring_value_settles_to_target() {
        let mut value = SpringValue::with_preset(0.0, SpringPreset::Snappy);
        value.set_target(100.0);
        assert!(!value.is_settled());

        let steps = settle(&mut value, 1.0 / 60.0, 10_000);
        assert!(steps < 10_000);
        assert!(value.is_settled());
        assert!((value.value() - 100.0).abs() < 0.5);
        assert!(value.velocity().abs() < 0.5);
    }

    #[test]
    fn impulse_changes_trajectory() {
        let mut without = SpringValue::with_preset(0.0, SpringPreset::Gentle);
        without.set_target(0.0);
        let mut with = without.clone();

        with.impulse(500.0);
        assert!(!with.is_settled());

        without.tick(1.0 / 60.0);
        with.tick(1.0 / 60.0);

        assert!(with.value() > without.value() + 1.0);
    }

    #[test]
    fn set_value_teleports_and_settles() {
        let mut value = SpringValue::with_preset(0.0, SpringPreset::Wobbly);
        value.impulse(800.0);
        value.set_target(50.0);
        assert!(!value.is_settled());

        value.set_value(25.0);
        assert!(value.is_settled());
        assert_eq!(value.value(), 25.0);
        assert_eq!(value.velocity(), 0.0);
        assert_eq!(value.target(), 25.0);
    }

    #[test]
    fn dt_clamping_keeps_step_bounded() {
        let mut clamped = SpringValue::with_preset(0.0, SpringPreset::Stiff);
        clamped.set_target(100.0);
        let mut reference = clamped.clone();

        clamped.tick(5.0);
        reference.tick(MAX_DT);

        assert!((clamped.value() - reference.value()).abs() < f32::EPSILON);
        assert!((clamped.velocity() - reference.velocity()).abs() < f32::EPSILON);
    }

    #[test]
    fn dt_clamping_floors_small_steps() {
        let mut floored = SpringValue::with_preset(0.0, SpringPreset::Stiff);
        floored.set_target(100.0);
        let mut reference = floored.clone();

        floored.tick(0.0);
        reference.tick(MIN_DT);

        assert!((floored.value() - reference.value()).abs() < f32::EPSILON);
    }

    #[test]
    fn frame_rate_parity_60hz_vs_120hz() {
        let mut at_60 = SpringValue::with_preset(0.0, SpringPreset::Gentle);
        let mut at_120 = SpringValue::with_preset(0.0, SpringPreset::Gentle);
        at_60.set_target(200.0);
        at_120.set_target(200.0);

        for _ in 0..60 {
            at_60.tick(1.0 / 60.0);
        }
        for _ in 0..120 {
            at_120.tick(1.0 / 120.0);
        }

        let span = at_60.value().abs().max(at_120.value().abs()).max(1.0);
        assert!((at_60.value() - at_120.value()).abs() < span * 0.05);
        assert!((at_60.velocity() - at_120.velocity()).abs() < 5.0);
    }

    #[test]
    fn settled_value_ignores_ticks() {
        let mut value = SpringValue::new(10.0);
        assert!(value.is_settled());
        assert!(!value.tick(1.0 / 60.0));
        assert_eq!(value.value(), 10.0);
    }

    #[test]
    fn spring_point_settles_on_both_axes() {
        let mut point = SpringPoint::with_preset(0.0, 0.0, SpringPreset::Snappy);
        point.set_target(100.0, -50.0);
        assert!(!point.is_settled());

        let mut steps = 0;
        while point.tick(1.0 / 60.0) && steps < 10_000 {
            steps += 1;
        }
        assert!(point.is_settled());
        let (x, y) = point.value();
        assert!((x - 100.0).abs() < 0.5);
        assert!((y + 50.0).abs() < 0.5);
    }

    #[test]
    fn spring_point_impulse_affects_velocity() {
        let mut point = SpringPoint::with_preset(0.0, 0.0, SpringPreset::Gentle);
        point.impulse(300.0, -200.0);
        let (vx, vy) = point.velocity();
        assert!((vx - 300.0).abs() < f32::EPSILON);
        assert!((vy + 200.0).abs() < f32::EPSILON);
        assert!(!point.is_settled());
    }

    #[test]
    fn real_dt_tick_advances_and_settles() {
        let mut value = SpringValue::with_preset(0.0, SpringPreset::Snappy);
        value.set_target(10.0);
        let mut steps = 0;
        while value.advance() && steps < 100_000 {
            steps += 1;
        }
        assert!(value.is_settled());
        assert!((value.value() - 10.0).abs() < 0.5);
    }
}
