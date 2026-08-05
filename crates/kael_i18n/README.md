# kael_i18n

Lightweight internationalization primitives for Kael applications.

`kael_i18n` provides bounded JSON string catalogs, deterministic locale
fallback, one-pass named interpolation, integer plural categories, text
direction, and number/date conventions for common locales without embedding a
large locale database.

The built-in formatter is intentionally not a complete CLDR implementation.
Applications that require every numbering system, calendar, collation rule, or
fractional plural form can use ICU4X alongside Kael while keeping this crate for
catalog and fallback management.

The JSON loader rejects duplicate keys and bounds catalog bytes, entry count,
key length, and value length before accepting a catalog. Named interpolation is
single-pass, so text supplied as an argument cannot trigger another argument
replacement.

Part of the [Kael](https://github.com/Augani/kael) native application framework.
See the [documentation](https://augani.github.io/kael/) for usage and guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
