# Multi-Process & IPC

Kael supports an Electron-style multi-process architecture: the UI runs in the main process while heavy or untrusted work runs in supervised child processes that communicate over typed IPC. Transport is platform-native — Unix domain sockets on macOS/Linux, named pipes on Windows — and the framework handles framing, request/response correlation, progress streaming, and crash reporting for you.

## Process model

Every process has a class describing its role:

```rust
use kael::ProcessClass;

ProcessClass::Ui;        // the main UI process
ProcessClass::Worker;    // background compute
ProcessClass::Media;     // media decode/playback
ProcessClass::Extension; // sandboxed plugins (see Plugins & Extensions)
```

A child process is described by a `ProcessInfo`, built fluently:

```rust
use kael::{ProcessId, ProcessInfo};

let info = ProcessInfo::worker(ProcessId(0), "thumbnailer")
    .executable("/path/to/worker-binary")
    .arg("--quiet")
    .env("RUST_LOG", "warn");
```

Constructors exist for each role: `ProcessInfo::worker`, `ProcessInfo::media`, and `ProcessInfo::extension`.

## Spawning a worker (host side)

`WorkerHost` owns the socket directory and supervises spawned children. `request` sends a typed payload and blocks for the response; `fire_and_forget` sends without waiting; `health_check` pings the child.

```rust
use kael::{ProcessClass, ProcessId, ProcessInfo, WorkerHost};

let mut host = WorkerHost::with_temp_dir();
let info = ProcessInfo::worker(ProcessId(0), "thumbnailer")
    .executable(worker_binary_path);

let worker = host.spawn_worker(ProcessClass::Worker, info)?;

worker.health_check()?; // round-trip ping

let response: serde_json::Value = worker.request(serde_json::json!({
    "op": "echo",
    "message": "hello from host",
}))?;
assert_eq!(response["message"], "hello from host");
```

## The worker child

The child binary connects back to the host with `WorkerClient::connect_from_env` (it reads the `GPUI_WORKER_SOCKET` / `GPUI_WORKER_PIPE` environment variable the host sets) and serves requests with `run`. The handler receives a `WorkerRequest` and a `progress` callback for streaming intermediate updates, and returns a `WorkerResponse` or `WorkerError`.

```rust
use anyhow::Result;
use kael::{WorkerClient, WorkerProgress, WorkerRequest, WorkerResponse};

fn main() -> Result<()> {
    let client = WorkerClient::connect_from_env()?;
    client.run(|request, progress| match request {
        WorkerRequest::Ping => Ok(WorkerResponse::Pong),
        WorkerRequest::Execute { payload } => {
            progress(WorkerProgress::Update(serde_json::json!({ "step": 1 })));
            // ... do work ...
            Ok(WorkerResponse::Result(payload))
        }
    })
}
```

The message types:

| Type | Variants |
|------|----------|
| `WorkerRequest` | `Ping`, `Execute { payload: serde_json::Value }` |
| `WorkerResponse` | `Pong`, `Result(serde_json::Value)` |
| `WorkerProgress` | `Update(serde_json::Value)` |
| `WorkerError` | `Execution(String)`, `Cancelled` |

## Supervision & crash handling

Register an event callback to observe lifecycle events. A child that crashes is reported as a `SupervisorEvent::Exited` rather than taking down the host:

```rust
use kael::SupervisorEvent;

host.on_event(|event| match event {
    SupervisorEvent::Exited { id, .. } => eprintln!("worker {id:?} exited"),
    _ => {}
});
```

Each `WorkerHandle` exposes `id()` to correlate it with supervisor events.

## Extension processes

Extension children use the same transport but a richer RPC envelope (handshake, contribution discovery, command dispatch). They are managed by `ExtensionHostRuntime` rather than `WorkerHost` — see [Plugins & Extensions](plugins.md).
