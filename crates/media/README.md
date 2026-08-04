# kael_media_sys

Focused macOS media interoperability for Kael.

This low-level crate provides the small CoreMedia and CoreVideo surface Kael
needs for native capture and GPU rendering:

- owned wrappers for sample, format-description, and block buffers;
- timing and H.264 parameter-set access;
- a CoreVideo-to-Metal texture-cache bridge;
- the pixel formats used by Kael's video renderers.

The API is available on macOS only. Most applications should depend on
[`kael`](https://docs.rs/kael) or [`kael-media`](https://docs.rs/kael-media)
instead; use `kael_media_sys` directly when integrating native media pipelines
or custom Metal renderers.

## Ownership and safety

Core Foundation values returned by this crate are retained Rust wrappers.
Borrowed byte and Metal-texture references cannot outlive their owning wrapper.
Fallible system calls return `anyhow::Result` and validate null pointers before
constructing Rust values.

The one raw entry point, `CVMetalTextureCache::new`, is unsafe because it bridges
an Objective-C `MTLDevice` pointer from another Metal binding. Its API
documentation states the pointer and lifetime contract. Image-to-texture
creation accepts borrowed typed CoreVideo pixel buffers and is safe.

```rust,no_run
use kael_media_sys::core_media::CMSampleBuffer;

fn inspect_sample(sample: &CMSampleBuffer) -> anyhow::Result<()> {
    let timing = sample.sample_timing_info(0)?;
    if let Some(data) = sample.data() {
        println!("{} encoded bytes at {:?}", data.copy_bytes()?.len(), timing);
    }
    Ok(())
}
```

See the [API documentation](https://docs.rs/kael_media_sys) and the
[Kael repository](https://github.com/Augani/kael) for the complete framework.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
