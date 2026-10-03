# kael_media_engines

Optional media/NLE engines for timelines, compositing, audio mixing, playback,
scopes, subtitles, and export.

`FrameCache` bounds decoded payload bytes and retained frame count (4,096 by
default). Use `FrameCache::with_entry_limit` to choose a metadata bound for your
working set, and `set_budget_bytes` to shed least-recently-used frames when
memory pressure changes. Empty frames are rejected, zero limits disable
retention, and `clear` releases both payloads and metadata capacity. Each lookup
and eviction uses a linked LRU in expected O(1) time; bulk eviction does not
rescan the cache for every removed frame. Payload accounting excludes cache
metadata and `Arc` clones retained elsewhere in the application.

This is a **leaf domain stack** in the
[Kael](https://github.com/Augani/kael) native application framework: it builds
media-application capability on top of the general-purpose runtime, and the
core `kael` crate never depends on it. See the
[documentation](https://augani.github.io/kael/) for usage and guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
