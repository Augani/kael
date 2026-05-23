//! Audio playback services for Kael.

#![deny(missing_docs)]

/// Shared clamps for playback values.
pub mod effects;
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
pub use kael_media::AudioPlaybackError;
pub use player::{AudioPlayer, AudioSource, PlaybackState, Subscription, Track};
pub use playlist::{Playlist, RepeatMode};
pub use session::{AudioCategory, AudioRoute, AudioSession, Interruption};
pub use spatial::SpatialAudioPlayer;