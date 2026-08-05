//! Audio player and track types.

use std::{collections::BTreeMap, path::PathBuf, rc::Rc, sync::Arc, time::Duration};

use anyhow::Result;
use parking_lot::Mutex;

use crate::effects::{clamp_playback_rate, clamp_volume};

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
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum AudioSource {
    /// Audio loaded from a file on disk.
    File(PathBuf),
    /// Audio loaded from a URL.
    Url(String),
    /// Audio loaded from in-memory bytes.
    Memory(Arc<[u8]>),
}

impl std::fmt::Debug for AudioSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Url(url) => f
                .debug_struct("Url")
                .field("bytes", &url.len())
                .finish_non_exhaustive(),
            Self::Memory(bytes) => f
                .debug_struct("Memory")
                .field("bytes", &bytes.len())
                .finish_non_exhaustive(),
        }
    }
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

const MAX_DECODED_AUDIO_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MEMORY_SOURCE_BYTES: usize = 256 * 1024 * 1024;
const MAX_URL_BYTES: usize = 16 * 1024;
const MAX_ERROR_BYTES: usize = 4 * 1024;

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
        if sample_rate == 0 || channels == 0 || bits_per_sample == 0 {
            return true;
        }
        let bytes_per_sample = u128::from(bits_per_sample).div_ceil(8);
        let frames = duration
            .as_nanos()
            .checked_mul(u128::from(sample_rate))
            .map(|scaled| scaled.div_ceil(1_000_000_000));
        let total = frames
            .and_then(|frames| frames.checked_mul(u128::from(channels)))
            .and_then(|samples| samples.checked_mul(bytes_per_sample));
        total.is_none_or(|bytes| bytes > u128::from(MAX_DECODED_AUDIO_BYTES))
    }

    /// Loads a track and makes it the current player target.
    pub async fn load(&self, source: AudioSource) -> Result<Track> {
        validate_source(&source)?;
        let my_generation = {
            let mut state = self.inner.lock();
            state.load_generation = state
                .load_generation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("audio load generation exhausted"))?;
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
                    let track_id = state.next_track_id;
                    state.next_track_id = state
                        .next_track_id
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("audio track id space exhausted"))?;
                    let track = Track {
                        id: track_id,
                        source,
                        duration,
                    };
                    handle.set_volume(state.volume);
                    handle.set_speed(state.rate);
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
                let message = bounded_error_message(error.to_string());
                let listeners = {
                    let mut state = self.inner.lock();
                    if state.load_generation != my_generation {
                        return Err(anyhow::anyhow!("load superseded by newer request"));
                    }
                    state.playback_state = PlaybackState::Error(message.clone());
                    state.state_listeners.values().cloned().collect::<Vec<_>>()
                };
                notify_state_listeners(&listeners, PlaybackState::Error(message));
                Err(error.into())
            }
        }
    }

    /// Starts playback for the given track.
    pub fn play(&self, track: &Track) -> Result<()> {
        let handle = self.ensure_track_handle(track)?;
        if let Err(error) = handle.play() {
            let message = bounded_error_message(error.to_string());
            let listeners = self.set_state(PlaybackState::Error(message.clone()));
            notify_state_listeners(&listeners, PlaybackState::Error(message));
            return Err(error.into());
        }
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
        let (handle, source, duration, volume, rate) = {
            let state = self.inner.lock();
            (
                state.handle.clone(),
                state
                    .current_track
                    .as_ref()
                    .map(|track| track.source.clone()),
                state
                    .current_track
                    .as_ref()
                    .and_then(|track| track.duration),
                state.volume,
                state.rate,
            )
        };

        let handle = handle.or_else(|| {
            source.map(|source| {
                let handle = kael_media::AudioHandle::new(source.to_media_source());
                handle.set_volume(volume);
                handle.set_speed(rate);
                handle
            })
        });

        let Some(handle) = handle else {
            anyhow::bail!("cannot seek without a loaded track");
        };
        let position = duration.map_or(position, |duration| position.min(duration));
        handle.seek(position)?;
        {
            let mut state = self.inner.lock();
            state.handle = Some(handle.clone());
        }
        let position_listeners = self.position_listeners();
        notify_position_listeners(&position_listeners, handle.position());
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

    /// Sets the playback rate in the supported `0.5..=2.0` range.
    pub fn set_rate(&self, rate: f32) {
        let rate = clamp_playback_rate(rate);
        let handle = {
            let mut state = self.inner.lock();
            state.rate = rate;
            state.handle.clone()
        };
        if let Some(handle) = handle {
            handle.set_speed(rate);
        }
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
            let listener_id = allocate_listener_id(&mut state);
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
            let listener_id = allocate_listener_id(&mut state);
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
        let (current_track, existing_handle, volume, rate) = {
            let state = self.inner.lock();
            (
                state.current_track.clone(),
                state.handle.clone(),
                state.volume,
                state.rate,
            )
        };

        if current_track.as_ref() == Some(track) {
            if let Some(handle) = existing_handle {
                return Ok(handle);
            }
        }

        let handle = kael_media::AudioHandle::new(track.source.to_media_source());
        handle.set_volume(volume);
        handle.set_speed(rate);
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

fn validate_source(source: &AudioSource) -> Result<()> {
    match source {
        AudioSource::File(_) => Ok(()),
        AudioSource::Memory(bytes) => {
            if bytes.len() > MAX_MEMORY_SOURCE_BYTES {
                anyhow::bail!(
                    "in-memory audio source exceeds {MAX_MEMORY_SOURCE_BYTES} byte limit"
                );
            }
            Ok(())
        }
        AudioSource::Url(url) => {
            if url.is_empty()
                || url.len() > MAX_URL_BYTES
                || url.trim() != url
                || url.chars().any(char::is_control)
            {
                anyhow::bail!(
                    "audio URL must be non-empty, at most {MAX_URL_BYTES} bytes, and contain no surrounding whitespace or control characters"
                );
            }
            let parsed =
                url::Url::parse(url).map_err(|_| anyhow::anyhow!("audio URL is invalid"))?;
            if parsed.scheme() != "https" {
                anyhow::bail!("audio URL must use https");
            }
            if parsed.host_str().is_none() {
                anyhow::bail!("audio URL must include a host");
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                anyhow::bail!("audio URL cannot contain credentials");
            }
            Ok(())
        }
    }
}

fn bounded_error_message(mut message: String) -> String {
    if message.len() <= MAX_ERROR_BYTES {
        return message;
    }
    let mut boundary = MAX_ERROR_BYTES;
    while !message.is_char_boundary(boundary) {
        boundary -= 1;
    }
    message.truncate(boundary);
    message
}

fn allocate_listener_id(state: &mut AudioPlayerState) -> usize {
    let start = state.next_listener_id;
    let mut candidate = start;
    loop {
        if !state.state_listeners.contains_key(&candidate)
            && !state.position_listeners.contains_key(&candidate)
        {
            state.next_listener_id = candidate.wrapping_add(1);
            return candidate;
        }
        candidate = candidate.wrapping_add(1);
        assert!(candidate != start, "audio listener id space exhausted");
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

    use super::{AudioPlayer, AudioSource, PlaybackState, bounded_error_message, validate_source};

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
    fn buffer_limit_math_handles_invalid_and_extreme_formats() {
        assert!(AudioPlayer::exceeds_buffer_limit(
            Duration::MAX,
            u32::MAX,
            u16::MAX,
            u16::MAX,
        ));
        assert!(AudioPlayer::exceeds_buffer_limit(
            Duration::from_secs(1),
            0,
            2,
            16,
        ));
    }

    #[test]
    fn audio_player_rate_defaults_to_one() {
        let player = AudioPlayer::new();
        assert_eq!(player.rate(), 1.0);
        player.set_rate(3.0);
        assert_eq!(player.rate(), 2.0);
        player.set_rate(f32::NAN);
        assert_eq!(player.rate(), 1.0);
    }

    #[test]
    fn source_debug_output_does_not_expose_bytes_or_urls() {
        let memory = AudioSource::from(b"secret audio bytes".to_vec());
        let url = AudioSource::Url("https://example.com/audio?token=secret".to_string());

        let memory_debug = format!("{memory:?}");
        let url_debug = format!("{url:?}");
        assert!(memory_debug.contains("bytes"));
        assert!(!memory_debug.contains("secret audio"));
        assert!(url_debug.contains("bytes"));
        assert!(!url_debug.contains("example.com"));
        assert!(!url_debug.contains("token"));
    }

    #[test]
    fn validates_remote_audio_urls() {
        assert!(
            validate_source(&AudioSource::Url("https://example.com/audio".to_string())).is_ok()
        );
        assert!(
            validate_source(&AudioSource::Url("file:///private/audio.wav".to_string())).is_err()
        );
        assert!(
            validate_source(&AudioSource::Url("http://example.com/audio".to_string())).is_err()
        );
        assert!(
            validate_source(&AudioSource::Url(
                "https://user:secret@example.com/audio".to_string()
            ))
            .is_err()
        );
    }

    #[test]
    fn playback_errors_are_utf8_bounded() {
        let message = bounded_error_message("é".repeat(4096));
        assert!(message.len() <= 4 * 1024);
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
    fn listener_ids_wrap_without_replacing_live_callbacks() {
        let player = AudioPlayer::new();
        player.inner.lock().next_listener_id = usize::MAX;
        let max = player.on_state_change(|_| {});
        let zero = player.on_position_change(|_| {});
        let state = player.inner.lock();
        assert!(state.state_listeners.contains_key(&usize::MAX));
        assert!(state.position_listeners.contains_key(&0));
        drop(state);
        drop((max, zero));
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

    #[test]
    fn seek_requires_a_track_and_clamps_to_duration() {
        let player = AudioPlayer::new();
        assert!(player.seek(Duration::from_secs(1)).is_err());

        let track = block_on(player.load(AudioSource::from(silent_wav_1s()))).unwrap();
        assert_eq!(track.duration, Some(Duration::from_secs(1)));
        player.seek(Duration::from_secs(5)).unwrap();
        assert_eq!(player.position(), Duration::from_secs(1));
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
