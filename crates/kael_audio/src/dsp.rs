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

/// A min/max sample pair for one waveform display bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformPeak {
    /// Minimum (most negative) mono sample in the bucket.
    pub min: f32,
    /// Maximum (most positive) mono sample in the bucket.
    pub max: f32,
}

/// Downsample interleaved audio to `buckets` min/max peaks of the channel-summed
/// mono signal, for timeline waveform rendering.
pub fn waveform_peaks(samples: &[f32], channels: u16, buckets: usize) -> Vec<WaveformPeak> {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    if frames == 0 || buckets == 0 {
        return Vec::new();
    }
    let mut peaks = Vec::with_capacity(buckets);
    for bucket in 0..buckets {
        let start = bucket * frames / buckets;
        let end = (((bucket + 1) * frames / buckets).max(start + 1)).min(frames);
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for frame in start..end {
            let base = frame * channels;
            let mono = samples[base..base + channels].iter().sum::<f32>() / channels as f32;
            min = min.min(mono);
            max = max.max(mono);
        }
        if min > max {
            min = 0.0;
            max = 0.0;
        }
        peaks.push(WaveformPeak { min, max });
    }
    peaks
}

/// A second-order (biquad) IIR filter using the RBJ Audio-EQ-Cookbook coefficients.
///
/// Direct Form I; coefficients are normalized by `a0` at construction. One instance
/// holds the per-channel delay state, so use one filter per channel. These are the
/// per-track EQ inserts in the mixing graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn from_coeffs(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn omega(freq_hz: f32, sample_rate: u32) -> (f32, f32, f32) {
        let w0 = 2.0 * std::f32::consts::PI * (freq_hz / sample_rate as f32);
        (w0.sin(), w0.cos(), w0.sin())
    }

    /// A low-pass filter at `freq_hz` with resonance `q` (0.707 is maximally flat).
    pub fn low_pass(freq_hz: f32, sample_rate: u32, q: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        Self::from_coeffs(
            (1.0 - cos_w) / 2.0,
            1.0 - cos_w,
            (1.0 - cos_w) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w,
            1.0 - alpha,
        )
    }

    /// A high-pass filter at `freq_hz` with resonance `q`.
    pub fn high_pass(freq_hz: f32, sample_rate: u32, q: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        Self::from_coeffs(
            (1.0 + cos_w) / 2.0,
            -(1.0 + cos_w),
            (1.0 + cos_w) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w,
            1.0 - alpha,
        )
    }

    /// A band-pass filter (0 dB peak gain) centered at `freq_hz` with width set by `q`.
    pub fn band_pass(freq_hz: f32, sample_rate: u32, q: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        Self::from_coeffs(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
    }

    /// A peaking EQ that boosts or cuts `gain_db` around `freq_hz` (the parametric band).
    pub fn peaking_eq(freq_hz: f32, sample_rate: u32, q: f32, gain_db: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        let amp = 10f32.powf(gain_db / 40.0);
        Self::from_coeffs(
            1.0 + alpha * amp,
            -2.0 * cos_w,
            1.0 - alpha * amp,
            1.0 + alpha / amp,
            -2.0 * cos_w,
            1.0 - alpha / amp,
        )
    }

    /// A low-shelf filter applying `gain_db` below `freq_hz`.
    pub fn low_shelf(freq_hz: f32, sample_rate: u32, q: f32, gain_db: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        let amp = 10f32.powf(gain_db / 40.0);
        let beta = 2.0 * amp.sqrt() * alpha;
        Self::from_coeffs(
            amp * ((amp + 1.0) - (amp - 1.0) * cos_w + beta),
            2.0 * amp * ((amp - 1.0) - (amp + 1.0) * cos_w),
            amp * ((amp + 1.0) - (amp - 1.0) * cos_w - beta),
            (amp + 1.0) + (amp - 1.0) * cos_w + beta,
            -2.0 * ((amp - 1.0) + (amp + 1.0) * cos_w),
            (amp + 1.0) + (amp - 1.0) * cos_w - beta,
        )
    }

    /// A high-shelf filter applying `gain_db` above `freq_hz`.
    pub fn high_shelf(freq_hz: f32, sample_rate: u32, q: f32, gain_db: f32) -> Self {
        let (sin_w, cos_w, _) = Self::omega(freq_hz, sample_rate);
        let alpha = sin_w / (2.0 * q.max(1e-4));
        let amp = 10f32.powf(gain_db / 40.0);
        let beta = 2.0 * amp.sqrt() * alpha;
        Self::from_coeffs(
            amp * ((amp + 1.0) + (amp - 1.0) * cos_w + beta),
            -2.0 * amp * ((amp - 1.0) + (amp + 1.0) * cos_w),
            amp * ((amp + 1.0) + (amp - 1.0) * cos_w - beta),
            (amp + 1.0) - (amp - 1.0) * cos_w + beta,
            2.0 * ((amp - 1.0) - (amp + 1.0) * cos_w),
            (amp + 1.0) - (amp - 1.0) * cos_w - beta,
        )
    }

    /// Process one sample, advancing the filter state.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    /// Process a mono buffer in place.
    pub fn process(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }

    /// The steady-state gain at DC (0 Hz), useful for validating filter type.
    pub fn dc_gain(&self) -> f32 {
        (self.b0 + self.b1 + self.b2) / (1.0 + self.a1 + self.a2)
    }

    /// Clear the delay state without changing the coefficients.
    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// A feed-forward peak compressor: a dynamics insert that attenuates signal above a
/// threshold by `ratio`, with attack/release smoothing and make-up gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Compressor {
    threshold_db: f32,
    ratio: f32,
    attack_coeff: f32,
    release_coeff: f32,
    makeup_db: f32,
    envelope: f32,
}

impl Compressor {
    /// Build a compressor. `attack_ms`/`release_ms` set the envelope time constants.
    pub fn new(
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
        sample_rate: u32,
    ) -> Self {
        Self {
            threshold_db,
            ratio: ratio.max(1.0),
            attack_coeff: time_coeff(attack_ms, sample_rate),
            release_coeff: time_coeff(release_ms, sample_rate),
            makeup_db,
            envelope: 0.0,
        }
    }

    /// Process one sample, returning the gain-adjusted output.
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let level = input.abs();
        let coeff = if level > self.envelope {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.envelope = coeff * self.envelope + (1.0 - coeff) * level;

        let env_db = 20.0 * self.envelope.max(1e-9).log10();
        let over = env_db - self.threshold_db;
        let reduction_db = if over > 0.0 {
            over * (1.0 / self.ratio - 1.0)
        } else {
            0.0
        };
        let gain = 10f32.powf((reduction_db + self.makeup_db) / 20.0);
        input * gain
    }

    /// Process a mono buffer in place.
    pub fn process(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }
}

fn time_coeff(time_ms: f32, sample_rate: u32) -> f32 {
    if time_ms <= 0.0 {
        return 0.0;
    }
    (-1.0 / (time_ms * 0.001 * sample_rate as f32)).exp()
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
        assert!((center[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((center[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);

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

    #[test]
    fn waveform_peaks_bucket_a_ramp() {
        let samples: Vec<f32> = (0..8).map(|i| i as f32).collect();
        let peaks = waveform_peaks(&samples, 1, 4);
        assert_eq!(peaks.len(), 4);
        assert_eq!(peaks[0], WaveformPeak { min: 0.0, max: 1.0 });
        assert_eq!(peaks[3], WaveformPeak { min: 6.0, max: 7.0 });
    }

    #[test]
    fn waveform_peaks_constant_and_edges() {
        let samples = vec![0.5f32; 100];
        let peaks = waveform_peaks(&samples, 1, 10);
        assert_eq!(peaks.len(), 10);
        assert!(
            peaks
                .iter()
                .all(|peak| (peak.min - 0.5).abs() < 1e-6 && (peak.max - 0.5).abs() < 1e-6)
        );
        assert!(waveform_peaks(&[], 1, 4).is_empty());
        assert!(waveform_peaks(&samples, 1, 0).is_empty());
    }

    #[test]
    fn biquad_lowpass_passes_dc_highpass_blocks_it() {
        let lp = Biquad::low_pass(1000.0, 48_000, 0.707);
        assert!((lp.dc_gain() - 1.0).abs() < 1e-4, "lp dc {}", lp.dc_gain());
        let hp = Biquad::high_pass(1000.0, 48_000, 0.707);
        assert!(hp.dc_gain().abs() < 1e-4, "hp dc {}", hp.dc_gain());
    }

    #[test]
    fn biquad_lowpass_attenuates_nyquist() {
        let mut lp = Biquad::low_pass(1000.0, 48_000, 0.707);
        let mut last = 0.0;
        for i in 0..2000 {
            let x = if i % 2 == 0 { 1.0 } else { -1.0 };
            last = lp.process_sample(x);
        }
        assert!(last.abs() < 0.1, "nyquist not attenuated: {last}");
    }

    #[test]
    fn biquad_peaking_eq_zero_gain_is_identity() {
        let mut eq = Biquad::peaking_eq(1000.0, 48_000, 1.0, 0.0);
        for x in [0.3f32, -0.7, 0.1, 0.9, -0.2] {
            let y = eq.process_sample(x);
            assert!((y - x).abs() < 1e-5, "0dB peaking changed {x} -> {y}");
        }
    }

    #[test]
    fn biquad_shelves_have_expected_dc_gain() {
        // +6 dB low shelf boosts DC by A^2 = 10^(6/20) ~= 1.995.
        let low = Biquad::low_shelf(1000.0, 48_000, 0.707, 6.0);
        assert!(
            (low.dc_gain() - 1.995).abs() < 0.05,
            "low shelf dc {}",
            low.dc_gain()
        );
        // High shelf leaves DC at unity.
        let high = Biquad::high_shelf(1000.0, 48_000, 0.707, 6.0);
        assert!(
            (high.dc_gain() - 1.0).abs() < 1e-3,
            "high shelf dc {}",
            high.dc_gain()
        );
    }

    #[test]
    fn biquad_reset_clears_state() {
        let mut lp = Biquad::low_pass(1000.0, 48_000, 0.707);
        lp.process_sample(1.0);
        lp.reset();
        assert_eq!((lp.x1, lp.x2, lp.y1, lp.y2), (0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn compressor_passes_quiet_attenuates_loud() {
        let mut quiet = Compressor::new(-20.0, 4.0, 1.0, 50.0, 0.0, 48_000);
        let mut out_quiet = 0.0;
        for _ in 0..4800 {
            out_quiet = quiet.process_sample(0.01);
        }
        assert!(
            (out_quiet - 0.01).abs() < 1e-3,
            "quiet altered: {out_quiet}"
        );

        let mut loud = Compressor::new(-20.0, 4.0, 1.0, 50.0, 0.0, 48_000);
        let mut out_loud = 0.0;
        for _ in 0..4800 {
            out_loud = loud.process_sample(0.5);
        }
        let reduction_db = 20.0 * (out_loud.abs() / 0.5).log10();
        assert!(
            reduction_db < -1.0,
            "loud not compressed: {reduction_db} dB ({out_loud})"
        );
    }

    #[test]
    fn compressor_makeup_gain_lifts_sub_threshold_level() {
        let mut comp = Compressor::new(0.0, 2.0, 1.0, 10.0, 6.0, 48_000);
        let mut out = 0.0;
        for _ in 0..960 {
            out = comp.process_sample(0.1);
        }
        // Below threshold, only +6 dB make-up applies (~x1.995).
        assert!(
            (out / 0.1 - 1.995).abs() < 0.05,
            "makeup off: {}",
            out / 0.1
        );
    }
}
