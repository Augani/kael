//! Shared clamps for audio values.

/// Clamps a playback volume to the supported `0.0..=1.0` range.
pub fn clamp_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Clamps a playback rate to the supported `0.5..=2.0` range.
pub fn clamp_playback_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(0.5, 2.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_controls_use_safe_defaults() {
        assert_eq!(clamp_volume(f32::NAN), 1.0);
        assert_eq!(clamp_volume(f32::INFINITY), 1.0);
        assert_eq!(clamp_playback_rate(f32::NAN), 1.0);
        assert_eq!(clamp_playback_rate(f32::NEG_INFINITY), 1.0);
    }
}
