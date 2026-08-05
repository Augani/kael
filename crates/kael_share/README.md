# kael_share

Validated outbound sharing for [Kael](https://github.com/Augani/kael).

`ShareSheet` accepts text, URLs, images, and files, enforces payload limits, and
reports the destinations supported by the current backend.

Validation runs before platform handoff. It rejects subject-only requests,
invalid URLs, missing or non-file attachments, oversized text, images over 64
MiB each, and more than 256 MiB of in-memory images per sheet. Linux image
attachments are materialized in private 0700 directories as 0600 files and are
only created when an installed mail launcher can use them.

- macOS provides the broadest native outbound sharing path.
- Windows currently supports mail and clipboard handoff; file/image
  DataTransferManager support is not implemented.
- Linux uses available mail and clipboard tools.
- Registering the application as a share receiver is not implemented in Kael
  0.3.

Check `ShareSheet::platform_support()` before presenting a destination and keep
a product-specific fallback for unsupported payloads.

## License

Apache-2.0. See `LICENSE-APACHE`.
