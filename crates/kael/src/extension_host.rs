//! Extension host runtime managing the full lifecycle of extensions.
#![allow(missing_docs)]

use std::{
    collections::{HashMap, VecDeque},
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow};

use crate::{
    extension_rpc::{
        EXTENSION_RPC_VERSION, ExtensionHandshake, ExtensionMessage, ExtensionNotification,
        ExtensionRequest, ExtensionResponse, ExtensionTransport,
    },
    ipc_transport::{Transport, decode_exact_frame, encode_frame},
    plugin::{
        ExecutionModel, ExtensionHost, ExtensionInfo, HOST_API_VERSION, PluginManifest,
        is_api_compatible,
    },
    process_model::{ProcessId, ProcessInfo, RestartPolicy},
    security::{PermissionBroker, PermissionResult},
    supervisor::ProcessSupervisor,
};

#[cfg(unix)]
use crate::ipc_transport::try_ipc_socket_path;

#[cfg(unix)]
const EXTENSION_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(unix)]
struct ExtensionSocketGuard(PathBuf);

#[cfg(unix)]
impl Drop for ExtensionSocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn validate_extension_app_id(app_id: &str) -> Result<()> {
    anyhow::ensure!(!app_id.is_empty(), "extension host app id cannot be empty");
    anyhow::ensure!(
        app_id.len() <= 255,
        "extension host app id cannot exceed 255 bytes"
    );
    anyhow::ensure!(
        app_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')),
        "extension host app id contains unsupported characters"
    );
    Ok(())
}

const MAX_EXTENSION_PACKAGE_ENTRIES: usize = 10_000;
const MAX_EXTENSION_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTENSION_PACKAGE_DEPTH: usize = 64;

#[derive(Default)]
struct ExtensionCopyBudget {
    entries: usize,
    bytes: u64,
}

/// Manages the full lifecycle of extensions including loading,
/// activation, process spawning, and RPC communication.
pub struct ExtensionHostRuntime {
    host: ExtensionHost,
    supervisor: ProcessSupervisor,
    extensions_dir: PathBuf,
    transports: HashMap<String, ExtensionTransport>,
    app_id: String,
    next_request_id: u64,
}

struct WasmExtensionTransport {
    contributions: crate::plugin::Contributions,
    pending_frames: VecDeque<Vec<u8>>,
}

impl WasmExtensionTransport {
    fn new(contributions: crate::plugin::Contributions) -> Self {
        Self {
            contributions,
            pending_frames: VecDeque::new(),
        }
    }

    fn queue_message(&mut self, message: ExtensionMessage) -> Result<()> {
        let payload = serde_json::to_vec(&message).context("failed to serialize WASM message")?;
        self.pending_frames.push_back(encode_frame(&payload)?);
        Ok(())
    }
}

impl Transport for WasmExtensionTransport {
    fn send_frame(&mut self, data: &[u8]) -> Result<()> {
        let payload = decode_exact_frame(data)?;
        let message: ExtensionMessage =
            serde_json::from_slice(&payload).context("failed to decode WASM message")?;

        match message {
            ExtensionMessage::Handshake(ExtensionHandshake::Host { version, .. }) => self
                .queue_message(ExtensionMessage::Handshake(ExtensionHandshake::Extension {
                    version,
                    accepted: version == EXTENSION_RPC_VERSION,
                })),
            ExtensionMessage::Rpc(crate::process_model::IpcMessage::Request { id, body }) => {
                let response = match body {
                    ExtensionRequest::Activate
                    | ExtensionRequest::Deactivate
                    | ExtensionRequest::Shutdown
                    | ExtensionRequest::ExecuteCommand { .. } => Ok(ExtensionResponse::Ack),
                    ExtensionRequest::GetContributions => {
                        Ok(ExtensionResponse::Contributions(self.contributions.clone()))
                    }
                };

                self.queue_message(ExtensionMessage::Rpc(
                    crate::process_model::IpcMessage::Response {
                        id,
                        result: response,
                    },
                ))
            }
            ExtensionMessage::Notification(_) => Ok(()),
            other => Err(anyhow!(
                "unexpected host message for WASM extension: {}",
                other.to_text()
            )),
        }
    }

    fn recv_frame(&mut self) -> Result<Vec<u8>> {
        self.pending_frames
            .pop_front()
            .ok_or_else(|| anyhow!("WASM transport has no pending response"))
    }

    fn close(&mut self) -> Result<()> {
        self.pending_frames.clear();
        Ok(())
    }
}

impl ExtensionHostRuntime {
    /// Creates a new extension host runtime.
    pub fn new(extensions_dir: impl AsRef<Path>, app_id: impl Into<String>) -> Self {
        Self {
            host: ExtensionHost::new(),
            supervisor: ProcessSupervisor::new(),
            extensions_dir: extensions_dir.as_ref().to_path_buf(),
            transports: HashMap::new(),
            app_id: app_id.into(),
            next_request_id: 1,
        }
    }

    /// Creates a checked extension host runtime and prepares its install directory.
    pub fn try_new(extensions_dir: impl AsRef<Path>, app_id: impl Into<String>) -> Result<Self> {
        let app_id = app_id.into();
        validate_extension_app_id(&app_id)?;
        std::fs::create_dir_all(extensions_dir.as_ref()).with_context(|| {
            format!(
                "failed to create extensions directory {}",
                extensions_dir.as_ref().display()
            )
        })?;
        Ok(Self::new(extensions_dir, app_id))
    }

    /// Loads a manifest into the host.
    pub fn load(&mut self, manifest: PluginManifest) -> Result<()> {
        Self::validate_api_compatibility(&manifest)?;
        self.host.load_manifest(manifest)
    }

    /// Loads an extension from a directory without copying (dev mode).
    pub fn load_from_directory(&mut self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let manifest = Self::read_manifest_from_dir(path)?;
        Self::validate_api_compatibility(&manifest)?;
        let id = manifest.id.clone();
        self.host
            .load_manifest_with_options(manifest, Some(path.to_path_buf()), true)?;
        Ok(id)
    }

    /// Installs an extension by copying it into the extensions directory.
    pub fn install_from_path(&mut self, path: impl AsRef<Path>) -> Result<String> {
        let path = path.as_ref();
        let source_metadata = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            source_metadata.file_type().is_dir(),
            "install path must be a regular directory"
        );
        let manifest = Self::read_manifest_from_dir(path)?;
        Self::validate_api_compatibility(&manifest)?;
        let id = manifest.id.clone();
        anyhow::ensure!(
            self.host.get(&id).is_none(),
            "extension already loaded: {id}"
        );
        std::fs::create_dir_all(&self.extensions_dir).with_context(|| {
            format!(
                "failed to create extensions directory {}",
                self.extensions_dir.display()
            )
        })?;
        let target_dir = self.extensions_dir.join(&id);
        if target_dir.exists() {
            anyhow::bail!("extension already installed: {}", id);
        }
        let stage_dir = self
            .extensions_dir
            .join(format!(".install-{}", uuid::Uuid::new_v4()));
        let install_result = (|| -> Result<()> {
            copy_dir_all(path, &stage_dir).with_context(|| {
                format!("failed to stage extension for {}", target_dir.display())
            })?;
            std::fs::rename(&stage_dir, &target_dir).with_context(|| {
                format!("failed to install extension to {}", target_dir.display())
            })?;
            if let Err(error) =
                self.host
                    .load_manifest_with_options(manifest, Some(target_dir.clone()), false)
            {
                let _ = std::fs::remove_dir_all(&target_dir);
                return Err(error);
            }
            Ok(())
        })();
        if install_result.is_err() {
            let _ = std::fs::remove_dir_all(&stage_dir);
        }
        install_result?;
        Ok(id)
    }

    /// Lists all installed extensions.
    pub fn list_installed(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut result = Vec::new();
        if !self.extensions_dir.exists() {
            return Ok(result);
        }
        for entry in std::fs::read_dir(&self.extensions_dir).with_context(|| {
            format!(
                "failed to read extensions dir {}",
                self.extensions_dir.display()
            )
        })? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dir = entry.path();
            if let Ok(manifest) = Self::read_manifest_from_dir(&dir) {
                result.push((manifest.id, dir));
            }
        }
        result.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(result)
    }

    /// Removes an installed extension.
    pub fn uninstall(&mut self, id: &str) -> Result<()> {
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        if info.is_active {
            anyhow::bail!("cannot uninstall active extension: {}", id);
        }
        let target_dir = self.extensions_dir.join(id);
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir)
                .with_context(|| format!("failed to remove {}", target_dir.display()))?;
        }
        self.host.unload(id)
    }

    /// Activates a loaded extension.
    pub fn activate(&mut self, id: &str) -> Result<()> {
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        if info.is_active {
            return Ok(());
        }
        match info.manifest.execution_model {
            ExecutionModel::ExternalProcess => self.spawn_external_process(id)?,
            ExecutionModel::Wasm => self.spawn_wasm_extension(id)?,
        }
        self.host.activate(id)
    }

    /// Activates an extension after validating capabilities against a broker.
    pub fn activate_with_broker(&mut self, id: &str, broker: &PermissionBroker) -> Result<()> {
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        let temp_id = ProcessId(u64::MAX);
        let mut missing: Vec<crate::security::Capability> = Vec::new();
        for capability in &info.manifest.capabilities {
            if broker.check(temp_id, capability) != PermissionResult::Granted {
                missing.push(capability.clone());
            }
        }
        if !missing.is_empty() {
            anyhow::bail!(
                "extension {} activation blocked: missing capabilities {:?}",
                id,
                missing
            );
        }
        self.activate(id)
    }

    /// Deactivates an extension.
    pub fn deactivate(&mut self, id: &str) -> Result<()> {
        let (is_active, process_id) = self
            .host
            .get(id)
            .map(|info| (info.is_active, info.process_id))
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        if !is_active {
            return Ok(());
        }
        let mut shutdown_error = None;
        if let Some(transport) = self.transports.get_mut(id) {
            if let Err(error) = transport.send_request(0, ExtensionRequest::Shutdown) {
                shutdown_error = Some(error);
            }
        }
        if let Some(process_id) = process_id
            && let Err(stop_error) = self.supervisor.stop(process_id)
        {
            if let Some(shutdown_error) = shutdown_error {
                return Err(stop_error).context(format!(
                    "failed to stop extension after shutdown request failed: {shutdown_error}"
                ));
            }
            return Err(stop_error).context("failed to stop extension process");
        }
        self.transports.remove(id);
        self.host.deactivate(id)?;
        if let Some(error) = shutdown_error {
            return Err(error).context("extension was stopped after its shutdown request failed");
        }
        Ok(())
    }

    /// Unloads an extension.
    pub fn unload(&mut self, id: &str) -> Result<()> {
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        if info.is_active {
            self.deactivate(id)?;
        }
        self.host.unload(id)
    }

    /// Sends a command to an active extension.
    pub fn send_command(
        &mut self,
        id: &str,
        command_id: impl Into<String>,
        args: Option<serde_json::Value>,
    ) -> Result<()> {
        let request_id = self.allocate_request_id()?;
        let transport = self
            .transports
            .get_mut(id)
            .ok_or_else(|| anyhow!("extension not active: {}", id))?;
        transport.send_request(
            request_id,
            ExtensionRequest::ExecuteCommand {
                command_id: command_id.into(),
                args,
            },
        )?;
        let result = Self::recv_rpc_response(transport, request_id)?;
        match result {
            Ok(ExtensionResponse::Ack) => Ok(()),
            Ok(other) => Err(anyhow!("unexpected response: {}", other.to_text())),
            Err(error) => Err(anyhow!("extension error: {}", error)),
        }
    }

    /// Query an active extension process for its current contributions.
    pub fn request_contributions(&mut self, id: &str) -> Result<crate::plugin::Contributions> {
        let request_id = self.allocate_request_id()?;
        let transport = self
            .transports
            .get_mut(id)
            .ok_or_else(|| anyhow!("extension not active: {}", id))?;
        transport.send_request(request_id, ExtensionRequest::GetContributions)?;
        let result = Self::recv_rpc_response(transport, request_id)?;
        match result {
            Ok(ExtensionResponse::Contributions(contributions)) => Ok(contributions),
            Ok(other) => Err(anyhow!("unexpected response: {}", other.to_text())),
            Err(error) => Err(anyhow!("extension error: {}", error)),
        }
    }

    /// Broadcasts a notification to all active extensions.
    pub fn broadcast_notification(&mut self, notification: ExtensionNotification) {
        for transport in self.transports.values_mut() {
            let _ = transport.send_notification(notification.clone());
        }
    }

    /// Returns info for the extension with the given ID.
    pub fn get(&self, id: &str) -> Option<&ExtensionInfo> {
        self.host.get(id)
    }

    /// Returns all loaded extensions.
    pub fn all(&self) -> Vec<&ExtensionInfo> {
        self.host.all()
    }

    /// Returns all active extensions.
    pub fn active(&self) -> Vec<&ExtensionInfo> {
        self.host.active()
    }

    /// Returns active command contributions.
    pub fn active_commands(&self) -> Vec<&crate::plugin::ContributedCommand> {
        self.host.active_commands()
    }

    /// Returns active menu item contributions.
    pub fn active_menu_items(&self) -> Vec<&crate::plugin::ContributedMenuItem> {
        self.host.active_menu_items()
    }

    /// Returns active panel contributions.
    pub fn active_panels(&self) -> Vec<&crate::plugin::ContributedPanel> {
        self.host.active_panels()
    }

    /// Returns a reference to the process supervisor.
    pub fn supervisor(&self) -> &ProcessSupervisor {
        &self.supervisor
    }

    /// Returns a mutable reference to the process supervisor.
    pub fn supervisor_mut(&mut self) -> &mut ProcessSupervisor {
        &mut self.supervisor
    }

    fn allocate_request_id(&mut self) -> Result<u64> {
        let id = self.next_request_id;
        self.next_request_id = id
            .checked_add(1)
            .ok_or_else(|| anyhow!("extension request identifier space exhausted"))?;
        Ok(id)
    }

    fn validate_api_compatibility(manifest: &PluginManifest) -> Result<()> {
        anyhow::ensure!(
            is_api_compatible(&manifest.api_version),
            "extension {} API version {} is incompatible with host {}",
            manifest.id,
            manifest.api_version,
            HOST_API_VERSION
        );
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn spawn_external_process(&mut self, id: &str) -> Result<()> {
        let _ = id;
        Err(anyhow!(
            "external process extensions not supported on this platform"
        ))
    }

    #[cfg(any(unix, windows))]
    fn spawn_external_process(&mut self, id: &str) -> Result<()> {
        validate_extension_app_id(&self.app_id)?;
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        let load_path = info.load_path.clone();
        let executable = PathBuf::from(&info.manifest.entry_point);
        let executable = match (&load_path, executable.is_relative()) {
            (Some(load_path), true) => load_path.join(executable),
            _ => executable,
        };

        #[cfg(unix)]
        let socket_path =
            try_ipc_socket_path("kael-extension", &uuid::Uuid::new_v4().simple().to_string())?;
        #[cfg(windows)]
        let pipe_name = crate::platform::pipe_name(&self.app_id, id);

        let mut process_info = ProcessInfo::extension(ProcessId(0), &info.manifest.name)
            .executable(executable)
            .args(&info.manifest.args);
        if let Some(load_path) = &load_path {
            process_info = process_info.working_dir(load_path);
        }
        #[cfg(unix)]
        {
            process_info = process_info.env(
                "GPUI_EXTENSION_SOCKET",
                socket_path.to_string_lossy().to_string(),
            );
        }
        #[cfg(windows)]
        {
            process_info =
                process_info.env("GPUI_EXTENSION_SOCKET", format!("\\\\.\\pipe\\{pipe_name}"));
        }
        let process_info = process_info
            .env("GPUI_EXTENSION_ID", id)
            .env("GPUI_API_VERSION", HOST_API_VERSION);

        #[cfg(unix)]
        let (listener, _socket_guard) = {
            use std::os::unix::fs::PermissionsExt as _;

            let listener = std::os::unix::net::UnixListener::bind(&socket_path)
                .with_context(|| format!("failed to bind extension socket for {}", id))?;
            let socket_guard = ExtensionSocketGuard(socket_path.clone());
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("failed to protect extension socket for {id}"))?;
            listener
                .set_nonblocking(true)
                .with_context(|| format!("failed to configure extension listener for {id}"))?;
            (listener, socket_guard)
        };

        let process_id = self.supervisor.spawn(
            process_info,
            RestartPolicy::OnFailure {
                max_restarts: 3,
                backoff: Duration::from_secs(1),
            },
        )?;

        let setup = (|| -> Result<ExtensionTransport> {
            #[cfg(unix)]
            let (mut transport, timeout_control) = {
                use crate::ipc_transport::UnixDomainSocketTransport;

                let deadline = std::time::Instant::now() + EXTENSION_BOOTSTRAP_TIMEOUT;
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return Err(anyhow!(
                                    "timed out waiting for extension {id} to connect"
                                ));
                            }
                            if matches!(
                                self.supervisor.health(process_id),
                                Some(crate::process_model::ProcessHealth::Dead)
                                    | Some(crate::process_model::ProcessHealth::Stopped)
                            ) {
                                return Err(anyhow!("extension {id} exited before connecting"));
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => {
                            return Err(error)
                                .with_context(|| format!("failed to accept connection for {id}"));
                        }
                    }
                };
                stream
                    .set_read_timeout(Some(EXTENSION_BOOTSTRAP_TIMEOUT))
                    .with_context(|| format!("failed to set handshake timeout for {id}"))?;
                let timeout_control = stream
                    .try_clone()
                    .with_context(|| format!("failed to clone extension socket for {id}"))?;
                let transport = UnixDomainSocketTransport::from_stream(stream)
                    .with_context(|| format!("failed to open extension transport for {id}"))?;
                (
                    ExtensionTransport::new(Box::new(transport)),
                    timeout_control,
                )
            };

            #[cfg(windows)]
            let mut transport = {
                use crate::ipc_transport::NamedPipeTransport;
                let transport = NamedPipeTransport::server(&pipe_name)
                    .with_context(|| format!("failed to accept connection for {id}"))?;
                ExtensionTransport::new(Box::new(transport))
            };

            Self::initialize_transport(id, &mut transport, &info.manifest.capabilities)?;

            #[cfg(unix)]
            timeout_control
                .set_read_timeout(None)
                .with_context(|| format!("failed to clear handshake timeout for {id}"))?;

            Ok(transport)
        })();

        match setup {
            Ok(transport) => {
                if let Err(error) = self.host.attach_process(id, process_id) {
                    let _ = self.supervisor.stop(process_id);
                    return Err(error);
                }
                self.transports.insert(id.to_string(), transport);
                Ok(())
            }
            Err(error) => {
                if let Err(stop_error) = self.supervisor.stop(process_id) {
                    return Err(error).context(format!(
                        "extension bootstrap failed and cleanup also failed: {stop_error}"
                    ));
                }
                Err(error)
            }
        }
    }

    fn spawn_wasm_extension(&mut self, id: &str) -> Result<()> {
        let info = self
            .host
            .get(id)
            .ok_or_else(|| anyhow!("extension not found: {}", id))?;
        let mut transport = ExtensionTransport::new(Box::new(WasmExtensionTransport::new(
            info.manifest.contributions.clone(),
        )));

        Self::initialize_transport(id, &mut transport, &info.manifest.capabilities)?;
        self.transports.insert(id.to_string(), transport);
        Ok(())
    }

    fn initialize_transport(
        id: &str,
        transport: &mut ExtensionTransport,
        capabilities: &[crate::security::Capability],
    ) -> Result<()> {
        let capabilities: Vec<serde_json::Value> = capabilities
            .iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<_, _>>()
            .context("failed to serialize extension capabilities")?;

        transport
            .send_handshake(ExtensionHandshake::Host {
                version: EXTENSION_RPC_VERSION,
                capabilities,
            })
            .with_context(|| format!("failed to send handshake to {}", id))?;

        let handshake_response = transport
            .recv_message()
            .with_context(|| format!("handshake timeout or failure for extension {}", id))?;

        match handshake_response {
            ExtensionMessage::Handshake(ExtensionHandshake::Extension { version, accepted }) => {
                if !accepted {
                    anyhow::bail!("extension {} rejected handshake", id);
                }
                if version != EXTENSION_RPC_VERSION {
                    anyhow::bail!(
                        "extension {} protocol version mismatch: expected {}, got {}",
                        id,
                        EXTENSION_RPC_VERSION,
                        version
                    );
                }
                Ok(())
            }
            _ => anyhow::bail!("unexpected handshake response from extension {}", id),
        }
    }

    fn recv_rpc_response(
        transport: &mut ExtensionTransport,
        expected_id: u64,
    ) -> Result<Result<ExtensionResponse, String>> {
        match transport.recv_message()? {
            ExtensionMessage::Rpc(crate::process_model::IpcMessage::Response { id, result })
                if id == expected_id =>
            {
                Ok(result)
            }
            ExtensionMessage::Rpc(crate::process_model::IpcMessage::Response { id, .. }) => {
                Err(anyhow!(
                    "extension response correlation mismatch: expected {expected_id}, got {id}"
                ))
            }
            other => Err(anyhow!("unexpected RPC message: {}", other.to_text())),
        }
    }

    fn read_manifest_from_dir(dir: &Path) -> Result<PluginManifest> {
        let json_path = dir.join("manifest.json");
        let toml_path = dir.join("manifest.toml");
        if json_path.exists() {
            PluginManifest::load(json_path)
        } else if toml_path.exists() {
            PluginManifest::load(toml_path)
        } else {
            anyhow::bail!(
                "no manifest.json or manifest.toml found in {}",
                dir.display()
            )
        }
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    copy_dir_all_bounded(
        src.as_ref(),
        dst.as_ref(),
        0,
        &mut ExtensionCopyBudget::default(),
    )
}

fn copy_dir_all_bounded(
    src: &Path,
    dst: &Path,
    depth: usize,
    budget: &mut ExtensionCopyBudget,
) -> Result<()> {
    anyhow::ensure!(
        depth <= MAX_EXTENSION_PACKAGE_DEPTH,
        "extension package nesting exceeds {MAX_EXTENSION_PACKAGE_DEPTH} levels"
    );
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        budget.entries = budget
            .entries
            .checked_add(1)
            .ok_or_else(|| anyhow!("extension package entry count overflow"))?;
        anyhow::ensure!(
            budget.entries <= MAX_EXTENSION_PACKAGE_ENTRIES,
            "extension package exceeds {MAX_EXTENSION_PACKAGE_ENTRIES} entries"
        );
        let file_type = entry.file_type()?;
        anyhow::ensure!(
            !file_type.is_symlink(),
            "extension packages cannot contain symbolic links"
        );
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all_bounded(&src_path, &dst_path, depth + 1, budget)?;
        } else if file_type.is_file() {
            let remaining = MAX_EXTENSION_PACKAGE_BYTES.saturating_sub(budget.bytes);
            let mut source = std::fs::File::open(&src_path)
                .with_context(|| format!("failed to open {}", src_path.display()))?
                .take(remaining.saturating_add(1));
            let mut destination = std::fs::File::create(&dst_path)
                .with_context(|| format!("failed to create {}", dst_path.display()))?;
            let copied = std::io::copy(&mut source, &mut destination).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
            anyhow::ensure!(
                copied <= remaining,
                "extension package exceeds {MAX_EXTENSION_PACKAGE_BYTES} byte limit"
            );
            budget.bytes = budget
                .bytes
                .checked_add(copied)
                .ok_or_else(|| anyhow!("extension package byte count overflow"))?;
            std::fs::set_permissions(&dst_path, entry.metadata()?.permissions()).with_context(
                || format!("failed to preserve permissions for {}", dst_path.display()),
            )?;
        } else {
            anyhow::bail!("extension packages can contain only files and directories");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{ContributedCommand, PluginManifestBuilder};
    use crate::process_model::{ProcessClass, SupervisorEvent};
    use crate::security::{Capability, PermissionBroker};
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_extension_rpc_roundtrip() {
        use crate::ipc_transport::InMemoryTransport;
        let (ta, tb) = InMemoryTransport::pair();
        let mut host = ExtensionTransport::new(Box::new(ta));
        let mut client = ExtensionTransport::new(Box::new(tb));

        host.send_request(1, ExtensionRequest::GetContributions)
            .unwrap();
        let msg = client.recv_message().unwrap();
        assert!(matches!(
            msg,
            ExtensionMessage::Rpc(crate::process_model::IpcMessage::Request {
                id: 1,
                body: ExtensionRequest::GetContributions
            })
        ));

        client.send_response(1, Ok(ExtensionResponse::Ack)).unwrap();
        let msg = host.recv_message().unwrap();
        assert!(matches!(
            msg,
            ExtensionMessage::Rpc(crate::process_model::IpcMessage::Response {
                id: 1,
                result: Ok(ExtensionResponse::Ack)
            })
        ));
    }

    #[test]
    fn extension_rpc_rejects_mismatched_response_ids() {
        use crate::ipc_transport::InMemoryTransport;

        let (host_side, extension_side) = InMemoryTransport::pair();
        let mut host = ExtensionTransport::new(Box::new(host_side));
        let mut extension = ExtensionTransport::new(Box::new(extension_side));
        extension
            .send_response(99, Ok(ExtensionResponse::Ack))
            .unwrap();

        let error = ExtensionHostRuntime::recv_rpc_response(&mut host, 7).unwrap_err();
        assert!(error.to_string().contains("correlation mismatch"));
    }

    #[test]
    fn checked_extension_runtime_rejects_path_like_app_ids() {
        let tmp =
            std::env::temp_dir().join(format!("kael-extension-runtime-{}", uuid::Uuid::new_v4()));
        assert!(ExtensionHostRuntime::try_new(&tmp, "../../unsafe").is_err());
        assert!(!tmp.exists());
    }

    #[test]
    fn test_permission_validation_allows_when_granted() {
        let tmp = std::env::temp_dir().join(format!("kael-test-perm-allow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut runtime = ExtensionHostRuntime::new(&tmp, "test-app");
        let manifest = PluginManifestBuilder::new(
            "com.test.ext",
            "Test",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();

        runtime.load(manifest).unwrap();
        let broker = PermissionBroker::new();
        runtime
            .activate_with_broker("com.test.ext", &broker)
            .unwrap();
        assert!(runtime.get("com.test.ext").unwrap().is_active);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_permission_validation_blocks_when_denied() {
        let tmp = std::env::temp_dir().join(format!("kael-test-perm-deny-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut runtime = ExtensionHostRuntime::new(&tmp, "test-app");
        let manifest = PluginManifestBuilder::new(
            "com.test.ext2",
            "Test",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .capability(Capability::ShellExecute)
        .build()
        .unwrap();

        runtime.load(manifest).unwrap();
        let broker = PermissionBroker::new();
        let result = runtime.activate_with_broker("com.test.ext2", &broker);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing capabilities"));
        assert!(err.contains("ShellExecute"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_lifecycle_load_activate_deactivate_unload() {
        let tmp = std::env::temp_dir().join(format!("kael-test-life-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut runtime = ExtensionHostRuntime::new(&tmp, "test-app");
        let manifest = PluginManifestBuilder::new(
            "com.test.lifecycle",
            "Lifecycle",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();

        runtime.load(manifest).unwrap();
        assert!(runtime.get("com.test.lifecycle").is_some());
        assert!(!runtime.get("com.test.lifecycle").unwrap().is_active);

        runtime.activate("com.test.lifecycle").unwrap();
        assert!(runtime.get("com.test.lifecycle").unwrap().is_active);

        runtime.deactivate("com.test.lifecycle").unwrap();
        assert!(!runtime.get("com.test.lifecycle").unwrap().is_active);

        runtime.unload("com.test.lifecycle").unwrap();
        assert!(runtime.get("com.test.lifecycle").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_wasm_runtime_serves_manifest_contributions_and_commands() {
        let tmp = std::env::temp_dir().join(format!("kael-test-wasm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut runtime = ExtensionHostRuntime::new(&tmp, "test-app");
        let manifest = PluginManifestBuilder::new(
            "com.test.wasm",
            "Wasm",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .command(ContributedCommand {
            id: "wasm.echo".to_string(),
            title: "Echo".to_string(),
            keybinding: None,
        })
        .build()
        .unwrap();

        runtime.load(manifest).unwrap();
        runtime.activate("com.test.wasm").unwrap();

        let contributions = runtime.request_contributions("com.test.wasm").unwrap();
        assert_eq!(contributions.commands.len(), 1);
        assert_eq!(contributions.commands[0].id, "wasm.echo");

        runtime
            .send_command("com.test.wasm", "wasm.echo", None)
            .unwrap();

        runtime.deactivate("com.test.wasm").unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_crash_isolation_via_supervisor() {
        let tmp = std::env::temp_dir().join(format!("kael-test-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut runtime = ExtensionHostRuntime::new(&tmp, "test-app");

        let events = Arc::new(Mutex::new(Vec::new()));
        runtime.supervisor.on_event({
            let events = events.clone();
            move |event| events.lock().unwrap().push(event)
        });

        let info = ProcessInfo::extension(ProcessId(0), "crash-test").executable("false");
        let id = runtime
            .supervisor
            .spawn(info, RestartPolicy::Never)
            .unwrap();

        std::thread::sleep(std::time::Duration::from_secs(2));

        let events = events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SupervisorEvent::Spawned { .. }))
        );
        assert!(
            events.iter().any(
                |e| matches!(e, SupervisorEvent::Exited { id: event_id, .. } if *event_id == id)
            )
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_install_and_list() {
        let tmp = std::env::temp_dir().join(format!("kael-test-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ext_dir = tmp.join("my-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.install",
            "InstallTest",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
        let id = runtime.install_from_path(&ext_dir).unwrap();
        assert_eq!(id, "com.test.install");

        let installed = runtime.list_installed().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].0, "com.test.install");

        runtime.uninstall(&id).unwrap();
        assert!(runtime.get(&id).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn extension_install_rejects_symlinks_and_cleans_staging() {
        use std::os::unix::fs::symlink;

        let tmp =
            std::env::temp_dir().join(format!("kael-test-install-link-{}", uuid::Uuid::new_v4()));
        let source = tmp.join("source");
        let installs = tmp.join("extensions");
        std::fs::create_dir_all(&source).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.link",
            "Link",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();
        std::fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(tmp.join("outside"), b"private").unwrap();
        symlink(tmp.join("outside"), source.join("linked-secret")).unwrap();

        let mut runtime = ExtensionHostRuntime::try_new(&installs, "test-app").unwrap();
        assert!(runtime.install_from_path(&source).is_err());
        assert!(!installs.join("com.test.link").exists());
        assert_eq!(std::fs::read_dir(&installs).unwrap().count(), 0);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn extension_install_rejects_incompatible_api_before_copying() {
        let tmp =
            std::env::temp_dir().join(format!("kael-test-install-api-{}", uuid::Uuid::new_v4()));
        let source = tmp.join("source");
        let installs = tmp.join("extensions");
        std::fs::create_dir_all(&source).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.future",
            "Future",
            "1.0.0",
            "2.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();
        std::fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::try_new(&installs, "test-app").unwrap();
        assert!(runtime.install_from_path(&source).is_err());
        assert_eq!(std::fs::read_dir(&installs).unwrap().count(), 0);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn extension_package_copy_rejects_excessive_nesting() {
        let tmp =
            std::env::temp_dir().join(format!("kael-test-install-depth-{}", uuid::Uuid::new_v4()));
        let source = tmp.join("source");
        let mut nested = source.clone();
        for index in 0..=MAX_EXTENSION_PACKAGE_DEPTH {
            nested = nested.join(format!("level-{index}"));
        }
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("payload"), b"data").unwrap();

        assert!(copy_dir_all(&source, tmp.join("copy")).is_err());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exited_external_extension_fails_activation_and_is_stopped() {
        let tmp =
            std::env::temp_dir().join(format!("kael-test-external-exit-{}", uuid::Uuid::new_v4()));
        let mut runtime = ExtensionHostRuntime::try_new(&tmp, "test-app").unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.exits",
            "Exits",
            "1.0.0",
            "1.0.0",
            "/usr/bin/true",
            ExecutionModel::ExternalProcess,
        )
        .build()
        .unwrap();
        runtime.load(manifest).unwrap();

        let started = std::time::Instant::now();
        assert!(runtime.activate("com.test.exits").is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!runtime.get("com.test.exits").unwrap().is_active);
        let processes = runtime.supervisor().processes();
        assert_eq!(processes.len(), 1);
        assert_eq!(
            runtime.supervisor().health(processes[0]),
            Some(crate::process_model::ProcessHealth::Stopped)
        );
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_dev_mode_load() {
        let tmp = std::env::temp_dir().join(format!("kael-test-dev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ext_dir = tmp.join("dev-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.dev",
            "DevTest",
            "1.0.0",
            "1.0.0",
            "ext.wasm",
            ExecutionModel::Wasm,
        )
        .build()
        .unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
        let id = runtime.load_from_directory(&ext_dir).unwrap();
        assert_eq!(id, "com.test.dev");
        let info = runtime.get("com.test.dev").unwrap();
        assert!(info.dev_mode);
        assert_eq!(info.load_path, Some(ext_dir));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_external_process_manifest_load() {
        let tmp = std::env::temp_dir().join(format!("kael-test-ext-{}-2", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ext_dir = tmp.join("sample-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.example.sample",
            "Sample Extension",
            "1.0.0",
            "1.0.0",
            "./sample_ext",
            ExecutionModel::ExternalProcess,
        )
        .description("A sample external-process extension for builder validation")
        .author("Augustus Otu")
        .capability(Capability::ClipboardRead)
        .capability(Capability::Notification)
        .command(ContributedCommand {
            id: "sample.greet".to_string(),
            title: "Greet".to_string(),
            keybinding: None,
        })
        .build()
        .unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
        let id = runtime.load_from_directory(&ext_dir).unwrap();
        assert_eq!(id, "com.example.sample");
        let info = runtime.get("com.example.sample").unwrap();
        assert!(info.dev_mode);
        assert!(
            !info.is_active,
            "extension should not auto-activate after load"
        );
        assert_eq!(
            info.manifest.execution_model,
            ExecutionModel::ExternalProcess
        );
        assert_eq!(info.manifest.capabilities.len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_permission_broker_blocks_external_process_extension() {
        let tmp = std::env::temp_dir().join(format!("kael-test-broker-{}-3", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ext_dir = tmp.join("broker-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.broker",
            "BrokerTest",
            "1.0.0",
            "1.0.0",
            "./broker_ext",
            ExecutionModel::ExternalProcess,
        )
        .capability(Capability::ClipboardRead)
        .capability(Capability::ShellExecute)
        .build()
        .unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
        runtime.load_from_directory(&ext_dir).unwrap();

        let mut broker = PermissionBroker::new();
        let temp_id = ProcessId(u64::MAX);
        broker.register_process(temp_id, ProcessClass::Extension);
        broker.grant(temp_id, Capability::ClipboardRead);
        // ShellExecute is NOT granted

        let result = runtime.activate_with_broker("com.test.broker", &broker);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing capabilities"));
        assert!(err.contains("ShellExecute"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_security_capability_sandboxing() {
        let tmp = std::env::temp_dir().join(format!("kael-test-sec-{}-4", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let ext_dir = tmp.join("sandbox-ext");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let manifest = PluginManifestBuilder::new(
            "com.test.sandbox",
            "SandboxTest",
            "1.0.0",
            "1.0.0",
            "./sandbox",
            ExecutionModel::ExternalProcess,
        )
        .capability(Capability::FilesystemRead {
            scope: crate::PathScope::UserSelected,
        })
        .capability(Capability::ShellExecute)
        .build()
        .unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
        runtime.load_from_directory(&ext_dir).unwrap();

        // Even with a broker that grants some capabilities, denied ones
        // should still prevent activation.
        let mut broker = PermissionBroker::new();
        let temp_id = ProcessId(u64::MAX);
        broker.register_process(temp_id, ProcessClass::Extension);
        broker.grant(
            temp_id,
            Capability::FilesystemRead {
                scope: crate::PathScope::UserSelected,
            },
        );
        // ShellExecute is NOT granted

        let result = runtime.activate_with_broker("com.test.sandbox", &broker);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
