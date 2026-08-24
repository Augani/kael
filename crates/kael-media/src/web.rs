//! WebAssembly-safe media API surface.
//!
//! Kael's browser renderer hosts URL media in an HTML media element. Native FFmpeg decoding and
//! Rodio audio output are deliberately unavailable on this target, so operations that require
//! either backend return an explicit unsupported error instead of pulling native libraries into a
//! WebAssembly build.

use std::{
    cell::RefCell,
    fmt,
    hash::{Hash, Hasher},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;

const WEB_MEDIA_UNSUPPORTED: &str = "native media decoding and audio output are unavailable on WebAssembly; use Kael's browser media element route";

trait MediaReadSeek: Read + Seek + Send + Sync {}

impl<T> MediaReadSeek for T where T: Read + Seek + Send + Sync {}

type MediaReaderFactory = dyn Fn() -> io::Result<Box<dyn MediaReadSeek>> + Send + Sync;

/// Internal backing state for keyed reader-based media sources.
#[doc(hidden)]
pub struct ReaderMediaSource {
    key: Arc<str>,
    _open: Arc<MediaReaderFactory>,
}

/// Internal backing state for byte-based media sources.
#[doc(hidden)]
pub struct BytesMediaSource {
    bytes: Arc<[u8]>,
}

/// A source of media content that can be handed to a browser media element.
#[derive(Clone)]
pub enum MediaSource {
    /// Media content identified by a file path.
    File(PathBuf),
    /// Media content loaded from a URL.
    Url(Arc<str>),
    /// Media content already available in memory.
    Bytes(Arc<BytesMediaSource>),
    /// Media content opened on demand from a keyed reader factory.
    Reader(Arc<ReaderMediaSource>),
}

impl MediaSource {
    /// Create a media source backed by a file path.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    /// Create a media source backed by a URL.
    pub fn url(url: impl Into<Arc<str>>) -> Self {
        Self::Url(url.into())
    }

    /// Create a media source backed by in-memory bytes.
    pub fn bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::Bytes(Arc::new(BytesMediaSource {
            bytes: bytes.into(),
        }))
    }

    /// Create a media source backed by a keyed reader factory.
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
            _open: open,
        }))
    }

    /// Create a media source backed by compile-time bytes.
    pub fn from_static_bytes(bytes: &'static [u8]) -> Self {
        Self::bytes(Arc::<[u8]>::from(bytes))
    }

    /// Return the content of a byte-backed media source.
    pub fn byte_data(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(source) => Some(source.bytes.as_ref()),
            _ => None,
        }
    }

    /// Return the cache key for a reader-backed media source.
    pub fn reader_key(&self) -> Option<&str> {
        match self {
            Self::Reader(source) => Some(&source.key),
            _ => None,
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
        Self::bytes(value)
    }
}

impl From<Arc<str>> for MediaSource {
    fn from(value: Arc<str>) -> Self {
        Self::Url(value)
    }
}

impl From<Vec<u8>> for MediaSource {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value)
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
            Self::Url(_) => f.debug_tuple("Url").field(&"<redacted>").finish(),
            Self::Bytes(source) => f
                .debug_tuple("Bytes")
                .field(&format_args!("{} bytes", source.bytes.len()))
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
            (Self::Bytes(left), Self::Bytes(right)) => left.bytes == right.bytes,
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
            Self::Bytes(source) => source.bytes.hash(state),
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
    /// The stream duration when the backend reports one.
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
    /// The media source could not be read.
    #[error("media I/O error: {0}")]
    Io(#[from] io::Error),
    /// The requested source or backend is unsupported.
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    /// The media source does not contain a video stream.
    #[error("no video stream found")]
    NoVideoStream,
    /// The media source does not contain an audio stream.
    #[error("no audio stream found")]
    NoAudioStream,
    /// The media backend failed to decode the source.
    #[error("media decode error: {0}")]
    Decode(String),
    /// A bounded operation exceeded its safety limit.
    #[error("media resource limit exceeded: {0}")]
    ResourceLimit(String),
}

fn unsupported_decode() -> MediaDecodeError {
    MediaDecodeError::UnsupportedSource(WEB_MEDIA_UNSUPPORTED.into())
}

/// A decoder facade for media metadata and video frames.
#[derive(Clone, Debug)]
pub struct MediaDecoder {
    source: MediaSource,
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

    /// Return an unsupported error because browser media decoding is DOM-managed.
    pub fn video_metadata(&self) -> Result<VideoMetadata, MediaDecodeError> {
        Err(unsupported_decode())
    }

    /// Return an unsupported error because browser media decoding is DOM-managed.
    pub fn decode_video_frames(&self) -> Result<Vec<VideoFrame>, MediaDecodeError> {
        Err(unsupported_decode())
    }
}

/// A sequential decoder facade for browser builds.
pub struct VideoFrameStream {
    source: MediaSource,
}

impl VideoFrameStream {
    /// Return an unsupported error because frame extraction requires a native decoder.
    pub fn new(source: impl Into<MediaSource>) -> Result<Self, MediaDecodeError> {
        let _ = source.into();
        Err(unsupported_decode())
    }

    /// Return the source associated with this stream.
    pub fn source(&self) -> &MediaSource {
        &self.source
    }

    /// Return placeholder metadata for an unreachable unsupported stream.
    pub fn metadata(&self) -> VideoMetadata {
        VideoMetadata {
            width: 0,
            height: 0,
            duration: None,
        }
    }

    /// Return an unsupported error because frame extraction requires a native decoder.
    pub fn restart(&mut self) -> Result<(), MediaDecodeError> {
        Err(unsupported_decode())
    }

    /// Return an unsupported error because frame seeking requires a native decoder.
    pub fn seek(&mut self, _position: Duration) -> Result<(), MediaDecodeError> {
        Err(unsupported_decode())
    }

    /// Return an unsupported error because frame extraction requires a native decoder.
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaDecodeError> {
        Err(unsupported_decode())
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
    Io(#[from] io::Error),
    /// The requested source or backend is unsupported.
    #[error("unsupported source: {0}")]
    UnsupportedSource(String),
    /// The media data could not be decoded.
    #[error("decoder error: {0}")]
    Decoder(String),
    /// Media preparation or decoding failed.
    #[error(transparent)]
    Media(#[from] MediaDecodeError),
    /// The host audio output stream could not be created.
    #[error("audio output error: {0}")]
    Output(String),
}

#[derive(Debug)]
struct AudioHandleState {
    source: MediaSource,
    volume: f32,
    speed: f32,
    position: Duration,
    state: PlaybackState,
}

/// A clonable audio-controller facade for browser builds.
#[derive(Clone, Debug)]
pub struct AudioHandle {
    state: Rc<RefCell<AudioHandleState>>,
}

impl AudioHandle {
    /// Create a new audio handle for the given source.
    pub fn new(source: impl Into<MediaSource>) -> Self {
        Self {
            state: Rc::new(RefCell::new(AudioHandleState {
                source: source.into(),
                volume: 1.0,
                speed: 1.0,
                position: Duration::ZERO,
                state: PlaybackState::Stopped,
            })),
        }
    }

    /// Return an unsupported error because audio output is owned by the browser media element.
    pub fn play(&self) -> Result<(), AudioPlaybackError> {
        Err(AudioPlaybackError::UnsupportedSource(
            WEB_MEDIA_UNSUPPORTED.into(),
        ))
    }

    /// Mark the facade as paused without attempting native output.
    pub fn pause(&self) {
        let mut state = self.state.borrow_mut();
        if state.state == PlaybackState::Playing {
            state.state = PlaybackState::Paused;
        }
    }

    /// Reset the facade to its stopped state.
    pub fn stop(&self) {
        let mut state = self.state.borrow_mut();
        state.position = Duration::ZERO;
        state.state = PlaybackState::Stopped;
    }

    /// Return an unsupported error because seeking is owned by the browser media element.
    pub fn seek(&self, _position: Duration) -> Result<(), AudioPlaybackError> {
        Err(AudioPlaybackError::UnsupportedSource(
            WEB_MEDIA_UNSUPPORTED.into(),
        ))
    }

    /// Set the cached playback volume, clamped to `0.0..=1.0`.
    pub fn set_volume(&self, volume: f32) {
        self.state.borrow_mut().volume = sanitize_volume(volume);
    }

    /// Set the cached playback speed, clamped to `0.5..=2.0`.
    pub fn set_speed(&self, speed: f32) {
        self.state.borrow_mut().speed = sanitize_speed(speed);
    }

    /// Return the cached playback speed.
    pub fn speed(&self) -> f32 {
        self.state.borrow().speed
    }

    /// Return the cached playback volume.
    pub fn volume(&self) -> f32 {
        self.state.borrow().volume
    }

    /// Return the facade playback state.
    pub fn state(&self) -> PlaybackState {
        self.state.borrow().state
    }

    /// Return the cached playback position.
    pub fn position(&self) -> Duration {
        self.state.borrow().position
    }

    /// Return an unsupported error because duration probing requires a media decoder.
    pub fn duration(&self) -> Result<Option<Duration>, AudioPlaybackError> {
        Err(AudioPlaybackError::UnsupportedSource(
            WEB_MEDIA_UNSUPPORTED.into(),
        ))
    }

    /// Return the source that this facade owns.
    pub fn source(&self) -> MediaSource {
        self.state.borrow().source.clone()
    }
}

/// Return an unsupported error because duration probing requires a media decoder.
pub fn probe_audio_duration(
    _source: impl Into<MediaSource>,
) -> Result<Option<Duration>, AudioPlaybackError> {
    Err(AudioPlaybackError::UnsupportedSource(
        WEB_MEDIA_UNSUPPORTED.into(),
    ))
}

fn sanitize_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn sanitize_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(0.5, 2.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_keep_browser_safe_cache_identity() {
        let url = MediaSource::url("https://example.com/video.mp4");
        assert_eq!(url, url.clone());
        assert!(!format!("{url:?}").contains("example.com"));

        let bytes = MediaSource::bytes(Arc::<[u8]>::from([1, 2, 3]));
        assert_eq!(bytes.byte_data(), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn native_decode_and_audio_fail_explicitly() {
        let source = MediaSource::url("https://example.com/video.mp4");
        let decode_error = MediaDecoder::new(source.clone())
            .video_metadata()
            .expect_err("native frame decoding must not be implied on WebAssembly");
        assert!(matches!(
            decode_error,
            MediaDecodeError::UnsupportedSource(_)
        ));

        let audio_error = AudioHandle::new(source)
            .play()
            .expect_err("native audio output must not be implied on WebAssembly");
        assert!(matches!(
            audio_error,
            AudioPlaybackError::UnsupportedSource(_)
        ));
    }

    #[test]
    fn facade_values_are_sanitized_without_native_output() {
        let audio = AudioHandle::new(MediaSource::from_static_bytes(&[1]));
        audio.set_volume(f32::NAN);
        audio.set_speed(f32::INFINITY);

        assert_eq!(audio.volume(), 0.0);
        assert_eq!(audio.speed(), 1.0);
        assert_eq!(audio.state(), PlaybackState::Stopped);
    }
}
