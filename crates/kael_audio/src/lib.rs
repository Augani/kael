//! Audio playback services for Kael.

#![deny(missing_docs)]

/// Audio DSP processors: gain, pan, filtering, limiting, fades, metering.
pub mod dsp;
/// Shared clamps for playback values.
pub mod effects;
/// Real-time mixing graph with a device-sample-counter master clock.
pub mod mixer;
/// Platform metadata for audio services.
pub mod platform;
/// Audio player and track types.
pub mod player;
/// Playlist management.
pub mod playlist;
/// Audio-session state.
pub mod session;
/// Spatial-audio bookkeeping.
pub mod spatial;

pub use anyhow::Result;
pub use dsp::{OnePole, WaveformPeak, waveform_peaks};
pub use kael_media::AudioPlaybackError;
pub use mixer::{
    AudioClock, AudioEngine, BufferSource, Mixer, SampleSource, SineSource, VoiceId,
    resample_linear,
};
pub use player::{AudioPlayer, AudioSource, PlaybackState, Subscription, Track};
pub use playlist::{Playlist, RepeatMode};
pub use session::{AudioCategory, AudioRoute, AudioSession, Interruption};
pub use spatial::SpatialAudioPlayer;
