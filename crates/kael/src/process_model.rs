//! Process-isolation model and typed IPC for GPUI.
//!
//! This module defines the shared contracts for the GPUI process model:
//! process classes, IPC messages, supervision policies, and worker APIs.
//! Platform-specific backends implement the actual transport and spawning.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Process Class
// ---------------------------------------------------------------------------

/// The class of a GPUI child process, which determines its capabilities and
/// expected lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProcessClass {
    /// The main UI process. There is exactly one per application.
    Ui,
    /// A background worker for CPU-intensive or blocking tasks.
    Worker,
    /// A media/capture pipeline process.
    Media,
    /// An extension or plugin host process.
    Extension,
}

impl ProcessClass {
    /// Human-readable label used in logging and diagnostics.
    pub fn label(&self) -> &'static str {
        match self {
            ProcessClass::Ui => "ui",
            ProcessClass::Worker => "worker",
            ProcessClass::Media => "media",
            ProcessClass::Extension => "extension",
        }
    }
}

impl std::fmt::Display for ProcessClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// Process Identity
// ---------------------------------------------------------------------------

/// A stable identifier for a GPUI child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessId(pub u64);

/// Metadata describing a running or requested child process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// The unique process identifier.
    pub id: ProcessId,
    /// The process class.
    pub class: ProcessClass,
    /// A human-readable name for diagnostics.
    pub name: String,
    /// The executable path.
    pub executable: PathBuf,
    /// Command-line arguments passed to the child process.
    pub args: Vec<String>,
    /// Environment variables passed to the child.
    pub env: HashMap<String, String>,
    /// Working directory for the child.
    pub working_dir: Option<PathBuf>,
}

impl ProcessInfo {
    /// Create a new process info with the given class and name.
    pub fn new(id: ProcessId, class: ProcessClass, name: impl Into<String>) -> Self {
        Self {
            id,
            class,
            name: name.into(),
            executable: PathBuf::new(),
            args: Vec::new(),
            env: HashMap::new(),
            working_dir: None,
        }
    }

    /// Create a UI-process descriptor.
    pub fn ui(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Ui, name)
    }

    /// Create a worker-process descriptor.
    pub fn worker(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Worker, name)
    }

    /// Create a media-process descriptor.
    pub fn media(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Media, name)
    }

    /// Create an extension-process descriptor.
    pub fn extension(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Extension, name)
    }

    /// Set the executable path.
    pub fn executable(mut self, path: impl AsRef<Path>) -> Self {
        self.executable = path.as_ref().to_path_buf();
        self
    }

    /// Append a single command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set command-line arguments for the child process.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Add an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.working_dir = Some(path.as_ref().to_path_buf());
        self
    }
}

// ---------------------------------------------------------------------------
// IPC Message Protocol
// ---------------------------------------------------------------------------

/// A typed IPC message exchanged between GPUI processes.
///
/// The generic parameters allow application-specific request, response,
/// progress, and error types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcMessage<Request, Response, Progress, Error> {
    /// A request that expects a correlated response.
    Request {
        /// Correlation identifier matching request to response.
        id: u64,
        /// The request payload.
        body: Request,
    },
    /// A response to a previously-sent request.
    Response {
        /// Correlation identifier matching the request.
        id: u64,
        /// The result of the request.
        result: Result<Response, Error>,
    },
    /// A progress update for a long-running request.
    Progress {
        /// Correlation identifier matching the request.
        id: u64,
        /// Progress payload.
        body: Progress,
    },
    /// A cancellation signal for a pending request.
    Cancel {
        /// Correlation identifier matching the request.
        id: u64,
    },
}

// ---------------------------------------------------------------------------
// Supervision and Restart Policies
// ---------------------------------------------------------------------------

/// Policy controlling how the supervisor responds to child process failures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RestartPolicy {
    /// Never restart the process.
    Never,
    /// Restart only on non-zero exit or crash, with bounded restarts and
    /// optional exponential backoff.
    OnFailure {
        /// Maximum number of restart attempts before giving up.
        max_restarts: u32,
        /// Base duration for backoff between restart attempts.
        backoff: Duration,
    },
    /// Always restart the process, with optional backoff.
    Always {
        /// Base duration for backoff between restart attempts.
        backoff: Duration,
    },
}

/// The current health status of a supervised process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessHealth {
    /// The process is starting.
    Starting,
    /// The process is running and healthy.
    Healthy,
    /// The process has not sent a heartbeat recently.
    Unresponsive,
    /// The process has exited or crashed.
    Dead,
    /// The process has been stopped by the supervisor.
    Stopped,
}

/// Health-check configuration for a supervised process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// How often the child must send a heartbeat.
    pub heartbeat_interval: Duration,
    /// How many heartbeats may be missed before the process is declared
    /// unresponsive.
    pub missed_heartbeats_before_unhealthy: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(5),
            missed_heartbeats_before_unhealthy: 3,
        }
    }
}

/// Spawn-time options for a supervised process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSpawnOptions {
    /// Restart behavior for the process.
    pub restart_policy: RestartPolicy,
    /// Health-check configuration for the process.
    pub health_check: HealthCheckConfig,
}

impl ProcessSpawnOptions {
    /// Create spawn options with the given restart policy and health check.
    pub fn new(restart_policy: RestartPolicy, health_check: HealthCheckConfig) -> Self {
        Self {
            restart_policy,
            health_check,
        }
    }

    /// Override the restart policy.
    pub fn restart_policy(mut self, restart_policy: RestartPolicy) -> Self {
        self.restart_policy = restart_policy;
        self
    }

    /// Override the health-check configuration.
    pub fn health_check(mut self, health_check: HealthCheckConfig) -> Self {
        self.health_check = health_check;
        self
    }
}

impl Default for ProcessSpawnOptions {
    fn default() -> Self {
        Self {
            restart_policy: RestartPolicy::Never,
            health_check: HealthCheckConfig::default(),
        }
    }
}

/// Lifecycle events emitted by a process supervisor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SupervisorEvent {
    /// A process was spawned successfully.
    Spawned {
        /// Information about the spawned process.
        info: ProcessInfo,
    },
    /// The health of a process changed.
    HealthChanged {
        /// The process identifier.
        id: ProcessId,
        /// Previous health.
        old: ProcessHealth,
        /// New health.
        new: ProcessHealth,
    },
    /// A process exited.
    Exited {
        /// The process identifier.
        id: ProcessId,
        /// Exit code if available.
        exit_code: Option<i32>,
        /// Whether the supervisor will attempt a restart.
        will_restart: bool,
    },
    /// A process is about to be restarted.
    Restarting {
        /// The process identifier.
        id: ProcessId,
        /// Restart attempt number.
        attempt: u32,
        /// Backoff before restart.
        backoff: Duration,
    },
    /// A process restarted successfully.
    Restarted {
        /// Information about the restarted process.
        info: ProcessInfo,
    },
    /// A process was stopped intentionally.
    Stopped {
        /// The process identifier.
        id: ProcessId,
    },
    /// Spawning or restarting a process failed.
    SpawnFailed {
        /// Information about the process that failed to start.
        info: ProcessInfo,
        /// Human-readable error.
        error: String,
    },
}

// ---------------------------------------------------------------------------
// Supervisor Contract
// ---------------------------------------------------------------------------

/// Trait for a process supervisor that can launch, monitor, and restart child
/// processes.
///
/// Implementations are provided per-platform.
pub trait Supervisor: Send + Sync {
    /// Launch a child process with the given info and restart policy.
    fn spawn(&mut self, info: ProcessInfo, policy: RestartPolicy) -> Result<ProcessId>;

    /// Launch a child process with explicit spawn options.
    fn spawn_with_options(
        &mut self,
        info: ProcessInfo,
        options: ProcessSpawnOptions,
    ) -> Result<ProcessId> {
        self.spawn(info, options.restart_policy)
    }

    /// Stop a running child process.
    fn stop(&mut self, id: ProcessId) -> Result<()>;

    /// Get the current health of a child process.
    fn health(&self, id: ProcessId) -> Option<ProcessHealth>;

    /// Return the IDs of all currently supervised processes.
    fn processes(&self) -> Vec<ProcessId>;

    /// Subscribe to health changes for a specific process.
    fn on_health_change(
        &mut self,
        id: ProcessId,
        callback: Box<dyn FnMut(ProcessId, ProcessHealth) + Send>,
    );

    /// Subscribe to supervisor-wide lifecycle events.
    fn on_event(&mut self, _callback: Box<dyn FnMut(SupervisorEvent) + Send>) {}
}

// ---------------------------------------------------------------------------
// Worker Task Contract
// ---------------------------------------------------------------------------

/// A task that can be offloaded to a worker process.
pub trait WorkerTask: Send + 'static {
    /// The result type returned on completion.
    type Output: Send + 'static;
    /// The progress type emitted during execution.
    type Progress: Send + 'static;
    /// The error type on failure.
    type Error: Send + 'static;

    /// Execute the task. Called in the worker process.
    fn run(self, on_progress: impl Fn(Self::Progress) + Send) -> Result<Self::Output, Self::Error>;
}

// ---------------------------------------------------------------------------
// Worker IPC Protocol
// ---------------------------------------------------------------------------

/// A request sent from host to worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerRequest {
    /// Execute a task with the given JSON payload.
    Execute {
        /// The serialized task payload.
        payload: serde_json::Value,
    },
    /// Ping the worker for liveness.
    Ping,
}

/// A response sent from worker to host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerResponse {
    /// The result of a task execution.
    Result(serde_json::Value),
    /// Pong response to a ping.
    Pong,
}

/// A progress update sent from worker to host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerProgress {
    /// A progress update with a JSON payload.
    Update(serde_json::Value),
}

/// An error returned by a worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerError {
    /// The task failed with the given message.
    Execution(String),
    /// The request was cancelled.
    Cancelled,
}

/// Bootstrap message exchanged during worker initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BootstrapMessage {
    /// Host sends version and requested capabilities.
    Handshake {
        /// Protocol version.
        version: u32,
        /// Requested capabilities.
        capabilities: Vec<String>,
    },
    /// Worker acknowledges handshake with heartbeat config.
    HandshakeAck {
        /// Heartbeat interval in seconds.
        heartbeat_interval_secs: u64,
        /// Granted capabilities.
        granted_capabilities: Vec<String>,
    },
    /// Periodic heartbeat.
    Heartbeat,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_class_label() {
        assert_eq!(ProcessClass::Ui.label(), "ui");
        assert_eq!(ProcessClass::Worker.label(), "worker");
        assert_eq!(ProcessClass::Media.label(), "media");
        assert_eq!(ProcessClass::Extension.label(), "extension");
    }

    #[test]
    fn test_process_info_builder() {
        let info = ProcessInfo::worker(ProcessId(1), "indexer")
            .executable("/usr/local/bin/gpui_worker")
            .arg("--once")
            .arg("--verbose")
            .env("RUST_LOG", "info")
            .working_dir("/tmp");

        assert_eq!(info.id, ProcessId(1));
        assert_eq!(info.class, ProcessClass::Worker);
        assert_eq!(info.name, "indexer");
        assert_eq!(info.executable, PathBuf::from("/usr/local/bin/gpui_worker"));
        assert_eq!(
            info.args,
            vec!["--once".to_string(), "--verbose".to_string()]
        );
        assert_eq!(info.env.get("RUST_LOG"), Some(&"info".to_string()));
        assert_eq!(info.working_dir, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn test_ipc_message_roundtrip() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum MyRequest {
            DoWork,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum MyResponse {
            Done,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum MyProgress {
            Percent(u8),
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum MyError {
            Failed,
        }

        let msg = IpcMessage::<MyRequest, MyResponse, MyProgress, MyError>::Request {
            id: 42,
            body: MyRequest::DoWork,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let decoded: IpcMessage<MyRequest, MyResponse, MyProgress, MyError> =
            serde_json::from_str(&json).unwrap();

        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_restart_policy_serialization() {
        let policy = RestartPolicy::OnFailure {
            max_restarts: 5,
            backoff: Duration::from_secs(2),
        };
        let json = serde_json::to_string(&policy).unwrap();
        let decoded: RestartPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, decoded);
    }

    #[test]
    fn test_health_check_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.heartbeat_interval, Duration::from_secs(5));
        assert_eq!(config.missed_heartbeats_before_unhealthy, 3);
    }

    #[test]
    fn test_spawn_options_default() {
        let options = ProcessSpawnOptions::default();
        assert_eq!(options.restart_policy, RestartPolicy::Never);
        assert_eq!(options.health_check, HealthCheckConfig::default());
    }

    #[test]
    fn test_supervisor_event_serialization() {
        let event = SupervisorEvent::Exited {
            id: ProcessId(7),
            exit_code: Some(1),
            will_restart: true,
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: SupervisorEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, decoded);
    }
}
