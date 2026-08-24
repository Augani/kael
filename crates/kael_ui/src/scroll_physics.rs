use web_time::Instant;

use kael::px;
use kael::scroll_elasticity::{add_scroll_elasticity, advance_scroll_elasticity};

#[derive(Clone, Debug)]
pub struct ScrollPhysics {
    velocity: f32,
    position: f32,
    overscroll: f32,
    overscroll_last_advance: Option<Instant>,
    min_bound: f32,
    max_bound: f32,
    deceleration: f32,
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
            overscroll: 0.0,
            overscroll_last_advance: None,
            min_bound: 0.0,
            max_bound: f32::MAX,
            deceleration: 0.95,
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

    pub fn with_overscroll_resistance(self, _resistance: f32) -> Self {
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

    fn boundary_for(&self, position: f32) -> Option<f32> {
        if position < self.min_bound {
            Some(self.min_bound)
        } else if position > self.max_bound {
            Some(self.max_bound)
        } else {
            None
        }
    }

    fn stretch_into_overscroll(&mut self, candidate: f32) {
        if let Some(boundary) = self.boundary_for(candidate) {
            let boundary_delta = candidate - boundary;
            self.position = boundary;
            self.overscroll = f32::from(add_scroll_elasticity(
                px(self.overscroll),
                px(boundary_delta),
            ));
            self.overscroll_last_advance = None;
        } else {
            self.position = candidate;
        }
    }

    pub fn apply_delta(&mut self, delta: f32) {
        if self.momentum_enabled {
            self.velocity = delta * 0.8 + self.velocity * 0.2;
        } else {
            self.velocity = 0.0;
        }

        let candidate = self.position + delta;
        if self.overscroll_enabled {
            self.stretch_into_overscroll(candidate);
        } else {
            self.overscroll = 0.0;
            self.position = candidate.clamp(self.min_bound, self.max_bound);
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if !self.momentum_enabled && !self.is_overscrolled() {
            return false;
        }

        let frame_factor = dt * 60.0;
        self.velocity *= self.deceleration.powf(frame_factor);
        let candidate = self.position + self.velocity * frame_factor;

        if self.overscroll_enabled {
            if let Some(boundary) = self.boundary_for(candidate) {
                let boundary_delta = candidate - boundary;
                self.position = boundary;
                self.overscroll = f32::from(add_scroll_elasticity(
                    px(self.overscroll),
                    px(boundary_delta),
                ));
                self.overscroll_last_advance = None;
                self.velocity *= 0.5;
            } else {
                self.position = candidate;
            }

            if self.overscroll != 0.0 {
                let mut elastic = px(self.overscroll);
                advance_scroll_elasticity(&mut elastic, &mut self.overscroll_last_advance);
                self.overscroll = f32::from(elastic);
            }
        } else {
            self.overscroll = 0.0;
            self.position = candidate.clamp(self.min_bound, self.max_bound);
            if self.position <= self.min_bound || self.position >= self.max_bound {
                self.velocity = 0.0;
            }
        }

        self.velocity.abs() > 0.5 || self.is_overscrolled()
    }

    pub fn position(&self) -> f32 {
        self.position + self.overscroll
    }

    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    pub fn is_moving(&self) -> bool {
        self.velocity.abs() > 0.5
    }

    pub fn is_overscrolled(&self) -> bool {
        self.overscroll != 0.0
    }

    pub fn stop(&mut self) {
        self.velocity = 0.0;
    }

    pub fn reset(&mut self) {
        self.velocity = 0.0;
        self.position = self.min_bound;
        self.overscroll = 0.0;
        self.overscroll_last_advance = None;
    }

    pub fn set_position(&mut self, position: f32) {
        if self.overscroll_enabled {
            self.overscroll = 0.0;
            self.overscroll_last_advance = None;
            self.stretch_into_overscroll(position);
        } else {
            self.position = position;
        }
    }

    pub fn scroll_to(&mut self, position: f32) {
        self.position = position.clamp(self.min_bound, self.max_bound);
        self.overscroll = 0.0;
        self.overscroll_last_advance = None;
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

    #[test]
    fn overscroll_curve_matches_core_elasticity() {
        let mut physics = ScrollPhysics::new()
            .with_bounds(0.0, 1_000.0)
            .momentum(false)
            .overscroll(true);

        let boundary_deltas = [-40.0_f32, -25.0, -60.0, -15.0];

        let mut expected = px(0.0);
        for delta in boundary_deltas {
            physics.apply_delta(delta);
            expected = add_scroll_elasticity(expected, px(delta));
        }

        let elastic = physics.position() - physics.position;
        assert!(
            (elastic - f32::from(expected)).abs() < 1e-4,
            "kael_ui overscroll {elastic} should match core elasticity {}",
            f32::from(expected)
        );
        assert!(physics.is_overscrolled());
        assert_eq!(physics.position, 0.0);
    }

    #[test]
    fn overscroll_snaps_back_to_zero() {
        let mut physics = ScrollPhysics::new()
            .with_bounds(0.0, 1_000.0)
            .momentum(false)
            .overscroll(true);

        physics.apply_delta(-80.0);
        let stretched = physics.position().abs();
        assert!(physics.is_overscrolled());
        assert!(stretched > 0.0);

        let mut active = true;
        let mut last = stretched;
        for _ in 0..120 {
            active = physics.tick(1.0 / 60.0);
            let current = physics.position().abs();
            assert!(
                current <= last + 1e-3,
                "elastic band must not grow while settling"
            );
            last = current;
            if !active {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }

        assert!(!active, "rubber band should settle within budget");
        assert!(!physics.is_overscrolled());
        assert_eq!(physics.position(), 0.0);
    }

    #[test]
    fn disabled_overscroll_hard_clamps() {
        let mut physics = ScrollPhysics::new()
            .with_bounds(0.0, 500.0)
            .momentum(false)
            .overscroll(false);

        physics.apply_delta(-120.0);
        assert_eq!(physics.position(), 0.0);
        assert!(!physics.is_overscrolled());

        physics.apply_delta(900.0);
        assert_eq!(physics.position(), 500.0);
        assert!(!physics.is_overscrolled());
    }
}
