//! Browser playback backed by `HTMLAudioElement`.

use std::{cell::RefCell, fmt, rc::Rc, time::Duration};

use futures_channel::oneshot;
use js_sys::{Array, Uint8Array};
use kael_media::{AudioPlaybackError, PlaybackState};
use wasm_bindgen::{JsCast as _, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{Event, HtmlAudioElement, Url};

use super::AudioSource;

const WEB_FILE_UNSUPPORTED: &str =
    "filesystem audio paths are unavailable in browsers; use an HTTPS URL or in-memory bytes";
const METADATA_TIMEOUT_MS: i32 = 15_000;

/// A clonable browser audio controller.
#[derive(Clone)]
pub(super) struct WebAudioHandle {
    inner: Rc<WebAudioInner>,
}

struct WebAudioInner {
    element: HtmlAudioElement,
    source: AudioSource,
    object_url: Option<String>,
}

struct MetadataListeners {
    element: HtmlAudioElement,
    on_loaded: Closure<dyn FnMut(Event)>,
    on_error: Closure<dyn FnMut(Event)>,
    _on_timeout: Closure<dyn FnMut()>,
    timeout_id: i32,
}

impl Drop for MetadataListeners {
    fn drop(&mut self) {
        let _ = self.element.remove_event_listener_with_callback(
            "loadedmetadata",
            self.on_loaded.as_ref().unchecked_ref(),
        );
        let _ = self
            .element
            .remove_event_listener_with_callback("error", self.on_error.as_ref().unchecked_ref());
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(self.timeout_id);
        }
    }
}

impl WebAudioHandle {
    pub(super) fn new(source: AudioSource) -> Result<Self, AudioPlaybackError> {
        let element = HtmlAudioElement::new().map_err(js_output_error)?;
        element.set_preload("metadata");
        let (source_url, object_url) = source_url(&source)?;
        element.set_src(&source_url);
        Ok(Self {
            inner: Rc::new(WebAudioInner {
                element,
                source,
                object_url,
            }),
        })
    }

    pub(super) async fn load(
        source: AudioSource,
    ) -> Result<(Self, Option<Duration>), AudioPlaybackError> {
        let handle = Self::new(source)?;
        handle.wait_for_metadata().await?;
        Ok((handle.clone(), handle.duration()))
    }

    async fn wait_for_metadata(&self) -> Result<(), AudioPlaybackError> {
        // HAVE_METADATA. Cached object URLs and URLs already primed by the browser can
        // reach this state synchronously.
        if self.inner.element.ready_state() >= 1 {
            return Ok(());
        }

        let (sender, receiver) = oneshot::channel::<Result<(), String>>();
        let sender = Rc::new(RefCell::new(Some(sender)));
        let loaded_sender = sender.clone();
        let on_loaded = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(sender) = loaded_sender.borrow_mut().take() {
                let _ = sender.send(Ok(()));
            }
        });
        let error_sender = sender.clone();
        let element = self.inner.element.clone();
        let on_error = Closure::<dyn FnMut(Event)>::new(move |_| {
            if let Some(sender) = error_sender.borrow_mut().take() {
                let message = element.error().map_or_else(
                    || "the browser could not load the audio source".to_string(),
                    |error| {
                        format!(
                            "the browser rejected the audio source (media error code {})",
                            error.code()
                        )
                    },
                );
                let _ = sender.send(Err(message));
            }
        });
        let timeout_sender = sender;
        let on_timeout = Closure::<dyn FnMut()>::new(move || {
            if let Some(sender) = timeout_sender.borrow_mut().take() {
                let _ = sender.send(Err(format!(
                    "browser audio metadata did not load within {} seconds",
                    METADATA_TIMEOUT_MS / 1_000
                )));
            }
        });

        self.inner
            .element
            .add_event_listener_with_callback("loadedmetadata", on_loaded.as_ref().unchecked_ref())
            .map_err(js_output_error)?;
        if let Err(error) = self
            .inner
            .element
            .add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref())
        {
            let _ = self.inner.element.remove_event_listener_with_callback(
                "loadedmetadata",
                on_loaded.as_ref().unchecked_ref(),
            );
            return Err(js_output_error(error));
        }
        let Some(window) = web_sys::window() else {
            let _ = self.inner.element.remove_event_listener_with_callback(
                "loadedmetadata",
                on_loaded.as_ref().unchecked_ref(),
            );
            let _ = self
                .inner
                .element
                .remove_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());
            return Err(AudioPlaybackError::Output(
                "browser window is unavailable for audio metadata loading".into(),
            ));
        };
        let timeout_id = match window.set_timeout_with_callback_and_timeout_and_arguments_0(
            on_timeout.as_ref().unchecked_ref(),
            METADATA_TIMEOUT_MS,
        ) {
            Ok(timeout_id) => timeout_id,
            Err(error) => {
                let _ = self.inner.element.remove_event_listener_with_callback(
                    "loadedmetadata",
                    on_loaded.as_ref().unchecked_ref(),
                );
                let _ = self.inner.element.remove_event_listener_with_callback(
                    "error",
                    on_error.as_ref().unchecked_ref(),
                );
                return Err(js_output_error(error));
            }
        };
        let listeners = MetadataListeners {
            element: self.inner.element.clone(),
            on_loaded,
            on_error,
            _on_timeout: on_timeout,
            timeout_id,
        };
        self.inner.element.load();

        let result = receiver
            .await
            .map_err(|_| AudioPlaybackError::Output("audio metadata load was cancelled".into()))?;
        drop(listeners);
        result.map_err(AudioPlaybackError::Decoder)
    }

    pub(super) fn play(&self) -> Result<(), AudioPlaybackError> {
        let promise = self.inner.element.play().map_err(js_output_error)?;
        // The DOM API reports policy failures (most notably autoplay rejection)
        // asynchronously. Keep the synchronous Kael API useful for gesture-driven calls and
        // report later failures without leaking an unhandled promise rejection.
        spawn_local(async move {
            if let Err(error) = JsFuture::from(promise).await {
                log::warn!(
                    "browser audio playback was rejected asynchronously: {}",
                    js_message(&error)
                );
            }
        });
        Ok(())
    }

    pub(super) fn pause(&self) {
        let _ = self.inner.element.pause();
    }

    pub(super) fn stop(&self) {
        let _ = self.inner.element.pause();
        self.inner.element.set_current_time(0.0);
    }

    pub(super) fn seek(&self, position: Duration) -> Result<(), AudioPlaybackError> {
        let seconds = position.as_secs_f64();
        if !seconds.is_finite() {
            return Err(AudioPlaybackError::Output(
                "audio seek position is not finite".into(),
            ));
        }
        self.inner.element.set_current_time(seconds);
        Ok(())
    }

    pub(super) fn set_volume(&self, volume: f32) {
        self.inner
            .element
            .set_volume(f64::from(volume.clamp(0.0, 1.0)));
    }

    pub(super) fn set_speed(&self, speed: f32) {
        self.inner
            .element
            .set_playback_rate(f64::from(speed.clamp(0.5, 2.0)));
    }

    pub(super) fn position(&self) -> Duration {
        seconds_to_duration(self.inner.element.current_time()).unwrap_or(Duration::ZERO)
    }

    pub(super) fn duration(&self) -> Option<Duration> {
        seconds_to_duration(self.inner.element.duration())
    }

    pub(super) fn state(&self) -> PlaybackState {
        if self.inner.element.ended() {
            PlaybackState::Stopped
        } else if self.inner.element.paused() {
            if self.inner.element.current_time() <= 0.0 {
                PlaybackState::Stopped
            } else {
                PlaybackState::Paused
            }
        } else {
            PlaybackState::Playing
        }
    }
}

impl fmt::Debug for WebAudioHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebAudioHandle")
            .field("source", &self.inner.source)
            .field("position", &self.position())
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Drop for WebAudioInner {
    fn drop(&mut self) {
        let _ = self.element.pause();
        self.element.set_src("");
        self.element.load();
        if let Some(object_url) = self.object_url.take() {
            let _ = Url::revoke_object_url(&object_url);
        }
    }
}

fn source_url(source: &AudioSource) -> Result<(String, Option<String>), AudioPlaybackError> {
    match source {
        AudioSource::Url(url) => Ok((url.clone(), None)),
        AudioSource::Memory(bytes) => {
            let byte_length = u32::try_from(bytes.len()).map_err(|_| {
                AudioPlaybackError::Output("in-memory audio is too large for a browser blob".into())
            })?;
            let typed_array = Uint8Array::new_with_length(byte_length);
            typed_array.copy_from(bytes);
            let parts = Array::new();
            parts.push(&typed_array);
            let blob =
                web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(js_output_error)?;
            let object_url = Url::create_object_url_with_blob(&blob).map_err(js_output_error)?;
            Ok((object_url.clone(), Some(object_url)))
        }
        AudioSource::File(_) => Err(AudioPlaybackError::UnsupportedSource(
            WEB_FILE_UNSUPPORTED.into(),
        )),
    }
}

fn seconds_to_duration(seconds: f64) -> Option<Duration> {
    (seconds.is_finite() && seconds >= 0.0).then(|| Duration::from_secs_f64(seconds))
}

fn js_output_error(error: wasm_bindgen::JsValue) -> AudioPlaybackError {
    AudioPlaybackError::Output(js_message(&error))
}

fn js_message(error: &wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| "browser audio operation failed".into())
}

#[cfg(test)]
mod tests {
    use super::{AudioSource, WebAudioHandle, seconds_to_duration};
    use std::{sync::Arc, time::Duration};
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn duration_conversion_rejects_browser_sentinels() {
        assert_eq!(seconds_to_duration(f64::NAN), None);
        assert_eq!(seconds_to_duration(f64::INFINITY), None);
        assert_eq!(seconds_to_duration(-1.0), None);
        assert_eq!(
            seconds_to_duration(1.25),
            Some(Duration::from_millis(1_250))
        );
    }

    #[wasm_bindgen_test(async)]
    async fn in_memory_wav_loads_metadata_and_seeks() {
        let source = AudioSource::Memory(Arc::from(silent_wav(8_000, 400).into_boxed_slice()));
        let (handle, duration) = WebAudioHandle::load(source).await.unwrap();

        let duration = duration.expect("browser should report WAV duration");
        assert!((duration.as_secs_f64() - 0.4).abs() < 0.02);
        handle.seek(Duration::from_millis(125)).unwrap();
        assert!((handle.position().as_secs_f64() - 0.125).abs() < 0.02);
        handle.stop();
        assert_eq!(handle.position(), Duration::ZERO);
    }

    fn silent_wav(sample_rate: u32, frames: usize) -> Vec<u8> {
        let channels = 1u16;
        let bits_per_sample = 16u16;
        let bytes_per_sample = usize::from(bits_per_sample / 8);
        let data_len = frames * usize::from(channels) * bytes_per_sample;
        let byte_rate = sample_rate * u32::from(channels) * bytes_per_sample as u32;
        let block_align = channels * bits_per_sample / 8;
        let file_size = 36 + data_len as u32;
        let mut bytes = Vec::with_capacity(44 + data_len);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&file_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
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
