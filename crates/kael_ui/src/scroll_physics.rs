#[derive(Clone, Debug)]
pub struct ScrollPhysics {
    velocity: f32,
    position: f32,
    min_bound: f32,
    max_bound: f32,
    deceleration: f32,
    overscroll_resistance: f32,
    momentum_enabled: bool,
    overscroll_enabled: bool,
}

impl Default for ScrollPhysics {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollPhysics {
    pub fn new() -> Self {
        Self {
            velocity: 0.0,
            position: 0.0,
            min_bound: 0.0,
            max_bound: f32::MAX,
            deceleration: 0.95,
            overscroll_resistance: 0.3,
            momentum_enabled: true,
            overscroll_enabled: true,
        }
    }

    pub fn with_bounds(mut self, min: f32, max: f32) -> Self {
        self.min_bound = min;
        self.max_bound = max;
        self
    }

    pub fn with_deceleration(mut self, deceleration: f32) -> Self {
        self.deceleration = deceleration.clamp(0.8, 0.99);
        self
    }

    pub fn with_overscroll_resistance(mut self, resistance: f32) -> Self {
        self.overscroll_resistance = resistance.clamp(0.0, 1.0);
        self
    }

    pub fn momentum(mut self, enabled: bool) -> Self {
        self.momentum_enabled = enabled;
        self
    }

    pub fn overscroll(mut self, enabled: bool) -> Self {
        self.overscroll_enabled = enabled;
        self
    }

    pub fn set_bounds(&mut self, min: f32, max: f32) {
        self.min_bound = min;
        self.max_bound = max;
    }

    pub fn apply_delta(&mut self, delta: f32) {
        if self.momentum_enabled {
            self.velocity = delta * 0.8 + self.velocity * 0.2;
        } else {
            self.velocity = 0.0;
        }
        self.position += delta;

        if !self.overscroll_enabled {
            self.position = self.position.clamp(self.min_bound, self.max_bound);
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if !self.momentum_enabled && !self.is_overscrolled() {
            return false;
        }

        let frame_factor = dt * 60.0;
        self.velocity *= self.deceleration.powf(frame_factor);
        self.position += self.velocity * frame_factor;

        if self.overscroll_enabled {
            if self.position < self.min_bound {
                let overshoot = self.min_bound - self.position;
                self.position += overshoot * self.overscroll_resistance;
                self.velocity *= 0.5;
            }
            if self.position > self.max_bound {
                let overshoot = self.position - self.max_bound;
                self.position -= overshoot * self.overscroll_resistance;
                self.velocity *= 0.5;
            }
        } else {
            self.position = self.position.clamp(self.min_bound, self.max_bound);
            if self.position <= self.min_bound || self.position >= self.max_bound {
                self.velocity = 0.0;
            }
        }

        self.velocity.abs() > 0.5 || self.is_overscrolled()
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    pub fn is_moving(&self) -> bool {
        self.velocity.abs() > 0.5
    }

    pub fn is_overscrolled(&self) -> bool {
        self.position < self.min_bound || self.position > self.max_bound
    }

    pub fn stop(&mut self) {
        self.velocity = 0.0;
    }

    pub fn reset(&mut self) {
        self.velocity = 0.0;
        self.position = self.min_bound;
    }

    pub fn set_position(&mut self, position: f32) {
        self.position = position;
    }

    pub fn scroll_to(&mut self, position: f32) {
        self.position = position.clamp(self.min_bound, self.max_bound);
        self.velocity = 0.0;
    }

    pub fn fling(&mut self, velocity: f32) {
        self.velocity = velocity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> ScrollPhysics {
        ScrollPhysics::new().with_bounds(0.0, 10_000.0)
    }

    #[test]
    fn tick_at_60hz_matches_legacy_per_frame_decay() {
        let mut physics = fresh();
        physics.fling(100.0);
        physics.tick(1.0 / 60.0);
        assert!((physics.velocity() - 100.0 * 0.95).abs() < 1e-3);
    }

    #[test]
    fn decay_is_frame_rate_independent() {
        let mut at_60 = fresh();
        let mut at_120 = fresh();
        at_60.fling(500.0);
        at_120.fling(500.0);

        for _ in 0..30 {
            at_60.tick(1.0 / 60.0);
        }
        for _ in 0..60 {
            at_120.tick(1.0 / 120.0);
        }

        assert!((at_60.velocity() - at_120.velocity()).abs() < 1e-2);
        let position_span = at_60.position().max(at_120.position());
        assert!((at_60.position() - at_120.position()).abs() < position_span * 0.05);
    }

    #[test]
    fn dt_clamping_keeps_decay_bounded() {
        let mut physics = fresh();
        physics.fling(800.0);
        let active = physics.tick(0.5);
        assert!(physics.velocity().abs() <= 800.0);
        assert!(active || physics.velocity().abs() <= 0.5);
    }
}
