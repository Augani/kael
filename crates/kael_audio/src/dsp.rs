//! Audio DSP processors for the mixer: gain, pan, filtering, limiting, fades.
//!
//! These operate on interleaved `f32` buffers and are the building blocks for
//! per-track inserts and automation in the mixing graph. They are deterministic
//! and device-free, so they are fully unit tested.

/// Apply a linear gain to an interleaved buffer in place.
pub fn apply_gain(buffer: &mut [f32], gain: f32) {
    for sample in buffer.iter_mut() {
        *sample *= gain;
    }
}

/// Apply a per-frame linear gain ramp from `start_gain` to `end_gain` across the
/// buffer — the primitive for fades and crossfades. `channels` interleaved.
pub fn apply_fade(buffer: &mut [f32], channels: u16, start_gain: f32, end_gain: f32) {
    let channels = channels.max(1) as usize;
    let frames = buffer.len() / channels;
    if frames == 0 {
        return;
    }
    for frame in 0..frames {
        let t = if frames > 1 {
            frame as f32 / (frames - 1) as f32
        } else {
            1.0
        };
        let gain = start_gain + (end_gain - start_gain) * t;
        for channel in 0..channels {
            buffer[frame * channels + channel] *= gain;
        }
    }
}

/// Equal-power pan for an interleaved stereo buffer. `pan` is `-1.0` (hard left)
/// to `1.0` (hard right); `0.0` is center (both channels × ~0.707).
pub fn apply_stereo_pan(buffer: &mut [f32], pan: f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
    let (left_gain, right_gain) = (angle.cos(), angle.sin());
    for frame in buffer.chunks_exact_mut(2) {
        frame[0] *= left_gain;
        frame[1] *= right_gain;
    }
}

/// Hard peak limiter: clamps every sample to `±ceiling`.
pub fn apply_hard_limit(buffer: &mut [f32], ceiling: f32) {
    let ceiling = ceiling.abs();
    for sample in buffer.iter_mut() {
        *sample = sample.clamp(-ceiling, ceiling);
    }
}

/// Peak absolute sample value in the buffer (for metering).
pub fn peak(buffer: &[f32]) -> f32 {
    buffer
        .iter()
        .fold(0.0, |peak, &sample| peak.max(sample.abs()))
}

/// Root-mean-square level of the buffer (for metering).
pub fn rms(buffer: &[f32]) -> f32 {
    if buffer.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = buffer.iter().map(|&sample| sample * sample).sum();
    (sum_squares / buffer.len() as f32).sqrt()
}

/// A one-pole IIR filter (low- or high-pass), processed sample-by-sample.
#[derive(Debug, Clone, Copy)]
pub struct OnePole {
    alpha: f32,
    state: f32,
    high_pass: bool,
}

impl OnePole {
    /// A low-pass filter with the given cutoff frequency.
    pub fn low_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        Self {
            alpha: Self::alpha(cutoff_hz, sample_rate),
            state: 0.0,
            high_pass: false,
        }
    }

    /// A high-pass filter with the given cutoff frequency.
    pub fn high_pass(cutoff_hz: f32, sample_rate: u32) -> Self {
        Self {
            alpha: Self::alpha(cutoff_hz, sample_rate),
            state: 0.0,
            high_pass: true,
        }
    }

    fn alpha(cutoff_hz: f32, sample_rate: u32) -> f32 {
        let sample_rate = sample_rate.max(1) as f32;
        let cutoff = cutoff_hz.clamp(0.0, sample_rate * 0.5);
        1.0 - (-2.0 * std::f32::consts::PI * cutoff / sample_rate).exp()
    }

    /// Process a single sample.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.state += self.alpha * (input - self.state);
        if self.high_pass {
            input - self.state
        } else {
            self.state
        }
    }

    /// Process an interleaved buffer in place (mono coefficients applied per sample).
    pub fn process(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }

    /// Reset the filter state.
    pub fn reset(&mut self) {
        self.state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_scales_samples() {
        let mut buffer = [1.0, -2.0, 0.5];
        apply_gain(&mut buffer, 0.5);
        assert_eq!(buffer, [0.5, -1.0, 0.25]);
    }

    #[test]
    fn fade_ramps_across_frames() {
        let mut buffer = [1.0; 5];
        apply_fade(&mut buffer, 1, 0.0, 1.0);
        assert!((buffer[0] - 0.0).abs() < 1e-6);
        assert!((buffer[4] - 1.0).abs() < 1e-6);
        assert!((buffer[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stereo_pan_distributes_power() {
        let mut center = [1.0, 1.0];
        apply_stereo_pan(&mut center, 0.0);
        assert!((center[0] - 0.7071).abs() < 1e-3);
        assert!((center[1] - 0.7071).abs() < 1e-3);

        let mut left = [1.0, 1.0];
        apply_stereo_pan(&mut left, -1.0);
        assert!((left[0] - 1.0).abs() < 1e-3);
        assert!(left[1].abs() < 1e-3);

        let mut right = [1.0, 1.0];
        apply_stereo_pan(&mut right, 1.0);
        assert!(right[0].abs() < 1e-3);
        assert!((right[1] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn hard_limit_clamps() {
        let mut buffer = [2.0, -2.0, 0.3];
        apply_hard_limit(&mut buffer, 1.0);
        assert_eq!(buffer, [1.0, -1.0, 0.3]);
    }

    #[test]
    fn peak_and_rms() {
        let buffer = [0.0, 1.0, -0.5];
        assert!((peak(&buffer) - 1.0).abs() < 1e-6);
        assert!((rms(&buffer) - (1.25f32 / 3.0).sqrt()).abs() < 1e-6);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn low_pass_passes_dc_high_pass_blocks_it() {
        let mut low = OnePole::low_pass(1_000.0, 48_000);
        let mut high = OnePole::high_pass(1_000.0, 48_000);
        let mut low_out = 0.0;
        let mut high_out = 0.0;
        for _ in 0..2_000 {
            low_out = low.process_sample(1.0);
            high_out = high.process_sample(1.0);
        }
        assert!((low_out - 1.0).abs() < 1e-2, "low-pass DC -> {low_out}");
        assert!(high_out.abs() < 1e-2, "high-pass DC -> {high_out}");
    }
}
