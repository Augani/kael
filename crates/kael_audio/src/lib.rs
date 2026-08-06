#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

/// Cross-platform microphone and audio-input capture.
pub mod capture;
/// Cross-platform audio device discovery.
pub mod devices;
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
/// Lightweight spatial-audio scene and stereo source processing.
pub mod spatial;

pub use anyhow::Result;
pub use capture::{AudioInputConfig, AudioInputStream};
pub use devices::{
    AudioInputDevice, AudioOutputDevice, default_input_device, default_output_device,
    input_devices, output_devices,
};
pub use dsp::{Biquad, Compressor, OnePole, WaveformPeak, waveform_peaks};
pub use kael_media::AudioPlaybackError;
pub use mixer::{
    AudioClock, AudioEngine, AudioEngineHandle, BufferSource, Mixer, SampleSource, SineSource,
    VoiceId, resample_linear,
};
pub use player::{AudioPlayer, AudioSource, PlaybackState, Subscription, Track};
pub use playlist::{Playlist, RepeatMode};
pub use session::{AudioCategory, AudioRoute, AudioSession, Interruption};
pub use spatial::{SpatialAudioScene, SpatialSourceId};
