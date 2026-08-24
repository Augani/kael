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
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessEnvironmentPolicy {
    /// Pass only environment variables explicitly set on the process info.
    #[default]
    ExplicitOnly,
    /// Inherit a checked allowlist from the parent environment.
    InheritAllowlist(Vec<String>),
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

/// Known helper-process profiles for common browser-runtime stack escape hatches.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelperProcessProfile {
    /// No specialized profile; caller owns the launch shape.
    #[default]
    Generic,
    /// Media transcode/extract helper, commonly backed by FFmpeg.
    FfmpegTranscoder,
    /// Language-server helper for editor/dev-tool features.
    LanguageServer,
    /// Extension or plugin host helper.
    PluginHost,
}

impl HelperProcessProfile {
    /// Stable profile key for summaries and telemetry.
    pub fn key(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::FfmpegTranscoder => "ffmpeg-transcoder",
            Self::LanguageServer => "language-server",
            Self::PluginHost => "plugin-host",
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
    #[serde(default)]
    profile: HelperProcessProfile,
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

    /// Start building a media helper profile for FFmpeg-style transcode work.
    pub fn ffmpeg_transcoder(
        id: ProcessId,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::ffmpeg_transcoder(id, executable)
    }

    /// Start building a language-server helper profile.
    pub fn language_server(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::language_server(id, name, executable)
    }

    /// Start building an extension/plugin-host helper profile.
    pub fn plugin_host(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> HelperProcessLaunchBuilder {
        HelperProcessLaunchBuilder::plugin_host(id, name, executable)
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

    /// Return the helper profile used to seed this launch.
    pub fn profile(&self) -> HelperProcessProfile {
        self.profile
    }

    /// Number of explicit environment variables passed to the helper.
    pub fn env_count(&self) -> usize {
        self.info.env.len()
    }

    /// Number of inherited parent-environment keys.
    pub fn inherited_env_count(&self) -> usize {
        self.environment_policy.inherited_keys().len()
    }

    /// Number of command-line arguments.
    pub fn arg_count(&self) -> usize {
        self.info.args.len()
    }

    /// Whether the launch has a working directory.
    pub fn has_working_dir(&self) -> bool {
        self.info.working_dir.is_some()
    }

    /// Content-safe launch summary that avoids names, paths, args, env values, and capabilities.
    pub fn to_text(&self) -> String {
        format!(
            "helper_process_launch profile={} class={} args={} env={} inherited_env={} capabilities={} working_dir={} restart={} heartbeat={}",
            self.profile.key(),
            self.info.class.label(),
            self.arg_count(),
            self.env_count(),
            self.inherited_env_count(),
            self.capabilities.len(),
            self.has_working_dir(),
            restart_policy_key(&self.options.restart_policy),
            self.options.health_check.heartbeat_interval > Duration::ZERO
        )
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

    /// Build a content-safe execution plan for the next helper/plugin host setup step.
    pub fn execution_plan(&self) -> HelperProcessExecutionPlan {
        HelperProcessExecutionPlan::from_launch(self)
    }
}

/// Next builder action for a checked helper process or plugin host launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperProcessPlanAction {
    /// Plugin manifest, plugin permission manifest, IPC schema, and crash policy must be paired.
    ConfigurePluginHostContracts,
    /// Permission broker grants and process context need to be installed before launch.
    InstallBrokerAndContext,
    /// Restart or heartbeat policy must be attached to a supervisor before spawning.
    ConfigureSupervisorPolicy,
    /// The checked descriptor can be handed to the native process supervisor.
    SpawnNativeHelper,
}

impl HelperProcessPlanAction {
    /// Stable action label for logs, setup screens, and generated agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::ConfigurePluginHostContracts => "configure plugin host contracts",
            Self::InstallBrokerAndContext => "install broker and context",
            Self::ConfigureSupervisorPolicy => "configure supervisor policy",
            Self::SpawnNativeHelper => "spawn native helper",
        }
    }
}

/// Builder-facing execution plan for a checked helper process launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperProcessExecutionPlan {
    profile: HelperProcessProfile,
    class: ProcessClass,
    action: HelperProcessPlanAction,
    arg_count: usize,
    env_count: usize,
    inherited_env_count: usize,
    capability_count: usize,
    has_working_dir: bool,
    has_restart_policy: bool,
    has_heartbeat: bool,
}

impl HelperProcessExecutionPlan {
    /// Build an execution plan from a checked helper launch descriptor.
    pub fn from_launch(launch: &HelperProcessLaunch) -> Self {
        let has_restart_policy = !matches!(launch.options.restart_policy, RestartPolicy::Never);
        let has_heartbeat =
            has_restart_policy && launch.options.health_check.heartbeat_interval > Duration::ZERO;
        let capability_count = launch.capabilities.len();
        let action = helper_process_action(launch.profile, capability_count, has_restart_policy);

        Self {
            profile: launch.profile,
            class: launch.info.class,
            action,
            arg_count: launch.arg_count(),
            env_count: launch.env_count(),
            inherited_env_count: launch.inherited_env_count(),
            capability_count,
            has_working_dir: launch.has_working_dir(),
            has_restart_policy,
            has_heartbeat,
        }
    }

    /// Helper profile used by this launch.
    pub fn profile(&self) -> HelperProcessProfile {
        self.profile
    }

    /// Process class that will be spawned.
    pub fn class(&self) -> ProcessClass {
        self.class
    }

    /// Recommended next builder action.
    pub fn next_action(&self) -> HelperProcessPlanAction {
        self.action
    }

    /// Whether plugin-host manifests, permissions, IPC, and crash policy must be paired first.
    pub fn requires_plugin_host_contracts(&self) -> bool {
        self.action == HelperProcessPlanAction::ConfigurePluginHostContracts
    }

    /// Whether permission broker grants and a process context are required before spawn.
    pub fn requires_broker_and_context(&self) -> bool {
        self.capability_count > 0
            || self.profile == HelperProcessProfile::PluginHost
            || self.class == ProcessClass::Extension
    }

    /// Whether restart/heartbeat policy should be attached to a supervisor before spawn.
    pub fn requires_supervisor_policy(&self) -> bool {
        self.has_restart_policy || self.has_heartbeat
    }

    /// Whether the checked descriptor is ready for the native process supervisor.
    pub fn can_spawn_native_helper(&self) -> bool {
        self.action == HelperProcessPlanAction::SpawnNativeHelper
    }

    /// Number of command-line arguments in the checked descriptor.
    pub fn arg_count(&self) -> usize {
        self.arg_count
    }

    /// Number of explicit environment variables in the checked descriptor.
    pub fn env_count(&self) -> usize {
        self.env_count
    }

    /// Number of inherited environment keys in the checked descriptor.
    pub fn inherited_env_count(&self) -> usize {
        self.inherited_env_count
    }

    /// Number of declared capability labels in the checked descriptor.
    pub fn capability_count(&self) -> usize {
        self.capability_count
    }

    /// Whether the launch has a working directory.
    pub fn has_working_dir(&self) -> bool {
        self.has_working_dir
    }

    /// Whether the launch has an automatic restart policy.
    pub fn has_restart_policy(&self) -> bool {
        self.has_restart_policy
    }

    /// Whether the launch has heartbeat supervision.
    pub fn has_heartbeat(&self) -> bool {
        self.has_heartbeat
    }

    /// Content-safe summary for logs, setup screens, and generated agents.
    pub fn to_text(&self) -> String {
        format!(
            "helper process execution plan profile={} class={} next action {} args={} env={} inherited_env={} capabilities={} working_dir={} restart={} heartbeat={}",
            self.profile.key(),
            self.class.label(),
            self.action.to_text(),
            self.arg_count,
            self.env_count,
            self.inherited_env_count,
            self.capability_count,
            self.has_working_dir,
            self.has_restart_policy,
            self.has_heartbeat
        )
    }
}

fn helper_process_action(
    profile: HelperProcessProfile,
    capability_count: usize,
    has_restart_policy: bool,
) -> HelperProcessPlanAction {
    if profile == HelperProcessProfile::PluginHost {
        HelperProcessPlanAction::ConfigurePluginHostContracts
    } else if capability_count > 0 {
        HelperProcessPlanAction::InstallBrokerAndContext
    } else if has_restart_policy {
        HelperProcessPlanAction::ConfigureSupervisorPolicy
    } else {
        HelperProcessPlanAction::SpawnNativeHelper
    }
}

/// Builder for checked app-owned helper process launches.
#[derive(Debug, Clone, PartialEq)]
pub struct HelperProcessLaunchBuilder {
    info: ProcessInfo,
    options: ProcessSpawnOptions,
    environment_policy: ProcessEnvironmentPolicy,
    capabilities: Vec<String>,
    profile: HelperProcessProfile,
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
            profile: HelperProcessProfile::Generic,
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

    /// Create a media helper profile suitable for FFmpeg-style transcode work.
    pub fn ffmpeg_transcoder(id: ProcessId, executable: impl AsRef<Path>) -> Self {
        Self::media(id, "ffmpeg-transcoder", executable)
            .profile(HelperProcessProfile::FfmpegTranscoder)
            .capability("media:transcode")
            .restart_on_failure(2, Duration::from_millis(250))
            .heartbeat_interval(Duration::from_secs(1))
            .missed_heartbeats_before_unhealthy(3)
    }

    /// Create a utility helper profile for a language server.
    pub fn language_server(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> Self {
        Self::utility(id, name, executable)
            .profile(HelperProcessProfile::LanguageServer)
            .capability("language-server")
            .restart_on_failure(3, Duration::from_millis(500))
            .heartbeat_interval(Duration::from_secs(2))
            .missed_heartbeats_before_unhealthy(3)
    }

    /// Create an extension helper profile for a plugin host.
    pub fn plugin_host(
        id: ProcessId,
        name: impl Into<String>,
        executable: impl AsRef<Path>,
    ) -> Self {
        Self::extension(id, name, executable)
            .profile(HelperProcessProfile::PluginHost)
            .capability("plugin:host")
            .restart_on_failure(3, Duration::from_millis(500))
            .heartbeat_interval(Duration::from_secs(2))
            .missed_heartbeats_before_unhealthy(3)
    }

    /// Override the helper profile summary label.
    pub fn profile(mut self, profile: HelperProcessProfile) -> Self {
        self.profile = profile;
        self
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

    /// Return the helper profile configured on this builder.
    pub fn configured_profile(&self) -> HelperProcessProfile {
        self.profile
    }

    /// Number of command-line arguments configured on this builder.
    pub fn arg_count(&self) -> usize {
        self.info.args.len()
    }

    /// Number of explicit environment variables configured on this builder.
    pub fn env_count(&self) -> usize {
        self.explicit_environment.len()
    }

    /// Number of inherited parent-environment keys.
    pub fn inherited_env_count(&self) -> usize {
        self.environment_policy.inherited_keys().len()
    }

    /// Whether this builder has a working directory.
    pub fn has_working_dir(&self) -> bool {
        self.info.working_dir.is_some()
    }

    /// Content-safe builder summary before launch construction.
    pub fn to_text(&self) -> String {
        format!(
            "helper_process_launch_builder profile={} class={} args={} env={} inherited_env={} capabilities={} working_dir={} restart={} heartbeat={}",
            self.profile.key(),
            self.info.class.label(),
            self.arg_count(),
            self.env_count(),
            self.inherited_env_count(),
            self.capabilities.len(),
            self.has_working_dir(),
            restart_policy_key(&self.options.restart_policy),
            self.options.health_check.heartbeat_interval > Duration::ZERO
        )
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
            profile: self.profile,
        };
        launch.validate()?;
        Ok(launch)
    }
}

// ---------------------------------------------------------------------------
// Terminal Session Descriptor
// ---------------------------------------------------------------------------

/// Checked descriptor for an app-owned terminal or shell session.
///
/// This is the contract layer for IDE-like apps before a platform PTY backend
/// is attached. It validates shell path, arguments, working directory,
/// environment policy, and terminal dimensions without treating user input as a
/// shell string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSessionRequest {
    id: ProcessId,
    name: String,
    shell: PathBuf,
    args: Vec<String>,
    env: HashMap<String, String>,
    inherited_env_keys: Vec<String>,
    working_dir: Option<PathBuf>,
    columns: u16,
    rows: u16,
    scrollback_lines: Option<usize>,
    login_shell: bool,
}

impl TerminalSessionRequest {
    /// Start building a checked terminal session request.
    pub fn builder(
        id: ProcessId,
        name: impl Into<String>,
        shell: impl AsRef<Path>,
    ) -> TerminalSessionRequestBuilder {
        TerminalSessionRequestBuilder::new(id, name, shell)
    }

    /// Process id the terminal backend should use for supervision.
    pub fn id(&self) -> ProcessId {
        self.id
    }

    /// Human-readable terminal session name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Shell executable path.
    pub fn shell(&self) -> &Path {
        &self.shell
    }

    /// Shell arguments in argv form.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Explicit environment variables.
    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Parent environment keys that may be inherited.
    pub fn inherited_env_keys(&self) -> &[String] {
        &self.inherited_env_keys
    }

    /// Working directory for the shell.
    pub fn working_dir(&self) -> Option<&Path> {
        self.working_dir.as_deref()
    }

    /// Initial terminal size as `(columns, rows)`.
    pub fn size(&self) -> (u16, u16) {
        (self.columns, self.rows)
    }

    /// Optional scrollback line budget.
    pub fn scrollback_lines(&self) -> Option<usize> {
        self.scrollback_lines
    }

    /// Whether this terminal should launch the shell as a login shell.
    pub fn is_login_shell(&self) -> bool {
        self.login_shell
    }

    /// Number of shell arguments.
    pub fn arg_count(&self) -> usize {
        self.args.len()
    }

    /// Number of explicit environment variables.
    pub fn env_count(&self) -> usize {
        self.env.len()
    }

    /// Number of inherited parent-environment keys.
    pub fn inherited_env_count(&self) -> usize {
        self.inherited_env_keys.len()
    }

    /// Whether a working directory was configured.
    pub fn has_working_dir(&self) -> bool {
        self.working_dir.is_some()
    }

    /// Whether a scrollback budget was configured.
    pub fn has_scrollback_limit(&self) -> bool {
        self.scrollback_lines.is_some()
    }

    /// Validate this descriptor before handing it to a PTY backend.
    pub fn validate(&self) -> Result<()> {
        validate_terminal_session_name(&self.name)?;
        validate_executable_path(&self.shell, false)?;
        validate_process_args(&self.args)?;
        validate_process_env(&self.env)?;
        let policy = ProcessEnvironmentPolicy::InheritAllowlist(self.inherited_env_keys.clone());
        policy.validate()?;
        if let Some(working_dir) = &self.working_dir {
            validate_working_dir(working_dir, false)?;
        }
        validate_terminal_size(self.columns, self.rows)?;
        validate_terminal_scrollback(self.scrollback_lines)?;
        Ok(())
    }

    /// Content-safe summary for terminal panes, launch plans, and agents.
    pub fn to_text(&self) -> String {
        format!(
            "terminal session: args {}, env {}, inherited-env {}, working-dir {}, size {}x{}, scrollback {}, login-shell {}",
            self.arg_count(),
            self.env_count(),
            self.inherited_env_count(),
            self.has_working_dir(),
            self.columns,
            self.rows,
            self.has_scrollback_limit(),
            self.login_shell
        )
    }

    /// Convert the shell process shape into a checked process descriptor builder.
    pub fn process_info_builder(&self) -> ProcessInfoBuilder {
        let mut builder = ProcessInfoBuilder::utility(self.id, self.name.clone())
            .executable(self.shell.clone())
            .args(self.args.clone());
        for (key, value) in &self.env {
            builder = builder.env(key.clone(), value.clone());
        }
        if let Some(working_dir) = &self.working_dir {
            builder = builder.working_dir(working_dir);
        }
        builder
    }
}

/// Builder for checked terminal session requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSessionRequestBuilder {
    request: TerminalSessionRequest,
    require_existing_shell: bool,
    canonicalize_shell: bool,
    require_existing_working_dir: bool,
    canonicalize_working_dir: bool,
}

impl TerminalSessionRequestBuilder {
    /// Create a terminal session builder.
    pub fn new(id: ProcessId, name: impl Into<String>, shell: impl AsRef<Path>) -> Self {
        Self {
            request: TerminalSessionRequest {
                id,
                name: name.into(),
                shell: shell.as_ref().to_path_buf(),
                args: Vec::new(),
                env: HashMap::new(),
                inherited_env_keys: Vec::new(),
                working_dir: None,
                columns: 80,
                rows: 24,
                scrollback_lines: Some(10_000),
                login_shell: false,
            },
            require_existing_shell: false,
            canonicalize_shell: false,
            require_existing_working_dir: false,
            canonicalize_working_dir: false,
        }
    }

    /// Require the shell executable to exist.
    pub fn require_existing_shell(mut self) -> Self {
        self.require_existing_shell = true;
        self
    }

    /// Canonicalize the shell executable path during checked build.
    pub fn canonicalize_shell(mut self) -> Self {
        self.canonicalize_shell = true;
        self.require_existing_shell = true;
        self
    }

    /// Add one shell argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.request.args.push(arg.into());
        self
    }

    /// Add multiple shell arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.request.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Add one explicit environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.request.env.insert(key.into(), value.into());
        self
    }

    /// Allow selected parent environment keys to be inherited.
    pub fn inherit_environment_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.request.inherited_env_keys = keys.into_iter().map(Into::into).collect();
        self
    }

    /// Set the terminal working directory.
    pub fn working_dir(mut self, path: impl AsRef<Path>) -> Self {
        self.request.working_dir = Some(path.as_ref().to_path_buf());
        self
    }

    /// Require the terminal working directory to exist.
    pub fn require_existing_working_dir(mut self) -> Self {
        self.require_existing_working_dir = true;
        self
    }

    /// Canonicalize the terminal working directory during checked build.
    pub fn canonicalize_working_dir(mut self) -> Self {
        self.canonicalize_working_dir = true;
        self.require_existing_working_dir = true;
        self
    }

    /// Set initial terminal dimensions.
    pub fn size(mut self, columns: u16, rows: u16) -> Self {
        self.request.columns = columns;
        self.request.rows = rows;
        self
    }

    /// Set the scrollback line budget.
    pub fn scrollback_lines(mut self, lines: usize) -> Self {
        self.request.scrollback_lines = Some(lines);
        self
    }

    /// Disable scrollback retention for privacy-sensitive sessions.
    pub fn without_scrollback(mut self) -> Self {
        self.request.scrollback_lines = None;
        self
    }

    /// Launch the shell as a login shell.
    pub fn login_shell(mut self) -> Self {
        self.request.login_shell = true;
        self
    }

    /// Number of shell arguments.
    pub fn arg_count(&self) -> usize {
        self.request.arg_count()
    }

    /// Number of explicit environment variables.
    pub fn env_count(&self) -> usize {
        self.request.env_count()
    }

    /// Number of inherited environment keys.
    pub fn inherited_env_count(&self) -> usize {
        self.request.inherited_env_count()
    }

    /// Whether a working directory was configured.
    pub fn has_working_dir(&self) -> bool {
        self.request.has_working_dir()
    }

    /// Whether a scrollback budget was configured.
    pub fn has_scrollback_limit(&self) -> bool {
        self.request.has_scrollback_limit()
    }

    /// Content-safe builder summary before PTY/backend dispatch.
    pub fn to_text(&self) -> String {
        format!(
            "terminal session builder: args {}, env {}, inherited-env {}, working-dir {}, size {}x{}, scrollback {}, login-shell {}",
            self.arg_count(),
            self.env_count(),
            self.inherited_env_count(),
            self.has_working_dir(),
            self.request.columns,
            self.request.rows,
            self.has_scrollback_limit(),
            self.request.login_shell
        )
    }

    /// Validate the configured terminal request.
    pub fn validate(&self) -> Result<()> {
        validate_terminal_session_name(&self.request.name)?;
        validate_executable_path(&self.request.shell, self.require_existing_shell)?;
        validate_process_args(&self.request.args)?;
        validate_process_env(&self.request.env)?;
        let policy =
            ProcessEnvironmentPolicy::InheritAllowlist(self.request.inherited_env_keys.clone());
        policy.validate()?;
        if let Some(working_dir) = &self.request.working_dir {
            validate_working_dir(working_dir, self.require_existing_working_dir)?;
        } else {
            anyhow::ensure!(
                !self.require_existing_working_dir && !self.canonicalize_working_dir,
                "terminal working directory is required"
            );
        }
        validate_terminal_size(self.request.columns, self.request.rows)?;
        validate_terminal_scrollback(self.request.scrollback_lines)?;
        Ok(())
    }

    /// Build a checked terminal session request.
    pub fn build_checked(mut self) -> Result<TerminalSessionRequest> {
        self.validate()?;
        if self.canonicalize_shell {
            self.request.shell = self.request.shell.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize terminal shell {}",
                    self.request.shell.display()
                )
            })?;
        }
        if self.canonicalize_working_dir
            && let Some(working_dir) = self.request.working_dir.take()
        {
            self.request.working_dir = Some(working_dir.canonicalize().with_context(|| {
                format!(
                    "could not canonicalize terminal working directory {}",
                    working_dir.display()
                )
            })?);
        }
        self.request.validate()?;
        Ok(self.request)
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

impl<Request, Response, Progress, Error> IpcMessage<Request, Response, Progress, Error> {
    /// Stable IPC message kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Request { .. } => "request",
            Self::Response { .. } => "response",
            Self::Progress { .. } => "progress",
            Self::Cancel { .. } => "cancel",
        }
    }

    /// Correlation identifier attached to the message.
    pub fn correlation_id(&self) -> u64 {
        match self {
            Self::Request { id, .. }
            | Self::Response { id, .. }
            | Self::Progress { id, .. }
            | Self::Cancel { id } => *id,
        }
    }

    /// Returns true when the message is a successful response.
    pub fn is_success_response(&self) -> bool {
        matches!(self, Self::Response { result: Ok(_), .. })
    }

    /// Returns true when the message is an error response.
    pub fn is_error_response(&self) -> bool {
        matches!(self, Self::Response { result: Err(_), .. })
    }

    /// Content-safe IPC message summary that never formats the payload.
    pub fn to_text(&self) -> String {
        format!(
            "ipc_message(kind={}, correlation_id={}, success_response={}, error_response={})",
            self.kind(),
            self.correlation_id(),
            self.is_success_response(),
            self.is_error_response()
        )
    }
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
                    (1..=1_000).contains(max_restarts),
                    "restart policy max_restarts must be between 1 and 1000"
                );
                validate_backoff(*backoff)
            }
            RestartPolicy::Always { backoff } => validate_backoff(*backoff),
        }
    }
}

fn restart_policy_key(policy: &RestartPolicy) -> &'static str {
    match policy {
        RestartPolicy::Never => "never",
        RestartPolicy::OnFailure { .. } => "on-failure",
        RestartPolicy::Always { .. } => "always",
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
            self.heartbeat_interval > Duration::ZERO
                && self.heartbeat_interval <= Duration::from_secs(86_400),
            "heartbeat interval must be between 1ns and 1 day"
        );
        anyhow::ensure!(
            (1..=10_000).contains(&self.missed_heartbeats_before_unhealthy),
            "missed heartbeat threshold must be between 1 and 10000"
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

/// Version shared by native process workers and browser Web Workers.
///
/// A peer must reject a handshake carrying any other version. Keeping this
/// constant next to the typed worker protocol prevents browser and native
/// transports from silently drifting apart.
pub const WORKER_PROTOCOL_VERSION: u32 = 1;

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

impl WorkerRequest {
    /// Stable worker request kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Execute { .. } => "execute",
            Self::Ping => "ping",
        }
    }

    /// Returns true when this request carries a JSON payload.
    pub fn has_payload(&self) -> bool {
        matches!(self, Self::Execute { .. })
    }

    /// Coarse JSON payload kind, or `none`.
    pub fn payload_kind(&self) -> &'static str {
        match self {
            Self::Execute { payload } => json_value_kind(payload),
            Self::Ping => "none",
        }
    }

    /// Number of top-level JSON items for arrays/objects, without exposing keys or values.
    pub fn payload_item_count(&self) -> usize {
        match self {
            Self::Execute { payload } => json_value_item_count(payload),
            Self::Ping => 0,
        }
    }

    /// Content-safe worker request summary.
    pub fn to_text(&self) -> String {
        format!(
            "worker_request(kind={}, has_payload={}, payload_kind={}, payload_items={})",
            self.kind(),
            self.has_payload(),
            self.payload_kind(),
            self.payload_item_count()
        )
    }
}

/// A response sent from worker to host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerResponse {
    /// The result of a task execution.
    Result(serde_json::Value),
    /// Pong response to a ping.
    Pong,
}

impl WorkerResponse {
    /// Stable worker response kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Result(_) => "result",
            Self::Pong => "pong",
        }
    }

    /// Returns true when this response carries a JSON payload.
    pub fn has_payload(&self) -> bool {
        matches!(self, Self::Result(_))
    }

    /// Coarse JSON payload kind, or `none`.
    pub fn payload_kind(&self) -> &'static str {
        match self {
            Self::Result(payload) => json_value_kind(payload),
            Self::Pong => "none",
        }
    }

    /// Number of top-level JSON items for arrays/objects, without exposing keys or values.
    pub fn payload_item_count(&self) -> usize {
        match self {
            Self::Result(payload) => json_value_item_count(payload),
            Self::Pong => 0,
        }
    }

    /// Content-safe worker response summary.
    pub fn to_text(&self) -> String {
        format!(
            "worker_response(kind={}, has_payload={}, payload_kind={}, payload_items={})",
            self.kind(),
            self.has_payload(),
            self.payload_kind(),
            self.payload_item_count()
        )
    }
}

/// A progress update sent from worker to host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerProgress {
    /// A progress update with a JSON payload.
    Update(serde_json::Value),
}

impl WorkerProgress {
    /// Coarse JSON payload kind.
    pub fn payload_kind(&self) -> &'static str {
        match self {
            Self::Update(payload) => json_value_kind(payload),
        }
    }

    /// Number of top-level JSON items for arrays/objects, without exposing keys or values.
    pub fn payload_item_count(&self) -> usize {
        match self {
            Self::Update(payload) => json_value_item_count(payload),
        }
    }

    /// Content-safe worker progress summary.
    pub fn to_text(&self) -> String {
        format!(
            "worker_progress(kind=update, payload_kind={}, payload_items={})",
            self.payload_kind(),
            self.payload_item_count()
        )
    }
}

/// An error returned by a worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerError {
    /// The task failed with the given message.
    Execution(String),
    /// The request was cancelled.
    Cancelled,
}

impl WorkerError {
    /// Stable worker error kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Execution(_) => "execution",
            Self::Cancelled => "cancelled",
        }
    }

    /// Returns true when the error carries a message.
    pub fn has_message(&self) -> bool {
        matches!(self, Self::Execution(_))
    }

    /// Byte length of the error message without exposing it.
    pub fn message_len_bytes(&self) -> usize {
        match self {
            Self::Execution(message) => message.len(),
            Self::Cancelled => 0,
        }
    }

    /// Content-safe worker error summary.
    pub fn to_text(&self) -> String {
        format!(
            "worker_error(kind={}, has_message={}, message_len_bytes={})",
            self.kind(),
            self.has_message(),
            self.message_len_bytes()
        )
    }
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

impl BootstrapMessage {
    /// Validate bootstrap fields received across the process boundary.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Handshake {
                version,
                capabilities,
            } => {
                anyhow::ensure!(*version > 0, "bootstrap protocol version must be non-zero");
                validate_capability_labels(capabilities)
            }
            Self::HandshakeAck {
                heartbeat_interval_secs,
                granted_capabilities,
            } => {
                anyhow::ensure!(
                    (1..=86_400).contains(heartbeat_interval_secs),
                    "bootstrap heartbeat interval must be between 1 second and 1 day"
                );
                validate_capability_labels(granted_capabilities)
            }
            Self::Heartbeat => Ok(()),
        }
    }

    /// Stable bootstrap message kind key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Handshake { .. } => "handshake",
            Self::HandshakeAck { .. } => "handshake-ack",
            Self::Heartbeat => "heartbeat",
        }
    }

    /// Protocol version if present.
    pub fn version(&self) -> Option<u32> {
        match self {
            Self::Handshake { version, .. } => Some(*version),
            Self::HandshakeAck { .. } | Self::Heartbeat => None,
        }
    }

    /// Number of requested or granted capabilities.
    pub fn capability_count(&self) -> usize {
        match self {
            Self::Handshake { capabilities, .. } => capabilities.len(),
            Self::HandshakeAck {
                granted_capabilities,
                ..
            } => granted_capabilities.len(),
            Self::Heartbeat => 0,
        }
    }

    /// Returns true when a heartbeat interval is present.
    pub fn has_heartbeat_interval(&self) -> bool {
        matches!(self, Self::HandshakeAck { .. })
    }

    /// Content-safe bootstrap message summary.
    pub fn to_text(&self) -> String {
        format!(
            "bootstrap_message(kind={}, version={}, capabilities={}, has_heartbeat_interval={})",
            self.kind(),
            self.version().unwrap_or(0),
            self.capability_count(),
            self.has_heartbeat_interval()
        )
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn json_value_item_count(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => items.len(),
        serde_json::Value::Object(items) => items.len(),
        _ => 0,
    }
}

fn validate_process_name(name: &str) -> Result<()> {
    validate_non_empty_trimmed(name, "process name")?;
    anyhow::ensure!(
        !name.chars().any(|ch| ch == '/' || ch == '\\'),
        "process name cannot contain path separators"
    );
    Ok(())
}

fn validate_terminal_session_name(name: &str) -> Result<()> {
    validate_non_empty_trimmed(name, "terminal session name")?;
    anyhow::ensure!(
        name.len() <= 128,
        "terminal session name cannot exceed 128 bytes"
    );
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "terminal session name cannot contain control characters"
    );
    Ok(())
}

fn validate_terminal_size(columns: u16, rows: u16) -> Result<()> {
    anyhow::ensure!(
        (1..=1000).contains(&columns),
        "terminal columns must be between 1 and 1000"
    );
    anyhow::ensure!(
        (1..=300).contains(&rows),
        "terminal rows must be between 1 and 300"
    );
    Ok(())
}

fn validate_terminal_scrollback(scrollback_lines: Option<usize>) -> Result<()> {
    if let Some(lines) = scrollback_lines {
        anyhow::ensure!(
            lines > 0,
            "terminal scrollback lines must be greater than zero"
        );
        anyhow::ensure!(
            lines <= 1_000_000,
            "terminal scrollback lines cannot exceed 1000000"
        );
    }
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
    anyhow::ensure!(
        args.len() <= 4_096,
        "process cannot have more than 4096 arguments"
    );
    for arg in args {
        validate_no_nul(arg, "process argument")?;
        anyhow::ensure!(arg.len() <= 1024 * 1024, "process argument is too large");
    }
    Ok(())
}

fn validate_process_env(env: &HashMap<String, String>) -> Result<()> {
    anyhow::ensure!(
        env.len() <= 4_096,
        "process environment cannot have more than 4096 entries"
    );
    for (key, value) in env {
        validate_environment_key(key, "process environment key")?;
        validate_no_nul(value, "process environment value")?;
        anyhow::ensure!(
            value.len() <= 1024 * 1024,
            "process environment value is too large"
        );
    }
    Ok(())
}

fn validate_explicit_environment_entries(env: &[(String, String)]) -> Result<()> {
    anyhow::ensure!(
        env.len() <= 4_096,
        "process environment cannot have more than 4096 entries"
    );
    let mut seen = HashSet::new();
    for (key, value) in env {
        validate_environment_key(key, "process environment key")?;
        anyhow::ensure!(
            seen.insert(key),
            "process environment key configured more than once: {key}"
        );
        validate_no_nul(value, "process environment value")?;
        anyhow::ensure!(
            value.len() <= 1024 * 1024,
            "process environment value is too large"
        );
    }
    Ok(())
}

fn validate_environment_key(key: &str, label: &str) -> Result<()> {
    validate_non_empty_trimmed(key, label)?;
    anyhow::ensure!(!key.contains('='), "{label} cannot contain '='");
    anyhow::ensure!(key.len() <= 255, "{label} cannot exceed 255 bytes");
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
    anyhow::ensure!(
        capabilities.len() <= 1_024,
        "process cannot declare more than 1024 capabilities"
    );
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
        backoff > Duration::ZERO && backoff <= Duration::from_secs(86_400),
        "restart backoff must be between 1ns and 1 day"
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
    fn helper_process_profile_builders_are_content_safe() {
        let executable = std::env::current_exe().unwrap();
        let working_dir = executable.parent().unwrap();
        let builder = HelperProcessLaunch::ffmpeg_transcoder(ProcessId(9), &executable)
            .require_existing_executable()
            .working_dir(working_dir)
            .require_existing_working_dir()
            .arg("--input")
            .arg("/Users/alice/private/movie.mov")
            .env("SECRET_TOKEN", "sensitive")
            .inherit_environment_keys(["PATH"]);

        assert_eq!(
            builder.configured_profile(),
            HelperProcessProfile::FfmpegTranscoder
        );
        assert_eq!(builder.arg_count(), 2);
        assert_eq!(builder.env_count(), 1);
        assert_eq!(builder.inherited_env_count(), 1);
        assert!(builder.has_working_dir());

        let builder_summary = builder.to_text();
        assert!(builder_summary.contains("profile=ffmpeg-transcoder"));
        assert!(builder_summary.contains("class=media"));
        assert!(builder_summary.contains("args=2"));
        assert!(builder_summary.contains("env=1"));
        assert!(builder_summary.contains("inherited_env=1"));
        assert!(builder_summary.contains("capabilities=1"));
        assert!(builder_summary.contains("restart=on-failure"));
        assert!(!builder_summary.contains("movie.mov"));
        assert!(!builder_summary.contains("SECRET_TOKEN"));
        assert!(!builder_summary.contains("sensitive"));
        assert!(!builder_summary.contains(&executable.display().to_string()));

        let launch = builder.build_checked().unwrap();
        assert_eq!(launch.profile(), HelperProcessProfile::FfmpegTranscoder);
        assert_eq!(launch.info().class, ProcessClass::Media);
        assert_eq!(launch.arg_count(), 2);
        assert_eq!(launch.env_count(), 1);
        assert_eq!(launch.inherited_env_count(), 1);
        assert!(launch.has_working_dir());
        assert_eq!(launch.capabilities(), &["media:transcode".to_string()]);
        assert_eq!(
            launch.options().restart_policy,
            RestartPolicy::OnFailure {
                max_restarts: 2,
                backoff: Duration::from_millis(250),
            }
        );

        let launch_summary = launch.to_text();
        assert!(launch_summary.contains("helper_process_launch"));
        assert!(launch_summary.contains("profile=ffmpeg-transcoder"));
        assert!(launch_summary.contains("class=media"));
        assert!(!launch_summary.contains("movie.mov"));
        assert!(!launch_summary.contains("SECRET_TOKEN"));
        assert!(!launch_summary.contains("sensitive"));
        assert!(!launch_summary.contains(&executable.display().to_string()));
    }

    #[test]
    fn helper_process_language_server_and_plugin_host_profiles() {
        let executable = std::env::current_exe().unwrap();
        let language_server =
            HelperProcessLaunch::language_server(ProcessId(10), "rust-analyzer", &executable)
                .arg("--stdio")
                .build_checked()
                .unwrap();
        let plugin_host =
            HelperProcessLaunch::plugin_host(ProcessId(11), "extension-host", &executable)
                .build_checked()
                .unwrap();

        assert_eq!(
            language_server.profile(),
            HelperProcessProfile::LanguageServer
        );
        assert_eq!(language_server.info().class, ProcessClass::Utility);
        assert_eq!(
            language_server.capabilities(),
            &["language-server".to_string()]
        );
        assert!(
            language_server
                .to_text()
                .contains("profile=language-server")
        );
        assert!(!language_server.to_text().contains("rust-analyzer"));

        assert_eq!(plugin_host.profile(), HelperProcessProfile::PluginHost);
        assert_eq!(plugin_host.info().class, ProcessClass::Extension);
        assert_eq!(plugin_host.capabilities(), &["plugin:host".to_string()]);
        assert!(plugin_host.to_text().contains("profile=plugin-host"));
        assert!(!plugin_host.to_text().contains("extension-host"));
    }

    #[test]
    fn helper_process_execution_plan_guides_builder_next_actions() {
        let executable = std::env::current_exe().unwrap();

        let plugin_host =
            HelperProcessLaunch::plugin_host(ProcessId(11), "private-extension-host", &executable)
                .arg("--plugin")
                .arg("secret.plugin")
                .build_checked()
                .unwrap();
        let plugin_plan = plugin_host.execution_plan();
        assert_eq!(plugin_plan.profile(), HelperProcessProfile::PluginHost);
        assert_eq!(plugin_plan.class(), ProcessClass::Extension);
        assert_eq!(
            plugin_plan.next_action(),
            HelperProcessPlanAction::ConfigurePluginHostContracts
        );
        assert!(plugin_plan.requires_plugin_host_contracts());
        assert!(plugin_plan.requires_broker_and_context());
        assert!(plugin_plan.requires_supervisor_policy());
        assert_eq!(plugin_plan.arg_count(), 2);
        assert_eq!(plugin_plan.capability_count(), 1);
        let plugin_text = plugin_plan.to_text();
        assert!(plugin_text.contains("helper process execution plan"));
        assert!(plugin_text.contains("next action configure plugin host contracts"));
        assert!(!plugin_text.contains("private-extension-host"));
        assert!(!plugin_text.contains("secret.plugin"));
        assert!(!plugin_text.contains(&executable.display().to_string()));

        let brokered = HelperProcessLaunch::utility(ProcessId(12), "asset-worker", &executable)
            .capability("network:read")
            .build_checked()
            .unwrap();
        let brokered_plan = brokered.execution_plan();
        assert_eq!(
            brokered_plan.next_action(),
            HelperProcessPlanAction::InstallBrokerAndContext
        );
        assert!(brokered_plan.requires_broker_and_context());
        assert!(!brokered_plan.requires_plugin_host_contracts());

        let supervised = HelperProcessLaunch::worker(ProcessId(13), "indexer", &executable)
            .restart_on_failure(2, Duration::from_millis(100))
            .heartbeat_interval(Duration::from_secs(1))
            .build_checked()
            .unwrap();
        let supervised_plan = supervised.execution_plan();
        assert_eq!(
            supervised_plan.next_action(),
            HelperProcessPlanAction::ConfigureSupervisorPolicy
        );
        assert!(supervised_plan.requires_supervisor_policy());
        assert!(!supervised_plan.requires_broker_and_context());
        assert!(supervised_plan.has_restart_policy());
        assert!(supervised_plan.has_heartbeat());

        let bare = HelperProcessLaunch::utility(ProcessId(14), "thumbnailer", &executable)
            .build_checked()
            .unwrap();
        let bare_plan = bare.execution_plan();
        assert_eq!(
            bare_plan.next_action(),
            HelperProcessPlanAction::SpawnNativeHelper
        );
        assert!(bare_plan.can_spawn_native_helper());
        assert!(!bare_plan.requires_supervisor_policy());
        assert_eq!(
            HelperProcessPlanAction::SpawnNativeHelper.to_text(),
            "spawn native helper"
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
    fn terminal_session_request_builder_validates_shell_sessions() {
        let shell = std::env::current_exe().unwrap();
        let working_dir = shell.parent().unwrap();
        let request = TerminalSessionRequest::builder(ProcessId(12), "workspace shell", &shell)
            .require_existing_shell()
            .working_dir(working_dir)
            .require_existing_working_dir()
            .arg("-lc")
            .arg("echo private-token")
            .env("TERM", "xterm-256color")
            .inherit_environment_keys(["PATH", "HOME"])
            .size(120, 32)
            .scrollback_lines(20_000)
            .login_shell()
            .build_checked()
            .unwrap();

        assert_eq!(request.id(), ProcessId(12));
        assert_eq!(request.name(), "workspace shell");
        assert_eq!(request.shell(), shell.as_path());
        assert_eq!(request.args().len(), 2);
        assert_eq!(
            request.env().get("TERM"),
            Some(&"xterm-256color".to_string())
        );
        assert_eq!(
            request.inherited_env_keys(),
            &["PATH".to_string(), "HOME".to_string()]
        );
        assert_eq!(request.working_dir(), Some(working_dir));
        assert_eq!(request.size(), (120, 32));
        assert_eq!(request.scrollback_lines(), Some(20_000));
        assert!(request.is_login_shell());

        let info = request.process_info_builder().build_checked().unwrap();
        assert_eq!(info.id, ProcessId(12));
        assert_eq!(info.class, ProcessClass::Utility);
        assert_eq!(info.executable, shell);
        assert_eq!(info.args.len(), 2);
        assert_eq!(info.working_dir, Some(working_dir.to_path_buf()));
    }

    #[test]
    fn terminal_session_request_summary_is_content_safe() {
        let shell = std::env::current_exe().unwrap();
        let builder = TerminalSessionRequest::builder(ProcessId(13), "Private Project", &shell)
            .arg("-lc")
            .arg("cat /Users/alice/secrets.txt")
            .env("SECRET_TOKEN", "sensitive")
            .inherit_environment_keys(["PATH"])
            .working_dir("/Users/alice/private-project")
            .size(100, 30)
            .without_scrollback()
            .login_shell();

        let builder_summary = builder.to_text();
        assert_eq!(
            builder_summary,
            "terminal session builder: args 2, env 1, inherited-env 1, working-dir true, size 100x30, scrollback false, login-shell true"
        );
        assert!(!builder_summary.contains("Private Project"));
        assert!(!builder_summary.contains("secrets"));
        assert!(!builder_summary.contains("SECRET_TOKEN"));
        assert!(!builder_summary.contains("sensitive"));
        assert!(!builder_summary.contains("/Users/alice"));
        assert!(!builder_summary.contains(&shell.display().to_string()));

        let request = builder.build_checked().unwrap();
        let request_summary = request.to_text();
        assert_eq!(
            request_summary,
            "terminal session: args 2, env 1, inherited-env 1, working-dir true, size 100x30, scrollback false, login-shell true"
        );
        assert!(!request_summary.contains("Private Project"));
        assert!(!request_summary.contains("secrets"));
        assert!(!request_summary.contains("SECRET_TOKEN"));
        assert!(!request_summary.contains("sensitive"));
        assert!(!request_summary.contains("/Users/alice"));
        assert!(!request_summary.contains(&shell.display().to_string()));
    }

    #[test]
    fn terminal_session_request_rejects_unsafe_inputs() {
        let shell = std::env::current_exe().unwrap();

        assert!(
            TerminalSessionRequest::builder(ProcessId(14), " bad ", &shell)
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", "")
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", "/missing/shell")
                .require_existing_shell()
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .arg("bad\0arg")
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .env("BAD=KEY", "value")
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .inherit_environment_keys(["PATH", "PATH"])
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .working_dir("/definitely/not/a/dir")
                .require_existing_working_dir()
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .size(0, 24)
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .size(80, 301)
                .validate()
                .is_err()
        );
        assert!(
            TerminalSessionRequest::builder(ProcessId(14), "shell", &shell)
                .scrollback_lines(0)
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
    fn ipc_and_worker_message_summary_is_content_safe() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct PrivateRequest {
            secret: String,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct PrivateResponse {
            secret_result: String,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct PrivateProgress {
            secret_stage: String,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct PrivateError {
            secret_error: String,
        }

        let request =
            IpcMessage::<PrivateRequest, PrivateResponse, PrivateProgress, PrivateError>::Request {
                id: 7,
                body: PrivateRequest {
                    secret: "customer-token".to_string(),
                },
            };
        let response = IpcMessage::<PrivateRequest, PrivateResponse, PrivateProgress, PrivateError>::Response {
            id: 7,
            result: Err(PrivateError {
                secret_error: "private failure".to_string(),
            }),
        };
        let progress = IpcMessage::<PrivateRequest, PrivateResponse, PrivateProgress, PrivateError>::Progress {
            id: 7,
            body: PrivateProgress {
                secret_stage: "private stage".to_string(),
            },
        };
        let cancel =
            IpcMessage::<PrivateRequest, PrivateResponse, PrivateProgress, PrivateError>::Cancel {
                id: 7,
            };

        assert_eq!(request.kind(), "request");
        assert_eq!(request.correlation_id(), 7);
        assert!(!request.is_success_response());
        assert!(!request.is_error_response());
        assert!(response.is_error_response());
        assert_eq!(progress.kind(), "progress");
        assert_eq!(cancel.kind(), "cancel");

        for summary in [
            request.to_text(),
            response.to_text(),
            progress.to_text(),
            cancel.to_text(),
        ] {
            assert!(summary.contains("correlation_id=7"));
            assert!(!summary.contains("customer-token"));
            assert!(!summary.contains("private failure"));
            assert!(!summary.contains("private stage"));
        }

        let worker_request = WorkerRequest::Execute {
            payload: serde_json::json!({
                "secret": "customer-token",
                "items": [1, 2, 3]
            }),
        };
        let worker_response =
            WorkerResponse::Result(serde_json::json!(["private-result", "another-secret"]));
        let worker_progress = WorkerProgress::Update(serde_json::json!({
            "stage": "secret-progress",
            "percent": 50
        }));
        let worker_error = WorkerError::Execution("private worker failed".to_string());

        assert_eq!(worker_request.payload_kind(), "object");
        assert_eq!(worker_request.payload_item_count(), 2);
        assert_eq!(worker_response.payload_kind(), "array");
        assert_eq!(worker_response.payload_item_count(), 2);
        assert_eq!(worker_progress.payload_kind(), "object");
        assert_eq!(worker_progress.payload_item_count(), 2);
        assert_eq!(worker_error.kind(), "execution");
        assert_eq!(
            worker_error.message_len_bytes(),
            "private worker failed".len()
        );

        for summary in [
            worker_request.to_text(),
            worker_response.to_text(),
            worker_progress.to_text(),
            worker_error.to_text(),
        ] {
            assert!(!summary.contains("customer-token"));
            assert!(!summary.contains("private-result"));
            assert!(!summary.contains("secret-progress"));
            assert!(!summary.contains("private worker failed"));
            assert!(!summary.contains("secret"));
        }
    }

    #[test]
    fn bootstrap_message_summary_is_content_safe() {
        let handshake = BootstrapMessage::Handshake {
            version: 3,
            capabilities: vec![
                "private:filesystem".to_string(),
                "secret:network".to_string(),
            ],
        };
        let ack = BootstrapMessage::HandshakeAck {
            heartbeat_interval_secs: 15,
            granted_capabilities: vec!["private:filesystem".to_string()],
        };
        let heartbeat = BootstrapMessage::Heartbeat;

        assert_eq!(handshake.kind(), "handshake");
        assert_eq!(handshake.version(), Some(3));
        assert_eq!(handshake.capability_count(), 2);
        assert!(!handshake.has_heartbeat_interval());
        assert_eq!(ack.kind(), "handshake-ack");
        assert_eq!(ack.capability_count(), 1);
        assert!(ack.has_heartbeat_interval());
        assert_eq!(heartbeat.kind(), "heartbeat");
        assert!(handshake.validate().is_ok());
        assert!(ack.validate().is_ok());
        assert!(heartbeat.validate().is_ok());

        assert!(
            BootstrapMessage::Handshake {
                version: 0,
                capabilities: Vec::new(),
            }
            .validate()
            .is_err()
        );
        assert!(
            BootstrapMessage::HandshakeAck {
                heartbeat_interval_secs: 0,
                granted_capabilities: Vec::new(),
            }
            .validate()
            .is_err()
        );
        assert!(
            BootstrapMessage::HandshakeAck {
                heartbeat_interval_secs: 5,
                granted_capabilities: vec!["network access".to_string()],
            }
            .validate()
            .is_err()
        );

        for summary in [handshake.to_text(), ack.to_text(), heartbeat.to_text()] {
            assert!(!summary.contains("private:filesystem"));
            assert!(!summary.contains("secret:network"));
            assert!(!summary.contains("15"));
        }
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
