//! Cross-platform microphone and audio-input capture.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

use anyhow::{Context as _, Result};
use parking_lot::Mutex;

use crate::{AudioInputDevice, default_input_device, mixer::MAX_REALTIME_CHANNELS};

const CAPTURE_CHUNK_FRAMES: usize = 2 * 1024;
const MAX_ERROR_BYTES: usize = 4 * 1024;

/// The format of normalized, interleaved samples delivered by an input stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputConfig {
    /// Samples per second for each channel.
    pub sample_rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
}

/// A live audio-input stream.
///
/// The callback runs on the host audio thread and receives normalized interleaved `f32` samples.
/// It must return promptly and must not perform blocking I/O. Dropping this value stops capture.
/// With unwind-enabled builds, a callback panic is contained, disables future delivery, and is
/// returned by [`AudioInputStream::take_input_error`].
#[must_use = "dropping the stream stops audio capture"]
pub struct AudioInputStream {
    _stream: cpal::Stream,
    config: AudioInputConfig,
    last_input_error: Arc<Mutex<Option<String>>>,
}

impl AudioInputStream {
    /// Start capturing from the default input device.
    pub fn new(callback: impl FnMut(&[f32], AudioInputConfig) + Send + 'static) -> Result<Self> {
        Self::from_input_device(&default_input_device()?, callback)
    }

    /// Start capturing from a selected input device.
    pub fn from_input_device(
        device: &AudioInputDevice,
        callback: impl FnMut(&[f32], AudioInputConfig) + Send + 'static,
    ) -> Result<Self> {
        use cpal::traits::{DeviceTrait as _, StreamTrait as _};

        let supported = device
            .device
            .default_input_config()
            .with_context(|| format!("no default input config for {}", device.name()))?;
        let sample_format = supported.sample_format();
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        anyhow::ensure!(sample_rate > 0, "audio input sample rate must be non-zero");
        anyhow::ensure!(
            (1..=MAX_REALTIME_CHANNELS).contains(&channels),
            "unsupported input channel count {channels}; expected 1..={MAX_REALTIME_CHANNELS}"
        );
        let config = AudioInputConfig {
            sample_rate,
            channels,
        };
        let stream_config: cpal::StreamConfig = supported.into();
        let last_input_error = Arc::new(Mutex::new(None));
        let stream = build_input_stream(
            &device.device,
            &stream_config,
            sample_format,
            config,
            callback,
            last_input_error.clone(),
        )?;
        stream
            .play()
            .context("failed to start audio input stream")?;

        Ok(Self {
            _stream: stream,
            config,
            last_input_error,
        })
    }

    /// Return the format delivered to the capture callback.
    pub fn config(&self) -> AudioInputConfig {
        self.config
    }

    /// Take the most recent asynchronous input-stream error, if one occurred.
    pub fn take_input_error(&self) -> Option<String> {
        self.last_input_error.lock().take()
    }
}

fn build_input_stream<C>(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    input_config: AudioInputConfig,
    callback: C,
    last_input_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    C: FnMut(&[f32], AudioInputConfig) + Send + 'static,
{
    match sample_format {
        cpal::SampleFormat::F32 => build_f32_input_stream(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::I8 => build_converting_input_stream::<i8, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::I16 => build_converting_input_stream::<i16, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::I32 => build_converting_input_stream::<i32, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::I64 => build_converting_input_stream::<i64, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::U8 => build_converting_input_stream::<u8, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::U16 => build_converting_input_stream::<u16, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::U32 => build_converting_input_stream::<u32, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::U64 => build_converting_input_stream::<u64, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        cpal::SampleFormat::F64 => build_converting_input_stream::<f64, _>(
            device,
            stream_config,
            input_config,
            callback,
            last_input_error,
        ),
        _ => anyhow::bail!("unsupported audio input sample format: {sample_format}"),
    }
}

fn build_f32_input_stream<C>(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    input_config: AudioInputConfig,
    mut callback: C,
    last_input_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    C: FnMut(&[f32], AudioInputConfig) + Send + 'static,
{
    use cpal::traits::DeviceTrait as _;

    let chunk_samples = CAPTURE_CHUNK_FRAMES * usize::from(input_config.channels);
    let callback_error = last_input_error.clone();
    let mut callback_active = true;
    device
        .build_input_stream(
            stream_config,
            move |input: &[f32], _| {
                let usable = complete_samples(input.len(), input_config.channels);
                for chunk in input[..usable].chunks(chunk_samples) {
                    invoke_capture_callback(
                        &mut callback,
                        &mut callback_active,
                        chunk,
                        input_config,
                        &callback_error,
                    );
                }
            },
            input_error_callback(last_input_error),
            None,
        )
        .context("failed to build f32 audio input stream")
}

fn build_converting_input_stream<T, C>(
    device: &cpal::Device,
    stream_config: &cpal::StreamConfig,
    input_config: AudioInputConfig,
    mut callback: C,
    last_input_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
    C: FnMut(&[f32], AudioInputConfig) + Send + 'static,
{
    use cpal::Sample as _;
    use cpal::traits::DeviceTrait as _;

    let chunk_samples = CAPTURE_CHUNK_FRAMES * usize::from(input_config.channels);
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(chunk_samples)
        .context("failed to reserve audio input conversion buffer")?;
    scratch.resize(chunk_samples, 0.0f32);
    let callback_error = last_input_error.clone();
    let mut callback_active = true;
    device
        .build_input_stream(
            stream_config,
            move |input: &[T], _| {
                let usable = complete_samples(input.len(), input_config.channels);
                for input_chunk in input[..usable].chunks(chunk_samples) {
                    let converted = &mut scratch[..input_chunk.len()];
                    for (output, sample) in converted.iter_mut().zip(input_chunk.iter().copied()) {
                        *output = f32::from_sample(sample);
                    }
                    invoke_capture_callback(
                        &mut callback,
                        &mut callback_active,
                        converted,
                        input_config,
                        &callback_error,
                    );
                }
            },
            input_error_callback(last_input_error),
            None,
        )
        .context("failed to build converted audio input stream")
}

fn input_error_callback(
    last_input_error: Arc<Mutex<Option<String>>>,
) -> impl FnMut(cpal::StreamError) + Send + 'static {
    move |error| {
        let message = bounded_error_message(error.to_string());
        log::error!("audio input stream error: {message}");
        *last_input_error.lock() = Some(message);
    }
}

fn complete_samples(samples: usize, channels: u16) -> usize {
    let channels = usize::from(channels.max(1));
    samples - samples % channels
}

fn invoke_capture_callback<C>(
    callback: &mut C,
    active: &mut bool,
    samples: &[f32],
    config: AudioInputConfig,
    last_input_error: &Mutex<Option<String>>,
) where
    C: FnMut(&[f32], AudioInputConfig),
{
    if !*active {
        return;
    }
    if catch_unwind(AssertUnwindSafe(|| callback(samples, config))).is_err() {
        *active = false;
        let message = "audio input callback panicked; capture callback disabled".to_string();
        log::error!("{message}");
        *last_input_error.lock() = Some(message);
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn input_config_is_copyable_and_explicit() {
        let config = AudioInputConfig {
            sample_rate: 48_000,
            channels: 2,
        };
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.channels, 2);
        assert_eq!(config, config);
    }

    #[test]
    fn incomplete_input_frames_are_ignored() {
        assert_eq!(complete_samples(7, 2), 6);
        assert_eq!(complete_samples(7, 1), 7);
    }

    #[test]
    fn panicking_callbacks_are_disabled_and_reported() {
        let calls = AtomicUsize::new(0);
        let mut callback = |_: &[f32], _: AudioInputConfig| {
            calls.fetch_add(1, Ordering::Relaxed);
            panic!("boom");
        };
        let mut active = true;
        let error = Mutex::new(None);
        let config = AudioInputConfig {
            sample_rate: 48_000,
            channels: 2,
        };

        invoke_capture_callback(&mut callback, &mut active, &[0.0, 0.0], config, &error);
        invoke_capture_callback(&mut callback, &mut active, &[0.0, 0.0], config, &error);

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(!active);
        assert!(
            error
                .lock()
                .as_deref()
                .is_some_and(|message| message.contains("disabled"))
        );
    }

    #[test]
    fn input_errors_are_utf8_bounded() {
        let message = bounded_error_message("é".repeat(MAX_ERROR_BYTES));
        assert!(message.len() <= MAX_ERROR_BYTES);
        assert!(message.is_char_boundary(message.len()));
    }
}
