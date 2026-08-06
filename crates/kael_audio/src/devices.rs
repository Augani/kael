//! Cross-platform audio device discovery.

use std::{fmt, sync::Arc};

use anyhow::{Context as _, Result};
use cpal::traits::{DeviceTrait as _, HostTrait as _};

const MAX_AUDIO_DEVICES: usize = 1024;
const MAX_DEVICE_NAME_BYTES: usize = 1024;

/// An output device that can be passed to [`crate::AudioEngine::from_output_device`].
#[derive(Clone)]
pub struct AudioOutputDevice {
    pub(crate) device: cpal::Device,
    name: Arc<str>,
}

impl AudioOutputDevice {
    /// Return the operating-system display name for this device.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for AudioOutputDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioOutputDevice")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// An input device that can be passed to [`crate::AudioInputStream::from_input_device`].
#[derive(Clone)]
pub struct AudioInputDevice {
    pub(crate) device: cpal::Device,
    name: Arc<str>,
}

impl AudioInputDevice {
    /// Return the operating-system display name for this device.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for AudioInputDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioInputDevice")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Return up to 1024 of the current host's available output devices.
pub fn output_devices() -> Result<Vec<AudioOutputDevice>> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .context("failed to enumerate audio output devices")?;
    let mut output = Vec::new();
    for device in devices.take(MAX_AUDIO_DEVICES) {
        let name = match device.name() {
            Ok(name) => bounded_device_name(name),
            Err(error) => {
                log::warn!("skipping audio output device with unreadable name: {error}");
                continue;
            }
        };
        output
            .try_reserve(1)
            .context("failed to reserve audio output device list")?;
        output.push(AudioOutputDevice { device, name });
    }
    Ok(output)
}

/// Return up to 1024 of the current host's available input devices.
pub fn input_devices() -> Result<Vec<AudioInputDevice>> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .context("failed to enumerate audio input devices")?;
    let mut input = Vec::new();
    for device in devices.take(MAX_AUDIO_DEVICES) {
        let name = match device.name() {
            Ok(name) => bounded_device_name(name),
            Err(error) => {
                log::warn!("skipping audio input device with unreadable name: {error}");
                continue;
            }
        };
        input
            .try_reserve(1)
            .context("failed to reserve audio input device list")?;
        input.push(AudioInputDevice { device, name });
    }
    Ok(input)
}

/// Return the current host's default output device.
pub fn default_output_device() -> Result<AudioOutputDevice> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device")?;
    let name = bounded_device_name(
        device
            .name()
            .unwrap_or_else(|_| "Default audio output".into()),
    );
    Ok(AudioOutputDevice { device, name })
}

/// Return the current host's default input device.
pub fn default_input_device() -> Result<AudioInputDevice> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default audio input device")?;
    let name = bounded_device_name(
        device
            .name()
            .unwrap_or_else(|_| "Default audio input".into()),
    );
    Ok(AudioInputDevice { device, name })
}

fn bounded_device_name(mut name: String) -> Arc<str> {
    if name.len() > MAX_DEVICE_NAME_BYTES {
        let mut boundary = MAX_DEVICE_NAME_BYTES;
        while !name.is_char_boundary(boundary) {
            boundary -= 1;
        }
        name.truncate(boundary);
    }
    Arc::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_utf8_bounded() {
        let name = bounded_device_name("é".repeat(MAX_DEVICE_NAME_BYTES));
        assert!(name.len() <= MAX_DEVICE_NAME_BYTES);
        assert!(name.is_char_boundary(name.len()));
    }
}
