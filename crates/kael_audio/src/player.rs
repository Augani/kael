//! Audio player and track types.

use std::{collections::BTreeMap, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use anyhow::Result;
use parking_lot::Mutex;

use crate::effects::clamp_volume;

type StateListener = Arc<dyn Fn(PlaybackState) + Send + Sync + 'static>;
type PositionListener = Arc<dyn Fn(Duration) + Send + Sync + 'static>;

/// A handle that unregisters an audio callback when it is dropped.
#[must_use]
pub struct Subscription {
    unsubscribe: Option<Box<dyn FnOnce() + 'static>>,
}

impl Subscription {
    /// Creates a new subscription.
    pub fn new(unsubscribe: impl FnOnce() + 'static) -> Self {
        Self {
            unsubscribe: Some(Box::new(unsubscribe)),
        }
    }

    /// Detaches the callback from this handle.
    pub fn detach(mut self) {
        self.unsubscribe.take();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.unsubscribe.take() {
            unsubscribe();
        }
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").finish()
    }
}

/// A source of audio content.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AudioSource {
    /// Audio loaded from a file on disk.
    File(PathBuf),
    /// Audio loaded from a URL.
    Url(String),
    /// Audio loaded from in-memory bytes.
    Memory(Arc<[u8]>),
}

impl AudioSource {
    fn to_media_source(&self) -> kael_media::MediaSource {
        match self {
            Self::File(path) => kael_media::MediaSource::file(path.clone()),
            Self::Url(url) => kael_media::MediaSource::url(Arc::<str>::from(url.as_str())),
            Self::Memory(bytes) => kael_media::MediaSource::bytes(bytes.clone()),
        }
    }
}

impl From<PathBuf> for AudioSource {
    fn from(value: PathBuf) -> Self {
        Self::File(value)
    }
}

impl From<Vec<u8>> for AudioSource {
    fn from(value: Vec<u8>) -> Self {
        Self::Memory(Arc::<[u8]>::from(value))
    }
}

impl From<&'static [u8]> for AudioSource {
    fn from(value: &'static [u8]) -> Self {
        Self::Memory(Arc::<[u8]>::from(value))
    }
}

/// The externally visible playback state for the audio player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    /// No track is loaded.
    Idle,
    /// A track is being prepared.
    Loading,
    /// A track is currently playing.
    Playing,
    /// Playback is paused.
    Paused,
    /// Playback is stopped with a track loaded.
    Stopped,
    /// The most recent operation failed.
    Error(String),
}

/// Metadata for a loaded track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// The unique track identifier within a player.
    pub id: u64,
    /// The source used to load the track.
    pub source: AudioSource,
    /// The detected track duration when available.
    pub duration: Option<Duration>,
}

const MAX_DECODED_AUDIO_BYTES: u64 = 256 * 1024 * 1024;

/// A clonable audio player that wraps `kael-media` playback.
#[derive(Clone)]
pub struct AudioPlayer {
    inner: Rc<Mutex<AudioPlayerState>>,
}

struct AudioPlayerState {
    current_track: Option<Track>,
    handle: Option<kael_media::AudioHandle>,
    playback_state: PlaybackState,
    volume: f32,
    rate: f32,
    load_generation: u64,
    next_track_id: u64,
    next_listener_id: usize,
    state_listeners: BTreeMap<usize, StateListener>,
    position_listeners: BTreeMap<usize, PositionListener>,
}

impl AudioPlayer {
    /// Creates a new audio player.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Mutex::new(AudioPlayerState {
                current_track: None,
                handle: None,
                playback_state: PlaybackState::Idle,
                volume: 1.0,
                rate: 1.0,
                load_generation: 0,
                next_track_id: 1,
                next_listener_id: 0,
                state_listeners: BTreeMap::new(),
                position_listeners: BTreeMap::new(),
            })),
        }
    }

    /// Returns true when the decoded audio for a given format would exceed the buffer limit.
    pub fn exceeds_buffer_limit(
        duration: Duration,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
    ) -> bool {
        let bytes_per_sample = (bits_per_sample as u64).div_ceil(8);
        let total = (duration.as_secs_f64() * sample_rate as f64) as u64
            * channels as u64
            * bytes_per_sample;
        total > MAX_DECODED_AUDIO_BYTES
    }

    /// Loads a track and makes it the current player target.
    pub async fn load(&self, source: AudioSource) -> Result<Track> {
        let my_generation = {
            let mut state = self.inner.lock();
            state.load_generation += 1;
            state.load_generation
        };

        let loading_listeners = self.set_state(PlaybackState::Loading);
        notify_state_listeners(&loading_listeners, PlaybackState::Loading);

        let duration = smol::unblock({
            let media_source = source.to_media_source();
            move || kael_media::probe_audio_duration(media_source)
        })
        .await;

        match duration {
            Ok(duration) => {
                let handle = kael_media::AudioHandle::new(source.to_media_source());
                let (track, listeners) = {
                    let mut state = self.inner.lock();
                    if state.load_generation != my_generation {
                        return Err(anyhow::anyhow!("load superseded by newer request"));
                    }
                    let track = Track {
                        id: state.next_track_id,
                        source,
                        duration,
                    };
                    state.next_track_id += 1;
                    handle.set_volume(state.volume);
                    state.current_track = Some(track.clone());
                    state.handle = Some(handle);
                    state.playback_state = PlaybackState::Stopped;
                    (
                        track,
                        state.state_listeners.values().cloned().collect::<Vec<_>>(),
                    )
                };
                notify_state_listeners(&listeners, PlaybackState::Stopped);
                Ok(track)
            }
            Err(error) => {
                let message = error.to_string();
                let listeners = self.set_state(PlaybackState::Error(message.clone()));
                notify_state_listeners(&listeners, PlaybackState::Error(message));
                Err(error.into())
            }
        }
    }

    /// Starts playback for the given track.
    pub fn play(&self, track: &Track) -> Result<()> {
        let handle = self.ensure_track_handle(track)?;
        handle.play()?;
        let listeners = self.set_state(PlaybackState::Playing);
        notify_state_listeners(&listeners, PlaybackState::Playing);
        let position = handle.position();
        let position_listeners = self.position_listeners();
        notify_position_listeners(&position_listeners, position);
        Ok(())
    }

    /// Pauses playback.
    pub fn pause(&self) {
        let handle = {
            let state = self.inner.lock();
            state.handle.clone()
        };

        if let Some(handle) = handle {
            handle.pause();
            let listeners = self.set_state(PlaybackState::Paused);
            notify_state_listeners(&listeners, PlaybackState::Paused);
            let position_listeners = self.position_listeners();
            notify_position_listeners(&position_listeners, handle.position());
        }
    }

    /// Stops playback and resets the current position.
    pub fn stop(&self) {
        let handle = {
            let state = self.inner.lock();
            state.handle.clone()
        };

        if let Some(handle) = handle {
            handle.stop();
            let listeners = self.set_state(PlaybackState::Stopped);
            notify_state_listeners(&listeners, PlaybackState::Stopped);
            let position_listeners = self.position_listeners();
            notify_position_listeners(&position_listeners, Duration::ZERO);
        }
    }

    /// Seeks the current track.
    pub fn seek(&self, position: Duration) -> Result<()> {
        let (handle, source) = {
            let state = self.inner.lock();
            (
                state.handle.clone(),
                state
                    .current_track
                    .as_ref()
                    .map(|track| track.source.clone()),
            )
        };

        let handle = handle.or_else(|| {
            source.map(|source| kael_media::AudioHandle::new(source.to_media_source()))
        });

        if let Some(handle) = handle {
            handle.seek(position)?;
            {
                let mut state = self.inner.lock();
                state.handle = Some(handle.clone());
            }
            let position_listeners = self.position_listeners();
            notify_position_listeners(&position_listeners, handle.position());
        }
        Ok(())
    }

    /// Sets the playback volume.
    pub fn set_volume(&self, volume: f32) {
        let volume = clamp_volume(volume);
        let handle = {
            let mut state = self.inner.lock();
            state.volume = volume;
            state.handle.clone()
        };

        if let Some(handle) = handle {
            handle.set_volume(volume);
        }
    }

    /// Returns the current playback rate.
    pub fn rate(&self) -> f32 {
        self.inner.lock().rate
    }

    /// Returns the current playback position.
    pub fn position(&self) -> Duration {
        let handle = {
            let state = self.inner.lock();
            state.handle.clone()
        };
        handle
            .map(|handle| handle.position())
            .unwrap_or(Duration::ZERO)
    }

    /// Returns the current track duration when available.
    pub fn duration(&self) -> Option<Duration> {
        self.inner
            .lock()
            .current_track
            .as_ref()
            .and_then(|track| track.duration)
    }

    /// Returns the current externally visible playback state.
    pub fn state(&self) -> PlaybackState {
        let (cached_state, handle) = {
            let state = self.inner.lock();
            (state.playback_state.clone(), state.handle.clone())
        };

        match cached_state {
            PlaybackState::Loading | PlaybackState::Error(_) | PlaybackState::Idle => cached_state,
            _ => handle
                .map(|handle| map_playback_state(handle.state()))
                .unwrap_or(cached_state),
        }
    }

    /// Registers a listener for state changes.
    pub fn on_state_change(
        &self,
        callback: impl Fn(PlaybackState) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.inner.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .state_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        Subscription::new(move || {
            state.lock().state_listeners.remove(&listener_id);
        })
    }

    /// Registers a listener for position changes.
    pub fn on_position_change(
        &self,
        callback: impl Fn(Duration) + Send + Sync + 'static,
    ) -> Subscription {
        let state = self.inner.clone();
        let listener_id = {
            let mut state = state.lock();
            let listener_id = state.next_listener_id;
            state.next_listener_id += 1;
            state
                .position_listeners
                .insert(listener_id, Arc::new(callback));
            listener_id
        };

        Subscription::new(move || {
            state.lock().position_listeners.remove(&listener_id);
        })
    }

    fn ensure_track_handle(&self, track: &Track) -> Result<kael_media::AudioHandle> {
        let (current_track, existing_handle, volume) = {
            let state = self.inner.lock();
            (
                state.current_track.clone(),
                state.handle.clone(),
                state.volume,
            )
        };

        if current_track.as_ref().map(|current| current.id) == Some(track.id) {
            if let Some(handle) = existing_handle {
                return Ok(handle);
            }
        }

        let handle = kael_media::AudioHandle::new(track.source.to_media_source());
        handle.set_volume(volume);
        let mut state = self.inner.lock();
        state.current_track = Some(track.clone());
        state.handle = Some(handle.clone());
        Ok(handle)
    }

    fn set_state(&self, playback_state: PlaybackState) -> Vec<StateListener> {
        let mut state = self.inner.lock();
        state.playback_state = playback_state;
        state.state_listeners.values().cloned().collect()
    }

    fn position_listeners(&self) -> Vec<PositionListener> {
        let state = self.inner.lock();
        state.position_listeners.values().cloned().collect()
    }
}

fn map_playback_state(playback_state: kael_media::PlaybackState) -> PlaybackState {
    match playback_state {
        kael_media::PlaybackState::Playing => PlaybackState::Playing,
        kael_media::PlaybackState::Paused => PlaybackState::Paused,
        kael_media::PlaybackState::Stopped => PlaybackState::Stopped,
    }
}

fn notify_state_listeners(listeners: &[StateListener], playback_state: PlaybackState) {
    for listener in listeners {
        listener(playback_state.clone());
    }
}

fn notify_position_listeners(listeners: &[PositionListener], position: Duration) {
    for listener in listeners {
        listener(position);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use futures::executor::block_on;

    use super::{AudioPlayer, AudioSource, PlaybackState};

    #[test]
    fn rejects_oversized_audio() {
        assert!(AudioPlayer::exceeds_buffer_limit(
            Duration::from_secs(7200),
            48000,
            2,
            16,
        ));
    }

    #[test]
    fn accepts_reasonable_audio() {
        assert!(!AudioPlayer::exceeds_buffer_limit(
            Duration::from_secs(60),
            44100,
            2,
            16,
        ));
    }

    #[test]
    fn audio_player_rate_defaults_to_one() {
        let player = AudioPlayer::new();
        assert_eq!(player.rate(), 1.0);
    }

    #[test]
    fn load_generation_increments_on_load() {
        let player = AudioPlayer::new();
        let gen_before = player.inner.lock().load_generation;
        {
            let mut state = player.inner.lock();
            state.load_generation += 1;
        }
        let gen_after = player.inner.lock().load_generation;
        assert_eq!(gen_after, gen_before + 1);
    }

    #[test]
    fn load_seek_and_stop_update_state_and_position() {
        let player = AudioPlayer::new();
        let states = Arc::new(Mutex::new(Vec::new()));
        let observed_states = states.clone();
        let _subscription = player.on_state_change(move |state| {
            observed_states.lock().unwrap().push(state);
        });

        let track = block_on(player.load(AudioSource::from(silent_wav_1s()))).unwrap();
        assert_eq!(track.duration, Some(Duration::from_secs(1)));
        assert_eq!(player.state(), PlaybackState::Stopped);

        player.seek(Duration::from_millis(250)).unwrap();
        assert_eq!(player.position(), Duration::from_millis(250));

        player.stop();
        assert_eq!(player.position(), Duration::ZERO);
        assert_eq!(player.state(), PlaybackState::Stopped);

        let states = states.lock().unwrap().clone();
        assert_eq!(states[0], PlaybackState::Loading);
        assert_eq!(states[1], PlaybackState::Stopped);
    }

    fn silent_wav_1s() -> Vec<u8> {
        let sample_rate = 8_000u32;
        let channels = 1u16;
        let bits_per_sample = 16u16;
        let samples = sample_rate as usize;
        let bytes_per_sample = (bits_per_sample / 8) as usize;
        let data_len = samples * channels as usize * bytes_per_sample;
        let byte_rate = sample_rate * channels as u32 * bytes_per_sample as u32;
        let block_align = channels * bits_per_sample / 8;
        let file_size = 36 + data_len as u32;

        let mut bytes = Vec::with_capacity(44 + data_len);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&file_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
        bytes.resize(44 + data_len, 0);
        bytes
    }
}
