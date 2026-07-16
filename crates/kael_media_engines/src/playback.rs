//! Frame-accurate playback clock, seek resolution, and the A/V-sync contract (P0-G).
//!
//! This is pure timing logic with no decoder attached. It defines the contract a
//! hardware decoder plugs into:
//!
//! * [`Timebase`] maps between wall-clock time and integer frame indices using an
//!   exact rational frame rate (so 29.97 fps does not drift).
//! * [`resolve_seek`] turns a target frame into the keyframe a decoder must start
//!   from plus the number of frames to discard — the essence of a frame-accurate seek.
//! * [`AvSync`] decides whether a decoded frame is presented, dropped, or held
//!   against the master clock (audio is the master in this engine).
//! * [`PlaybackClock`] is the play/pause/seek/rate transport, driven by an external
//!   master time source such as the audio device clock.

use std::time::Duration;

const NANOS_PER_SEC: u128 = 1_000_000_000;

fn duration_from_nanos_saturating(nanos: u128) -> Duration {
    if nanos >= Duration::MAX.as_nanos() {
        return Duration::MAX;
    }
    Duration::new(
        (nanos / NANOS_PER_SEC) as u64,
        (nanos % NANOS_PER_SEC) as u32,
    )
}

fn scale_duration_saturating(duration: Duration, factor: f64) -> Duration {
    let seconds = duration.as_secs_f64() * factor;
    if !seconds.is_finite() {
        return if seconds.is_sign_positive() {
            Duration::MAX
        } else {
            Duration::ZERO
        };
    }
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// A video frame rate expressed as an exact rational `num/den` frames per second.
///
/// Broadcast rates are not whole numbers: 29.97 fps is exactly 30000/1001 and
/// 23.976 is 24000/1001. Carrying these as `f64` accumulates a full frame of drift
/// within minutes, so the timebase keeps the rational form and reduces it on
/// construction (making structural equality meaningful).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timebase {
    num: u32,
    den: u32,
}

impl Timebase {
    /// 24 fps — cinema.
    pub const FILM: Timebase = Timebase { num: 24, den: 1 };
    /// 25 fps — PAL.
    pub const PAL: Timebase = Timebase { num: 25, den: 1 };
    /// 30 fps — integer NTSC-ish.
    pub const THIRTY: Timebase = Timebase { num: 30, den: 1 };
    /// 23.976 fps — NTSC film pulldown (24000/1001).
    pub const NTSC_FILM: Timebase = Timebase {
        num: 24000,
        den: 1001,
    };
    /// 29.97 fps — NTSC (30000/1001).
    pub const NTSC: Timebase = Timebase {
        num: 30000,
        den: 1001,
    };
    /// 59.94 fps — NTSC double-rate (60000/1001).
    pub const NTSC_60: Timebase = Timebase {
        num: 60000,
        den: 1001,
    };

    /// Construct a reduced timebase from a rational frame rate.
    ///
    /// Returns `None` if either component is zero.
    pub fn new(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let divisor = gcd(num, den);
        Some(Self {
            num: num / divisor,
            den: den / divisor,
        })
    }

    /// Best-effort rational from a decimal fps, snapping the common broadcast
    /// rates to their exact rational form (so `29.97` becomes 30000/1001).
    ///
    /// Returns `None` for non-finite or non-positive input.
    pub fn from_fps(fps: f64) -> Option<Self> {
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        const EPS: f64 = 0.01;
        let candidates = [
            Timebase::NTSC_FILM,
            Timebase::NTSC,
            Timebase::NTSC_60,
            Timebase::FILM,
            Timebase::PAL,
            Timebase::THIRTY,
            Timebase { num: 50, den: 1 },
            Timebase { num: 60, den: 1 },
        ];
        for candidate in candidates {
            if (candidate.fps() - fps).abs() < EPS {
                return Some(candidate);
            }
        }
        let numerator = (fps * 1000.0).round();
        if !(1.0..=f64::from(u32::MAX)).contains(&numerator) {
            return None;
        }
        Timebase::new(numerator as u32, 1000)
    }

    /// The numerator of the reduced rational rate.
    pub fn numerator(&self) -> u32 {
        self.num
    }

    /// The denominator of the reduced rational rate.
    pub fn denominator(&self) -> u32 {
        self.den
    }

    /// The frame rate as a decimal, for display only.
    pub fn fps(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Presentation time of frame `index` — the first integer nanosecond at which
    /// the frame is on screen (`ceil(index * den / num)` seconds), so that
    /// [`Timebase::time_to_frame`] of this value round-trips back to `index`.
    pub fn frame_to_time(&self, index: u64) -> Duration {
        let numerator = index as u128 * self.den as u128 * NANOS_PER_SEC;
        let nanos = numerator.div_ceil(self.num as u128);
        duration_from_nanos_saturating(nanos)
    }

    /// Index of the frame visible at `time` — the frame whose presentation
    /// interval contains `time`, i.e. `floor(time * fps)`.
    pub fn time_to_frame(&self, time: Duration) -> u64 {
        let nanos = time.as_nanos();
        let frame = (nanos * self.num as u128) / (self.den as u128 * NANOS_PER_SEC);
        frame.min(u128::from(u64::MAX)) as u64
    }

    /// Duration a single frame is on screen.
    pub fn frame_duration(&self) -> Duration {
        let nanos = (self.den as u128 * NANOS_PER_SEC) / self.num as u128;
        Duration::from_nanos(nanos.max(1) as u64)
    }
}

/// A resolved frame-accurate seek.
///
/// To land on [`SeekPlan::target`], a decoder seeks to [`SeekPlan::keyframe`] (the
/// nearest sync sample at or before the target) and decodes forward, discarding
/// [`SeekPlan::discard`] frames before presenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeekPlan {
    /// The frame the user asked to land on.
    pub target: u64,
    /// The keyframe (sync sample) a decoder must start decoding from.
    pub keyframe: u64,
    /// How many decoded frames to discard between the keyframe and the target.
    pub discard: u64,
}

/// Resolve a frame-accurate seek against a list of keyframe indices.
///
/// Input order does not matter. When no keyframe lies at or before the target
/// (or the list is empty) the plan starts from frame 0, matching the convention
/// that the first frame of a stream is always a sync sample.
pub fn resolve_seek(target: u64, keyframes: &[u64]) -> SeekPlan {
    let keyframe = keyframes
        .iter()
        .copied()
        .filter(|&keyframe| keyframe <= target)
        .max()
        .unwrap_or(0);
    SeekPlan {
        target,
        keyframe,
        discard: target.saturating_sub(keyframe),
    }
}

/// What to do with a decoded video frame relative to the master clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDecision {
    /// Present the frame now; it is within tolerance of the master clock.
    Present,
    /// The frame is late by this much — drop it and advance to the next.
    Drop(Duration),
    /// The frame is early by this much — hold it until the clock catches up.
    Hold(Duration),
}

/// The A/V-sync contract: compares a frame's presentation time to the master clock.
///
/// Audio is the master clock; video frames are presented, dropped, or held to track
/// it. Frames within `tolerance` of the clock are presented immediately.
#[derive(Debug, Clone, Copy)]
pub struct AvSync {
    /// Frames within `±tolerance` of the master clock are presented immediately.
    pub tolerance: Duration,
}

impl AvSync {
    /// Construct a sync policy with an explicit tolerance window.
    pub fn new(tolerance: Duration) -> Self {
        Self { tolerance }
    }

    /// A policy tolerant to half a frame of jitter at the given timebase.
    pub fn for_timebase(timebase: Timebase) -> Self {
        Self {
            tolerance: timebase.frame_duration() / 2,
        }
    }

    /// Decide what to do with a frame whose presentation time is `frame_pts`
    /// given the current `master_clock` position.
    pub fn decide(&self, frame_pts: Duration, master_clock: Duration) -> SyncDecision {
        if frame_pts >= master_clock {
            let early = frame_pts - master_clock;
            if early > self.tolerance {
                SyncDecision::Hold(early)
            } else {
                SyncDecision::Present
            }
        } else {
            let late = master_clock - frame_pts;
            if late > self.tolerance {
                SyncDecision::Drop(late)
            } else {
                SyncDecision::Present
            }
        }
    }
}

/// A play/pause/seek transport whose position is driven by an external master time
/// source (e.g. the audio device clock) scaled by playback [`PlaybackClock::rate`].
///
/// The transport stores an anchor pair `(media_position, master_time)`; while
/// playing, the media position advances as `(master_now - anchor_master) * rate`.
/// Pausing freezes the position, seeking moves it, and changing rate re-anchors so
/// the position stays continuous. Playback is forward-only (`rate >= 0`).
#[derive(Debug, Clone)]
pub struct PlaybackClock {
    timebase: Timebase,
    rate: f64,
    playing: bool,
    anchor_media: Duration,
    anchor_master: Option<Duration>,
}

impl PlaybackClock {
    /// Create a paused transport at position zero.
    pub fn new(timebase: Timebase) -> Self {
        Self {
            timebase,
            rate: 1.0,
            playing: false,
            anchor_media: Duration::ZERO,
            anchor_master: None,
        }
    }

    /// The timebase frames are counted in.
    pub fn timebase(&self) -> Timebase {
        self.timebase
    }

    /// The current playback rate (`1.0` is real time).
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Whether the transport is currently advancing.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Start advancing from the current position, anchored to `master_now`.
    pub fn play(&mut self, master_now: Duration) {
        if !self.playing {
            self.anchor_media = self.position(master_now);
            self.playing = true;
            self.anchor_master = Some(master_now);
        }
    }

    /// Freeze the transport at its current position.
    pub fn pause(&mut self, master_now: Duration) {
        if self.playing {
            self.anchor_media = self.position(master_now);
            self.playing = false;
            self.anchor_master = None;
        }
    }

    /// Jump to `position`, keeping play/pause state. Re-anchors to `master_now`.
    pub fn seek(&mut self, position: Duration, master_now: Duration) {
        self.anchor_media = position;
        if self.playing {
            self.anchor_master = Some(master_now);
        }
    }

    /// Set the playback rate, re-anchoring so the position is continuous. Negative
    /// rates are clamped to zero (forward-only transport).
    pub fn set_rate(&mut self, rate: f64, master_now: Duration) {
        self.anchor_media = self.position(master_now);
        if self.playing {
            self.anchor_master = Some(master_now);
        }
        self.rate = if rate.is_finite() { rate.max(0.0) } else { 0.0 };
    }

    /// The media-time position given the master clock's current value.
    pub fn position(&self, master_now: Duration) -> Duration {
        match (self.playing, self.anchor_master) {
            (true, Some(anchor)) => {
                let elapsed = master_now.saturating_sub(anchor);
                self.anchor_media
                    .saturating_add(scale_duration_saturating(elapsed, self.rate))
            }
            _ => self.anchor_media,
        }
    }

    /// The frame index visible given the master clock's current value.
    pub fn current_frame(&self, master_now: Duration) -> u64 {
        self.timebase.time_to_frame(self.position(master_now))
    }

    /// The frame visible when looping over `loop_region` — the position wrapped into the
    /// region (for looped review playback).
    pub fn current_frame_looped(&self, master_now: Duration, loop_region: &LoopRegion) -> u64 {
        self.timebase
            .time_to_frame(loop_region.wrap(self.position(master_now)))
    }
}

/// A half-open `[start, end)` loop region in media time, for looped review playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopRegion {
    /// Loop start (inclusive).
    pub start: Duration,
    /// Loop end (exclusive).
    pub end: Duration,
}

impl LoopRegion {
    /// Create a loop region, normalizing so `start <= end`.
    pub fn new(start: Duration, end: Duration) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// The loop length.
    pub fn length(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Wrap `position` into the region: positions before `start` pass through; positions at
    /// or past `end` fold back from `start` by the loop length. A zero-length region pins to
    /// `start`.
    pub fn wrap(&self, position: Duration) -> Duration {
        let length = self.length();
        if length.is_zero() {
            return self.start;
        }
        if position < self.end {
            return position;
        }
        let offset_nanos = (position - self.start).as_nanos() % length.as_nanos();
        self.start
            .saturating_add(duration_from_nanos_saturating(offset_nanos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn from_fps_snaps_broadcast_rates_to_exact_rationals() {
        assert_eq!(Timebase::from_fps(29.97), Some(Timebase::NTSC));
        assert_eq!(Timebase::from_fps(23.976), Some(Timebase::NTSC_FILM));
        assert_eq!(Timebase::from_fps(59.94), Some(Timebase::NTSC_60));
        assert_eq!(Timebase::from_fps(24.0), Some(Timebase::FILM));
        assert_eq!(Timebase::from_fps(25.0), Some(Timebase::PAL));
        assert_eq!(Timebase::from_fps(0.0), None);
        assert_eq!(Timebase::from_fps(f64::NAN), None);
        assert_eq!(Timebase::from_fps(0.000_1), None);
        assert_eq!(Timebase::from_fps(f64::from(u32::MAX)), None);
    }

    #[test]
    fn new_reduces_rational_so_equality_is_structural() {
        assert_eq!(Timebase::new(60, 2), Timebase::new(30, 1));
        assert_eq!(Timebase::new(30000, 1001), Some(Timebase::NTSC));
        assert_eq!(Timebase::new(0, 1), None);
        assert_eq!(Timebase::new(1, 0), None);
    }

    #[test]
    fn ntsc_rational_does_not_drift_and_differs_from_integer_30() {
        let ntsc = Timebase::NTSC;
        // ~1 nominal hour at 29.97 fps.
        let frames = 107_892u64;
        let time = ntsc.frame_to_time(frames);
        // Round-trips exactly even after ~100k frames — no accumulated drift.
        assert_eq!(ntsc.time_to_frame(time), frames);
        // ~3599.996s: just under a wall-clock hour.
        assert!(time > Duration::from_millis(3_599_000));
        assert!(time < Duration::from_secs(3600));
        // The exact rational lands later than naive 30 fps would — the reason the
        // rational form exists instead of carrying 29.97 as a float.
        let naive = Timebase::THIRTY.frame_to_time(frames);
        assert!(time > naive);
    }

    #[test]
    fn time_to_frame_uses_floor_semantics() {
        let tb = Timebase::THIRTY;
        let frame_two_start = tb.frame_to_time(2);
        assert_eq!(tb.time_to_frame(frame_two_start), 2);
        // One nanosecond before frame 2 begins is still frame 1.
        let just_before = frame_two_start - Duration::from_nanos(1);
        assert_eq!(tb.time_to_frame(just_before), 1);
    }

    #[test]
    fn resolve_seek_starts_at_preceding_keyframe() {
        let keyframes = [0u64, 30, 60, 90];
        assert_eq!(
            resolve_seek(75, &keyframes),
            SeekPlan {
                target: 75,
                keyframe: 60,
                discard: 15
            }
        );
        assert_eq!(
            resolve_seek(30, &keyframes),
            SeekPlan {
                target: 30,
                keyframe: 30,
                discard: 0
            }
        );
        assert_eq!(
            resolve_seek(5, &keyframes),
            SeekPlan {
                target: 5,
                keyframe: 0,
                discard: 5
            }
        );
    }

    #[test]
    fn resolve_seek_handles_empty_keyframe_list() {
        assert_eq!(
            resolve_seek(42, &[]),
            SeekPlan {
                target: 42,
                keyframe: 0,
                discard: 42
            }
        );
        assert_eq!(resolve_seek(75, &[90, 30, 60, 0]).keyframe, 60);
    }

    #[test]
    fn av_sync_presents_drops_and_holds() {
        let sync = AvSync::new(ms(10));
        assert_eq!(sync.decide(ms(100), ms(100)), SyncDecision::Present);
        assert_eq!(sync.decide(ms(105), ms(100)), SyncDecision::Present);
        assert_eq!(sync.decide(ms(95), ms(100)), SyncDecision::Present);
        assert_eq!(sync.decide(ms(80), ms(100)), SyncDecision::Drop(ms(20)));
        assert_eq!(sync.decide(ms(130), ms(100)), SyncDecision::Hold(ms(30)));
    }

    #[test]
    fn av_sync_tolerance_tracks_timebase() {
        let sync = AvSync::for_timebase(Timebase::THIRTY);
        // Half a 1/30s frame is ~16.6ms.
        assert_eq!(sync.tolerance, Timebase::THIRTY.frame_duration() / 2);
    }

    #[test]
    fn playback_clock_advances_only_while_playing() {
        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        assert_eq!(clock.position(ms(0)), Duration::ZERO);
        // Paused: master time advancing does not move media position.
        assert_eq!(clock.position(ms(500)), Duration::ZERO);

        clock.play(ms(1000));
        assert_eq!(clock.position(ms(1000)), Duration::ZERO);
        assert_eq!(clock.position(ms(1500)), ms(500));
        assert_eq!(clock.current_frame(ms(1500)), 15);

        clock.pause(ms(1500));
        // Frozen at 500ms even as the master clock keeps moving.
        assert_eq!(clock.position(ms(9999)), ms(500));
    }

    #[test]
    fn playback_clock_rate_scales_media_time() {
        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        clock.set_rate(2.0, ms(0));
        clock.play(ms(0));
        // At 2x, 500ms of wall time is 1000ms of media time.
        assert_eq!(clock.position(ms(500)), ms(1000));

        // Changing rate mid-play keeps the position continuous.
        clock.set_rate(0.5, ms(500));
        assert_eq!(clock.position(ms(500)), ms(1000));
        assert_eq!(clock.position(ms(1500)), ms(1500));
    }

    #[test]
    fn playback_clock_seek_moves_position_in_both_states() {
        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        clock.seek(ms(2000), ms(0));
        assert_eq!(clock.position(ms(0)), ms(2000));
        assert_eq!(clock.current_frame(ms(0)), 60);

        clock.play(ms(0));
        clock.seek(ms(4000), ms(1000));
        assert_eq!(clock.position(ms(1000)), ms(4000));
        assert_eq!(clock.position(ms(1500)), ms(4500));
    }

    #[test]
    fn negative_rate_is_clamped_to_zero() {
        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        clock.set_rate(-2.0, ms(0));
        assert_eq!(clock.rate(), 0.0);
        clock.play(ms(0));
        assert_eq!(clock.position(ms(1000)), Duration::ZERO);
    }

    #[test]
    fn loop_region_wraps_position() {
        let region = LoopRegion::new(ms(1000), ms(3000));
        assert_eq!(region.length(), ms(2000));
        // Before and within the region pass through.
        assert_eq!(region.wrap(ms(500)), ms(500));
        assert_eq!(region.wrap(ms(2000)), ms(2000));
        // At/after the end fold back from the start.
        assert_eq!(region.wrap(ms(3000)), ms(1000));
        assert_eq!(region.wrap(ms(4000)), ms(2000));
        assert_eq!(region.wrap(ms(5000)), ms(1000));
        // Reversed arguments normalize; a zero-length region pins to start.
        assert_eq!(LoopRegion::new(ms(3000), ms(1000)), region);
        assert_eq!(LoopRegion::new(ms(2000), ms(2000)).wrap(ms(5000)), ms(2000));
    }

    #[test]
    fn playback_clock_loops_frames_within_region() {
        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        clock.play(ms(0));
        let region = LoopRegion::new(ms(0), ms(1000)); // 30 frames @ 30 fps
        // 1.5s media -> looped 0.5s -> frame 15.
        assert_eq!(clock.current_frame_looped(ms(1500), &region), 15);
    }

    #[test]
    fn extreme_timing_values_saturate_without_panicking() {
        let slow = Timebase::new(1, u32::MAX).unwrap();
        assert_eq!(slow.frame_to_time(u64::MAX), Duration::MAX);

        let fast = Timebase::new(u32::MAX, 1).unwrap();
        assert_eq!(fast.time_to_frame(Duration::MAX), u64::MAX);
        assert_eq!(fast.frame_duration(), Duration::from_nanos(1));

        let mut clock = PlaybackClock::new(Timebase::THIRTY);
        clock.seek(Duration::MAX, Duration::ZERO);
        clock.set_rate(f64::MAX, Duration::ZERO);
        clock.play(Duration::ZERO);
        assert_eq!(clock.position(Duration::MAX), Duration::MAX);
    }

    #[test]
    fn long_loop_regions_preserve_offsets_beyond_u64_nanoseconds() {
        let region = LoopRegion::new(Duration::ZERO, Duration::from_secs(1_000_000_000_000));
        let position = Duration::from_secs(1_500_000_000_000);
        assert_eq!(region.wrap(position), Duration::from_secs(500_000_000_000));
    }
}
