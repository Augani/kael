//! Deterministic fixed-timestep scheduling for simulations and real-time applications.
//!
//! [`FixedFrameClock`] separates variable display-frame timing from fixed simulation
//! updates. A frame may run zero or more fixed updates, then render between the latest two
//! simulation states using [`FrameAdvance::interpolation_alpha`]. Long stalls are clamped,
//! and catch-up work is bounded so a slow frame cannot trigger an unbounded spiral of death.
//!
//! Use [`FixedFrameClock::advance_by`] when a replay, test harness, or host runtime already
//! provides frame deltas. [`FixedFrameClock::tick`] uses a browser-compatible monotonic clock
//! for ordinary real-time use.
//!
//! A Kael view can drive the clock and schedule its next display frame like this:
//!
//! ```rust,ignore
//! use kael::Window;
//! use kael_engines::game_loop::FixedFrameClock;
//!
//! struct Game {
//!     clock: FixedFrameClock,
//!     previous_x: f32,
//!     current_x: f32,
//! }
//!
//! fn render_game(game: &mut Game, window: &mut Window) {
//!     let frame = game.clock.tick();
//!     for _ in frame.updates() {
//!         game.previous_x = game.current_x;
//!         game.current_x += 120.0 * frame.fixed_timestep().as_secs_f32();
//!     }
//!
//!     let alpha = frame.interpolation_alpha() as f32;
//!     let render_x = game.previous_x + (game.current_x - game.previous_x) * alpha;
//!     draw_game_at(render_x);
//!
//!     if !game.clock.is_paused() {
//!         window.request_animation_frame();
//!     }
//! }
//! ```

use std::{fmt, ops::Range, time::Duration};

use web_time::Instant;

/// Default fixed simulation frequency.
pub const DEFAULT_UPDATES_PER_SECOND: u32 = 60;

/// Configuration for a [`FixedFrameClock`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedFrameClockConfig {
    /// Duration advanced by every deterministic simulation update.
    pub fixed_timestep: Duration,
    /// Maximum wall-clock delta accepted from one display frame.
    ///
    /// Time beyond this limit is reported as dropped rather than fed into the accumulator.
    pub max_frame_delta: Duration,
    /// Maximum fixed updates returned for one display frame.
    ///
    /// Whole updates beyond this limit are dropped while the fractional remainder is retained
    /// for interpolation.
    pub max_catch_up_steps: u32,
}

impl FixedFrameClockConfig {
    /// Construct the default bounded configuration for `updates_per_second`.
    ///
    /// The default frame-delta clamp and catch-up limit remain 250 milliseconds and eight
    /// updates. A zero rate or a rate too high to produce a nonzero [`Duration`] is rejected.
    pub fn from_updates_per_second(updates_per_second: u32) -> Result<Self, FrameClockConfigError> {
        if updates_per_second == 0 {
            return Err(FrameClockConfigError::ZeroUpdateRate);
        }
        let fixed_timestep = Duration::try_from_secs_f64(1.0 / f64::from(updates_per_second))
            .map_err(|_| FrameClockConfigError::ZeroFixedTimestep)?;
        if fixed_timestep.is_zero() {
            return Err(FrameClockConfigError::ZeroFixedTimestep);
        }
        Ok(Self {
            fixed_timestep,
            ..Self::default()
        })
    }

    /// Set the maximum delta accepted from one display frame.
    ///
    /// [`FixedFrameClock::new`] validates the resulting configuration.
    pub fn with_max_frame_delta(mut self, max_frame_delta: Duration) -> Self {
        self.max_frame_delta = max_frame_delta;
        self
    }

    /// Set the maximum number of catch-up updates returned for one display frame.
    ///
    /// [`FixedFrameClock::new`] validates the resulting configuration.
    pub fn with_max_catch_up_steps(mut self, max_catch_up_steps: u32) -> Self {
        self.max_catch_up_steps = max_catch_up_steps;
        self
    }

    /// Validate that all limits can make forward progress.
    pub fn validate(&self) -> Result<(), FrameClockConfigError> {
        if self.fixed_timestep.is_zero() {
            return Err(FrameClockConfigError::ZeroFixedTimestep);
        }
        if self.max_frame_delta.is_zero() {
            return Err(FrameClockConfigError::ZeroMaxFrameDelta);
        }
        if self.max_catch_up_steps == 0 {
            return Err(FrameClockConfigError::ZeroMaxCatchUpSteps);
        }
        Ok(())
    }
}

impl Default for FixedFrameClockConfig {
    fn default() -> Self {
        Self {
            // Rounded to the nearest nanosecond, matching Duration::from_secs_f64(1.0 / 60.0).
            fixed_timestep: Duration::from_nanos(16_666_667),
            max_frame_delta: Duration::from_millis(250),
            max_catch_up_steps: 8,
        }
    }
}

/// Invalid fixed-frame clock configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameClockConfigError {
    /// The requested update frequency was zero.
    ZeroUpdateRate,
    /// The fixed timestep was zero or the requested frequency rounded down to zero duration.
    ZeroFixedTimestep,
    /// The maximum accepted frame delta was zero.
    ZeroMaxFrameDelta,
    /// The maximum number of catch-up updates was zero.
    ZeroMaxCatchUpSteps,
}

impl fmt::Display for FrameClockConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroUpdateRate => {
                formatter.write_str("update frequency must be greater than zero")
            }
            Self::ZeroFixedTimestep => formatter.write_str("fixed timestep must be nonzero"),
            Self::ZeroMaxFrameDelta => formatter.write_str("maximum frame delta must be nonzero"),
            Self::ZeroMaxCatchUpSteps => {
                formatter.write_str("maximum catch-up steps must be greater than zero")
            }
        }
    }
}

impl std::error::Error for FrameClockConfigError {}

/// Work and interpolation produced for one display frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameAdvance {
    input_delta: Duration,
    clamped_delta: Duration,
    fixed_timestep: Duration,
    update_steps: u32,
    simulation_delta: Duration,
    interpolation_alpha: f64,
    clamped_away_time: Duration,
    catch_up_dropped_time: Duration,
    paused: bool,
}

impl FrameAdvance {
    /// Wall-clock delta supplied for this display frame.
    pub fn input_delta(&self) -> Duration {
        self.input_delta
    }

    /// Delta retained after applying the configured per-frame clamp.
    ///
    /// This is zero while the clock is paused. Some of this duration may still be reported by
    /// [`Self::catch_up_dropped_time`] when the catch-up step budget is exhausted.
    pub fn clamped_delta(&self) -> Duration {
        self.clamped_delta
    }

    /// Duration of one deterministic simulation update.
    pub fn fixed_timestep(&self) -> Duration {
        self.fixed_timestep
    }

    /// Number of fixed simulation updates to run before rendering this frame.
    pub fn update_steps(&self) -> u32 {
        self.update_steps
    }

    /// Iterator range for running this frame's fixed simulation updates.
    ///
    /// The index has no simulation meaning; it exists for ergonomic `for` loops.
    pub fn updates(&self) -> Range<u32> {
        0..self.update_steps
    }

    /// Total deterministic simulation time represented by [`Self::update_steps`].
    pub fn simulation_delta(&self) -> Duration {
        self.simulation_delta
    }

    /// Fractional position between the latest and next fixed simulation states.
    ///
    /// The value is finite and in the half-open range `0.0..1.0`. Renderers commonly blend
    /// the previous and current state with this value; deterministic simulation logic should
    /// use [`Self::fixed_timestep`] instead.
    pub fn interpolation_alpha(&self) -> f64 {
        self.interpolation_alpha
    }

    /// Input time discarded by the configured per-frame delta clamp.
    pub fn clamped_away_time(&self) -> Duration {
        self.clamped_away_time
    }

    /// Whole accumulated updates discarded after exhausting the catch-up step budget.
    pub fn catch_up_dropped_time(&self) -> Duration {
        self.catch_up_dropped_time
    }

    /// Total time discarded by clamping and catch-up protection for this frame.
    pub fn dropped_time(&self) -> Duration {
        self.clamped_away_time
            .saturating_add(self.catch_up_dropped_time)
    }

    /// Input time intentionally ignored because the clock was paused.
    ///
    /// Paused time is not included in [`Self::dropped_time`] or the clock's cumulative dropped
    /// time because it was intentionally excluded rather than lost to overload.
    pub fn ignored_time(&self) -> Duration {
        if self.paused {
            self.input_delta
        } else {
            Duration::ZERO
        }
    }

    /// Whether this frame was advanced while the clock was paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

/// Allocation-free fixed-timestep frame clock with bounded catch-up work.
///
/// The clock does not execute application code. Call [`Self::advance_by`] or [`Self::tick`],
/// run the returned number of fixed updates, render using the interpolation alpha, and request
/// another host animation frame while the simulation is active.
#[derive(Clone, Debug)]
pub struct FixedFrameClock {
    config: FixedFrameClockConfig,
    accumulator: Duration,
    simulation_time: Duration,
    total_dropped_time: Duration,
    completed_updates: u64,
    last_instant: Option<Instant>,
    paused: bool,
}

impl FixedFrameClock {
    /// Construct a clock after validating its fixed step and overload limits.
    pub fn new(config: FixedFrameClockConfig) -> Result<Self, FrameClockConfigError> {
        config.validate()?;
        Ok(Self::from_valid_config(config))
    }

    /// Return the active configuration.
    pub fn config(&self) -> FixedFrameClockConfig {
        self.config
    }

    /// Advance using the elapsed time from a browser-compatible monotonic clock.
    ///
    /// The first call establishes a time origin and returns no updates. Use [`Self::tick_at`]
    /// to inject a monotonic timestamp, or [`Self::advance_by`] when the elapsed duration is
    /// already known.
    pub fn tick(&mut self) -> FrameAdvance {
        self.tick_at(Instant::now())
    }

    /// Advance to an explicitly supplied monotonic timestamp.
    ///
    /// Timestamps earlier than the previous sample are treated as zero elapsed time and become
    /// the new origin. This keeps resettable virtual clocks and tests safe without allowing a
    /// negative simulation delta.
    pub fn tick_at(&mut self, now: Instant) -> FrameAdvance {
        let delta = self
            .last_instant
            .and_then(|last| now.checked_duration_since(last))
            .unwrap_or_default();
        self.last_instant = Some(now);
        self.advance_delta(delta)
    }

    /// Advance by an explicit display-frame delta.
    ///
    /// This is the deterministic entry point for replays and tests. Calling it clears the
    /// monotonic timestamp origin so a later [`Self::tick`] starts with a zero-delta anchor
    /// instead of double-counting elapsed time.
    pub fn advance_by(&mut self, frame_delta: Duration) -> FrameAdvance {
        self.last_instant = None;
        self.advance_delta(frame_delta)
    }

    /// Pause simulation advancement while preserving the fractional accumulator.
    ///
    /// Paused input is reported by [`FrameAdvance::ignored_time`] and is not counted as dropped.
    /// Resuming clears the monotonic time origin so time spent paused cannot leak into the next
    /// real-time frame.
    pub fn pause(&mut self) {
        if !self.paused {
            self.paused = true;
            self.last_instant = None;
        }
    }

    /// Resume simulation advancement without consuming time spent paused.
    pub fn resume(&mut self) {
        if self.paused {
            self.paused = false;
            self.last_instant = None;
        }
    }

    /// Set the paused state.
    pub fn set_paused(&mut self, paused: bool) {
        if paused {
            self.pause();
        } else {
            self.resume();
        }
    }

    /// Whether simulation advancement is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Reset accumulated time and cumulative metrics, and resume the clock.
    ///
    /// The validated configuration is retained. The next [`Self::tick`] establishes a new time
    /// origin and returns no updates.
    pub fn reset(&mut self) {
        *self = Self::from_valid_config(self.config);
    }

    /// Fractional unconsumed time retained for the next display frame.
    pub fn accumulated_time(&self) -> Duration {
        self.accumulator
    }

    /// Deterministic simulation time represented by all completed fixed updates.
    ///
    /// The value saturates at [`Duration::MAX`] after an impractically long run.
    pub fn simulation_time(&self) -> Duration {
        self.simulation_time
    }

    /// Number of fixed updates produced since construction or reset.
    ///
    /// The counter saturates at [`u64::MAX`].
    pub fn completed_updates(&self) -> u64 {
        self.completed_updates
    }

    /// Total overload time discarded by frame clamping and catch-up protection.
    ///
    /// The value saturates at [`Duration::MAX`]. Paused time is excluded.
    pub fn total_dropped_time(&self) -> Duration {
        self.total_dropped_time
    }

    fn from_valid_config(config: FixedFrameClockConfig) -> Self {
        Self {
            config,
            accumulator: Duration::ZERO,
            simulation_time: Duration::ZERO,
            total_dropped_time: Duration::ZERO,
            completed_updates: 0,
            last_instant: None,
            paused: false,
        }
    }

    fn advance_delta(&mut self, frame_delta: Duration) -> FrameAdvance {
        if self.paused {
            return FrameAdvance {
                input_delta: frame_delta,
                clamped_delta: Duration::ZERO,
                fixed_timestep: self.config.fixed_timestep,
                update_steps: 0,
                simulation_delta: Duration::ZERO,
                interpolation_alpha: self.interpolation_alpha(),
                clamped_away_time: Duration::ZERO,
                catch_up_dropped_time: Duration::ZERO,
                paused: true,
            };
        }

        let clamped_delta = frame_delta.min(self.config.max_frame_delta);
        let clamped_away_time = frame_delta.saturating_sub(clamped_delta);
        let fixed_nanos = self.config.fixed_timestep.as_nanos();
        // The sum of two valid Durations is far below u128::MAX even when it exceeds the
        // representable Duration range. Keeping the intermediate as nanoseconds lets overload
        // accounting remain exact before individual public Duration metrics saturate.
        let accumulated_nanos = self.accumulator.as_nanos() + clamped_delta.as_nanos();
        let available_steps = accumulated_nanos / fixed_nanos;
        let update_steps = available_steps
            .min(u128::from(self.config.max_catch_up_steps))
            .min(u128::from(u32::MAX)) as u32;
        let simulation_nanos = fixed_nanos * u128::from(update_steps);
        let after_updates_nanos = accumulated_nanos - simulation_nanos;
        let catch_up_dropped_nanos = (after_updates_nanos / fixed_nanos) * fixed_nanos;
        let retained_nanos = after_updates_nanos - catch_up_dropped_nanos;
        let simulation_delta = duration_from_nanos(simulation_nanos);
        let catch_up_dropped_time = duration_from_nanos(catch_up_dropped_nanos);

        self.accumulator = duration_from_nanos(retained_nanos);
        self.simulation_time = self.simulation_time.saturating_add(simulation_delta);
        self.completed_updates = self
            .completed_updates
            .saturating_add(u64::from(update_steps));
        let dropped_time = clamped_away_time.saturating_add(catch_up_dropped_time);
        self.total_dropped_time = self.total_dropped_time.saturating_add(dropped_time);

        FrameAdvance {
            input_delta: frame_delta,
            clamped_delta,
            fixed_timestep: self.config.fixed_timestep,
            update_steps,
            simulation_delta,
            interpolation_alpha: self.interpolation_alpha(),
            clamped_away_time,
            catch_up_dropped_time,
            paused: false,
        }
    }

    fn interpolation_alpha(&self) -> f64 {
        (self.accumulator.as_secs_f64() / self.config.fixed_timestep.as_secs_f64())
            .min(1.0 - f64::EPSILON)
    }
}

impl Default for FixedFrameClock {
    fn default() -> Self {
        Self::from_valid_config(FixedFrameClockConfig::default())
    }
}

fn duration_from_nanos(total_nanos: u128) -> Duration {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    if total_nanos >= Duration::MAX.as_nanos() {
        return Duration::MAX;
    }
    let seconds = total_nanos / NANOS_PER_SECOND;
    let subsec_nanos = (total_nanos % NANOS_PER_SECOND) as u32;
    Duration::new(seconds as u64, subsec_nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(fixed_ms: u64, max_delta_ms: u64, max_steps: u32) -> FixedFrameClock {
        FixedFrameClock::new(FixedFrameClockConfig {
            fixed_timestep: Duration::from_millis(fixed_ms),
            max_frame_delta: Duration::from_millis(max_delta_ms),
            max_catch_up_steps: max_steps,
        })
        .expect("test configuration is valid")
    }

    #[test]
    fn accumulates_fractional_frames_and_returns_fixed_updates() {
        let mut clock = clock(10, 100, 8);

        let first = clock.advance_by(Duration::from_millis(5));
        assert_eq!(first.update_steps(), 0);
        assert_eq!(first.interpolation_alpha(), 0.5);

        let second = clock.advance_by(Duration::from_millis(25));
        assert_eq!(second.updates().count(), 3);
        assert_eq!(second.simulation_delta(), Duration::from_millis(30));
        assert_eq!(second.interpolation_alpha(), 0.0);
        assert_eq!(clock.completed_updates(), 3);
        assert_eq!(clock.simulation_time(), Duration::from_millis(30));
    }

    #[test]
    fn clamps_long_display_frames_and_reports_discarded_time() {
        let mut clock = clock(10, 30, 8);
        let frame = clock.advance_by(Duration::from_millis(55));

        assert_eq!(frame.input_delta(), Duration::from_millis(55));
        assert_eq!(frame.clamped_delta(), Duration::from_millis(30));
        assert_eq!(frame.clamped_away_time(), Duration::from_millis(25));
        assert_eq!(frame.catch_up_dropped_time(), Duration::ZERO);
        assert_eq!(frame.dropped_time(), Duration::from_millis(25));
        assert_eq!(frame.update_steps(), 3);
        assert_eq!(clock.total_dropped_time(), Duration::from_millis(25));
    }

    #[test]
    fn bounds_catch_up_work_and_retains_only_fractional_time() {
        let mut clock = clock(10, 100, 3);
        let overloaded = clock.advance_by(Duration::from_millis(58));

        assert_eq!(overloaded.update_steps(), 3);
        assert_eq!(
            overloaded.catch_up_dropped_time(),
            Duration::from_millis(20)
        );
        assert_eq!(overloaded.interpolation_alpha(), 0.8);
        assert_eq!(clock.accumulated_time(), Duration::from_millis(8));

        let recovered = clock.advance_by(Duration::from_millis(2));
        assert_eq!(recovered.update_steps(), 1);
        assert_eq!(recovered.interpolation_alpha(), 0.0);
        assert_eq!(clock.total_dropped_time(), Duration::from_millis(20));
    }

    #[test]
    fn clamp_and_spiral_protection_report_separate_dropped_time() {
        let mut clock = clock(10, 100, 3);
        let frame = clock.advance_by(Duration::from_millis(500));

        assert_eq!(frame.clamped_away_time(), Duration::from_millis(400));
        assert_eq!(frame.catch_up_dropped_time(), Duration::from_millis(70));
        assert_eq!(frame.dropped_time(), Duration::from_millis(470));
        assert_eq!(clock.total_dropped_time(), Duration::from_millis(470));
    }

    #[test]
    fn pause_resume_and_reset_do_not_inject_wall_clock_time() {
        let mut clock = clock(10, 100, 8);
        clock.advance_by(Duration::from_millis(5));
        clock.pause();

        let paused = clock.advance_by(Duration::from_secs(1));
        assert!(paused.is_paused());
        assert_eq!(paused.ignored_time(), Duration::from_secs(1));
        assert_eq!(paused.dropped_time(), Duration::ZERO);
        assert_eq!(paused.interpolation_alpha(), 0.5);

        clock.resume();
        let resumed = clock.advance_by(Duration::from_millis(5));
        assert_eq!(resumed.update_steps(), 1);
        assert_eq!(clock.total_dropped_time(), Duration::ZERO);

        clock.reset();
        assert!(!clock.is_paused());
        assert_eq!(clock.accumulated_time(), Duration::ZERO);
        assert_eq!(clock.simulation_time(), Duration::ZERO);
        assert_eq!(clock.completed_updates(), 0);
        assert_eq!(clock.total_dropped_time(), Duration::ZERO);
    }

    #[test]
    fn tick_at_anchors_first_frame_and_reanchors_after_resume() {
        let mut clock = clock(10, 100, 8);
        let origin = Instant::now();

        assert_eq!(clock.tick_at(origin).update_steps(), 0);
        let frame = clock.tick_at(origin + Duration::from_millis(15));
        assert_eq!(frame.update_steps(), 1);
        assert_eq!(frame.interpolation_alpha(), 0.5);

        clock.pause();
        assert!(clock.tick_at(origin + Duration::from_secs(1)).is_paused());
        clock.resume();
        assert_eq!(
            clock.tick_at(origin + Duration::from_secs(2)).input_delta(),
            Duration::ZERO
        );
        assert_eq!(
            clock
                .tick_at(origin + Duration::from_secs(2) + Duration::from_millis(5))
                .update_steps(),
            1
        );
    }

    #[test]
    fn pause_and_resume_are_idempotent() {
        let mut clock = clock(10, 100, 8);
        let origin = Instant::now();
        clock.tick_at(origin);
        clock.tick_at(origin + Duration::from_millis(5));

        clock.resume();
        clock.set_paused(false);
        assert_eq!(
            clock
                .tick_at(origin + Duration::from_millis(10))
                .update_steps(),
            1
        );

        clock.pause();
        clock.pause();
        clock.set_paused(true);
        assert!(clock.is_paused());
    }

    #[test]
    fn explicit_delta_sequences_are_replay_deterministic() {
        let sequence = [1_u64, 17, 4, 33, 8, 250, 16, 16, 3, 90];
        let mut first = clock(16, 100, 4);
        let mut replay = clock(16, 100, 4);

        for milliseconds in sequence {
            let delta = Duration::from_millis(milliseconds);
            assert_eq!(first.advance_by(delta), replay.advance_by(delta));
        }
        assert_eq!(first.accumulated_time(), replay.accumulated_time());
        assert_eq!(first.simulation_time(), replay.simulation_time());
        assert_eq!(first.completed_updates(), replay.completed_updates());
        assert_eq!(first.total_dropped_time(), replay.total_dropped_time());
    }

    #[test]
    fn extreme_deltas_saturate_without_overflow() {
        let mut clock = FixedFrameClock::new(FixedFrameClockConfig {
            fixed_timestep: Duration::from_nanos(1),
            max_frame_delta: Duration::MAX,
            max_catch_up_steps: 1,
        })
        .expect("test configuration is valid");
        let frame = clock.advance_by(Duration::MAX);

        assert_eq!(frame.update_steps(), 1);
        assert_eq!(frame.interpolation_alpha(), 0.0);
        assert_eq!(
            frame.catch_up_dropped_time(),
            Duration::MAX - Duration::from_nanos(1)
        );

        let mut huge_step = FixedFrameClock::new(FixedFrameClockConfig {
            fixed_timestep: Duration::MAX,
            max_frame_delta: Duration::MAX,
            max_catch_up_steps: 1,
        })
        .expect("test configuration is valid");
        let fractional = huge_step.advance_by(Duration::MAX - Duration::from_nanos(1));
        assert!(fractional.interpolation_alpha() < 1.0);
        assert!(fractional.interpolation_alpha().is_finite());
    }

    #[test]
    fn rejects_configuration_that_cannot_advance() {
        let default = FixedFrameClockConfig::default();
        assert_eq!(
            FixedFrameClockConfig::from_updates_per_second(0),
            Err(FrameClockConfigError::ZeroUpdateRate)
        );
        assert_eq!(
            FixedFrameClock::new(FixedFrameClockConfig {
                fixed_timestep: Duration::ZERO,
                ..default
            })
            .unwrap_err(),
            FrameClockConfigError::ZeroFixedTimestep
        );
        assert_eq!(
            FixedFrameClock::new(FixedFrameClockConfig {
                max_frame_delta: Duration::ZERO,
                ..default
            })
            .unwrap_err(),
            FrameClockConfigError::ZeroMaxFrameDelta
        );
        assert_eq!(
            FixedFrameClock::new(FixedFrameClockConfig {
                max_catch_up_steps: 0,
                ..default
            })
            .unwrap_err(),
            FrameClockConfigError::ZeroMaxCatchUpSteps
        );
    }

    #[test]
    fn high_volume_stress_keeps_work_and_interpolation_bounded() {
        let mut clock = clock(8, 64, 4);
        let pattern = [1_u64, 7, 8, 9, 16, 33, 64, 250];

        for frame_index in 0..250_000 {
            let frame =
                clock.advance_by(Duration::from_millis(pattern[frame_index % pattern.len()]));
            assert!(frame.update_steps() <= 4);
            assert!(frame.interpolation_alpha().is_finite());
            assert!((0.0..1.0).contains(&frame.interpolation_alpha()));
            assert!(clock.accumulated_time() < clock.config().fixed_timestep);
        }
        assert!(clock.completed_updates() > 0);
        assert!(clock.total_dropped_time() > Duration::ZERO);
    }
}
