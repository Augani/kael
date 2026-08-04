# kael_http_client

Transport primitives and a ready-to-use HTTP client for Kael applications.

The crate keeps application code on the standard [`http`](https://docs.rs/http)
request and response types behind an object-safe `HttpClient` trait. Applications
can provide their own transport or use the included `ReqwestClient`. Proxy-aware
and base-URL wrappers compose over either choice.

```no_run
use kael_http_client::{AsyncBody, HttpClient, Request, ReqwestClient};

# async fn fetch() -> kael_http_client::Result<()> {
let client = ReqwestClient::user_agent("my-app/1.0")?;
let request = Request::get("https://example.com/status")
    .body(AsyncBody::empty())?;
let response = client.send(request).await?;

assert!(response.status().is_success());
# Ok(())
# }
```

## Design and limits

- Redirects are disabled by default and can be enabled per request with
  `HttpRequestExt`.
- `ReqwestClient` reads standard proxy environment variables and exposes the
  selected proxy through the transport trait.
- The built-in adapter currently buffers request and response bodies with a
  256 MiB hard limit. Custom transports can preserve streaming semantics when
  an application needs a different memory or backpressure policy.
- Enable `test-support` for a programmable fake client. Use
  `BlockedHttpClient` when a subsystem must be prevented from reaching the
  network.

Part of the [Kael](https://github.com/Augani/kael) native application framework.
See the [Kael documentation](https://augani.github.io/kael/) for framework-level
guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
