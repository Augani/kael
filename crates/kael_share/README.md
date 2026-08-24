# kael_share

Validated outbound sharing for [Kael](https://github.com/Augani/kael).

`ShareSheet` accepts text, URLs, images, native file paths, and portable
in-memory `ShareFile` bytes, enforces payload limits, and reports the
destinations supported by the current backend. `show_portable()` is the shared
typed async entry point for desktop and browser builds; the existing `show()`
entry point remains source-compatible and wraps typed failures in `anyhow`.

Validation runs before platform handoff. It rejects subject-only requests,
invalid URLs, missing or non-file attachments, oversized text, images over 64
MiB each, and more than 256 MiB of in-memory images per sheet. Linux image
attachments are materialized in private 0700 directories as 0600 files and are
only created when an installed mail launcher can use them.

- macOS provides the broadest native outbound sharing path.
- Windows currently supports mail and clipboard handoff; file/image
  DataTransferManager support is not implemented.
- Linux uses available mail and clipboard tools.
- Browsers use `navigator.share` and `navigator.canShare`. The call must start
  during a transient user activation. Text, one primary URL, additional URLs
  folded into text, in-memory files, and images are supported when the browser
  accepts their MIME types. Native `PathBuf` attachments cannot cross the
  browser sandbox; use `ShareFile`. Browser cancellation, permission/policy
  denial, unavailable APIs, lost activation, and unsupported file payloads are
  distinct `ShareError` variants.
- The Web Share picker does not reveal or let Kael filter its destinations, so
  browser requests with excluded destination families fail explicitly.
- Registering the application as a share receiver is not implemented in Kael
  0.4. Browser share-target registration belongs to an installed PWA manifest
  and product service worker.

Check `ShareSheet::platform_support()` before presenting a destination and keep
a product-specific fallback for unsupported payloads.

## License

Apache-2.0. See `LICENSE-APACHE`.
