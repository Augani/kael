#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

/// Typed browser Web Audio bounds, errors, and lifecycle events.
#[cfg(target_arch = "wasm32")]
mod browser_audio;
#[cfg(any(target_arch = "wasm32", test))]
mod browser_audio_protocol;
/// Cross-platform microphone and audio-input capture.
#[cfg(not(target_arch = "wasm32"))]
pub mod capture;
/// Browser-safe microphone facade for the synchronous capture API.
#[cfg(target_arch = "wasm32")]
#[path = "capture_web.rs"]
pub mod capture;
/// Cross-platform audio device discovery.
#[cfg(not(target_arch = "wasm32"))]
pub mod devices;
/// Browser-safe facade for synchronous audio device discovery.
#[cfg(target_arch = "wasm32")]
#[path = "devices_web.rs"]
pub mod devices;
/// Audio DSP processors: gain, pan, filtering, limiting, fades, metering.
pub mod dsp;
/// Shared clamps for playback values.
pub mod effects;
/// Real-time mixing graph with a device-sample-counter master clock.
#[cfg(not(target_arch = "wasm32"))]
pub mod mixer;
/// Device-free mixing and explicit browser live-engine facade.
#[cfg(target_arch = "wasm32")]
#[path = "mixer_web.rs"]
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
#[cfg(target_arch = "wasm32")]
pub use browser_audio::{
    BrowserAudioError, BrowserAudioErrorKind, BrowserAudioEvent, MAX_BROWSER_AUDIO_CHANNELS,
    MAX_BROWSER_AUDIO_CHUNK_FRAMES, MAX_BROWSER_AUDIO_DEVICES, MAX_BROWSER_AUDIO_PENDING_CHUNKS,
    MAX_BROWSER_DEVICE_TEXT_BYTES,
};
#[cfg(target_arch = "wasm32")]
pub use capture::BrowserAudioCaptureConfig;
pub use capture::{AudioInputConfig, AudioInputStream};
pub use devices::{
    AudioInputDevice, AudioOutputDevice, default_input_device, default_output_device,
    input_devices, output_devices,
};
#[cfg(target_arch = "wasm32")]
pub use devices::{
    default_input_device_async, default_output_device_async, input_devices_async,
    output_devices_async,
};
pub use dsp::{Biquad, Compressor, OnePole, WaveformPeak, waveform_peaks};
pub use kael_media::AudioPlaybackError;
#[cfg(target_arch = "wasm32")]
pub use mixer::BrowserAudioEngineConfig;
pub use mixer::{
    AudioClock, AudioEngine, AudioEngineHandle, BufferSource, Mixer, SampleSource, SineSource,
    VoiceId, resample_linear,
};
pub use player::{AudioPlayer, AudioSource, PlaybackState, Subscription, Track};
pub use playlist::{Playlist, RepeatMode};
pub use session::{AudioCategory, AudioRoute, AudioSession, Interruption};
pub use spatial::{SpatialAudioScene, SpatialSourceId};
