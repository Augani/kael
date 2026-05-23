//! Shared clamps for audio values.

/// Clamps a playback volume to the supported `0.0..=1.0` range.
pub fn clamp_volume(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0)
}

/// Clamps a playback rate to the supported `0.5..=2.0` range.
pub fn clamp_playback_rate(rate: f32) -> f32 {
    rate.clamp(0.5, 2.0)
}
