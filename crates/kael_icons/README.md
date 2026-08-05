# kael_icons

Compact, typed SVG icons for Kael applications and component libraries.

The crate bundles the small Lucide subset used by `kael_ui`, plus Kael's core
control icons. It provides typed names, SVG source, and virtual
`kael-icons/<name>.svg` asset resolution without requiring an application's
working directory to contain framework assets.

Applications can still replace any icon through their own asset source or by
configuring `kael_ui` with a branded icon directory.

Part of the [Kael](https://github.com/Augani/kael) native application framework.
See the [documentation](https://augani.github.io/kael/) for usage and guides.

## Third-party assets

The bundled Lucide subset is licensed under ISC, with Feather-derived portions
under MIT. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
