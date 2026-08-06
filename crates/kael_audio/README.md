# kael_audio

Cross-platform playback, device discovery, input capture, real-time and offline
mixing, DSP, playlists, and lightweight spatial audio for desktop products built
with Kael primitives or another UI stack.

The crate has two playback layers. `AudioPlayer` is a thread-local convenience
API for file, credential-free HTTPS, and bounded in-memory media. `Mixer` and
`AudioEngine` accept caller-defined sample sources and expose a device-frame
master clock for A/V synchronization. The clonable `AudioEngineHandle` provides
cross-thread control while the host stream remains on its creation thread.

## Device-free mixing

```rust
use kael_audio::{BufferSource, Mixer};

fn main() -> kael_audio::Result<()> {
    let mut mixer = Mixer::new(48_000, 2);
    mixer.insert_voice(
        1,
        Box::new(BufferSource::new(vec![0.25; 512], 2)),
        1.0,
    )?;

    let mut output = vec![0.0; 512];
    mixer.process(&mut output);
    assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1e-6));
    Ok(())
}
```

`render_offline` and `resample_linear` return explicit errors for invalid or
oversized buffers instead of silently truncating work. The linear resampler is
suited to previews and UI audio; use a band-limited resampler for mastering.

## Live output and capture

```no_run
use kael_audio::{AudioEngine, AudioInputStream, SineSource};

fn main() -> kael_audio::Result<()> {
    let engine = AudioEngine::new()?;
    let voice = engine.play_source(
        Box::new(SineSource::new(440.0, engine.sample_rate(), 0.2)),
        1.0,
    )?;
    engine.set_voice_gain(voice, 0.5)?;

    let _input = AudioInputStream::new(|samples, format| {
        // Copy or enqueue promptly; this slice is valid only for the callback.
        let _ = (samples, format);
    })?;

    if let Some(error) = engine.take_error() {
        eprintln!("audio output needs recovery: {error}");
    }
    Ok(())
}
```

Use `output_devices`, `input_devices`, and the corresponding `from_*_device`
constructors for explicit routing. Enumeration returns a bounded list. Recreate
an engine or input stream after the operating system changes or removes its
device; asynchronous failures are available through `take_error` and
`take_input_error`.

`SampleSource::fill` and input callbacks run on host audio threads. They must not
block, perform I/O, or allocate in steady state. The output engine reserves its
voice, command, and conversion storage before playback; supports at most
`AudioEngine::MAX_VOICES` live or pending voices; coalesces gain commands; and
clips submitted device samples to `-1.0..=1.0`. Ended source destruction runs on
a cleanup thread. A panicking sample source is retired, while a panicking input
callback is disabled and reported. With unwind-enabled builds, neither panic is
allowed to unwind through the host callback boundary.

## Spatial and application state

`SpatialAudioScene` wraps mixer sources with inexpensive equal-power stereo
panning and inverse-distance attenuation. It is intentionally not an HRTF or
room-acoustics renderer. Scene mutation stays off the device callback, and a
source retains its last safe gains if it is removed while a wrapper is alive.

`AudioSession` models category, activity, route, and interruption state inside
the application. It does not claim ownership of an operating-system media
session. `AudioPlayer` listeners run synchronously on the calling thread;
dropping their `Subscription` unregisters them, while `detach` deliberately keeps
them registered.

For policy-controlled downloads, fetch remote media with Kael's networking
battery and play a local file or bounded memory source. Direct player URLs are
restricted to credential-free HTTPS.

The API reference is available on [docs.rs](https://docs.rs/kael_audio). See the
[Kael repository](https://github.com/Augani/kael) for workspace architecture and
production guidance.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
