# kael_share

Validated outbound sharing for [Kael](https://github.com/Augani/kael).

`ShareSheet` accepts text, URLs, images, and files, enforces payload limits, and
reports the destinations supported by the current backend.

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
