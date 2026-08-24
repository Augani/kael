# Realtime Networking

`kael_net::WebSocketClient` is the shared live collaboration transport for
desktop and browser builds. It uses a private Tungstenite/Rustls worker on
native targets and `web_sys::WebSocket` in WebAssembly, but exposes the same
configuration, message, event, backpressure, close, and reconnection types.
It does not require a Tokio runtime.

## Open a checked connection

Core realtime descriptors bridge directly to the transport. A network policy
is mandatory at the side-effect boundary:

```rust,no_run
use kael::{
    AppRealtimeConnection, AppRealtimeReconnectPolicy, NetworkPolicyBuilder,
};
use std::time::Duration;

let policy = NetworkPolicyBuilder::new()
    .allow_host("collab.example.com")
    .build_checked()?;

let descriptor = AppRealtimeConnection::websocket(
    "wss://collab.example.com/session",
)
.protocol("kael.collab.v1")
.max_message_bytes(1024 * 1024)
.reconnect_policy(AppRealtimeReconnectPolicy::new(
    5,
    Duration::from_secs(1),
    Duration::from_secs(30),
))
.network_policy(policy)
.build_checked()?;

let socket = descriptor.open_websocket_transport()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Applications using `kael_net` without core descriptors can build a
`WebSocketConfig` directly and pass an implementation of
`WebSocketHostPolicy` to `WebSocketClient::connect`. `DenyAllWebSocketHosts`
and `AllowAllWebSocketHosts` make the decision explicit; production apps
should normally use Kael's checked host allow-list.

Poll `socket.poll_event()` from the application event loop. Every event has a
monotonically increasing sequence number and contains one of `Open`, `Message`,
`Error`, `Reconnecting`, or `Closed`. Text and binary payloads retain transport
order. `try_send` never blocks the UI thread and returns `Backpressure` when
either the outbound count or byte budget is full.

## Bounds

The production defaults are:

| Bound | Default | Checked maximum |
| --- | ---: | ---: |
| Queued inbound messages | 1,024 | 65,536 |
| Queued outbound messages | 256 | 65,536 |
| One message | 16 MiB | 128 MiB |
| Queued inbound payloads | 32 MiB | 512 MiB |
| Queued outbound payloads | 16 MiB | 512 MiB |
| Browser `bufferedAmount` threshold | 4 MiB | 512 MiB |
| Native connect/TLS timeout | 15 seconds | 5 minutes |
| Reconnect attempts | disabled | 100 |

Lifecycle events use a small bounded reserve in addition to the configured
message count. If an app does not drain inbound messages within its count or
byte budget, the transport emits `InboundBackpressure` and closes rather than
growing memory without limit. Native Tungstenite enforces the message limit
while framing. The browser only exposes a complete `MessageEvent`, so one
oversized browser message is necessarily materialized by the browser before
Kael can reject it and close the socket.

The native timeout covers TCP connection attempts and the WebSocket/TLS
handshake. Hostname resolution is performed by the operating system resolver;
like `std::net::ToSocketAddrs`, it does not expose a separately cancellable DNS
deadline.

## Delivery and reconnection

Automatic reconnect applies only to abnormal loss. An application close and
clean peer codes `1000` or `1001` are terminal. Messages still in Kael's
bounded outbound queue remain ordered across a reconnect. A message already
handed to the operating system or browser is not replayed because the transport
cannot know whether the peer processed it; collaboration protocols that need
exactly-once effects should use application message IDs and acknowledgements.

Each failed attempt emits sanitized `Error`, `Closed`, and `Reconnecting`
events in that order. A successful open resets the attempt counter. Dropping
the final client clone removes browser handlers and timers or asks the native
worker to terminate within its socket timeout. `close` is the deterministic
choice when application code needs a close event.

## Browser security and parity boundaries

- Browsers own DNS, proxies, certificates, cookies, CSP, mixed-content rules,
  and the HTTP upgrade. An HTTPS page normally needs a `wss://` endpoint.
- Browser WebSockets cannot set arbitrary handshake headers. The core adapter
  rejects descriptors containing headers on every target so desktop and web do
  not silently diverge. Authenticate with an appropriate cookie, a short-lived
  signed endpoint, or an application message after `Open`.
- Browser WebSockets cannot originate protocol ping frames. The core adapter
  therefore rejects descriptor-level protocol heartbeat intervals on every
  target; portable collaboration protocols can send a bounded application
  heartbeat with `try_send` instead.
- JavaScript only permits close code `1000` or application codes `3000..=4999`.
  Kael uses `4003`, `4009`, and `4013` on the browser wire for its own terminal
  guards and normalizes the public close metadata to `1003`, `1009`, and `1013`
  to match native behavior.
- Errors from browsers intentionally contain little diagnostic detail. Kael
  exposes stable categories and never places a URL, token, payload, or close
  reason in `Debug`/`Display` output.
- Server-sent events remain a checked `AppRealtimeConnectionKind` descriptor,
  but do not yet have a shared live Kael transport.

The maintained release probe starts a local server and verifies the real Chrome
path for policy rejection, pre-open queue backpressure, size limits,
subprotocol negotiation, ordered text/binary echoes, explicit cancellation,
normalized oversize error/close metadata, abnormal-loss reconnection, and
teardown:

```bash
bash scripts/verify-browser-websocket-smoke.sh
```
