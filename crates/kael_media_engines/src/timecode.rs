//! SMPTE non-drop-frame timecode (`HH:MM:SS:FF`).
//!
//! Converts between an absolute frame number and a timecode at an integer frame rate, and
//! parses/formats the standard `HH:MM:SS:FF` string. Drop-frame timecode (for 29.97/59.94)
//! is intentionally out of scope here.

use std::fmt;

/// A SMPTE non-drop-frame timecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timecode {
    /// Hours.
    pub hours: u64,
    /// Minutes (`0..60`).
    pub minutes: u32,
    /// Seconds (`0..60`).
    pub seconds: u32,
    /// Frames (`0..fps`).
    pub frames: u32,
}

impl Timecode {
    /// The timecode for absolute `frame` at `fps` frames per second (`fps` clamped to ≥ 1).
    pub fn from_frame(frame: u64, fps: u32) -> Self {
        let fps = fps.max(1) as u64;
        let frames = (frame % fps) as u32;
        let total_seconds = frame / fps;
        Self {
            hours: total_seconds / 3600,
            minutes: ((total_seconds / 60) % 60) as u32,
            seconds: (total_seconds % 60) as u32,
            frames,
        }
    }

    /// The absolute frame number this timecode represents at `fps`.
    pub fn to_frame(&self, fps: u32) -> u64 {
        let fps = fps.max(1) as u64;
        self.hours
            .saturating_mul(60)
            .saturating_add(u64::from(self.minutes))
            .saturating_mul(60)
            .saturating_add(u64::from(self.seconds))
            .saturating_mul(fps)
            .saturating_add(u64::from(self.frames))
    }

    /// Parse an `HH:MM:SS:FF` timecode string. Returns `None` if it is not four
    /// colon-separated non-negative integers with valid minute/second fields.
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.trim().split(':').collect();
        if parts.len() != 4 {
            return None;
        }
        let hours = parts[0].parse().ok()?;
        let minutes = parts[1].parse().ok()?;
        let seconds = parts[2].parse().ok()?;
        let frames = parts[3].parse().ok()?;
        if minutes >= 60 || seconds >= 60 {
            return None;
        }
        Some(Self {
            hours,
            minutes,
            seconds,
            frames,
        })
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_frame_at_30fps() {
        assert_eq!(Timecode::from_frame(0, 30).to_string(), "00:00:00:00");
        assert_eq!(Timecode::from_frame(29, 30).to_string(), "00:00:00:29");
        assert_eq!(Timecode::from_frame(30, 30).to_string(), "00:00:01:00");
        assert_eq!(Timecode::from_frame(1800, 30).to_string(), "00:01:00:00");
        assert_eq!(Timecode::from_frame(108_000, 30).to_string(), "01:00:00:00");
    }

    #[test]
    fn from_frame_respects_frame_rate() {
        // One hour at 25 fps is 90000 frames.
        assert_eq!(Timecode::from_frame(90_000, 25).to_string(), "01:00:00:00");
        // Frame index 24 is the last frame of second 0 at 25 fps.
        assert_eq!(Timecode::from_frame(24, 25).to_string(), "00:00:00:24");
        assert_eq!(Timecode::from_frame(25, 25).to_string(), "00:00:01:00");
    }

    #[test]
    fn frame_round_trips_through_timecode() {
        for &fps in &[24u32, 25, 30] {
            for &frame in &[0u64, 1, 47, 1800, 108_000, 1_234_567] {
                let tc = Timecode::from_frame(frame, fps);
                assert_eq!(tc.to_frame(fps), frame, "fps {fps} frame {frame} -> {tc}");
            }
        }
        let max = Timecode::from_frame(u64::MAX, 1);
        assert_eq!(max.to_frame(1), u64::MAX);
    }

    #[test]
    fn parses_and_rejects_timecode_strings() {
        assert_eq!(
            Timecode::parse("01:02:03:04"),
            Some(Timecode {
                hours: 1,
                minutes: 2,
                seconds: 3,
                frames: 4
            })
        );
        assert_eq!(
            Timecode::parse("01:02:03:04").unwrap().to_frame(30),
            111_694
        );
        assert_eq!(Timecode::parse("01:60:00:00"), None); // minute out of range
        assert_eq!(Timecode::parse("01:00:60:00"), None); // second out of range
        assert_eq!(Timecode::parse("1:2:3"), None); // too few fields
        assert_eq!(Timecode::parse("aa:bb:cc:dd"), None);
        assert_eq!(
            Timecode {
                hours: u64::MAX,
                minutes: 59,
                seconds: 59,
                frames: 59,
            }
            .to_frame(60),
            u64::MAX
        );
    }
}
