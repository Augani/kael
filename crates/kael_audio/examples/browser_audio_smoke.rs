#[cfg(not(target_arch = "wasm32"))]
fn main() {}

#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(run());
}

#[cfg(target_arch = "wasm32")]
async fn run() {
    use std::{path::PathBuf, time::Duration};

    use kael_audio::{
        AudioEngine, AudioInputStream, AudioPlayer, AudioSource, BrowserAudioEngineConfig,
        BrowserAudioErrorKind, BrowserAudioEvent, BufferSource, Mixer, SineSource,
        input_devices_async, output_devices_async,
    };

    publish(
        "data-kael-audio-backend",
        kael_audio::platform::support().playback_backend,
    );

    let player = AudioPlayer::new();
    let track = match player
        .load(AudioSource::from(silent_wav(8_000, 3_200)))
        .await
    {
        Ok(track) => track,
        Err(error) => {
            publish("data-kael-audio-memory", "failed");
            publish("data-kael-audio-error", &error.to_string());
            return;
        }
    };
    publish("data-kael-audio-memory", "loaded");
    let duration_verified = track
        .duration
        .is_some_and(|duration| (duration.as_secs_f64() - 0.4).abs() < 0.02);
    publish(
        "data-kael-audio-duration",
        if duration_verified {
            "verified"
        } else {
            "failed"
        },
    );

    let seek_verified = player.seek(Duration::from_millis(125)).is_ok()
        && (player.position().as_secs_f64() - 0.125).abs() < 0.02;
    publish(
        "data-kael-audio-seek",
        if seek_verified { "verified" } else { "failed" },
    );
    player.stop();

    let file_player = AudioPlayer::new();
    let file_unsupported = file_player
        .load(AudioSource::File(PathBuf::from("browser-audio.wav")))
        .await
        .is_err();
    publish(
        "data-kael-audio-filesystem",
        if file_unsupported {
            "unsupported"
        } else {
            "failed"
        },
    );

    let mut mixer = Mixer::new(48_000, 2);
    let mixer_verified = mixer
        .insert_voice(1, Box::new(BufferSource::new(vec![0.25, -0.25], 2)), 1.0)
        .and_then(|()| mixer.render_offline(1, 1))
        .is_ok_and(|samples| samples == [0.25, -0.25]);
    publish(
        "data-kael-audio-offline-mixer",
        if mixer_verified { "verified" } else { "failed" },
    );

    let devices_verified = match (output_devices_async().await, input_devices_async().await) {
        (Ok(outputs), Ok(inputs)) if outputs.len() <= 1_024 && inputs.len() <= 1_024 => {
            publish("data-kael-audio-output-devices", &outputs.len().to_string());
            publish("data-kael-audio-input-devices", &inputs.len().to_string());
            publish(
                "data-kael-audio-device-labels",
                if outputs.iter().any(|device| device.label_available())
                    || inputs.iter().any(|device| device.label_available())
                {
                    "permission-exposed"
                } else {
                    "privacy-hidden"
                },
            );
            true
        }
        _ => false,
    };
    publish(
        "data-kael-audio-devices",
        if devices_verified {
            "enumerated"
        } else {
            "failed"
        },
    );

    let bounds_verified = BrowserAudioEngineConfig::new(2, 256, 4).is_ok()
        && BrowserAudioEngineConfig::new(9, 256, 4).is_err()
        && BrowserAudioEngineConfig::new(2, 129, 4).is_err()
        && BrowserAudioEngineConfig::new(2, 256, 33).is_err();
    publish(
        "data-kael-audio-bounds",
        if bounds_verified {
            "verified"
        } else {
            "failed"
        },
    );

    let (worklet_verified, resume_verified, control_verified, cleanup_verified) =
        match AudioEngine::new_async().await {
            Ok(engine) => {
                publish("data-kael-audio-worklet", "constructed");
                let voice = engine.play_source(
                    Box::new(
                        SineSource::new(220.0, engine.sample_rate(), 0.0)
                            .with_frames(engine.sample_rate() as usize),
                    ),
                    1.0,
                );
                let controls = voice.is_ok()
                    && voice
                        .and_then(|voice| {
                            engine.set_voice_gain(voice, 0.5)?;
                            engine.set_master_gain(0.5)?;
                            Ok(voice)
                        })
                        .is_ok();
                let resume = engine.resume_async().await.is_ok();
                wait_for(Duration::from_millis(250)).await;
                let running = engine.handle().is_running();
                let clock_advanced = engine.clock().frames() > 0;
                let running_event = std::iter::from_fn(|| engine.poll_event())
                    .any(|event| event == BrowserAudioEvent::Running);
                publish(
                    "data-kael-audio-render-clock",
                    if clock_advanced { "advanced" } else { "failed" },
                );
                let handle = engine.handle();
                let cleanup = engine.close_async().await.is_ok() && !handle.is_running();
                (
                    clock_advanced,
                    resume && running && running_event,
                    controls,
                    cleanup,
                )
            }
            Err(error) => {
                publish("data-kael-audio-worklet", "failed");
                publish("data-kael-audio-error", &error.to_string());
                (false, false, false, false)
            }
        };
    publish(
        "data-kael-audio-resume",
        if resume_verified { "running" } else { "failed" },
    );
    publish(
        "data-kael-audio-control",
        if control_verified {
            "bounded"
        } else {
            "failed"
        },
    );
    publish(
        "data-kael-audio-cleanup",
        if cleanup_verified { "closed" } else { "failed" },
    );

    let capture_denied = match AudioInputStream::new_async(|_, _| {}).await {
        Err(error) => error.kind() == BrowserAudioErrorKind::PermissionDenied,
        Ok(stream) => {
            let _ = stream.close_async().await;
            false
        }
    };
    publish(
        "data-kael-audio-capture-permission",
        if capture_denied { "denied" } else { "failed" },
    );

    if duration_verified
        && seek_verified
        && file_unsupported
        && mixer_verified
        && devices_verified
        && bounds_verified
        && worklet_verified
        && resume_verified
        && control_verified
        && cleanup_verified
        && capture_denied
    {
        publish("data-kael-audio-ready", "true");
        report_success();
    } else {
        report_failure(&[
            ("duration", duration_verified),
            ("seek", seek_verified),
            ("filesystem", file_unsupported),
            ("offline", mixer_verified),
            ("devices", devices_verified),
            ("bounds", bounds_verified),
            ("worklet", worklet_verified),
            ("resume", resume_verified),
            ("control", control_verified),
            ("cleanup", cleanup_verified),
            ("capture_denied", capture_denied),
        ]);
    }
}

#[cfg(target_arch = "wasm32")]
async fn wait_for(duration: std::time::Duration) {
    use std::{cell::RefCell, rc::Rc};

    use futures_channel::oneshot;
    use wasm_bindgen::{JsCast as _, closure::Closure};

    let (sender, receiver) = oneshot::channel();
    let sender = Rc::new(RefCell::new(Some(sender)));
    let callback_sender = Rc::clone(&sender);
    let callback = Closure::<dyn FnMut()>::new(move || {
        if let Some(sender) = callback_sender.borrow_mut().take() {
            let _ = sender.send(());
        }
    });
    let Some(window) = web_sys::window() else {
        return;
    };
    let timeout = i32::try_from(duration.as_millis()).unwrap_or(i32::MAX);
    let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        timeout,
    ) else {
        return;
    };
    let _ = receiver.await;
    window.clear_timeout_with_handle(id);
    drop(callback);
}

#[cfg(target_arch = "wasm32")]
fn publish(name: &str, value: &str) {
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute(name, value);
    }
}

#[cfg(target_arch = "wasm32")]
fn report_success() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let _ = window.location().set_search(
        "?__kael_audio_pass__=1&worklet=passed&resume=passed&clock=passed&control=passed&bounds=passed&devices=passed&capture_denied=passed&cleanup=passed",
    );
}

#[cfg(target_arch = "wasm32")]
fn report_failure(statuses: &[(&str, bool)]) {
    use std::fmt::Write as _;

    let Some(window) = web_sys::window() else {
        return;
    };
    let mut query = String::from("?__kael_audio_failed__=1");
    for (name, passed) in statuses {
        let _ = write!(
            query,
            "&{name}={}",
            if *passed { "passed" } else { "failed" }
        );
    }
    let _ = window.location().set_search(&query);
}

#[cfg(target_arch = "wasm32")]
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
