# kael_net

Transport-agnostic auth, request models, offline queues, presence tracking,
retry policies, and a real bounded WebSocket client for Kael applications.

`WebSocketClient` has one API on native targets and
`wasm32-unknown-unknown`: checked `ws`/`wss` URLs and subprotocols, text and
binary messages, monotonic ordered events, count and byte queue limits,
non-blocking send backpressure, sanitized error/close metadata, bounded
reconnection, explicit close, and final-handle cleanup. Native connections use
Tungstenite with Rustls and native certificate roots on a private worker (no
Tokio runtime required). Browser connections use the browser's WebSocket API.

Opening a connection requires an explicit `WebSocketHostPolicy`; Kael's core
`NetworkPolicy` implements that trait. Debug and error output never includes a
URL, payload, or close reason. See the
[realtime networking guide](https://augani.github.io/kael/realtime-networking.html)
for limits, browser security behavior, and an end-to-end example.

Part of [Kael](https://github.com/Augani/kael), a native application framework
for responsive, resource-conscious desktop software. See the
[documentation](https://augani.github.io/kael/) for usage and guides.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
