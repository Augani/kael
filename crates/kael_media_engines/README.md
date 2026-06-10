# kael_media_engines

Optional media/NLE engines (timeline, compositing, audio mix, export) for the Kael UI framework

Part of the [Kael](https://github.com/Augani/kael) GPU-accelerated Rust UI framework. This is a **leaf domain stack**: it builds media-application capability on top of the general-purpose framework, and the core `kael` crate never depends on it. These modules previously lived in `kael_engines` and were split out so that crate stays domain-neutral. See the [documentation](https://augani.github.io/kael/) for usage and guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
