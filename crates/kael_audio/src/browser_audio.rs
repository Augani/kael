//! Shared browser Web Audio types and worklet support.

use std::error::Error;
use std::fmt;

use js_sys::{Array, Reflect};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{AudioContext, Blob, BlobPropertyBag, Url};

/// Maximum browser audio devices returned by one enumeration.
pub const MAX_BROWSER_AUDIO_DEVICES: usize = 1_024;
/// Maximum bytes retained from a browser-provided device label or identifier.
pub const MAX_BROWSER_DEVICE_TEXT_BYTES: usize = 1_024;
/// Maximum output or capture channels supported by the portable worklet bridge.
pub const MAX_BROWSER_AUDIO_CHANNELS: u16 = 8;
/// Maximum frames in one worklet transfer.
pub const MAX_BROWSER_AUDIO_CHUNK_FRAMES: usize = 4_096;
/// Maximum transferred chunks pending on either side of a worklet port.
pub const MAX_BROWSER_AUDIO_PENDING_CHUNKS: usize = 32;

pub(crate) const MIN_BROWSER_AUDIO_CHUNK_FRAMES: usize = 128;
pub(crate) const MIN_BROWSER_AUDIO_PENDING_CHUNKS: usize = 2;

const WORKLET_SOURCE: &str = r#"
class KaelOutputProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const config = options.processorOptions || {};
    this.channels = Math.max(1, Math.min(8, config.channels | 0));
    this.chunkFrames = Math.max(128, Math.min(4096, config.chunkFrames | 0));
    this.queueLimit = Math.max(2, Math.min(32, config.queueChunks | 0));
    this.queue = [];
    this.current = null;
    this.offset = 0;
    this.rendered = 0;
    this.underruns = 0;
    this.needOutstanding = false;
    this.idle = true;
    this.stopped = false;
    this.port.onmessage = event => {
      const data = event.data;
      if (data && data.type === "chunks" && Array.isArray(data.chunks)) {
        for (const chunk of data.chunks) {
          if (chunk instanceof Float32Array &&
              chunk.length === this.chunkFrames * this.channels &&
              this.queue.length < this.queueLimit) {
            this.queue.push(chunk);
          }
        }
        this.needOutstanding = false;
        this.idle = false;
        this.requestIfNeeded();
      } else if (data && data.type === "idle") {
        this.needOutstanding = false;
        this.idle = true;
      } else if (data && data.type === "wake") {
        this.idle = false;
        this.requestIfNeeded();
      } else if (data && data.type === "stop") {
        this.stopped = true;
        this.queue.length = 0;
        this.current = null;
      }
    };
  }

  requestIfNeeded() {
    if (this.stopped || this.idle || this.needOutstanding) return;
    const buffered = this.queue.length + (this.current ? 1 : 0);
    if (buffered >= this.queueLimit) return;
    this.needOutstanding = true;
    this.port.postMessage({
      type: "need",
      count: this.queueLimit - buffered,
      rendered: this.rendered,
      underruns: this.underruns
    });
  }

  process(_inputs, outputs) {
    if (this.stopped) return false;
    const output = outputs[0];
    if (!output || output.length === 0) return true;
    const frames = output[0].length;
    let underrunFrames = 0;
    for (let frame = 0; frame < frames; frame += 1) {
      if (!this.current || this.offset >= this.current.length) {
        this.current = this.queue.shift() || null;
        this.offset = 0;
      }
      for (let channel = 0; channel < output.length; channel += 1) {
        let sample = 0;
        if (this.current) {
          sample = this.current[this.offset + Math.min(channel, this.channels - 1)] || 0;
          if (!Number.isFinite(sample)) sample = 0;
          sample = Math.max(-1, Math.min(1, sample));
        } else {
          if (!this.idle) underrunFrames += 1;
        }
        output[channel][frame] = sample;
      }
      if (this.current) this.offset += this.channels;
    }
    this.rendered += frames;
    this.underruns += underrunFrames;
    this.requestIfNeeded();
    return true;
  }
}

class KaelCaptureProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const config = options.processorOptions || {};
    this.channels = Math.max(1, Math.min(8, config.channels | 0));
    this.chunkFrames = Math.max(128, Math.min(4096, config.chunkFrames | 0));
    this.maxCredits = Math.max(2, Math.min(32, config.pendingChunks | 0));
    this.credits = this.maxCredits;
    this.buffer = new Float32Array(this.chunkFrames * this.channels);
    this.cursor = 0;
    this.sequence = 0;
    this.dropped = 0;
    this.stopped = false;
    this.port.onmessage = event => {
      const data = event.data;
      if (data && data.type === "credit") {
        const count = Math.max(0, Math.min(this.maxCredits, data.count | 0));
        this.credits = Math.min(this.maxCredits, this.credits + count);
      } else if (data && data.type === "stop") {
        this.stopped = true;
      }
    };
  }

  process(inputs, outputs) {
    if (this.stopped) return false;
    const output = outputs[0];
    if (output) {
      for (const channel of output) channel.fill(0);
    }
    const input = inputs[0];
    if (!input || input.length === 0 || input[0].length === 0) return true;
    const frames = input[0].length;
    for (let frame = 0; frame < frames; frame += 1) {
      if (this.credits <= 0) {
        this.dropped += 1;
        continue;
      }
      for (let channel = 0; channel < this.channels; channel += 1) {
        let sample = 0;
        if (this.channels === 1 && input.length > 1) {
          for (const source of input) sample += source[frame] || 0;
          sample /= input.length;
        } else {
          sample = input[Math.min(channel, input.length - 1)][frame] || 0;
        }
        this.buffer[this.cursor++] = Number.isFinite(sample) ? sample : 0;
      }
      if (this.cursor === this.buffer.length) {
        const samples = this.buffer;
        this.buffer = new Float32Array(this.chunkFrames * this.channels);
        this.cursor = 0;
        this.credits -= 1;
        this.port.postMessage({
          type: "capture",
          samples,
          sequence: this.sequence++,
          dropped: this.dropped
        }, [samples.buffer]);
      }
    }
    return true;
  }
}

registerProcessor("kael-output-v1", KaelOutputProcessor);
registerProcessor("kael-capture-v1", KaelCaptureProcessor);
"#;

/// Stable browser audio failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAudioErrorKind {
    /// A configuration exceeded the documented browser audio bounds.
    InvalidConfiguration,
    /// The page is not in a secure context or the Web Audio API is unavailable.
    ApiUnavailable,
    /// The browser rejected an operation that requires a user gesture.
    UserActivationRequired,
    /// Microphone access was denied by the user, browser, or page policy.
    PermissionDenied,
    /// No matching browser audio device is currently available.
    DeviceUnavailable,
    /// The browser cannot route an `AudioContext` to the selected non-default sink.
    OutputRoutingUnsupported,
    /// AudioWorklet module installation or graph construction failed.
    WorkletUnavailable,
    /// A bounded worklet or voice queue is full.
    Backpressure,
    /// The browser audio object has already been stopped or closed.
    Closed,
    /// The browser reported an asynchronous worklet processor failure.
    Processor,
}

/// Typed, content-safe browser audio failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAudioError {
    kind: BrowserAudioErrorKind,
}

impl BrowserAudioError {
    /// Construct an error from its stable category.
    pub const fn new(kind: BrowserAudioErrorKind) -> Self {
        Self { kind }
    }

    /// Return the stable failure category.
    pub const fn kind(self) -> BrowserAudioErrorKind {
        self.kind
    }
}

impl fmt::Display for BrowserAudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            BrowserAudioErrorKind::InvalidConfiguration => {
                "invalid bounded browser audio configuration"
            }
            BrowserAudioErrorKind::ApiUnavailable => {
                "browser audio API is unavailable in this page context"
            }
            BrowserAudioErrorKind::UserActivationRequired => {
                "browser audio requires a transient user activation"
            }
            BrowserAudioErrorKind::PermissionDenied => "browser microphone permission was denied",
            BrowserAudioErrorKind::DeviceUnavailable => {
                "requested browser audio device is unavailable"
            }
            BrowserAudioErrorKind::OutputRoutingUnsupported => {
                "non-default browser audio output routing is unsupported"
            }
            BrowserAudioErrorKind::WorkletUnavailable => {
                "browser AudioWorklet could not be initialized"
            }
            BrowserAudioErrorKind::Backpressure => "browser audio queue is full",
            BrowserAudioErrorKind::Closed => "browser audio object is closed",
            BrowserAudioErrorKind::Processor => "browser audio processor failed",
        })
    }
}

impl Error for BrowserAudioError {}

/// Typed browser audio lifecycle and pressure event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserAudioEvent {
    /// The audio context entered its running state.
    Running,
    /// Output rendered silence because the bounded queue was not replenished in time.
    OutputUnderrun {
        /// Cumulative underrun frames reported by the worklet.
        total_frames: u64,
    },
    /// Capture dropped frames because all bounded delivery credits were in flight.
    CaptureOverflow {
        /// Cumulative frames dropped before delivery to application code.
        total_frames: u64,
    },
    /// The application capture callback panicked and was permanently disabled.
    CaptureCallbackDisabled,
    /// The browser ended the microphone track or removed its device.
    CaptureEnded,
    /// The browser terminated an AudioWorklet processor.
    ProcessorError,
    /// The graph was deterministically disconnected and closed.
    Closed,
}

pub(crate) fn validate_worklet_bounds(
    channels: u16,
    chunk_frames: usize,
    pending_chunks: usize,
) -> Result<(), BrowserAudioError> {
    if !(1..=MAX_BROWSER_AUDIO_CHANNELS).contains(&channels)
        || !(MIN_BROWSER_AUDIO_CHUNK_FRAMES..=MAX_BROWSER_AUDIO_CHUNK_FRAMES)
            .contains(&chunk_frames)
        || !chunk_frames.is_multiple_of(128)
        || !(MIN_BROWSER_AUDIO_PENDING_CHUNKS..=MAX_BROWSER_AUDIO_PENDING_CHUNKS)
            .contains(&pending_chunks)
        || chunk_frames
            .checked_mul(usize::from(channels))
            .and_then(|samples| samples.checked_mul(pending_chunks))
            .is_none()
    {
        return Err(BrowserAudioError::new(
            BrowserAudioErrorKind::InvalidConfiguration,
        ));
    }
    Ok(())
}

pub(crate) async fn install_worklet(context: &AudioContext) -> Result<(), BrowserAudioError> {
    let parts = Array::new();
    parts.push(&JsValue::from_str(WORKLET_SOURCE));
    let options = BlobPropertyBag::new();
    options.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(parts.as_ref(), &options)
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))?;
    let url = Url::create_object_url_with_blob(&blob)
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))?;
    let promise = context
        .audio_worklet()
        .and_then(|worklet| worklet.add_module(&url));
    let result = match promise {
        Ok(promise) => JsFuture::from(promise).await.map(|_| ()),
        Err(error) => Err(error),
    };
    let _ = Url::revoke_object_url(&url);
    result.map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::WorkletUnavailable))
}

pub(crate) struct PendingAudioContext {
    context: Option<AudioContext>,
}

impl PendingAudioContext {
    pub(crate) fn new(context: AudioContext) -> Self {
        Self {
            context: Some(context),
        }
    }

    pub(crate) fn context(&self) -> &AudioContext {
        self.context
            .as_ref()
            .expect("pending browser audio context must exist")
    }

    pub(crate) fn into_inner(mut self) -> AudioContext {
        self.context
            .take()
            .expect("pending browser audio context must exist")
    }
}

impl Drop for PendingAudioContext {
    fn drop(&mut self) {
        if let Some(context) = self.context.take() {
            close_audio_context_detached(&context);
        }
    }
}

pub(crate) fn close_audio_context_detached(context: &AudioContext) {
    if let Ok(promise) = context.close() {
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

pub(crate) fn classify_js_error(
    error: &JsValue,
    fallback: BrowserAudioErrorKind,
) -> BrowserAudioError {
    let name = Reflect::get(error, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default();
    let kind = classify_dom_error_name(&name).unwrap_or(fallback);
    BrowserAudioError::new(kind)
}

pub(crate) fn classify_dom_error_name(name: &str) -> Option<BrowserAudioErrorKind> {
    match name {
        "NotAllowedError" | "SecurityError" => Some(BrowserAudioErrorKind::PermissionDenied),
        "NotFoundError" | "OverconstrainedError" | "DevicesNotFoundError" => {
            Some(BrowserAudioErrorKind::DeviceUnavailable)
        }
        "InvalidStateError" => Some(BrowserAudioErrorKind::Closed),
        _ => None,
    }
}

pub(crate) fn bounded_browser_text(mut value: String) -> String {
    if value.len() <= MAX_BROWSER_DEVICE_TEXT_BYTES {
        return value;
    }
    let mut boundary = MAX_BROWSER_DEVICE_TEXT_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worklet_bounds_reject_unbounded_or_non_quantized_buffers() {
        assert!(validate_worklet_bounds(2, 256, 4).is_ok());
        assert!(validate_worklet_bounds(0, 256, 4).is_err());
        assert!(validate_worklet_bounds(2, 129, 4).is_err());
        assert!(validate_worklet_bounds(2, 256, 33).is_err());
    }

    #[test]
    fn permission_and_device_failures_are_stably_classified() {
        assert_eq!(
            classify_dom_error_name("NotAllowedError"),
            Some(BrowserAudioErrorKind::PermissionDenied)
        );
        assert_eq!(
            classify_dom_error_name("NotFoundError"),
            Some(BrowserAudioErrorKind::DeviceUnavailable)
        );
        assert_eq!(classify_dom_error_name("UnknownError"), None);
    }

    #[test]
    fn browser_text_is_utf8_and_byte_bounded() {
        let value = bounded_browser_text("é".repeat(MAX_BROWSER_DEVICE_TEXT_BYTES));
        assert!(value.len() <= MAX_BROWSER_DEVICE_TEXT_BYTES);
        assert!(value.is_char_boundary(value.len()));
    }
}
