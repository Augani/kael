# Browser workers

Kael maps its typed native worker contract onto dedicated browser Web Workers.
The host and worker exchange the same `WorkerRequest`, `WorkerResponse`,
`WorkerProgress`, `WorkerError`, `IpcMessage`, and bootstrap handshake types on
both targets. The browser transport adds a versioned envelope and transfers a
framed `Uint8Array` rather than accepting arbitrary JavaScript objects.

Use the asynchronous APIs in shared desktop/browser source:

```rust,ignore
let info = ProcessInfo::worker(ProcessId(0), "sheet-recalculation")
    .executable("./sheet_worker_bootstrap.js");
let mut host = WorkerHost::with_temp_dir();
let worker = host.spawn_worker(ProcessClass::Worker, info)?;

worker.health_check_async().await?;
let output: RecalculationResult = worker.request_async(input).await?;
host.terminate_worker(worker.id())?;
```

Desktop calls wait on the existing socket or named-pipe transport using a
blocking-pool task. Browser calls await `postMessage` responses and never block
the JavaScript UI thread. The legacy synchronous `request`, `health_check`, and
worker-pool request methods remain unchanged on desktop; browser builds return
an explicit error directing the caller to the async form.

`BackgroundExecutor::spawn` cannot automatically move an arbitrary `Send +
'static` Rust future into a Web Worker. A browser worker instantiates a separate
WebAssembly heap, so a closure containing Rust pointers is not transferable.
Use `BackgroundExecutor::spawn_worker_request` for a serializable, typed CPU
handoff. This schedules only the message-wait future on the UI event loop while
the registered worker handler performs the expensive work.

## Worker bootstrap

A module-worker bootstrap initializes the same generated WebAssembly module:

```js
import init from "./my_app.js";

const startupMessages = [];
const bufferStartupMessage = (event) => startupMessages.push(event);
self.addEventListener("message", bufferStartupMessage);

await init();

self.removeEventListener("message", bufferStartupMessage);
for (const event of startupMessages) {
  self.onmessage?.call(self, event);
}
```

Buffering the startup window makes the handshake independent of how long the
generated JavaScript and WebAssembly take to initialize.

The application's `main` or exported worker entry detects
`DedicatedWorkerGlobalScope`, calls `WorkerClient::connect_from_env`, and
installs its handler with `WorkerClient::run`. The host supplies the bootstrap
module URL through `ProcessInfo::executable`. Module workers are the default;
`WorkerHost::classic_worker_scripts` is available for a deliberately classic
script build.

## Bounds and browser boundary

- The wire protocol rejects an unsupported version, malformed/trailing frames,
  non-`Uint8Array` messages, and payloads over the shared 16 MiB IPC limit.
- Each worker retains at most 1,024 pending request callbacks. Async requests
  default to a 30-second response timeout; bootstrap defaults to 10 seconds.
- Capabilities are validated and must be granted exactly. The default is only
  `worker:execute`; configure both `WorkerHost::with_capabilities` and
  `WorkerClient::connect_with_capabilities` to expand it.
- `javascript:` and `data:` worker URLs are rejected. Browser same-origin/CORS,
  Content Security Policy, and module-worker policy still apply.
- Browser process arguments, environment variables, working directories, and
  automatic restart policies have no faithful Web Worker equivalent and return
  explicit errors. Recreate a failed worker from its `Exited` event.
- Cancellation is cooperative. A synchronous CPU handler cannot observe a
  queued cancel until it yields or returns; split interruptible algorithms into
  bounded requests.
- This bridge does not require shared memory or cross-origin isolation. That
  keeps deployment simple, but requests cross an explicit serialization
  boundary instead of sharing Rust references.

## Maintained release probe

Build the independent worker probe without modifying the retained-scene smoke:

```sh
bash scripts/build-browser-worker-smoke.sh
python3 -m http.server 8000 --directory target/browser-worker-smoke
```

Or run the build, headless browser, and marker checks together:

```sh
bash scripts/verify-browser-worker-smoke.sh
```

The probe validates handshake/health, typed progress, a one-million-item CPU
request, a bounded 25 ms worker CPU interval during which the UI event-loop
timer must fire, the explicit synchronous-browser error, and termination. A
headless CI gate should require all of these final DOM markers:

```text
data-kael-worker-probe="passed"
data-kael-worker-protocol="1"
data-kael-worker-items="1000000"
data-kael-worker-progress="passed"
data-kael-worker-ui-thread="responsive"
data-kael-worker-terminated="passed"
```

The CI verifier keeps headless Chrome alive on real wall-clock time and waits
for an equivalent `__kael_worker_pass__=1` HTTP beacon. Avoid Chrome virtual time
for this probe: it can advance the host's bounded request timer while the
independently scheduled worker is still loading JavaScript and WebAssembly.
