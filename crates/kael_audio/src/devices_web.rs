//! Browser audio-device discovery.

use std::{fmt, sync::Arc};

use anyhow::Result;
use js_sys::Array;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::JsFuture;
use web_sys::{MediaDeviceInfo, MediaDeviceKind};

use crate::browser_audio::{
    BrowserAudioError, BrowserAudioErrorKind, MAX_BROWSER_AUDIO_DEVICES, bounded_browser_text,
    classify_js_error,
};

const ASYNC_DEVICE_MESSAGE: &str = "browser audio device discovery is asynchronous; use output_devices_async or input_devices_async";

/// An output-device descriptor returned by browser enumeration.
///
/// Browser device labels may be empty until the origin has media permission.
/// Non-default output routing requires `AudioContext.setSinkId`, which is not
/// interoperable enough for Kael's stable browser API yet.
#[derive(Clone)]
pub struct AudioOutputDevice {
    id: Arc<str>,
    name: Arc<str>,
    label_available: bool,
}

impl AudioOutputDevice {
    /// Return the browser-provided label or a privacy-safe fallback.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether browser permission policy exposed the real device label.
    pub fn label_available(&self) -> bool {
        self.label_available
    }

    /// Whether this descriptor represents the browser's default route.
    pub fn is_default(&self) -> bool {
        self.id.as_ref() == "default"
    }
}

impl fmt::Debug for AudioOutputDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioOutputDevice")
            .field("label_available", &self.label_available)
            .field("is_default", &self.is_default())
            .finish_non_exhaustive()
    }
}

/// An input-device descriptor returned by browser enumeration.
///
/// Pass this value to [`crate::AudioInputStream::from_input_device_async`] to
/// request a specific microphone without exposing its origin-scoped id.
#[derive(Clone)]
pub struct AudioInputDevice {
    id: Arc<str>,
    name: Arc<str>,
    label_available: bool,
}

impl AudioInputDevice {
    /// Return the browser-provided label or a privacy-safe fallback.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether browser permission policy exposed the real device label.
    pub fn label_available(&self) -> bool {
        self.label_available
    }

    /// Whether this descriptor represents the browser's default route.
    pub fn is_default(&self) -> bool {
        self.id.as_ref() == "default"
    }

    pub(crate) fn device_id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for AudioInputDevice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AudioInputDevice")
            .field("label_available", &self.label_available)
            .field("is_default", &self.is_default())
            .finish_non_exhaustive()
    }
}

/// Return an explicit error because browser enumeration cannot be synchronous.
pub fn output_devices() -> Result<Vec<AudioOutputDevice>> {
    anyhow::bail!(ASYNC_DEVICE_MESSAGE)
}

/// Return an explicit error because browser enumeration cannot be synchronous.
pub fn input_devices() -> Result<Vec<AudioInputDevice>> {
    anyhow::bail!(ASYNC_DEVICE_MESSAGE)
}

/// Return an explicit error because browser enumeration cannot be synchronous.
pub fn default_output_device() -> Result<AudioOutputDevice> {
    anyhow::bail!(ASYNC_DEVICE_MESSAGE)
}

/// Return an explicit error because browser enumeration cannot be synchronous.
pub fn default_input_device() -> Result<AudioInputDevice> {
    anyhow::bail!(ASYNC_DEVICE_MESSAGE)
}

/// Asynchronously enumerate up to 1,024 browser output devices.
///
/// An empty result is valid when the browser does not expose output routes.
pub async fn output_devices_async() -> std::result::Result<Vec<AudioOutputDevice>, BrowserAudioError>
{
    let records = enumerate_device_records().await?;
    Ok(records
        .into_iter()
        .filter(|record| record.kind == DeviceKind::Output)
        .map(|record| AudioOutputDevice {
            id: record.id,
            name: record.name,
            label_available: record.label_available,
        })
        .collect())
}

/// Asynchronously enumerate up to 1,024 browser input devices.
///
/// Labels can remain hidden until microphone permission has been granted.
pub async fn input_devices_async() -> std::result::Result<Vec<AudioInputDevice>, BrowserAudioError>
{
    let records = enumerate_device_records().await?;
    Ok(records
        .into_iter()
        .filter(|record| record.kind == DeviceKind::Input)
        .map(|record| AudioInputDevice {
            id: record.id,
            name: record.name,
            label_available: record.label_available,
        })
        .collect())
}

/// Resolve the browser's default output descriptor asynchronously.
pub async fn default_output_device_async()
-> std::result::Result<AudioOutputDevice, BrowserAudioError> {
    select_default(output_devices_async().await?)
}

/// Resolve the browser's default microphone descriptor asynchronously.
pub async fn default_input_device_async() -> std::result::Result<AudioInputDevice, BrowserAudioError>
{
    select_default(input_devices_async().await?)
}

trait DefaultDevice {
    fn is_default(&self) -> bool;
}

impl DefaultDevice for AudioOutputDevice {
    fn is_default(&self) -> bool {
        self.is_default()
    }
}

impl DefaultDevice for AudioInputDevice {
    fn is_default(&self) -> bool {
        self.is_default()
    }
}

fn select_default<T: DefaultDevice>(devices: Vec<T>) -> std::result::Result<T, BrowserAudioError> {
    let default_index = devices
        .iter()
        .position(DefaultDevice::is_default)
        .unwrap_or(0);
    devices
        .into_iter()
        .nth(default_index)
        .ok_or_else(|| BrowserAudioError::new(BrowserAudioErrorKind::DeviceUnavailable))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    Input,
    Output,
}

struct DeviceRecord {
    kind: DeviceKind,
    id: Arc<str>,
    name: Arc<str>,
    label_available: bool,
}

async fn enumerate_device_records() -> std::result::Result<Vec<DeviceRecord>, BrowserAudioError> {
    let window = web_sys::window()
        .ok_or_else(|| BrowserAudioError::new(BrowserAudioErrorKind::ApiUnavailable))?;
    let media_devices = window
        .navigator()
        .media_devices()
        .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
    let promise = media_devices
        .enumerate_devices()
        .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
    let value = JsFuture::from(promise)
        .await
        .map_err(|error| classify_js_error(&error, BrowserAudioErrorKind::ApiUnavailable))?;
    let values = Array::from(&value);
    let limit = values.length().min(MAX_BROWSER_AUDIO_DEVICES as u32);
    let mut records = Vec::new();
    records
        .try_reserve_exact(limit as usize)
        .map_err(|_| BrowserAudioError::new(BrowserAudioErrorKind::Backpressure))?;
    for index in 0..limit {
        let Ok(info) = values.get(index).dyn_into::<MediaDeviceInfo>() else {
            continue;
        };
        let kind = match info.kind() {
            MediaDeviceKind::Audioinput => DeviceKind::Input,
            MediaDeviceKind::Audiooutput => DeviceKind::Output,
            MediaDeviceKind::Videoinput => continue,
            _ => continue,
        };
        let id = bounded_browser_text(info.device_id());
        let label = bounded_browser_text(info.label());
        let label_available = !label.is_empty();
        let fallback = match kind {
            DeviceKind::Input => "Browser audio input",
            DeviceKind::Output => "Browser audio output",
        };
        records.push(DeviceRecord {
            kind,
            id: Arc::from(id),
            name: Arc::from(if label_available {
                label
            } else {
                fallback.to_string()
            }),
            label_available,
        });
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_selection_prefers_browser_default_and_handles_empty_lists() {
        let devices = vec![
            AudioOutputDevice {
                id: Arc::from("secondary"),
                name: Arc::from("hidden"),
                label_available: false,
            },
            AudioOutputDevice {
                id: Arc::from("default"),
                name: Arc::from("default"),
                label_available: true,
            },
        ];
        assert!(select_default(devices).unwrap().is_default());
        assert_eq!(
            select_default::<AudioOutputDevice>(Vec::new())
                .unwrap_err()
                .kind(),
            BrowserAudioErrorKind::DeviceUnavailable
        );
    }

    #[test]
    fn device_debug_does_not_expose_origin_scoped_ids_or_labels() {
        let device = AudioInputDevice {
            id: Arc::from("private-origin-id"),
            name: Arc::from("Private microphone label"),
            label_available: true,
        };
        let debug = format!("{device:?}");
        assert!(!debug.contains("private-origin-id"));
        assert!(!debug.contains("Private microphone label"));
    }
}
