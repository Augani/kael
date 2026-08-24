//! Bounded audio playback and incremental video decoding primitives for Kael.
//!
//! The crate accepts regular local files, in-memory bytes, repeatable reader factories, and
//! credential-free HTTP(S) URLs through [`MediaSource`]. Remote reads have a 30-second I/O timeout.
//! Applications that accept untrusted URLs must apply their own host and network policy. Byte and
//! reader sources are staged in a private temporary directory only when FFmpeg needs a path; their
//! staged files are removed when the last source clone is dropped.
//!
//! [`AudioHandle`] is a UI-thread-local playback controller. Opening an output device, probing
//! remote media, and fallback FFmpeg decoding are synchronous operations, so applications should
//! keep potentially slow preparation off their UI thread. Common formats supported directly by
//! Rodio stream into the output sink; the FFmpeg audio fallback is decoded into a bounded 128 MiB
//! buffer. Video should normally be consumed incrementally with [`VideoFrameStream`]; individual
//! decoded BGRA frames are also capped at 128 MiB.
//!
//! Browser builds preserve the source and controller API while returning explicit unsupported
//! errors for operations that require native decoding or audio output. URL playback is hosted by
//! Kael's browser media-element route.
//!
//! # Example
//!
//! ```no_run
//! use kael_media::{MediaSource, VideoFrameStream};
//!
//! let mut frames = VideoFrameStream::new(MediaSource::file("clip.mp4"))?;
//! while let Some(frame) = frames.next_frame()? {
//!     // Upload `frame.data` (BGRA) to the application's renderer.
//! }
//! # Ok::<(), kael_media::MediaDecodeError>(())
//! ```

#![deny(missing_docs)]

#[cfg(all(not(target_arch = "wasm32"), feature = "native-media"))]
include!("native.rs");

#[cfg(all(target_arch = "wasm32", feature = "native-media"))]
mod web;

#[cfg(all(target_arch = "wasm32", feature = "native-media"))]
pub use web::*;
