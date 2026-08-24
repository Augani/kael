# Browser Audio

`kael_audio` keeps the Rust mixer, DSP sources, offline rendering, playlist,
session, and lightweight spatial-scene APIs available on desktop and
`wasm32-unknown-unknown`. Browser I/O is asynchronous where the platform is
asynchronous: output graph construction installs an `AudioWorklet`, device
enumeration awaits `enumerateDevices`, and capture awaits `getUserMedia`.
Native synchronous APIs and signatures are unchanged.

## Live output

Create the browser engine asynchronously, add the same `SampleSource` values
used by a desktop engine, and resume it from a click or key handler:

```rust,ignore
use kael_audio::{AudioEngine, BrowserAudioEngineConfig, SineSource};

let engine = AudioEngine::new_async_with_config(
    BrowserAudioEngineConfig::new(2, 256, 4)?,
).await?;
let voice = engine.play_source(
    Box::new(SineSource::new(440.0, engine.sample_rate(), 0.2)),
    1.0,
)?;
engine.set_voice_gain(voice, 0.5)?;
engine.resume_async().await?;
```

The output worklet requests only the space remaining in its fixed chunk window.
One request can be outstanding at a time. Rust reuses its mixer scratch buffer,
produces at most that requested count, and transfers each `Float32Array` rather
than retaining a second JavaScript copy. Samples are finite-checked and clamped
to `-1.0..=1.0` before they reach the device. Control is event-driven through
`MessagePort`; there is no timer or animation-frame polling loop.

`AudioEngineHandle` is main-thread-only and weak in a browser build. Cloned
handles can control voices while the owning `AudioEngine` exists, but cannot
keep the graph alive. `close_async`, owner drop, processor failure, and message
delivery failure detach handlers, stop the processor, disconnect the node,
discard sources, close the port, and close the `AudioContext`. A setup future
dropped while the worklet module is loading also closes its pending context.

Use `poll_event` for the bounded `Running`, `OutputUnderrun`, `ProcessorError`,
and `Closed` stream, `underrun_frames` for its cumulative counter, and
`take_error` for the latest content-safe diagnostic. Event storage holds at most
64 entries and drops the oldest entry when full.

## Devices and microphone capture

```rust,ignore
use kael_audio::{
    AudioInputStream, BrowserAudioCaptureConfig, default_input_device_async,
    input_devices_async, output_devices_async,
};

let outputs = output_devices_async().await?;
let inputs = input_devices_async().await?;

let microphone = default_input_device_async().await?;
let stream = AudioInputStream::from_input_device_async_with_config(
    &microphone,
    BrowserAudioCaptureConfig::new(1, 1_024, 4)?
        .with_signal_processing(true, true, true),
    |samples, format| {
        // Copy/enqueue promptly. Returning this callback releases one credit.
        consume_promptly(samples, format);
    },
).await?;
# let _ = (outputs, inputs, stream);
```

Browsers commonly hide device labels before an origin has media permission.
Kael preserves that distinction through `label_available` and a neutral fallback
name. Origin-scoped device identifiers stay private and are redacted from
`Debug`. Enumeration retains at most 1,024 audio descriptors and 1,024 UTF-8
bytes per label or identifier. An empty list is valid. A default lookup returns
`DeviceUnavailable` when no matching kind exists.

Stable non-default output routing is not yet interoperable across Kael's browser
targets. `AudioEngine::from_output_device_async` accepts the default descriptor
and returns `OutputRoutingUnsupported` for another sink; it never silently
routes to a different speaker.

Capture converts the selected stream into the requested 1–8 interleaved
channels. The worklet owns a fixed number of credits and transfers a chunk only
when a credit is available. The main-thread callback returns that credit after
it completes. When every credit is in flight, the worklet drops frames and
reports the monotonic total through `CaptureOverflow` rather than extending the
port queue. Chunk length, sequence, and drop counters are validated before
application code runs. Invalid delivery, processor failure, track end, a
panicking callback in an unwind-enabled build, explicit close, or drop stops all
tracks and closes the graph.

`getUserMedia` requires a secure context and normally a user activation and
permission. The stable typed outcomes distinguish `PermissionDenied`,
`DeviceUnavailable`, `UserActivationRequired`, `ApiUnavailable`, and graph
failures without retaining the browser's potentially sensitive exception text.
The Web platform does not provide a portable abort signal for an outstanding
permission prompt. Keep the returned future alive until the user decides; Rust
future cancellation is not permission-prompt revocation. After a stream is
granted, Kael's pending-track guard stops it on any later setup cancellation or
failure.

## Bounds and latency

| Boundary | Minimum | Default | Maximum |
| --- | ---: | ---: | ---: |
| Channels | 1 | output 2, capture 1 | 8 |
| Frames per chunk | 128 | output 256, capture 1,024 | 4,096 |
| Pending chunks/credits | 2 | 4 | 32 |
| Live output voices | — | — | 1,024 |
| Browser audio sample rate | 8 kHz | browser-selected | 192 kHz |

Chunks must be a multiple of the browser's 128-frame render quantum. At the
maximum configuration, one bridge window contains 32 × 4,096 × 8 = 1,048,576
`f32` samples (4 MiB), plus one chunk-sized assembly scratch buffer (at most
128 KiB) and browser-owned graph buffers. The default output window contains up
to 1,024 frames, about 21.3 ms at 48 kHz, before the browser's base/output/device
latency. Default capture produces one 1,024-frame callback, also about 21.3 ms at
48 kHz, with four delivery credits.

This design deliberately avoids `SharedArrayBuffer`, COOP/COEP headers, and a
shared-memory data race. The AudioWorklet stays on the browser audio rendering
thread, while Rust `Mixer`/DSP work and capture callbacks run only when a port
event reaches the browser main thread. UI or Wasm work that blocks that thread
can cause an underrun or capture overflow. Keep callbacks short and move larger
analysis through Kael's bounded Web Worker bridge.

For a game or workstation that requires worklet-owned Wasm DSP, sub-10-ms
synthesis, hundreds of continuously expensive sources, HRTF, room acoustics, or
multichannel device spatialization, use a specialized product audio worklet.
Kael's current portable spatial scene is equal-power stereo panning with
inverse-distance attenuation, not an HRTF or room renderer.

## Deployment and release evidence

`AudioWorklet` and `getUserMedia` require a secure context; `localhost` is valid
for development. Kael currently installs its worklet from a temporary `blob:`
URL and revokes that URL after `addModule` settles. A strict Content Security
Policy must permit this worklet module in the directives enforced by each target
browser. If policy rejects it, construction returns `WorkletUnavailable` rather
than falling back to a high-latency polling path.

`scripts/verify-browser-audio-smoke.sh` builds an optimized Wasm example, serves
it locally, and launches a fresh headless Chrome profile. The gate proves real
worklet graph construction/resume, frame-clock progress, bounded playback and
control, device-enumeration privacy semantics, typed permission-denied capture,
and explicit close/weak-handle cleanup. CI denies microphone permission and uses
a fake media device, so it intentionally does not claim successful physical
microphone capture. Pure injected protocol tests separately cover output request
clamping plus capture size/order/drop-counter validation.
