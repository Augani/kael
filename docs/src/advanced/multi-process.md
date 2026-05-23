# Multi-Process & IPC

Kael supports Electron-style multi-process architecture with typed IPC and process supervision.

## Process model

```rust
use kael::process_model::*;

// Define process classes
let worker = ProcessClass::Worker;
let media = ProcessClass::Media;
let extension = ProcessClass::Extension;
```

## IPC transport

Typed request/response communication between processes:

```rust
use kael::ipc_transport::*;

// Define message types
type MyIpc = IpcMessage<MyRequest, MyResponse, MyProgress, MyError>;

// Platform-native transport
// macOS/Linux: Unix Domain Sockets
// Windows: Named Pipes
```

## Supervisor

Process supervision with restart policies:

```rust
use kael::supervisor::*;

// Restart on failure with exponential backoff
let policy = RestartPolicy::OnFailure {
    max_retries: 5,
    backoff: Duration::from_secs(1),
};

// Health checks
let health = HealthCheckConfig {
    interval: Duration::from_secs(30),
    timeout: Duration::from_secs(5),
};
```
