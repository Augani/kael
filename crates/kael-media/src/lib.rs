//! Audio playback primitives for GPUI.

#![deny(missing_docs)]

use ffmpeg_next as ffmpeg;
use parking_lot::Mutex;
use rodio::{Decoder, OutputStream, Sink, Source, buffer::SamplesBuffer};
use std::{
    collections::hash_map::DefaultHasher,
    fmt,
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use thiserror::Error;

trait MediaReadSeek: Read + Seek + Send + Sync {}

impl<T> MediaReadSeek for T where T: Read + Seek + Send + Sync {}

type MediaReaderFactory = dyn Fn() -> io::Result<Box<dyn MediaReadSeek>> + Send + Sync;

/// Internal backing state for keyed reader-based media sources.
#[doc(hidden)]
pub struct ReaderMediaSource {
    key: Arc<str>,
    open: Arc<MediaReaderFactory>,
    staged_path: Mutex<Option<PathBuf>>,
}

enum ResolvedMediaInput {
    Path(PathBuf),
    Url(Arc<str>),
}

static STAGED_READER_COUNTER: AtomicU64 = AtomicU64::new(0);
const MAX_DECODED_VIDEO_FRAMES: usize = 256;
const MAX_DECODED_VIDEO_BYTES: u64 = 128 * 1024 * 1024;

/// A source of media content that can be played back.
#[derive(Clone)]
pub enum MediaSource {
    /// Media content loaded from a file on disk.
    File(PathBuf),
    /// Media content loaded from a URL that FFmpeg can open directly.
    Url(Arc<str>),
    /// Media content already available in memory.
    Bytes(Arc<[u8]>),
    /// Media content opened on demand from a keyed reader factory.
    Reader(Arc<ReaderMediaSource>),
}

impl MediaSource {
    /// Create a media source backed by a file on disk.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// Create a media source backed by a URL that FFmpeg can open directly.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::Url(url.into())
    }

    /// Create a media source backed by in-memory bytes.
    pub fn bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::Bytes(bytes.into())
    }

    /// Create a media source backed by a keyed reader factory.
    ///
    /// The key participates in hashing and equality for asset caching, so it must uniquely identify
    /// the underlying media content for the lifetime of the source.
    pub fn reader<R>(
        key: impl Into<Arc<str>>,
        open: impl Fn() -> io::Result<R> + Send + Sync + 'static,
    ) -> Self
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let open =
            Arc::new(move || open().map(|reader| -> Box<dyn MediaReadSeek> { Box::new(reader) }));
        Self::Reader(Arc::new(ReaderMediaSource {
            key: key.into(),
            open,
            staged_path: Mutex::new(None),
        }))
    }

    /// Create a media source backed by compile-time bytes.
    pub fn from_static_bytes(bytes: &'static [u8]) -> Self {
        Self::Bytes(Arc::<[u8]>::from(bytes))
    }

    fn open_reader(&self) -> Result<MediaReader, AudioPlaybackError> {
        match self {
            Self::File(path) => Ok(MediaReader::File(BufReader::new(File::open(path)?))),
            Self::Bytes(bytes) => Ok(MediaReader::Bytes(Cursor::new(bytes.clone()))),
            Self::Reader(source) => Ok(MediaReader::Reader((source.open)()?)),
            Self::Url(_) => Err(AudioPlaybackError::UnsupportedSource(
                "url-backed media cannot be opened as a direct rodio reader".into(),
            )),
        }
    }

    fn direct_reader_supported(&self) -> bool {
        !matches!(self, Self::Url(_))
    }

    fn resolve_ffmpeg_input(&self) -> Result<ResolvedMediaInput, MediaDecodeError> {
        match self {
            Self::File(path) => Ok(ResolvedMediaInput::Path(path.clone())),
            Self::Url(url) => Ok(ResolvedMediaInput::Url(url.clone())),
            Self::Bytes(bytes) => Ok(ResolvedMediaInput::Path(stage_bytes(bytes)?)),
            Self::Reader(source) => Ok(ResolvedMediaInput::Path(
                source.stage_to_path().map_err(MediaDecodeError::from_io)?,
            )),
        }
    }
}

impl From<PathBuf> for MediaSource {
    fn from(value: PathBuf) -> Self {
        Self::File(value)
    }
}

impl From<&Path> for MediaSource {
    fn from(value: &Path) -> Self {
        Self::File(value.to_path_buf())
    }
}

impl From<Arc<[u8]>> for MediaSource {
    fn from(value: Arc<[u8]>) -> Self {
        Self::Bytes(value)
    }
}

impl From<Arc<str>> for MediaSource {
    fn from(value: Arc<str>) -> Self {
        Self::Url(value)
    }
}

impl From<Vec<u8>> for MediaSource {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(Arc::<[u8]>::from(value))
    }
}

impl From<&'static [u8]> for MediaSource {
    fn from(value: &'static [u8]) -> Self {
        Self::from_static_bytes(value)
    }
}

impl fmt::Debug for MediaSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Url(url) => f.debug_tuple("Url").field(url).finish(),
            Self::Bytes(bytes) => f
                .debug_tuple("Bytes")
                .field(&format_args!("{} bytes", bytes.len()))
                .finish(),
            Self::Reader(source) => f.debug_tuple("Reader").field(&source.key).finish(),
        }
    }
}

impl PartialEq for MediaSource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File(left), Self::File(right)) => left == right,
            (Self::Url(left), Self::Url(right)) => left == right,
            (Self::Bytes(left), Self::Bytes(right)) => left == right,
            (Self::Reader(left), Self::Reader(right)) => left.key == right.key,
            _ => false,
        }
    }
}

impl Eq for MediaSource {}

impl Hash for MediaSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::File(path) => path.hash(state),
            Self::Url(url) => url.hash(state),
            Self::Bytes(bytes) => bytes.hash(state),
            Self::Reader(source) => source.key.hash(state),
        }
    }
}

/// Metadata for a decoded video stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoMetadata {
    /// The decoded frame width in pixels.
    pub width: u32,
    /// The decoded frame height in pixels.
    pub height: u32,
    /// The stream duration when FFmpeg reports one.
    pub duration: Option<Duration>,
}

/// A decoded video frame in BGRA format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFrame {
    /// Raw BGRA pixel data for the frame.
    pub data: Arc<[u8]>,
    /// The frame width in pixels.
    pub width: u32,
    /// The frame height in pixels.
    pub height: u32,
    /// The presentation timestamp for this frame.
    pub timestamp: Duration,
}

/// An error that can occur while decoding video content.
#[derive(Debug, Error)]
pub enum MediaDecodeError {
    /// The requested source type is not supported by the current decoder path.
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    /// The media source does not contain a video stream.
    #[error("no video stream found")]
    NoVideoStream,
    /// FFmpeg failed to open or decode the media source.
    #[error("ffmpeg decode error: {0}")]
    Decode(String),
}

/// A file-backed decoder for media metadata and video frames.
#[derive(Clone, Debug)]
pub struct MediaDecoder {
    source: MediaSource,
}

struct OpenedVideoStream {
    input_context: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: ffmpeg::software::scaling::context::Context,
    video_stream_index: usize,
    time_base: ffmpeg::Rational,
    metadata: VideoMetadata,
}

/// A sequential decoder for file-backed video frames.
///
/// This stream decodes frames on demand and can be restarted when playback seeks backward.
pub struct VideoFrameStream {
    source: MediaSource,
    input_context: ffmpeg::format::context::Input,
    decoder: ffmpeg::decoder::Video,
    scaler: ffmpeg::software::scaling::context::Context,
    video_stream_index: usize,
    time_base: ffmpeg::Rational,
    metadata: VideoMetadata,
    sent_eof: bool,
}

impl MediaDecoder {
    /// Create a decoder for the given media source.
    pub fn new(source: impl Into<MediaSource>) -> Self {
        Self {
            source: source.into(),
        }
    }

    /// Return the source associated with this decoder.
    pub fn source(&self) -> &MediaSource {
        &self.source
    }

    /// Read the primary video stream metadata from the media source.
    pub fn video_metadata(&self) -> Result<VideoMetadata, MediaDecodeError> {
        Ok(VideoFrameStream::new(self.source.clone())?.metadata())
    }

    /// Decode all frames from the primary video stream up to an in-memory safety cap.
    ///
    /// Use [`VideoFrameStream`] when decoding larger videos incrementally.
    pub fn decode_video_frames(&self) -> Result<Vec<VideoFrame>, MediaDecodeError> {
        let mut stream = VideoFrameStream::new(self.source.clone())?;
        let mut frames = Vec::new();
        let mut decoded_bytes = 0u64;
        while let Some(frame) = stream.next_frame()? {
            push_decoded_video_frame(&mut frames, &mut decoded_bytes, frame)?;
        }

        Ok(frames)
    }
}

fn push_decoded_video_frame(
    frames: &mut Vec<VideoFrame>,
    decoded_bytes: &mut u64,
    frame: VideoFrame,
) -> Result<(), MediaDecodeError> {
    if frames.len() >= MAX_DECODED_VIDEO_FRAMES {
        return Err(MediaDecodeError::Decode(format!(
            "video decode exceeded {} frames; use VideoFrameStream for larger videos",
            MAX_DECODED_VIDEO_FRAMES
        )));
    }

    let frame_bytes = u64::try_from(frame.data.len()).unwrap_or(u64::MAX);
    let next_total = decoded_bytes.saturating_add(frame_bytes);
    if next_total > MAX_DECODED_VIDEO_BYTES {
        return Err(MediaDecodeError::Decode(format!(
            "video decode exceeded {} bytes; use VideoFrameStream for larger videos",
            MAX_DECODED_VIDEO_BYTES
        )));
    }

    *decoded_bytes = next_total;
    frames.push(frame);
    Ok(())
}

impl VideoFrameStream {
    /// Create a new sequential frame stream for the given media source.
    pub fn new(source: impl Into<MediaSource>) -> Result<Self, MediaDecodeError> {
        let source = source.into();
        let OpenedVideoStream {
            input_context,
            decoder,
            scaler,
            video_stream_index,
            time_base,
            metadata,
        } = open_video_stream(&source)?;

        Ok(Self {
            source,
            input_context,
            decoder,
            scaler,
            video_stream_index,
            time_base,
            metadata,
            sent_eof: false,
        })
    }

    /// Return the source associated with this stream.
    pub fn source(&self) -> &MediaSource {
        &self.source
    }

    /// Return the decoded video metadata.
    pub fn metadata(&self) -> VideoMetadata {
        self.metadata
    }

    /// Restart decoding from the beginning of the media source.
    pub fn restart(&mut self) -> Result<(), MediaDecodeError> {
        *self = Self::new(self.source.clone())?;
        Ok(())
    }

    /// Decode and return the next video frame, or `None` after end-of-stream.
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaDecodeError> {
        loop {
            let mut decoded = ffmpeg::util::frame::video::Video::empty();
            if self.decoder.receive_frame(&mut decoded).is_ok() {
                return decode_video_frame(&mut self.scaler, &decoded, self.time_base).map(Some);
            }

            if self.sent_eof {
                return Ok(None);
            }

            if let Some(packet) = self.next_video_packet() {
                self.decoder
                    .send_packet(&packet)
                    .map_err(ffmpeg_decode_error)?;
            } else {
                self.decoder.send_eof().map_err(ffmpeg_decode_error)?;
                self.sent_eof = true;
            }
        }
    }

    fn next_video_packet(&mut self) -> Option<ffmpeg::Packet> {
        for (stream, packet) in self.input_context.packets() {
            if stream.index() == self.video_stream_index {
                return Some(packet);
            }
        }

        None
    }
}

enum MediaReader {
    File(BufReader<File>),
    Bytes(Cursor<Arc<[u8]>>),
    Reader(Box<dyn MediaReadSeek>),
}

impl Read for MediaReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(reader) => reader.read(buf),
            Self::Bytes(reader) => reader.read(buf),
            Self::Reader(reader) => reader.read(buf),
        }
    }
}

impl Seek for MediaReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(reader) => reader.seek(pos),
            Self::Bytes(reader) => reader.seek(pos),
            Self::Reader(reader) => reader.seek(pos),
        }
    }
}

/// The current playback state for a media handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    /// Playback is actively advancing.
    Playing,
    /// Playback is paused at the current position.
    Paused,
    /// Playback is stopped and positioned at the start or end.
    #[default]
    Stopped,
}

/// An error that can occur while preparing or controlling audio playback.
#[derive(Debug, Error)]
pub enum AudioPlaybackError {
    /// The media source could not be opened.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The media source cannot be opened through the direct rodio path.
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    /// The media data could not be decoded.
    #[error("decoder error: {0}")]
    Decoder(String),
    /// The host audio output stream could not be created.
    #[error("audio output error: {0}")]
    Output(String),
}

struct AudioEngine {
    _stream: OutputStream,
    sink: Sink,
}

struct DecodedAudio {
    channels: u16,
    sample_rate: u32,
    samples: Arc<[f32]>,
    duration: Duration,
}

struct AudioHandleState {
    source: MediaSource,
    volume: f32,
    duration: Option<Duration>,
    decoded_audio: Option<Arc<DecodedAudio>>,
    position: Duration,
    started_at: Option<Instant>,
    state: PlaybackState,
    engine: Option<AudioEngine>,
    generation: u64,
}

struct AudioPlaybackRequest {
    generation: u64,
    source: MediaSource,
    volume: f32,
    position: Duration,
    duration: Option<Duration>,
    decoded_audio: Option<Arc<DecodedAudio>>,
    playback_state: PlaybackState,
}

struct AudioProbeRequest {
    generation: u64,
    source: MediaSource,
    decoded_audio: Option<Arc<DecodedAudio>>,
}

/// A clonable controller for audio playback.
#[derive(Clone)]
pub struct AudioHandle {
    state: Arc<Mutex<AudioHandleState>>,
}

impl fmt::Debug for AudioHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock();
        f.debug_struct("AudioHandle")
            .field("source", &state.source)
            .field("volume", &state.volume)
            .field("duration", &state.duration)
            .field("position", &state.current_position())
            .field("state", &state.state)
            .finish()
    }
}

impl AudioHandle {
    /// Create a new audio handle for the given source.
    pub fn new(source: impl Into<MediaSource>) -> Self {
        Self {
            state: Arc::new(Mutex::new(AudioHandleState {
                source: source.into(),
                volume: 1.0,
                duration: None,
                decoded_audio: None,
                position: Duration::ZERO,
                started_at: None,
                state: PlaybackState::Stopped,
                engine: None,
                generation: 0,
            })),
        }
    }

    /// Start or resume playback.
    pub fn play(&self) -> Result<(), AudioPlaybackError> {
        let request = {
            let mut state = self.state.lock();
            state.refresh_finished();
            if state.state == PlaybackState::Playing {
                return Ok(());
            }

            if state.state == PlaybackState::Paused
                && let Some(engine) = state.engine.as_ref()
            {
                engine.sink.play();
                state.started_at = Some(Instant::now());
                state.state = PlaybackState::Playing;
                state.generation += 1;
                return Ok(());
            }

            let requested_position = if state
                .duration
                .is_some_and(|duration| state.position >= duration)
            {
                Duration::ZERO
            } else {
                state.position
            };

            AudioPlaybackRequest {
                generation: state.generation,
                source: state.source.clone(),
                volume: state.volume,
                position: requested_position,
                duration: state.duration,
                decoded_audio: state.decoded_audio.clone(),
                playback_state: state.state,
            }
        };

        let (engine, duration, position, decoded_audio) = create_engine_with_cache(
            &request.source,
            request.volume,
            request.position,
            request.decoded_audio,
        )?;

        let mut state = self.state.lock();
        state.refresh_finished();
        if state.generation != request.generation || state.state == PlaybackState::Playing {
            return Ok(());
        }

        if let Some(decoded_audio) = decoded_audio {
            state.decoded_audio = Some(decoded_audio);
        }
        state.duration = duration.or(state.duration);
        state.position = position;
        state.started_at = Some(Instant::now());
        state.state = PlaybackState::Playing;
        engine.sink.set_volume(state.volume.max(0.0));
        state.engine = Some(engine);
        state.generation += 1;
        Ok(())
    }

    /// Pause playback, preserving the current position.
    pub fn pause(&self) {
        let mut state = self.state.lock();
        state.refresh_finished();
        if state.state != PlaybackState::Playing {
            return;
        }

        state.position = state.current_position();
        state.started_at = None;
        if let Some(engine) = state.engine.as_ref() {
            engine.sink.pause();
        }
        state.state = PlaybackState::Paused;
        state.generation += 1;
    }

    /// Stop playback and reset the position to the start.
    pub fn stop(&self) {
        let mut state = self.state.lock();
        if let Some(engine) = state.engine.take() {
            engine.sink.stop();
        }
        state.position = Duration::ZERO;
        state.started_at = None;
        state.state = PlaybackState::Stopped;
        state.generation += 1;
    }

    /// Seek to the given playback position.
    pub fn seek(&self, position: Duration) -> Result<(), AudioPlaybackError> {
        let request = {
            let mut state = self.state.lock();
            state.refresh_finished();
            AudioPlaybackRequest {
                generation: state.generation,
                source: state.source.clone(),
                volume: state.volume,
                position,
                duration: state.duration,
                decoded_audio: state.decoded_audio.clone(),
                playback_state: state.state,
            }
        };

        let (duration, decoded_audio) = match request.duration {
            Some(duration) => (Some(duration), request.decoded_audio.clone()),
            None => probe_duration_with_cache(&request.source, request.decoded_audio.clone())?,
        };
        let clamped_position = duration
            .map(|duration| position.min(duration))
            .unwrap_or(position);
        let (engine, duration, actual_position, decoded_audio) =
            if request.playback_state == PlaybackState::Playing {
                let (engine, actual_duration, actual_position, decoded_audio) =
                    create_engine_with_cache(
                        &request.source,
                        request.volume,
                        clamped_position,
                        decoded_audio,
                    )?;
                (
                    Some(engine),
                    actual_duration.or(duration),
                    actual_position,
                    decoded_audio,
                )
            } else {
                (None, duration, clamped_position, decoded_audio)
            };

        let mut state = self.state.lock();
        state.refresh_finished();
        if state.generation != request.generation {
            return Ok(());
        }

        if let Some(decoded_audio) = decoded_audio {
            state.decoded_audio = Some(decoded_audio);
        }
        state.duration = duration.or(state.duration);
        state.position = actual_position;
        state.started_at = if request.playback_state == PlaybackState::Playing {
            Some(Instant::now())
        } else {
            None
        };
        if let Some(engine) = engine {
            engine.sink.set_volume(state.volume.max(0.0));
            state.engine = Some(engine);
        } else {
            state.engine = None;
        }
        state.generation += 1;

        Ok(())
    }

    /// Set the playback volume where `1.0` is the original amplitude.
    pub fn set_volume(&self, volume: f32) {
        let mut state = self.state.lock();
        let clamped_volume = volume.max(0.0);
        state.volume = clamped_volume;
        if let Some(engine) = state.engine.as_ref() {
            engine.sink.set_volume(clamped_volume);
        }
    }

    /// Return the current playback volume.
    pub fn volume(&self) -> f32 {
        self.state.lock().volume
    }

    /// Return the current playback state.
    pub fn state(&self) -> PlaybackState {
        let mut state = self.state.lock();
        state.refresh_finished();
        state.state
    }

    /// Return the current playback position.
    pub fn position(&self) -> Duration {
        let mut state = self.state.lock();
        state.refresh_finished();
        state.current_position()
    }

    /// Return the total duration if it can be determined from the media source.
    pub fn duration(&self) -> Result<Option<Duration>, AudioPlaybackError> {
        let request = {
            let state = self.state.lock();
            if state.duration.is_some() {
                return Ok(state.duration);
            }

            AudioProbeRequest {
                generation: state.generation,
                source: state.source.clone(),
                decoded_audio: state.decoded_audio.clone(),
            }
        };

        let (duration, decoded_audio) =
            probe_duration_with_cache(&request.source, request.decoded_audio)?;

        let mut state = self.state.lock();
        if state.generation == request.generation && state.duration.is_none() {
            if let Some(decoded_audio) = decoded_audio {
                state.decoded_audio = Some(decoded_audio);
            }
            state.duration = duration;
        }
        Ok(state.duration.or(duration))
    }

    /// Return the source that this audio handle will play.
    pub fn source(&self) -> MediaSource {
        self.state.lock().source.clone()
    }
}

/// Probe the total duration for an audio source without constructing a playback handle.
pub fn probe_audio_duration(
    source: impl Into<MediaSource>,
) -> Result<Option<Duration>, AudioPlaybackError> {
    let source = source.into();
    probe_duration(&source, &mut None)
}

impl AudioHandleState {
    fn current_position(&self) -> Duration {
        let position = if self.state == PlaybackState::Playing {
            self.started_at
                .map(|started_at| self.position + started_at.elapsed())
                .unwrap_or(self.position)
        } else {
            self.position
        };

        self.duration
            .map(|duration| position.min(duration))
            .unwrap_or(position)
    }

    fn refresh_finished(&mut self) {
        if self.state != PlaybackState::Playing {
            return;
        }

        let finished = self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.sink.empty());
        let position = self.current_position();
        let reached_end = self.duration.is_some_and(|duration| position >= duration);
        if !finished && !reached_end {
            return;
        }

        self.position = self.duration.unwrap_or(position);
        self.started_at = None;
        self.state = PlaybackState::Stopped;
        self.engine = None;
    }
}

fn probe_duration(
    source: &MediaSource,
    decoded_audio: &mut Option<Arc<DecodedAudio>>,
) -> Result<Option<Duration>, AudioPlaybackError> {
    match try_create_decoder(source)? {
        Some(decoder) => Ok(decoder.total_duration()),
        None => {
            let decoded_audio = ensure_decoded_audio(source, decoded_audio)
                .map_err(|decode_error| AudioPlaybackError::Decoder(decode_error.to_string()))?;
            Ok(Some(decoded_audio.duration))
        }
    }
}

fn probe_duration_with_cache(
    source: &MediaSource,
    decoded_audio: Option<Arc<DecodedAudio>>,
) -> Result<(Option<Duration>, Option<Arc<DecodedAudio>>), AudioPlaybackError> {
    let mut decoded_audio = decoded_audio;
    let duration = probe_duration(source, &mut decoded_audio)?;
    Ok((duration, decoded_audio))
}

fn create_engine_with_cache(
    source: &MediaSource,
    volume: f32,
    position: Duration,
    decoded_audio: Option<Arc<DecodedAudio>>,
) -> Result<
    (
        AudioEngine,
        Option<Duration>,
        Duration,
        Option<Arc<DecodedAudio>>,
    ),
    AudioPlaybackError,
> {
    let mut decoded_audio = decoded_audio;
    let (engine, duration, clamped_position) =
        create_engine(source, volume, position, &mut decoded_audio)?;
    Ok((engine, duration, clamped_position, decoded_audio))
}

fn create_engine(
    source: &MediaSource,
    volume: f32,
    position: Duration,
    decoded_audio: &mut Option<Arc<DecodedAudio>>,
) -> Result<(AudioEngine, Option<Duration>, Duration), AudioPlaybackError> {
    let (stream, stream_handle) = OutputStream::try_default()
        .map_err(|error| AudioPlaybackError::Output(error.to_string()))?;
    let sink = Sink::try_new(&stream_handle)
        .map_err(|error| AudioPlaybackError::Output(error.to_string()))?;
    let (duration, clamped_position) = match try_create_decoder(source)? {
        Some(decoder) => {
            let duration = decoder.total_duration();
            let clamped_position = duration
                .map(|duration| position.min(duration))
                .unwrap_or(position);
            sink.append(decoder.skip_duration(clamped_position));
            (duration, clamped_position)
        }
        None => {
            let decoded_audio = ensure_decoded_audio(source, decoded_audio)
                .map_err(|decode_error| AudioPlaybackError::Decoder(decode_error.to_string()))?;
            let clamped_position = position.min(decoded_audio.duration);
            sink.append(
                SamplesBuffer::new(
                    decoded_audio.channels,
                    decoded_audio.sample_rate,
                    decoded_audio.samples.as_ref().to_vec(),
                )
                .skip_duration(clamped_position),
            );
            (Some(decoded_audio.duration), clamped_position)
        }
    };
    sink.set_volume(volume.max(0.0));
    Ok((
        AudioEngine {
            _stream: stream,
            sink,
        },
        duration,
        clamped_position,
    ))
}

fn ensure_decoded_audio(
    source: &MediaSource,
    decoded_audio: &mut Option<Arc<DecodedAudio>>,
) -> Result<Arc<DecodedAudio>, MediaDecodeError> {
    if let Some(decoded_audio) = decoded_audio.as_ref() {
        return Ok(decoded_audio.clone());
    }

    let decoded = Arc::new(decode_audio(source)?);
    *decoded_audio = Some(decoded.clone());
    Ok(decoded)
}

fn decode_audio(source: &MediaSource) -> Result<DecodedAudio, MediaDecodeError> {
    ffmpeg::init().map_err(ffmpeg_decode_error)?;

    let mut input_context = source
        .resolve_ffmpeg_input()?
        .open_input()
        .map_err(ffmpeg_decode_error)?;
    let input_stream = input_context
        .streams()
        .best(ffmpeg::media::Type::Audio)
        .ok_or_else(|| MediaDecodeError::Decode("no audio stream found".into()))?;
    let audio_stream_index = input_stream.index();

    let context_decoder =
        ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())
            .map_err(ffmpeg_decode_error)?;
    let mut decoder = context_decoder
        .decoder()
        .audio()
        .map_err(ffmpeg_decode_error)?;

    let channel_layout = if decoder.channel_layout().is_empty() {
        ffmpeg::ChannelLayout::default(decoder.channels().into())
    } else {
        decoder.channel_layout()
    };
    let sample_rate = decoder.rate();
    let mut resampler = ffmpeg::software::resampling::context::Context::get(
        decoder.format(),
        channel_layout,
        sample_rate,
        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
        channel_layout,
        sample_rate,
    )
    .map_err(ffmpeg_decode_error)?;

    let mut samples = Vec::new();

    let mut receive_and_process_decoded_frames = |decoder: &mut ffmpeg::decoder::Audio,
                                                  samples: &mut Vec<f32>|
     -> Result<(), MediaDecodeError> {
        let mut decoded = ffmpeg::util::frame::Audio::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let mut output = ffmpeg::util::frame::Audio::empty();
            resampler
                .run(&decoded, &mut output)
                .map_err(ffmpeg_decode_error)?;
            samples.extend_from_slice(output.plane::<f32>(0));
        }

        Ok(())
    };

    for (stream, packet) in input_context.packets() {
        if stream.index() == audio_stream_index {
            decoder.send_packet(&packet).map_err(ffmpeg_decode_error)?;
            receive_and_process_decoded_frames(&mut decoder, &mut samples)?;
        }
    }

    decoder.send_eof().map_err(ffmpeg_decode_error)?;
    receive_and_process_decoded_frames(&mut decoder, &mut samples)?;

    loop {
        let mut output = ffmpeg::util::frame::Audio::empty();
        let delayed = resampler.flush(&mut output).map_err(ffmpeg_decode_error)?;
        if output.samples() > 0 {
            samples.extend_from_slice(output.plane::<f32>(0));
        }
        if delayed.is_none() {
            break;
        }
    }

    let channels = channel_layout.channels() as u16;
    let duration = if channels == 0 || sample_rate == 0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(samples.len() as f64 / channels as f64 / sample_rate as f64)
    };

    Ok(DecodedAudio {
        channels,
        sample_rate,
        samples: Arc::<[f32]>::from(samples),
        duration,
    })
}

fn open_video_stream(source: &MediaSource) -> Result<OpenedVideoStream, MediaDecodeError> {
    ffmpeg::init().map_err(ffmpeg_decode_error)?;

    let input_context = source
        .resolve_ffmpeg_input()?
        .open_input()
        .map_err(ffmpeg_decode_error)?;
    let input_stream = input_context
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or(MediaDecodeError::NoVideoStream)?;
    let video_stream_index = input_stream.index();
    let time_base = input_stream.time_base();
    let duration = if input_stream.duration() > 0 {
        Some(duration_from_time_base(input_stream.duration(), time_base))
    } else if input_context.duration() > 0 {
        Some(duration_from_time_base(
            input_context.duration(),
            ffmpeg::util::mathematics::rescale::TIME_BASE,
        ))
    } else {
        None
    };

    let context_decoder =
        ffmpeg::codec::context::Context::from_parameters(input_stream.parameters())
            .map_err(ffmpeg_decode_error)?;
    let decoder = context_decoder
        .decoder()
        .video()
        .map_err(ffmpeg_decode_error)?;
    let width = decoder.width();
    let height = decoder.height();
    let scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        width,
        height,
        ffmpeg::format::Pixel::BGRA,
        width,
        height,
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(ffmpeg_decode_error)?;

    Ok(OpenedVideoStream {
        input_context,
        decoder,
        scaler,
        video_stream_index,
        time_base,
        metadata: VideoMetadata {
            width,
            height,
            duration,
        },
    })
}

fn decode_video_frame(
    scaler: &mut ffmpeg::software::scaling::context::Context,
    decoded: &ffmpeg::util::frame::video::Video,
    time_base: ffmpeg::Rational,
) -> Result<VideoFrame, MediaDecodeError> {
    let mut bgra_frame = ffmpeg::util::frame::video::Video::empty();
    scaler
        .run(decoded, &mut bgra_frame)
        .map_err(ffmpeg_decode_error)?;

    Ok(VideoFrame {
        data: Arc::<[u8]>::from(copy_bgra_frame(&bgra_frame)),
        width: bgra_frame.width(),
        height: bgra_frame.height(),
        timestamp: duration_from_time_base(decoded.timestamp().unwrap_or_default(), time_base),
    })
}

fn try_create_decoder(
    source: &MediaSource,
) -> Result<Option<Decoder<MediaReader>>, AudioPlaybackError> {
    if !source.direct_reader_supported() {
        return Ok(None);
    }

    match Decoder::new(source.open_reader()?) {
        Ok(decoder) => Ok(Some(decoder)),
        Err(_) => Ok(None),
    }
}

impl ReaderMediaSource {
    fn stage_to_path(&self) -> io::Result<PathBuf> {
        if let Some(path) = self.staged_path.lock().clone() {
            return Ok(path);
        }

        let path = staged_media_dir().join(format!(
            "reader-{:016x}-{}",
            hash_value(&self.key),
            STAGED_READER_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        write_path_atomically(&path, |file| {
            let mut reader = (self.open)()?;
            io::copy(&mut reader, file)?;
            Ok(())
        })?;
        *self.staged_path.lock() = Some(path.clone());
        Ok(path)
    }
}

impl ResolvedMediaInput {
    fn open_input(&self) -> Result<ffmpeg::format::context::Input, ffmpeg::Error> {
        match self {
            Self::Path(path) => ffmpeg::format::input(path),
            Self::Url(url) => {
                ffmpeg::format::network::init();
                ffmpeg::format::input(url.as_ref())
            }
        }
    }
}

fn stage_bytes(bytes: &Arc<[u8]>) -> Result<PathBuf, MediaDecodeError> {
    let path = staged_media_dir().join(format!("bytes-{:016x}", hash_value(bytes)));
    write_path_atomically(&path, |file| {
        file.write_all(bytes.as_ref())?;
        Ok(())
    })
    .map_err(MediaDecodeError::from_io)?;
    Ok(path)
}

fn staged_media_dir() -> PathBuf {
    std::env::temp_dir().join("kael-media")
}

fn write_path_atomically(
    path: &Path,
    populate: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary_path = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        STAGED_READER_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    let populate_result = populate(&mut file).and_then(|_| file.flush());
    if let Err(error) = populate_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    match fs::rename(&temporary_path, path) {
        Ok(()) => Ok(()),
        Err(_error) if path.exists() => {
            let _ = fs::remove_file(&temporary_path);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary_path);
            Err(error)
        }
    }
}

fn hash_value(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl MediaDecodeError {
    fn from_io(error: io::Error) -> Self {
        Self::Decode(error.to_string())
    }
}

fn ffmpeg_decode_error(error: ffmpeg::Error) -> MediaDecodeError {
    MediaDecodeError::Decode(error.to_string())
}

fn duration_from_time_base(timestamp: i64, time_base: ffmpeg::Rational) -> Duration {
    if timestamp <= 0 {
        return Duration::ZERO;
    }

    Duration::from_secs_f64((timestamp as f64) * f64::from(time_base))
}

fn copy_bgra_frame(frame: &ffmpeg::util::frame::video::Video) -> Box<[u8]> {
    let width = frame.width() as usize;
    let height = frame.height() as usize;
    let row_len = width * 4;
    let stride = frame.stride(0);
    let source = frame.data(0);
    let mut bytes = vec![0u8; row_len * height];

    for row in 0..height {
        let source_offset = row * stride;
        let destination_offset = row * row_len;
        bytes[destination_offset..destination_offset + row_len]
            .copy_from_slice(&source[source_offset..source_offset + row_len]);
    }

    bytes.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::{
        AudioHandle, MAX_DECODED_VIDEO_BYTES, MAX_DECODED_VIDEO_FRAMES, MediaDecodeError,
        MediaDecoder, MediaSource, PlaybackState, VideoFrame, push_decoded_video_frame,
    };
    use std::{io::Cursor, sync::Arc, time::Duration};

    #[test]
    fn duration_probe_works_for_memory_backed_wav() {
        let handle = AudioHandle::new(MediaSource::bytes(silent_wav(8_000, 8_000)));

        assert_eq!(handle.state(), PlaybackState::Stopped);
        assert_eq!(handle.duration().unwrap(), Some(Duration::from_secs(1)));
        assert_eq!(handle.position(), Duration::ZERO);
    }

    #[test]
    fn seek_updates_position_without_starting_playback() {
        let handle = AudioHandle::new(MediaSource::bytes(silent_wav(8_000, 8_000)));

        handle.seek(Duration::from_millis(250)).unwrap();

        assert_eq!(handle.state(), PlaybackState::Stopped);
        assert_eq!(handle.position(), Duration::from_millis(250));
    }

    #[test]
    fn duration_probe_works_for_reader_backed_wav() {
        let wav = Arc::<[u8]>::from(silent_wav(8_000, 8_000));
        let handle = AudioHandle::new(MediaSource::reader("reader-wav", {
            let wav = wav.clone();
            move || Ok(Cursor::new(wav.clone()))
        }));

        assert_eq!(handle.duration().unwrap(), Some(Duration::from_secs(1)));
        assert_eq!(handle.position(), Duration::ZERO);
        assert_eq!(handle.state(), PlaybackState::Stopped);
    }

    #[test]
    fn stop_resets_position() {
        let handle = AudioHandle::new(MediaSource::bytes(silent_wav(8_000, 8_000)));

        handle.seek(Duration::from_millis(300)).unwrap();
        handle.stop();

        assert_eq!(handle.position(), Duration::ZERO);
        assert_eq!(handle.state(), PlaybackState::Stopped);
    }

    #[test]
    fn video_decoder_stages_in_memory_sources_before_decode() {
        let decoder = MediaDecoder::new(MediaSource::bytes([0u8; 16]));

        assert!(matches!(
            decoder.video_metadata().unwrap_err(),
            MediaDecodeError::Decode(_) | MediaDecodeError::NoVideoStream
        ));
        assert!(matches!(
            decoder.decode_video_frames().unwrap_err(),
            MediaDecodeError::Decode(_) | MediaDecodeError::NoVideoStream
        ));
    }

    #[test]
    fn video_decoder_accepts_reader_backed_sources() {
        let payload = Arc::<[u8]>::from([0u8; 16]);
        let decoder = MediaDecoder::new(MediaSource::reader("reader-video", {
            let payload = payload.clone();
            move || Ok(Cursor::new(payload.clone()))
        }));

        assert!(matches!(
            decoder.video_metadata().unwrap_err(),
            MediaDecodeError::Decode(_) | MediaDecodeError::NoVideoStream
        ));
        assert!(matches!(
            decoder.decode_video_frames().unwrap_err(),
            MediaDecodeError::Decode(_) | MediaDecodeError::NoVideoStream
        ));
    }

    #[test]
    fn full_video_decode_rejects_excessive_frame_counts() {
        let mut frames = Vec::new();
        let mut decoded_bytes = 0u64;

        for index in 0..MAX_DECODED_VIDEO_FRAMES {
            push_decoded_video_frame(
                &mut frames,
                &mut decoded_bytes,
                test_video_frame(index as u64, 4),
            )
            .unwrap();
        }

        let error = push_decoded_video_frame(
            &mut frames,
            &mut decoded_bytes,
            test_video_frame(MAX_DECODED_VIDEO_FRAMES as u64, 4),
        )
        .unwrap_err();

        assert!(matches!(error, MediaDecodeError::Decode(message) if message.contains("frames")));
    }

    #[test]
    fn full_video_decode_rejects_excessive_byte_counts() {
        let mut frames = Vec::new();
        let mut decoded_bytes = MAX_DECODED_VIDEO_BYTES - 1;

        let error =
            push_decoded_video_frame(&mut frames, &mut decoded_bytes, test_video_frame(0, 2))
                .unwrap_err();

        assert!(matches!(error, MediaDecodeError::Decode(message) if message.contains("bytes")));
    }

    fn test_video_frame(timestamp_millis: u64, len: usize) -> VideoFrame {
        VideoFrame {
            data: Arc::<[u8]>::from(vec![0; len]),
            width: 1,
            height: 1,
            timestamp: Duration::from_millis(timestamp_millis),
        }
    }

    fn silent_wav(sample_rate: u32, samples: u32) -> Vec<u8> {
        let channels = 1u16;
        let bits_per_sample = 16u16;
        let bytes_per_sample = (bits_per_sample / 8) as u32;
        let data_len = samples * channels as u32 * bytes_per_sample;
        let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
        let block_align = channels * (bits_per_sample / 8);
        let chunk_size = 36 + data_len;

        let mut wav = Vec::with_capacity((44 + data_len) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&chunk_size.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(44 + data_len as usize, 0);
        wav
    }
}
