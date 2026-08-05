# kael-media

Bounded audio playback and incremental video decoding primitives for Kael.

Use this crate directly when an application needs Kael's low-level media primitives without the ready-made UI layer. It provides:

- file, byte, repeatable-reader, and HTTP(S) media sources;
- a UI-thread-local audio playback handle with play, pause, seek, volume, and speed controls;
- incremental BGRA video-frame decoding, seeking, and metadata;
- bounded full-video and fallback-audio decoding paths.

```rust,no_run
use kael_media::{MediaSource, VideoFrameStream};

let mut frames = VideoFrameStream::new(MediaSource::file("clip.mp4"))?;
while let Some(frame) = frames.next_frame()? {
    // Upload `frame.data` to your renderer.
}
# Ok::<(), kael_media::MediaDecodeError>(())
```

## Production notes

- Local file sources must resolve to regular files. Remote sources accept only credential-free `http` and `https` URLs, and FFmpeg receives a restricted protocol allowlist.
- Reader and byte sources are staged with private permissions when FFmpeg requires a path. Staging is capped at 512 MiB and temporary files are removed with the final source clone.
- `VideoFrameStream` is the intended path for long video. `MediaDecoder::decode_video_frames` is capped at 256 frames and 128 MiB.
- Rodio-supported audio formats stream directly. FFmpeg fallback audio is decoded once into a shared buffer capped at 128 MiB.
- Opening media, probing remote sources, fallback decoding, and creating an output device are synchronous. Run potentially slow preparation away from the UI thread.
- `AudioHandle` is deliberately thread-local. Use the higher-level [`kael_audio`](https://docs.rs/kael_audio) services when you need playlists, mixing, DSP, sessions, or spatial-audio state.

This crate links to FFmpeg and the host audio backend. See the [API documentation](https://docs.rs/kael-media) and the [Kael guides](https://augani.github.io/kael/) for the complete contract.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
