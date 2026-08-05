# kael_audio

Audio playback, mixing, DSP, playlists, and application-level audio state for
desktop products built with Kael primitives or another UI stack.

The crate has two playback layers:

- `AudioPlayer` loads file, HTTPS, or bounded in-memory media through
  `kael-media` and exposes track, seek, rate, volume, and listener APIs.
- `Mixer` and `AudioEngine` mix caller-provided sample sources against a
  device-frame master clock. The live engine supports CPAL's standard integer
  and floating-point output formats.

## Device-free mixing

```rust
use kael_audio::{BufferSource, Mixer};

let mut mixer = Mixer::new(48_000, 2);
mixer.insert_voice(1, Box::new(BufferSource::new(vec![0.25; 512], 2)), 1.0);

let mut output = vec![0.0; 512];
mixer.process(&mut output);
assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1e-6));
```

## Live output

```no_run
use kael_audio::{AudioEngine, SineSource};

fn main() -> kael_audio::Result<()> {
    let engine = AudioEngine::new()?;
    let voice = engine.play_source(
        Box::new(SineSource::new(440.0, engine.sample_rate(), 0.2)),
        1.0,
    )?;
    engine.set_voice_gain(voice, 0.5)?;

    if let Some(error) = engine.take_error() {
        eprintln!("audio device failed: {error}");
    }
    Ok(())
}
```

`SampleSource::fill` runs on the device callback. Custom sources must avoid
blocking, I/O, and steady-state allocation. Control commands are bounded and
coalesced so a stalled device cannot grow an unlimited queue. If a callback is
larger than the preallocated safety bound, that callback emits silence instead
of allocating on the device thread.

`AudioSession` and `SpatialAudioPlayer` hold application-level route,
interruption, orientation, and source-position state; they do not install OS
audio-session handlers or perform spatial DSP. For policy-controlled downloads,
fetch remote media with the networking battery and play a local file or bounded
memory source. Direct URLs are restricted to credential-free HTTPS.

The API reference is available on [docs.rs](https://docs.rs/kael_audio). See the
[Kael repository](https://github.com/Augani/kael) for workspace architecture and
production guidance.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
