# kael_audio

Cross-platform playback, device discovery, input capture, real-time and offline
mixing, DSP, playlists, and lightweight spatial audio for desktop products built
with Kael primitives or another UI stack.

The crate has two playback layers. `AudioPlayer` is a thread-local convenience
API for file, credential-free HTTPS, and bounded in-memory media. `Mixer` and
`AudioEngine` accept caller-defined sample sources and expose a device-frame
master clock for A/V synchronization. Native `AudioEngineHandle` values provide
cross-thread control while the host stream remains on its creation thread. A
browser handle is deliberately main-thread and weak, so it cannot keep an
`AudioContext` alive after its owning engine is dropped.

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

## Browser builds

On `wasm32-unknown-unknown`, `AudioPlayer` uses `HTMLAudioElement` for
credential-free HTTPS and bounded in-memory media. Live sample mixing uses the
same Rust `Mixer`/DSP sources as desktop through a bounded, pull-driven
`AudioWorklet`; microphone capture uses `getUserMedia` and a credit-bounded
capture worklet. Device enumeration, graph construction, and permission are
explicitly asynchronous:

```rust,ignore
use kael_audio::{
    AudioEngine, AudioInputStream, BrowserAudioEngineConfig, SineSource,
    input_devices_async, output_devices_async,
};

let (outputs, inputs) = (output_devices_async().await?, input_devices_async().await?);
let engine = AudioEngine::new_async_with_config(
    BrowserAudioEngineConfig::new(2, 256, 4)?,
).await?;
engine.play_source(
    Box::new(SineSource::new(440.0, engine.sample_rate(), 0.2)),
    1.0,
)?;

// Call directly from a click/key activation when browser autoplay policy
// requires it.
engine.resume_async().await?;

// Request directly from a user activation. The callback is delivered through
// bounded MessagePort credits on the browser main thread.
let input = AudioInputStream::new_async(|samples, format| {
    consume_promptly(samples, format);
}).await?;
# let _ = (outputs, inputs, input);
```

The synchronous device, capture, and live-engine constructors still return an
explicit browser error; native signatures are unchanged. Browser device labels
may remain privacy-hidden until permission is granted. Only the default output
route is portable today: selecting another enumerated sink returns
`OutputRoutingUnsupported` rather than silently using the wrong speaker.

The default output window is four 256-frame chunks (1,024 frames, about 21.3 ms
at 48 kHz) plus the browser/device latency. Bounds are 1–8 channels,
128–4,096 frames per chunk in 128-frame increments, and 2–32 pending chunks.
This caps transferred audio at 1,048,576 `f32` samples (4 MiB) per bridge window,
with one additional chunk-sized assembly scratch buffer. Live output accepts at
most 1,024 voices, lifecycle/pressure events are bounded, and device enumeration
retains at most 1,024 descriptors with 1,024 bytes per browser string.

Kael does not require `SharedArrayBuffer` or cross-origin isolation. The
AudioWorklet owns real-time rendering/capture, but Rust source mixing and input
callbacks run on browser message turns; a blocked main thread can therefore
produce an `OutputUnderrun` or `CaptureOverflow`. The bridge is event-driven and
has no main-thread polling loop. Products requiring worklet-owned Wasm DSP,
sub-10-ms synthesis, HRTF/room processing, or very large game-audio graphs need
a specialized isolated audio backend; this adapter does not claim native
callback parity for those workloads.

`AudioWorklet` and microphone capture require a secure context (`localhost`
qualifies). A strict Content Security Policy must permit Kael's temporary
`blob:` worklet module. Resume and permission prompts should begin inside a
transient user activation. Browsers expose no portable way to cancel a pending
`getUserMedia` permission prompt; once it resolves, Kael stops granted tracks on
every later setup failure, cancellation, explicit close, or drop.

Browser filesystem paths remain explicitly unsupported. Device-free `Mixer`,
DSP, resampling, playlists, session state, and lightweight equal-power stereo
spatial processing remain available. See the
[browser audio guide](https://augani.github.io/kael/browser-audio.html) for the
full boundary and release probe.

The API reference is available on [docs.rs](https://docs.rs/kael_audio). See the
[Kael repository](https://github.com/Augani/kael) for workspace architecture and
production guidance.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
