//! Process-isolation model and typed IPC for GPUI.
//!
//! This module defines the shared contracts for the GPUI process model:
//! process classes, IPC messages, supervision policies, and worker APIs.
//! Platform-specific backends implement the actual transport and spawning.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result};
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
    /// A generic helper process for isolated app-owned native utilities.
    Utility,
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
            ProcessClass::Utility => "utility",
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

    /// Create a utility-process descriptor.
    pub fn utility(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Utility, name)
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

    /// Validate the descriptor before handing it to a process supervisor.
    pub fn validate(&self) -> Result<()> {
        validate_process_name(&self.name)?;
        validate_executable_path(&self.executable, false)?;
        validate_process_args(&self.args)?;
        validate_process_env(&self.env)?;
        if let Some(working_dir) = &self.working_dir {
            validate_working_dir(working_dir, false)?;
        }
        Ok(())
    }
}

/// Builder for checked child-process descriptors.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInfoBuilder {
    info: ProcessInfo,
    require_existing_executable: bool,
    canonicalize_executable: bool,
    require_existing_working_dir: bool,
    canonicalize_working_dir: bool,
}

impl ProcessInfoBuilder {
    /// Create a checked descriptor builder.
    pub fn new(id: ProcessId, class: ProcessClass, name: impl Into<String>) -> Self {
        Self::from(ProcessInfo::new(id, class, name))
    }

    /// Create a checked UI-process descriptor builder.
    pub fn ui(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Ui, name)
    }

    /// Create a checked worker-process descriptor builder.
    pub fn worker(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Worker, name)
    }

    /// Create a checked utility-process descriptor builder.
    pub fn utility(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Utility, name)
    }

    /// Create a checked media-process descriptor builder.
    pub fn media(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Media, name)
    }

    /// Create a checked extension-process descriptor builder.
    pub fn extension(id: ProcessId, name: impl Into<String>) -> Self {
        Self::new(id, ProcessClass::Extension, name)
    }

    /// Set the executable path.
    pub fn executable(mut self, path: impl AsRef<Path>) -> Self {
        self.info.executable = path.as_ref().to_path_buf();
        self
    }

    /// Require the executable to exist and be a file.
    pub fn require_existing_executable(mut self) -> Self {
        self.require_existing_executable = true;
        self
    }

    /// Canonicalize the executable path while building.
    pub fn canonicalize_executable(mut self) -> Self {
        self.canonicalize_executable = true;
        self.require_existing_executable = true;
        self
    }

    /// Append a single command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.info.args.push(arg.into());
        self
    }

    /// Set command-line arguments for the child process.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.info.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Add an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.info.env.insert(key.into(), value.into());
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.info.working_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Require the working directory to exist and be a directory.
    pub fn require_existing_working_dir(mut self) -> Self {
        self.require_existing_working_dir = true;
        self
    }

    /// Canonicalize the working directory while building.
    pub fn canonicalize_working_dir(mut self) -> Self {
        self.canonicalize_working_dir = true;
        self.require_existing_working_dir = true;
        self
    }

    /// Validate the configured process descriptor.
    pub fn validate(&self) -> Result<()> {
        self.info.validate()?;
        validate_executable_path(&self.info.executable, self.require_existing_executable)?;
        if let Some(working_dir) = &self.info.working_dir {
            validate_working_dir(working_dir, self.require_existing_working_dir)?;
        } else {
            anyhow::ensure!(
                !self.require_existing_working_dir && !self.canonicalize_working_dir,
                "process working directory is required"
            );
        }
        Ok(())
    }

    /// Build a validated process descriptor.
    pub fn build_checked(mut self) -> Result<ProcessInfo> {
        self.validate()?;
        if self.canonicalize_executable {
            self.info.executable = self.info.executable.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize executable {}",
                    self.info.executable.display()
                )
            })?;
        }
        if self.canonicalize_working_dir
            && let Some(working_dir) = self.info.working_dir.take()
        {
            self.info.working_dir = Some(working_dir.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize working directory {}",
                    working_dir.display()
                )
            })?);
        }
        Ok(self.info)
    }
}

impl From<ProcessInfo> for ProcessInfoBuilder {
    fn from(info: ProcessInfo) -> Self {
        Self {
            info,
            require_existing_executable: false,
            canonicalize_executable: false,
            require_existing_working_dir: false,
            canonicalize_working_dir: false,
        }
    }
}

// ---------------------------------------------------------------------------
// App-Facing Helper Launch Descriptor
// ---------------------------------------------------------------------------

/// Environment inheritance policy for a helper process launch.
///
/// Helper launches default to [`ExplicitOnly`](Self::ExplicitOnly) so generated
/// apps do not accidentally leak the entire parent process environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessEnvironmentPolicy {
    /// Pass only environment variables explicitly set on the process info.
    ExplicitOnly,
    /// Inherit a checked allowlist from the parent environment.
    InheritAllowlist(Vec<String>),
}

impl Default for ProcessEnvironmentPolicy {
    fn default() -> Self {
        Self::ExplicitOnly
    }
}

impl ProcessEnvironmentPolicy {
    /// Validate the environment policy before a launcher expands it.
    pub fn validate(&self) -> Result<()> {
        match self {
            ProcessEnvironmentPolicy::ExplicitOnly => Ok(()),
            ProcessEnvironmentPolicy::InheritAllowlist(keys) => {
                let mut seen = HashSet::new();
                for key in keys {
                    validate_environment_key(key, "process inherited environment key")?;
                    anyhow::ensure!(
                        seen.insert(key),
                        "process inherited environment key requested more than once: {key}"
                    );
                }
                Ok(())
            }
        }
    }

    /// Return inherited environment keys, if inheritance is enabled.
    pub fn inherited_keys(&self) -> &[String] {
        match self {
            ProcessEnvironmentPolicy::ExplicitOnly => &[],
            ProcessEnvironmentPolicy::InheritAllowlist(keys) => keys,
        }
    }
}

/// A validated request to launch an app-owned helper process.
///
/// This descriptor is intentionally transport-agnostic: it describes the
/// executable, arguments, environment policy, supervision policy, and declared
/// capability labels, while the platform supervisor owns the actual process
/// creation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelperProcessLaunch {
    info: ProcessInfo,
    options: ProcessSpawnOptions,
    environment_policy: ProcessEnvironmentPolicy,
    capabilities: Vec<String>,
}

impl HelperProcessLaunch {
    /// Start building a checked helper launch descriptor.
    pub fn builder(
        id: ProcessId,
        class: ProcessClass,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::new(id, class, name, executable)
    }

    /// Start building a checked utility-process launch descriptor.
    pub fn utility(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::utility(id, name, executable)
    }

    /// Start building a checked worker-process launch descriptor.
    pub fn worker(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::worker(id, name, executable)
    }

    /// Return process identity and command metadata.
    pub fn info(&self) -> &ProcessInfo {
        &self.info
    }

    /// Return supervision options for the launch.
    pub fn options(&self) -> &ProcessSpawnOptions {
        &self.options
    }

    /// Return the environment inheritance policy.
    pub fn environment_policy(&self) -> &ProcessEnvironmentPolicy {
        &self.environment_policy
    }

    /// Return declared capability labels for policy brokers or diagnostics.
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// Consume the descriptor into the pieces expected by a supervisor.
    pub fn into_spawn_parts(self) -> (ProcessInfo, ProcessSpawnOptions) {
        (self.info, self.options)
    }

    /// Validate a launch descriptor before handing it to a platform launcher.
    pub fn validate(&self) -> Result<()> {
        self.info.validate()?;
        self.options.validate()?;
        self.environment_policy.validate()?;
        validate_capability_labels(&self.capabilities)?;
        Ok(())
    }
}

/// Builder for checked app-owned helper process launches.
#[derive(Debug, Clone, PartialEq)]
pub struct HelperProcessLaunchBuilder {
    info: ProcessInfo,
    options: ProcessSpawnOptions,
    environment_policy: ProcessEnvironmentPolicy,
    capabilities: Vec<String>,
    explicit_environment: Vec<(String, String)>,
    require_existing_executable: bool,
    canonicalize_executable: bool,
    require_existing_working_dir: bool,
    canonicalize_working_dir: bool,
}

impl HelperProcessLaunchBuilder {
    /// Create a helper launch builder for a process class.
    pub fn new(
        id: ProcessId,
        class: ProcessClass,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> Self {
        Self {
            info: ProcessInfo::new(id, class, name).executable(executable),
            options: ProcessSpawnOptions::default(),
            environment_policy: ProcessEnvironmentPolicy::default(),
            capabilities: Vec::new(),
            explicit_environment: Vec::new(),
            require_existing_executable: false,
            canonicalize_executable: false,
            require_existing_working_dir: false,
            canonicalize_working_dir: false,
        }
    }

    /// Create a utility-process launch builder.
    pub fn utility(id: ProcessId, name: impl Into<String>, executable: impl AsRef<Path>) -> Self {
        Self::new(id, ProcessClass::Utility, name, executable)
    }

    /// Create a worker-process launch builder.
    pub fn worker(id: ProcessId, name: impl Into<String>, executable: impl AsRef<Path>) -> Self {
        Self::new(id, ProcessClass::Worker, name, executable)
    }

    /// Create a media-process launch builder.
    pub fn media(id: ProcessId, name: impl Into<String>, executable: impl AsRef<Path>) -> Self {
        Self::new(id, ProcessClass::Media, name, executable)
    }

    /// Create an extension-process launch builder.
    pub fn extension(id: ProcessId, name: impl Into<String>, executable: impl AsRef<Path>) -> Self {
        Self::new(id, ProcessClass::Extension, name, executable)
    }

    /// Replace the executable path.
    pub fn executable(mut self, path: impl AsRef<Path>) -> Self {
        self.info.executable = path.as_ref().to_path_buf();
        self
    }

    /// Require the executable to exist and be a file.
    pub fn require_existing_executable(mut self) -> Self {
        self.require_existing_executable = true;
        self
    }

    /// Canonicalize the executable path while building.
    pub fn canonicalize_executable(mut self) -> Self {
        self.canonicalize_executable = true;
        self.require_existing_executable = true;
        self
    }

    /// Append one command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.info.args.push(arg.into());
        self
    }

    /// Append multiple command-line arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.info.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Add one explicit environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.explicit_environment.push((key.into(), value.into()));
        self
    }

    /// Inherit selected environment keys from the parent process.
    pub fn inherit_environment_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.environment_policy =
            ProcessEnvironmentPolicy::InheritAllowlist(keys.into_iter().map(Into::into).collect());
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.info.working_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Require the working directory to exist and be a directory.
    pub fn require_existing_working_dir(mut self) -> Self {
        self.require_existing_working_dir = true;
        self
    }

    /// Canonicalize the working directory while building.
    pub fn canonicalize_working_dir(mut self) -> Self {
        self.canonicalize_working_dir = true;
        self.require_existing_working_dir = true;
        self
    }

    /// Add a declared capability label.
    pub fn capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Add declared capability labels.
    pub fn capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities
            .extend(capabilities.into_iter().map(Into::into));
        self
    }

    /// Replace the supervision options.
    pub fn spawn_options(mut self, options: ProcessSpawnOptions) -> Self {
        self.options = options;
        self
    }

    /// Restart failed helpers with a bounded retry count.
    pub fn restart_on_failure(mut self, max_restarts: u32, backoff: Duration) -> Self {
        self.options.restart_policy = RestartPolicy::OnFailure {
            max_restarts,
            backoff,
        };
        self
    }

    /// Always restart the helper process with backoff.
    pub fn always_restart(mut self, backoff: Duration) -> Self {
        self.options.restart_policy = RestartPolicy::Always { backoff };
        self
    }

    /// Set the heartbeat interval.
    pub fn heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        self.options.health_check.heartbeat_interval = heartbeat_interval;
        self
    }

    /// Set how many missed heartbeats make the helper unhealthy.
    pub fn missed_heartbeats_before_unhealthy(mut self, missed_heartbeats: u32) -> Self {
        self.options.health_check.missed_heartbeats_before_unhealthy = missed_heartbeats;
        self
    }

    /// Validate the configured launch descriptor.
    pub fn validate(&self) -> Result<()> {
        validate_process_name(&self.info.name)?;
        validate_executable_path(&self.info.executable, self.require_existing_executable)?;
        validate_process_args(&self.info.args)?;
        validate_explicit_environment_entries(&self.explicit_environment)?;
        if let Some(working_dir) = &self.info.working_dir {
            validate_working_dir(working_dir, self.require_existing_working_dir)?;
        } else {
            anyhow::ensure!(
                !self.require_existing_working_dir && !self.canonicalize_working_dir,
                "process working directory is required"
            );
        }
        self.options.validate()?;
        self.environment_policy.validate()?;
        validate_capability_labels(&self.capabilities)?;
        Ok(())
    }

    /// Build a validated launch descriptor.
    pub fn build_checked(mut self) -> Result<HelperProcessLaunch> {
        self.validate()?;
        if self.canonicalize_executable {
            self.info.executable = self.info.executable.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize executable {}",
                    self.info.executable.display()
                )
            })?;
        }
        if self.canonicalize_working_dir
            && let Some(working_dir) = self.info.working_dir.take()
        {
            self.info.working_dir = Some(working_dir.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize working directory {}",
                    working_dir.display()
                )
            })?);
        }
        self.info.env = self.explicit_environment.into_iter().collect();
        let launch = HelperProcessLaunch {
            info: self.info,
            options: self.options,
            environment_policy: self.environment_policy,
            capabilities: self.capabilities,
        };
        launch.validate()?;
        Ok(launch)
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

impl RestartPolicy {
    /// Validate restart policy values before supervision starts.
    pub fn validate(&self) -> Result<()> {
        match self {
            RestartPolicy::Never => Ok(()),
            RestartPolicy::OnFailure {
                max_restarts,
                backoff,
            } => {
                anyhow::ensure!(
                    *max_restarts > 0,
                    "restart policy max_restarts must be greater than zero"
                );
                validate_backoff(*backoff)
            }
            RestartPolicy::Always { backoff } => validate_backoff(*backoff),
        }
    }
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

impl HealthCheckConfig {
    /// Validate heartbeat settings before starting supervision.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.heartbeat_interval > Duration::ZERO,
            "heartbeat interval must be greater than zero"
        );
        anyhow::ensure!(
            self.missed_heartbeats_before_unhealthy > 0,
            "missed heartbeat threshold must be greater than zero"
        );
        Ok(())
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

    /// Validate spawn options before passing them to a supervisor.
    pub fn validate(&self) -> Result<()> {
        self.restart_policy.validate()?;
        self.health_check.validate()?;
        Ok(())
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

/// Builder for checked supervised-process spawn options.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSpawnOptionsBuilder {
    restart_policy: RestartPolicy,
    health_check: HealthCheckConfig,
}

impl ProcessSpawnOptionsBuilder {
    /// Create a spawn-options builder with safe defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable automatic restarts.
    pub fn never_restart(mut self) -> Self {
        self.restart_policy = RestartPolicy::Never;
        self
    }

    /// Restart failed workers with a bounded retry count.
    pub fn restart_on_failure(mut self, max_restarts: u32, backoff: Duration) -> Self {
        self.restart_policy = RestartPolicy::OnFailure {
            max_restarts,
            backoff,
        };
        self
    }

    /// Always restart the process with backoff.
    pub fn always_restart(mut self, backoff: Duration) -> Self {
        self.restart_policy = RestartPolicy::Always { backoff };
        self
    }

    /// Set the heartbeat interval.
    pub fn heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        self.health_check.heartbeat_interval = heartbeat_interval;
        self
    }

    /// Set how many missed heartbeats make a process unhealthy.
    pub fn missed_heartbeats_before_unhealthy(mut self, missed_heartbeats: u32) -> Self {
        self.health_check.missed_heartbeats_before_unhealthy = missed_heartbeats;
        self
    }

    /// Replace the restart policy.
    pub fn restart_policy(mut self, restart_policy: RestartPolicy) -> Self {
        self.restart_policy = restart_policy;
        self
    }

    /// Replace the health-check config.
    pub fn health_check(mut self, health_check: HealthCheckConfig) -> Self {
        self.health_check = health_check;
        self
    }

    /// Validate configured spawn options.
    pub fn validate(&self) -> Result<()> {
        self.as_options().validate()
    }

    /// Build validated spawn options.
    pub fn build_checked(self) -> Result<ProcessSpawnOptions> {
        let options = self.as_options();
        options.validate()?;
        Ok(options)
    }

    fn as_options(&self) -> ProcessSpawnOptions {
        ProcessSpawnOptions {
            restart_policy: self.restart_policy.clone(),
            health_check: self.health_check.clone(),
        }
    }
}

impl Default for ProcessSpawnOptionsBuilder {
    fn default() -> Self {
        Self {
            restart_policy: RestartPolicy::Never,
            health_check: HealthCheckConfig::default(),
        }
    }
}

impl From<ProcessSpawnOptions> for ProcessSpawnOptionsBuilder {
    fn from(options: ProcessSpawnOptions) -> Self {
        Self {
            restart_policy: options.restart_policy,
            health_check: options.health_check,
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

fn validate_process_name(name: &str) -> Result<()> {
    validate_non_empty_trimmed(name, "process name")?;
    anyhow::ensure!(
        !name.chars().any(|ch| ch == '/' || ch == '\\'),
        "process name cannot contain path separators"
    );
    Ok(())
}

fn validate_executable_path(path: &Path, require_existing_file: bool) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "process executable path cannot be empty"
    );
    validate_path_string(path, "process executable path")?;
    if require_existing_file {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("process executable does not exist: {}", path.display()))?;
        anyhow::ensure!(
            metadata.is_file(),
            "process executable path must be a file: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_working_dir(path: &Path, require_existing_dir: bool) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "process working directory cannot be empty"
    );
    validate_path_string(path, "process working directory")?;
    if require_existing_dir {
        let metadata = std::fs::metadata(path).with_context(|| {
            format!(
                "process working directory does not exist: {}",
                path.display()
            )
        })?;
        anyhow::ensure!(
            metadata.is_dir(),
            "process working directory must be a directory: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_process_args(args: &[String]) -> Result<()> {
    for arg in args {
        validate_no_nul(arg, "process argument")?;
    }
    Ok(())
}

fn validate_process_env(env: &HashMap<String, String>) -> Result<()> {
    for (key, value) in env {
        validate_environment_key(key, "process environment key")?;
        validate_no_nul(value, "process environment value")?;
    }
    Ok(())
}

fn validate_explicit_environment_entries(env: &[(String, String)]) -> Result<()> {
    let mut seen = HashSet::new();
    for (key, value) in env {
        validate_environment_key(key, "process environment key")?;
        anyhow::ensure!(
            seen.insert(key),
            "process environment key configured more than once: {key}"
        );
        validate_no_nul(value, "process environment value")?;
    }
    Ok(())
}

fn validate_environment_key(key: &str, label: &str) -> Result<()> {
    validate_non_empty_trimmed(key, label)?;
    anyhow::ensure!(!key.contains('='), "{label} cannot contain '='");
    let mut chars = key.chars();
    let first = chars.next().unwrap();
    anyhow::ensure!(
        first == '_' || first.is_ascii_alphabetic(),
        "{label} must start with an ASCII letter or '_'"
    );
    anyhow::ensure!(
        chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()),
        "{label} can contain only ASCII letters, numbers, or '_'"
    );
    Ok(())
}

fn validate_capability_labels(capabilities: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for capability in capabilities {
        validate_non_empty_trimmed(capability, "process capability label")?;
        anyhow::ensure!(
            capability.len() <= 128,
            "process capability label is too long: {capability}"
        );
        anyhow::ensure!(
            capability.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/')
            }),
            "process capability label contains unsupported characters: {capability}"
        );
        anyhow::ensure!(
            seen.insert(capability),
            "process capability label configured more than once: {capability}"
        );
    }
    Ok(())
}

fn validate_backoff(backoff: Duration) -> Result<()> {
    anyhow::ensure!(
        backoff > Duration::ZERO,
        "restart backoff must be greater than zero"
    );
    Ok(())
}

fn validate_path_string(path: &Path, label: &str) -> Result<()> {
    if let Some(text) = path.to_str() {
        validate_no_nul(text, label)?;
    }
    Ok(())
}

fn validate_non_empty_trimmed(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    validate_no_nul(value, label)
}

fn validate_no_nul(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.contains('\0'), "{label} cannot contain NUL bytes");
    Ok(())
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
        assert_eq!(ProcessClass::Utility.label(), "utility");
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
    fn test_process_info_builder_validates_generated_descriptors() {
        let executable = std::env::current_exe().unwrap();
        let working_dir = executable.parent().unwrap();
        let info = ProcessInfoBuilder::worker(ProcessId(2), "thumbnailer")
            .executable(&executable)
            .require_existing_executable()
            .working_dir(working_dir)
            .require_existing_working_dir()
            .arg("--once")
            .env("RUST_LOG", "info")
            .build_checked()
            .unwrap();

        assert_eq!(info.id, ProcessId(2));
        assert_eq!(info.class, ProcessClass::Worker);
        assert_eq!(info.name, "thumbnailer");
        assert_eq!(info.executable, executable);
        assert_eq!(info.working_dir, Some(working_dir.to_path_buf()));

        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), " bad ")
                .executable(&info.executable)
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "bad/name")
                .executable(&info.executable)
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "worker")
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "worker")
                .executable("/definitely/not/a/worker")
                .require_existing_executable()
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "worker")
                .executable(&info.executable)
                .env("BAD=KEY", "value")
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "worker")
                .executable(&info.executable)
                .arg("bad\0arg")
                .validate()
                .is_err()
        );
        assert!(
            ProcessInfoBuilder::worker(ProcessId(3), "worker")
                .executable(&info.executable)
                .working_dir("/definitely/not/a/dir")
                .require_existing_working_dir()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_helper_process_launch_builder_validates_utility_helpers() {
        let executable = std::env::current_exe().unwrap();
        let working_dir = executable.parent().unwrap();
        let launch = HelperProcessLaunch::utility(ProcessId(7), "video-transcoder", &executable)
            .require_existing_executable()
            .working_dir(working_dir)
            .require_existing_working_dir()
            .arg("--input")
            .arg("clip.mov")
            .env("RUST_LOG", "info")
            .inherit_environment_keys(["PATH", "HOME"])
            .capabilities(["media:transcode", "fs/app-data"])
            .restart_on_failure(2, Duration::from_millis(100))
            .heartbeat_interval(Duration::from_secs(1))
            .missed_heartbeats_before_unhealthy(2)
            .build_checked()
            .unwrap();

        assert_eq!(launch.info().class, ProcessClass::Utility);
        assert_eq!(launch.info().executable, executable);
        assert_eq!(launch.info().working_dir, Some(working_dir.to_path_buf()));
        assert_eq!(launch.info().args, vec!["--input", "clip.mov"]);
        assert_eq!(launch.info().env.get("RUST_LOG"), Some(&"info".to_string()));
        assert_eq!(
            launch.environment_policy().inherited_keys(),
            &["PATH".to_string(), "HOME".to_string()]
        );
        assert_eq!(
            launch.options().restart_policy,
            RestartPolicy::OnFailure {
                max_restarts: 2,
                backoff: Duration::from_millis(100),
            }
        );
        assert_eq!(
            launch.capabilities(),
            &["media:transcode".to_string(), "fs/app-data".to_string()]
        );

        let (info, options) = launch.into_spawn_parts();
        assert_eq!(info.class, ProcessClass::Utility);
        assert_eq!(
            options.health_check.heartbeat_interval,
            Duration::from_secs(1)
        );
    }

    #[test]
    fn test_helper_process_launch_builder_rejects_unsafe_inputs() {
        let executable = std::env::current_exe().unwrap();

        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .env("BAD=KEY", "value")
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .env("PATH", "one")
                .env("PATH", "two")
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .inherit_environment_keys(["PATH", "PATH"])
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .inherit_environment_keys(["1BAD"])
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .capability("network read")
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .capability("network:read")
                .capability("network:read")
                .validate()
                .is_err()
        );
        assert!(
            HelperProcessLaunch::utility(ProcessId(8), "bad helper", &executable)
                .restart_on_failure(0, Duration::from_millis(100))
                .validate()
                .is_err()
        );
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
    fn test_process_spawn_options_builder_validates_supervision_policy() {
        let options = ProcessSpawnOptionsBuilder::new()
            .restart_on_failure(3, Duration::from_millis(250))
            .heartbeat_interval(Duration::from_secs(2))
            .missed_heartbeats_before_unhealthy(2)
            .build_checked()
            .unwrap();

        assert_eq!(
            options.restart_policy,
            RestartPolicy::OnFailure {
                max_restarts: 3,
                backoff: Duration::from_millis(250),
            }
        );
        assert_eq!(
            options.health_check.heartbeat_interval,
            Duration::from_secs(2)
        );
        assert_eq!(options.health_check.missed_heartbeats_before_unhealthy, 2);
        assert!(options.validate().is_ok());
        assert!(
            ProcessSpawnOptionsBuilder::new()
                .restart_on_failure(0, Duration::from_millis(250))
                .validate()
                .is_err()
        );
        assert!(
            ProcessSpawnOptionsBuilder::new()
                .always_restart(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            ProcessSpawnOptionsBuilder::new()
                .heartbeat_interval(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            ProcessSpawnOptionsBuilder::new()
                .missed_heartbeats_before_unhealthy(0)
                .validate()
                .is_err()
        );
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
