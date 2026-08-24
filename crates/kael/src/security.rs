//! Security boundary: capability model and permission broker for GPUI.
//!
//! This module provides safe defaults and explicit capability grants for
//! dangerous actions such as opening external URLs, filesystem access, and
//! plugin execution. The model is designed to integrate with the process
//! isolation layer so that child processes receive only the capabilities they
//! need.
//!
//! ## Phase 11 additions
//!
//! * **Unified permission prompt flow** — [`PermissionKind`], [`PermissionRequest`],
//!   [`PermissionManager`].
//! * **Secure credential / keychain wrappers** — [`CredentialEntry`],
//!   [`KeychainStore`].
//! * **File-scoped access tokens** — [`AccessToken`], [`AccessTokenStore`].
//! * **Plugin permission manifests** — [`PluginPermissionManifest`].
//! * **Process capability limits** — [`ProcessLimits`], [`ProcessCapability`].
//! * **Network permission policy** — [`NetworkPolicy`].
//! * **Checked app network request descriptors** — [`AppNetworkRequest`],
//!   [`AppRealtimeConnection`].
//! * **IPC schema versioning** — [`IpcSchema`].

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::platform::PermissionStatus;
use crate::process_model::{ProcessClass, ProcessId};

type PromptHandler = Arc<dyn Fn(ProcessId, &Capability) -> PermissionResult + Send + Sync>;

const MAX_PERMISSION_REASON_BYTES: usize = 4 * 1024;
const MAX_CREDENTIAL_LABEL_BYTES: usize = 512;
const MAX_CREDENTIAL_SECRET_BYTES: usize = 1024 * 1024;
const MAX_CREDENTIAL_ENTRIES: usize = 4096;
const MAX_ACCESS_TOKENS: usize = 4096;
const MAX_BOOKMARK_PATH_BYTES: usize = 16 * 1024;
const MAX_NETWORK_POLICY_HOSTS: usize = 256;
const MAX_NETWORK_URL_BYTES: usize = 16 * 1024;
const MAX_NETWORK_HEADERS: usize = 128;
const MAX_NETWORK_HEADER_NAME_BYTES: usize = 256;
const MAX_NETWORK_BODY_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_PROCESS_VIOLATIONS: usize = 1024;
const MAX_PROCESS_VIOLATION_BYTES: usize = 1024;
const MAX_PROCESS_NAME_BYTES: usize = 256;
const MAX_REALTIME_CONNECTIONS: usize = 64;
const MAX_IPC_MESSAGE_TYPES: usize = 256;
const MAX_IPC_MESSAGE_TYPE_BYTES: usize = 128;

// ---------------------------------------------------------------------------
// Capability Model
// ---------------------------------------------------------------------------

/// The scope of filesystem access granted by a capability.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PathScope {
    /// Access restricted to the application's own data directory.
    AppData,
    /// Access restricted to the user's downloads directory.
    Downloads,
    /// Access restricted to paths explicitly selected by the user via a
    /// platform file dialog.
    UserSelected,
    /// Unrestricted filesystem access (use sparingly).
    Any,
}

/// A capability granted to a process or component.
///
/// Capabilities are explicit grants for dangerous actions. The default for all
/// high-risk capabilities is **deny**.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Open URLs in the system default browser.
    OpenExternalUrl,
    /// Read files within the given scope.
    FilesystemRead {
        /// The scope of allowed filesystem reads.
        scope: PathScope,
    },
    /// Write files within the given scope.
    FilesystemWrite {
        /// The scope of allowed filesystem writes.
        scope: PathScope,
    },
    /// Execute shell commands.
    ShellExecute,
    /// Read from the system clipboard.
    ClipboardRead,
    /// Write to the system clipboard.
    ClipboardWrite,
    /// Show native notifications.
    Notification,
    /// Make network requests to the given hosts.
    Network {
        /// Allowed host patterns.
        hosts: Vec<String>,
    },
    /// Access the microphone.
    Microphone,
    /// Access the camera.
    Camera,
    /// Capture screen content.
    ScreenCapture,
    /// Access device location/geolocation.
    Location,
    /// Access USB devices.
    UsbDevice,
    /// Access HID devices.
    HidDevice,
    /// Access serial ports.
    SerialPort,
    /// Access Bluetooth devices/services.
    Bluetooth,
}

impl Capability {
    /// Returns true if this capability is considered high-risk and should
    /// require explicit user or developer opt-in.
    pub fn is_high_risk(&self) -> bool {
        matches!(
            self,
            Capability::ShellExecute
                | Capability::ClipboardRead
                | Capability::Network { .. }
                | Capability::Camera
                | Capability::ScreenCapture
                | Capability::Location
                | Capability::UsbDevice
                | Capability::HidDevice
                | Capability::SerialPort
                | Capability::Bluetooth
                | Capability::FilesystemRead {
                    scope: PathScope::Any
                }
                | Capability::FilesystemWrite {
                    scope: PathScope::Any
                }
        )
    }
}

// ---------------------------------------------------------------------------
// Permission Result
// ---------------------------------------------------------------------------

/// The result of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionResult {
    /// The capability is granted.
    Granted,
    /// The capability is explicitly denied.
    Denied,
    /// The capability requires a user prompt to decide.
    Prompt,
}

// ---------------------------------------------------------------------------
// Permission Broker
// ---------------------------------------------------------------------------

/// A centralized broker for capability checks and permission requests.
///
/// Applications configure the broker at startup with the capabilities they
/// wish to grant to each process class or individual process. The broker can
/// also be used to implement runtime permission prompts.
#[derive(Clone, Default)]
pub struct PermissionBroker {
    grants: HashMap<ProcessId, HashSet<Capability>>,
    process_classes: HashMap<ProcessId, ProcessClass>,
    class_defaults: HashMap<ProcessClass, HashSet<Capability>>,
    prompt_handler: Option<PromptHandler>,
}

impl std::fmt::Debug for PermissionBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionBroker")
            .field("grant_count", &self.grants.len())
            .field("registered_processes", &self.process_classes.len())
            .field("class_default_count", &self.class_defaults.len())
            .field("has_prompt_handler", &self.prompt_handler.is_some())
            .finish()
    }
}

impl PermissionBroker {
    /// Create an empty permission broker.
    pub fn new() -> Self {
        Self {
            grants: HashMap::new(),
            process_classes: HashMap::new(),
            class_defaults: HashMap::new(),
            prompt_handler: None,
        }
    }

    /// Register a process and its process class with the broker.
    pub fn register_process(&mut self, process: ProcessId, class: ProcessClass) {
        self.process_classes.insert(process, class);
    }

    /// Return the registered process class, if this process is known to the broker.
    pub fn process_class(&self, process: ProcessId) -> Option<ProcessClass> {
        self.process_classes.get(&process).copied()
    }

    /// Return whether this process has been registered with the broker.
    pub fn is_process_registered(&self, process: ProcessId) -> bool {
        self.process_classes.contains_key(&process)
    }

    /// Remove a process from the broker, clearing grants and class metadata.
    pub fn unregister_process(&mut self, process: ProcessId) {
        self.process_classes.remove(&process);
        self.grants.remove(&process);
    }

    /// Configure default capabilities for a process class.
    pub fn set_default_capabilities<I>(&mut self, class: ProcessClass, capabilities: I)
    where
        I: IntoIterator<Item = Capability>,
    {
        self.class_defaults
            .insert(class, capabilities.into_iter().collect());
    }

    /// Configure default capabilities using the provided threat model.
    pub fn apply_threat_model(&mut self, model: &ThreatModel) {
        self.set_default_capabilities(
            ProcessClass::Ui,
            model.defaults_for(ProcessClass::Ui).iter().cloned(),
        );
        self.set_default_capabilities(
            ProcessClass::Worker,
            model.defaults_for(ProcessClass::Worker).iter().cloned(),
        );
        self.set_default_capabilities(
            ProcessClass::Utility,
            model.defaults_for(ProcessClass::Utility).iter().cloned(),
        );
        self.set_default_capabilities(
            ProcessClass::Media,
            model.defaults_for(ProcessClass::Media).iter().cloned(),
        );
        self.set_default_capabilities(
            ProcessClass::Extension,
            model.defaults_for(ProcessClass::Extension).iter().cloned(),
        );
    }

    /// Install a prompt handler for permissions that are not already granted.
    pub fn set_prompt_handler(
        &mut self,
        handler: impl Fn(ProcessId, &Capability) -> PermissionResult + Send + Sync + 'static,
    ) {
        self.prompt_handler = Some(Arc::new(handler));
    }

    /// Return a copy of the broker with a prompt handler installed.
    pub fn with_prompt_handler(
        mut self,
        handler: impl Fn(ProcessId, &Capability) -> PermissionResult + Send + Sync + 'static,
    ) -> Self {
        self.set_prompt_handler(handler);
        self
    }

    /// Grant a capability to a process.
    pub fn grant(&mut self, process: ProcessId, capability: Capability) {
        self.grants.entry(process).or_default().insert(capability);
    }

    /// Grant multiple capabilities to a process.
    pub fn grant_all<I>(&mut self, process: ProcessId, capabilities: I)
    where
        I: IntoIterator<Item = Capability>,
    {
        self.grants.entry(process).or_default().extend(capabilities);
    }

    /// Revoke a capability from a process.
    pub fn revoke(&mut self, process: ProcessId, capability: &Capability) {
        if let Some(set) = self.grants.get_mut(&process) {
            let _ = std::collections::HashSet::<Capability>::remove(set, capability);
        }
    }

    /// Revoke all capabilities from a process.
    pub fn revoke_all(&mut self, process: ProcessId) {
        self.grants.remove(&process);
    }

    fn granted_by_default(&self, process: ProcessId, capability: &Capability) -> bool {
        self.process_classes
            .get(&process)
            .and_then(|class| self.class_defaults.get(class))
            .is_some_and(|set| set.contains(capability))
    }

    /// Explicitly prompt for a capability.
    pub fn prompt(&self, process: ProcessId, capability: &Capability) -> PermissionResult {
        if let Some(handler) = &self.prompt_handler {
            invoke_prompt_handler(handler, process, capability)
        } else {
            PermissionResult::Denied
        }
    }

    /// Check whether a process holds a given capability.
    pub fn check(&self, process: ProcessId, capability: &Capability) -> PermissionResult {
        if let Some(set) = self.grants.get(&process) {
            if std::collections::HashSet::<Capability>::contains(set, capability) {
                return PermissionResult::Granted;
            }
        }
        if self.granted_by_default(process, capability) {
            return PermissionResult::Granted;
        }
        if let Some(prompt_handler) = &self.prompt_handler {
            return invoke_prompt_handler(prompt_handler, process, capability);
        }
        PermissionResult::Denied
    }

    /// Check whether a process holds any of the given capabilities.
    pub fn check_any(&self, process: ProcessId, capabilities: &[Capability]) -> PermissionResult {
        let mut prompted = false;
        for cap in capabilities {
            match self.check(process, cap) {
                PermissionResult::Granted => return PermissionResult::Granted,
                PermissionResult::Prompt => prompted = true,
                PermissionResult::Denied => {}
            }
        }
        if prompted {
            PermissionResult::Prompt
        } else {
            PermissionResult::Denied
        }
    }

    /// Return all capabilities granted to a process.
    pub fn capabilities(&self, process: ProcessId) -> Vec<Capability> {
        let mut capabilities = self
            .process_classes
            .get(&process)
            .and_then(|class| self.class_defaults.get(class))
            .cloned()
            .unwrap_or_default();
        if let Some(grants) = self.grants.get(&process) {
            capabilities.extend(grants.iter().cloned());
        }
        capabilities.into_iter().collect()
    }
}

fn invoke_prompt_handler(
    handler: &PromptHandler,
    process: ProcessId,
    capability: &Capability,
) -> PermissionResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handler(process, capability)
    }))
    .unwrap_or(PermissionResult::Denied)
}

// ---------------------------------------------------------------------------
// Threat Model Documentation Types
// ---------------------------------------------------------------------------

/// Categories of threats that the GPUI security model addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreatCategory {
    /// Untrusted or semi-trusted content rendered by the app.
    UntrustedContent,
    /// Malicious or compromised plugins or extensions.
    MaliciousPlugin,
    /// A compromised child process attempting to escape its sandbox.
    CompromisedChildProcess,
    /// Unsafe handling of external URLs or protocol schemes.
    UnsafeExternalUrl,
    /// Leakage of local privileges (filesystem, shell, etc.).
    LocalPrivilegeLeak,
}

/// A documented threat and its mitigations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Threat {
    /// The category of threat.
    pub category: ThreatCategory,
    /// Human-readable description.
    pub description: String,
    /// Framework surfaces that are vulnerable.
    pub surfaces: Vec<String>,
    /// Mitigations in place or planned.
    pub mitigations: Vec<String>,
}

/// A lightweight threat model for a GPUI application.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ThreatModel {
    /// Documented threats.
    pub threats: Vec<Threat>,
    /// Default capability grants for the UI process.
    pub ui_defaults: Vec<Capability>,
    /// Default capability grants for worker processes.
    pub worker_defaults: Vec<Capability>,
    /// Default capability grants for app-owned utility helper processes.
    #[serde(default)]
    pub utility_defaults: Vec<Capability>,
    /// Default capability grants for media processes.
    pub media_defaults: Vec<Capability>,
    /// Default capability grants for extension hosts.
    pub extension_defaults: Vec<Capability>,
}

impl ThreatModel {
    /// Create a new threat model with safe defaults.
    pub fn new() -> Self {
        Self {
            threats: Vec::new(),
            ui_defaults: vec![
                Capability::OpenExternalUrl,
                Capability::ClipboardRead,
                Capability::ClipboardWrite,
                Capability::Notification,
                Capability::FilesystemRead {
                    scope: PathScope::UserSelected,
                },
                Capability::FilesystemWrite {
                    scope: PathScope::UserSelected,
                },
            ],
            worker_defaults: vec![
                Capability::FilesystemRead {
                    scope: PathScope::AppData,
                },
                Capability::FilesystemWrite {
                    scope: PathScope::AppData,
                },
            ],
            utility_defaults: Vec::new(),
            media_defaults: vec![
                Capability::Microphone,
                Capability::Camera,
                Capability::ScreenCapture,
            ],
            extension_defaults: vec![],
        }
    }

    /// Create a threat model whose process-class defaults contain no
    /// high-risk capabilities. Apps can grant individual capabilities after
    /// explicit consent or policy evaluation.
    pub fn strict() -> Self {
        let mut model = Self::new();
        model
            .ui_defaults
            .retain(|capability| !capability.is_high_risk());
        model
            .worker_defaults
            .retain(|capability| !capability.is_high_risk());
        model
            .utility_defaults
            .retain(|capability| !capability.is_high_risk());
        model
            .media_defaults
            .retain(|capability| !capability.is_high_risk());
        model
            .extension_defaults
            .retain(|capability| !capability.is_high_risk());
        model
    }

    /// Add a threat to the model.
    pub fn add_threat(mut self, threat: Threat) -> Self {
        self.threats.push(threat);
        self
    }

    /// Return the default capabilities for a given process class.
    pub fn defaults_for(&self, class: ProcessClass) -> &[Capability] {
        match class {
            ProcessClass::Ui => &self.ui_defaults,
            ProcessClass::Worker => &self.worker_defaults,
            ProcessClass::Utility => &self.utility_defaults,
            ProcessClass::Media => &self.media_defaults,
            ProcessClass::Extension => &self.extension_defaults,
        }
    }
}

// ===========================================================================
// Phase 11: Unified Permission Prompt Flow
// ===========================================================================

/// The kind of system permission that can be requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionKind {
    /// Camera access.
    Camera,
    /// Microphone access.
    Microphone,
    /// Location services.
    Location,
    /// File system access.
    FileAccess,
    /// Network access.
    Network,
    /// Push / local notifications.
    Notifications,
    /// Accessibility APIs (e.g. screen reader control).
    Accessibility,
}

/// A request to grant a specific permission, including a human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// The permission being requested.
    pub kind: PermissionKind,
    /// A human-readable explanation of why the permission is needed.
    pub reason: String,
}

impl PermissionRequest {
    /// Validate prompt text before showing it to the user.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.reason.trim().is_empty(),
            "permission request reason cannot be empty"
        );
        anyhow::ensure!(
            self.reason == self.reason.trim(),
            "permission request reason cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            self.reason.len() <= MAX_PERMISSION_REASON_BYTES,
            "permission request reason exceeds {MAX_PERMISSION_REASON_BYTES} bytes"
        );
        anyhow::ensure!(
            !self.reason.chars().any(char::is_control),
            "permission request reason cannot contain control characters"
        );
        Ok(())
    }
}

/// Tracks and manages permission states for the application.
#[derive(Debug, Clone, Default)]
pub struct PermissionManager {
    states: HashMap<PermissionKind, PermissionStatus>,
}

impl PermissionManager {
    /// Create an empty permission manager with all permissions undetermined.
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    /// Return the current status for the given permission kind.
    pub fn status(&self, kind: PermissionKind) -> PermissionStatus {
        self.states
            .get(&kind)
            .copied()
            .unwrap_or(PermissionStatus::NotDetermined)
    }

    /// Request a permission. If the permission has not been determined yet,
    /// the provided callback decides the outcome.
    pub fn request(
        &mut self,
        request: &PermissionRequest,
        decide: impl FnOnce(&PermissionRequest) -> PermissionStatus,
    ) -> PermissionStatus {
        let current = self.status(request.kind);
        if current != PermissionStatus::NotDetermined {
            return current;
        }
        if request.validate().is_err() {
            return PermissionStatus::Denied;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decide(request)))
            .unwrap_or(PermissionStatus::Denied);
        self.states.insert(request.kind, result);
        result
    }

    /// Directly set the status of a permission (e.g. from a platform callback).
    pub fn set_status(&mut self, kind: PermissionKind, status: PermissionStatus) {
        self.states.insert(kind, status);
    }

    /// Revoke a previously granted permission, resetting it to [`PermissionStatus::Denied`].
    pub fn revoke(&mut self, kind: PermissionKind) {
        self.states.insert(kind, PermissionStatus::Denied);
    }

    /// Return all permissions that have been explicitly set.
    pub fn all_statuses(&self) -> &HashMap<PermissionKind, PermissionStatus> {
        &self.states
    }
}

// ===========================================================================
// Phase 11: Secure Credential / Keychain Wrappers
// ===========================================================================

/// A single credential entry stored in the keychain.
#[derive(Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    /// The service or application the credential belongs to.
    pub service: String,
    /// The account or username.
    pub account: String,
    /// The secret data (password, token, etc.).
    pub secret: Vec<u8>,
}

impl CredentialEntry {
    /// Validate credential identifiers and secret size before storage.
    pub fn validate(&self) -> Result<()> {
        validate_credential_label(&self.service, "credential service")?;
        validate_credential_label(&self.account, "credential account")?;
        anyhow::ensure!(!self.secret.is_empty(), "credential secret cannot be empty");
        anyhow::ensure!(
            self.secret.len() <= MAX_CREDENTIAL_SECRET_BYTES,
            "credential secret exceeds {MAX_CREDENTIAL_SECRET_BYTES} bytes"
        );
        Ok(())
    }
}

impl std::fmt::Debug for CredentialEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialEntry")
            .field("service", &self.service)
            .field("account", &self.account)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CredentialEntry {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

/// An in-memory credential store that mimics system keychain semantics.
///
/// In production, callers should back this with a platform keychain
/// (macOS Keychain, Windows Credential Manager, libsecret on Linux).
#[derive(Clone, Default)]
pub struct KeychainStore {
    entries: HashMap<(String, String), CredentialEntry>,
}

impl std::fmt::Debug for KeychainStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeychainStore")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

impl KeychainStore {
    /// Create an empty keychain store.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Store a credential. Overwrites any existing entry with the same
    /// service + account pair.
    pub fn store(&mut self, entry: CredentialEntry) -> Result<()> {
        entry.validate()?;
        let key = (entry.service.clone(), entry.account.clone());
        anyhow::ensure!(
            self.entries.contains_key(&key) || self.entries.len() < MAX_CREDENTIAL_ENTRIES,
            "credential store cannot contain more than {MAX_CREDENTIAL_ENTRIES} entries"
        );
        self.entries.insert(key, entry);
        Ok(())
    }

    /// Retrieve a credential by service and account.
    pub fn retrieve(&self, service: &str, account: &str) -> Option<&CredentialEntry> {
        validate_credential_label(service, "credential service").ok()?;
        validate_credential_label(account, "credential account").ok()?;
        self.entries.get(&(service.to_owned(), account.to_owned()))
    }

    /// Delete a credential by service and account. Returns `true` if removed.
    pub fn delete(&mut self, service: &str, account: &str) -> bool {
        if validate_credential_label(service, "credential service").is_err()
            || validate_credential_label(account, "credential account").is_err()
        {
            return false;
        }
        self.entries
            .remove(&(service.to_owned(), account.to_owned()))
            .is_some()
    }

    /// List all stored credentials (without exposing secrets directly).
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.entries
            .values()
            .map(|entry| (entry.service.as_str(), entry.account.as_str()))
            .collect()
    }

    /// Return the number of stored credentials.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return true if no credentials are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ===========================================================================
// Phase 11: File-Scoped Access Tokens
// ===========================================================================

/// A time-scoped token granting access to a specific file path.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessToken {
    /// The file path this token grants access to.
    pub path: PathBuf,
    /// An opaque token string.
    pub token: String,
    /// Unix timestamp (seconds) when the token was created.
    pub created_at: u64,
    /// Optional expiry as a Unix timestamp (seconds).
    pub expires_at: Option<u64>,
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessToken")
            .field("path", &"[REDACTED]")
            .field("token", &"[REDACTED]")
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Manages file-scoped access tokens with optional expiry.
#[derive(Clone, Default)]
pub struct AccessTokenStore {
    tokens: HashMap<String, AccessToken>,
}

impl std::fmt::Debug for AccessTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessTokenStore")
            .field("token_count", &self.tokens.len())
            .finish()
    }
}

impl AccessTokenStore {
    /// Create an empty token store.
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Issue a new access token for the given path. Returns the token string.
    pub fn issue(&mut self, path: PathBuf, now: u64, ttl_seconds: Option<u64>) -> Result<String> {
        validate_bookmark_path(&path)?;
        self.purge_expired(now);
        anyhow::ensure!(
            self.tokens.len() < MAX_ACCESS_TOKENS,
            "access token store cannot contain more than {MAX_ACCESS_TOKENS} active tokens"
        );
        let expires_at = ttl_seconds
            .map(|ttl| {
                anyhow::ensure!(ttl > 0, "access token TTL must be greater than zero");
                now.checked_add(ttl)
                    .ok_or_else(|| anyhow::anyhow!("access token expiry overflows u64"))
            })
            .transpose()?;
        let token_str = (0..8)
            .map(|_| format!("kat_{}", uuid::Uuid::new_v4().simple()))
            .find(|candidate| !self.tokens.contains_key(candidate))
            .ok_or_else(|| anyhow::anyhow!("failed to generate a unique access token"))?;
        let access_token = AccessToken {
            path,
            token: token_str.clone(),
            created_at: now,
            expires_at,
        };
        self.tokens.insert(token_str.clone(), access_token);
        Ok(token_str)
    }

    /// Validate a token at the given timestamp. Returns the associated path
    /// if valid.
    pub fn validate(&self, token: &str, now: u64) -> Option<&PathBuf> {
        let entry = self.tokens.get(token)?;
        if let Some(expires) = entry.expires_at {
            if now >= expires {
                return None;
            }
        }
        Some(&entry.path)
    }

    /// Revoke (remove) a token. Returns `true` if the token existed.
    pub fn revoke(&mut self, token: &str) -> bool {
        self.tokens.remove(token).is_some()
    }

    /// List all currently stored tokens (including expired ones).
    pub fn list(&self) -> Vec<&AccessToken> {
        self.tokens.values().collect()
    }

    /// Remove all expired tokens as of `now`. Returns the count removed.
    pub fn purge_expired(&mut self, now: u64) -> usize {
        let before = self.tokens.len();
        self.tokens
            .retain(|_, token| token.expires_at.map_or(true, |expires| now < expires));
        before - self.tokens.len()
    }
}

/// Access mode for a persisted file access bookmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileAccessMode {
    /// Read-only access to the bookmarked path.
    Read,
    /// Write-only access to the bookmarked path.
    Write,
    /// Read and write access to the bookmarked path.
    ReadWrite,
}

impl FileAccessMode {
    /// Returns true when this mode permits reads.
    pub fn allows_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    /// Returns true when this mode permits writes.
    pub fn allows_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    /// Return the filesystem capabilities represented by this mode and scope.
    pub fn capabilities(self, scope: PathScope) -> Vec<Capability> {
        let mut capabilities = Vec::new();
        if self.allows_read() {
            capabilities.push(Capability::FilesystemRead {
                scope: scope.clone(),
            });
        }
        if self.allows_write() {
            capabilities.push(Capability::FilesystemWrite { scope });
        }
        capabilities
    }
}

/// A validated app-level bookmark for user-approved file access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAccessBookmark {
    /// Stable app-owned bookmark identifier.
    pub id: String,
    /// Bookmarked path.
    pub path: PathBuf,
    /// Scope represented by this bookmark.
    pub scope: PathScope,
    /// Access mode granted for this bookmark.
    pub mode: FileAccessMode,
    /// Optional token time-to-live in seconds.
    pub ttl_seconds: Option<u64>,
}

impl FileAccessBookmark {
    /// Create a builder for a bookmark id and path.
    pub fn builder(id: impl Into<String>, path: impl Into<PathBuf>) -> FileAccessBookmarkBuilder {
        FileAccessBookmarkBuilder::new(id, path)
    }

    /// Validate the bookmark before persisting or issuing access tokens.
    pub fn validate(&self) -> Result<()> {
        validate_bookmark_id(&self.id)?;
        validate_bookmark_path(&self.path)?;
        if let Some(ttl) = self.ttl_seconds {
            anyhow::ensure!(
                ttl > 0,
                "file access bookmark TTL must be greater than zero"
            );
        }
        Ok(())
    }

    /// Return the filesystem capabilities represented by this bookmark.
    pub fn capabilities(&self) -> Vec<Capability> {
        self.mode.capabilities(self.scope.clone())
    }

    /// Issue an access token for this bookmark using the provided token store.
    pub fn issue_token(&self, store: &mut AccessTokenStore, now: u64) -> Result<String> {
        self.validate()?;
        store.issue(self.path.clone(), now, self.ttl_seconds)
    }
}

/// Builder for creating checked file access bookmarks.
#[derive(Debug, Clone)]
pub struct FileAccessBookmarkBuilder {
    id: String,
    path: PathBuf,
    scope: PathScope,
    mode: FileAccessMode,
    require_existing_path: bool,
    canonicalize_path: bool,
    ttl_seconds: Option<u64>,
}

impl FileAccessBookmarkBuilder {
    /// Create a builder for a bookmark id and path.
    pub fn new(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            scope: PathScope::UserSelected,
            mode: FileAccessMode::ReadWrite,
            require_existing_path: false,
            canonicalize_path: false,
            ttl_seconds: None,
        }
    }

    /// Mark this bookmark as read-only.
    pub fn read_only(mut self) -> Self {
        self.mode = FileAccessMode::Read;
        self
    }

    /// Mark this bookmark as write-only.
    pub fn write_only(mut self) -> Self {
        self.mode = FileAccessMode::Write;
        self
    }

    /// Mark this bookmark as read-write.
    pub fn read_write(mut self) -> Self {
        self.mode = FileAccessMode::ReadWrite;
        self
    }

    /// Set the bookmark scope.
    pub fn scope(mut self, scope: PathScope) -> Self {
        self.scope = scope;
        self
    }

    /// Require the bookmarked path to exist when the bookmark is built.
    pub fn require_existing_path(mut self) -> Self {
        self.require_existing_path = true;
        self
    }

    /// Allow missing paths. This is useful for future save targets.
    pub fn allow_missing_path(mut self) -> Self {
        self.require_existing_path = false;
        self
    }

    /// Canonicalize the path when the bookmark is built.
    pub fn canonicalize_path(mut self) -> Self {
        self.canonicalize_path = true;
        self
    }

    /// Preserve the path exactly as configured.
    pub fn preserve_path(mut self) -> Self {
        self.canonicalize_path = false;
        self
    }

    /// Set a token time-to-live in seconds.
    pub fn ttl_seconds(mut self, seconds: u64) -> Self {
        self.ttl_seconds = Some(seconds);
        self
    }

    /// Build and validate the bookmark.
    pub fn build_checked(self) -> Result<FileAccessBookmark> {
        validate_bookmark_id(&self.id)?;
        validate_bookmark_path(&self.path)?;

        let path = if self.canonicalize_path {
            std::fs::canonicalize(&self.path)
                .map_err(|error| anyhow::anyhow!("failed to canonicalize bookmark path: {error}"))?
        } else {
            self.path
        };

        if self.require_existing_path {
            anyhow::ensure!(
                path.exists(),
                "file access bookmark path must exist: {}",
                path.display()
            );
        }
        if let Some(ttl) = self.ttl_seconds {
            anyhow::ensure!(
                ttl > 0,
                "file access bookmark TTL must be greater than zero"
            );
        }

        let bookmark = FileAccessBookmark {
            id: self.id,
            path,
            scope: self.scope,
            mode: self.mode,
            ttl_seconds: self.ttl_seconds,
        };
        bookmark.validate()?;
        Ok(bookmark)
    }
}

fn validate_bookmark_id(id: &str) -> Result<()> {
    anyhow::ensure!(
        !id.trim().is_empty(),
        "file access bookmark id cannot be empty"
    );
    anyhow::ensure!(
        id == id.trim(),
        "file access bookmark id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        id.len() <= 128,
        "file access bookmark id cannot be longer than 128 bytes"
    );
    anyhow::ensure!(
        id.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':')),
        "file access bookmark id must contain only ASCII letters, numbers, '.', '-', '_' or ':'"
    );
    Ok(())
}

fn validate_bookmark_path(path: &PathBuf) -> Result<()> {
    let path_text = path.to_string_lossy();
    anyhow::ensure!(
        !path_text.trim().is_empty(),
        "file access bookmark path cannot be empty"
    );
    anyhow::ensure!(
        !path_text.chars().any(|ch| ch == '\0'),
        "file access bookmark path cannot contain NUL characters"
    );
    anyhow::ensure!(
        path_text.len() <= MAX_BOOKMARK_PATH_BYTES,
        "file access bookmark path exceeds {MAX_BOOKMARK_PATH_BYTES} bytes"
    );
    anyhow::ensure!(
        path.is_absolute(),
        "file access bookmark path must be absolute: {}",
        path.display()
    );
    Ok(())
}

fn validate_credential_label(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        value == value.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        value.len() <= MAX_CREDENTIAL_LABEL_BYTES,
        "{label} exceeds {MAX_CREDENTIAL_LABEL_BYTES} bytes"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    Ok(())
}

// ===========================================================================
// Phase 11: Plugin Permission Manifests
// ===========================================================================

/// A manifest declaring the permissions a plugin requires and optionally requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionManifest {
    /// Unique identifier for the plugin.
    pub plugin_id: String,
    /// Permissions the plugin must have to function.
    pub required: Vec<PermissionKind>,
    /// Permissions the plugin can use but does not strictly need.
    pub optional: Vec<PermissionKind>,
}

impl PluginPermissionManifest {
    /// Create a new manifest with the given plugin id and no permissions.
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            required: Vec::new(),
            optional: Vec::new(),
        }
    }

    /// Validate that all required permissions are present in the granted set.
    pub fn validate(&self, granted: &HashSet<PermissionKind>) -> Result<(), Vec<PermissionKind>> {
        if self.validate_declaration().is_err() {
            let mut declared = Vec::new();
            for permission in self.required.iter().copied() {
                if !declared.contains(&permission) {
                    declared.push(permission);
                }
            }
            return Err(declared);
        }
        let missing: Vec<PermissionKind> = self
            .required
            .iter()
            .filter(|perm| !granted.contains(perm))
            .copied()
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Validate the plugin identifier and permission declaration shape.
    pub fn validate_declaration(&self) -> Result<()> {
        validate_bookmark_id(&self.plugin_id)
            .map_err(|error| anyhow::anyhow!("invalid plugin permission manifest id: {error}"))?;
        anyhow::ensure!(
            self.required.len() <= 7 && self.optional.len() <= 7,
            "plugin permission manifest cannot contain more than 7 permissions per category"
        );
        let required = self.required.iter().copied().collect::<HashSet<_>>();
        let optional = self.optional.iter().copied().collect::<HashSet<_>>();
        anyhow::ensure!(
            required.len() == self.required.len(),
            "plugin permission manifest contains duplicate required permissions"
        );
        anyhow::ensure!(
            optional.len() == self.optional.len(),
            "plugin permission manifest contains duplicate optional permissions"
        );
        anyhow::ensure!(
            required.is_disjoint(&optional),
            "plugin permission cannot be both required and optional"
        );
        Ok(())
    }

    /// Check whether a specific permission is declared (required or optional).
    pub fn check_permission(&self, kind: PermissionKind) -> bool {
        self.required.contains(&kind) || self.optional.contains(&kind)
    }

    /// Return true if the manifest has at least one required permission.
    pub fn has_required(&self) -> bool {
        !self.required.is_empty()
    }

    /// Return all declared permissions (required + optional), deduplicated.
    pub fn all_permissions(&self) -> Vec<PermissionKind> {
        let mut permissions = Vec::new();
        for permission in self.required.iter().chain(&self.optional).copied() {
            if !permissions.contains(&permission) {
                permissions.push(permission);
            }
        }
        permissions
    }
}

// ===========================================================================
// Phase 11: Process Capability Limits
// ===========================================================================

/// Resource limits that can be applied to a sandboxed process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessLimits {
    /// Maximum memory in bytes the process may allocate.
    pub max_memory_bytes: Option<u64>,
    /// Maximum CPU usage as a percentage (0.0–100.0).
    pub max_cpu_percent: Option<f64>,
    /// Maximum number of open file descriptors.
    pub max_open_files: Option<u32>,
    /// Whether the process is allowed to make network connections.
    pub network_allowed: bool,
}

impl Default for ProcessLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: None,
            max_cpu_percent: None,
            max_open_files: None,
            network_allowed: true,
        }
    }
}

impl ProcessLimits {
    /// Validate configured resource limits before starting a process monitor.
    pub fn validate(&self) -> Result<()> {
        if let Some(max_memory_bytes) = self.max_memory_bytes {
            anyhow::ensure!(
                max_memory_bytes > 0,
                "process memory limit must be positive"
            );
        }
        if let Some(max_cpu_percent) = self.max_cpu_percent {
            anyhow::ensure!(
                max_cpu_percent.is_finite() && (0.0..=100.0).contains(&max_cpu_percent),
                "process CPU limit must be finite and between 0 and 100 percent"
            );
        }
        if let Some(max_open_files) = self.max_open_files {
            anyhow::ensure!(
                max_open_files > 0,
                "process open-file limit must be positive"
            );
        }
        Ok(())
    }
}

/// A process tracked by the capability system, including its limits and
/// any recorded violations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessCapability {
    /// The OS-level process identifier.
    pub pid: u32,
    /// A human-readable name for the process.
    pub name: String,
    /// Resource limits applied to this process.
    pub limits: ProcessLimits,
    /// Recorded violation descriptions.
    pub violations: Vec<String>,
}

impl ProcessCapability {
    /// Create a new process capability tracker with the given limits.
    pub fn new(pid: u32, name: impl Into<String>, limits: ProcessLimits) -> Self {
        Self {
            pid,
            name: name.into(),
            limits,
            violations: Vec::new(),
        }
    }

    /// Create a process capability tracker after validating its identity and limits.
    pub fn try_new(pid: u32, name: impl Into<String>, limits: ProcessLimits) -> Result<Self> {
        let name = name.into();
        anyhow::ensure!(pid > 0, "process id must be greater than zero");
        validate_process_name(&name)?;
        limits.validate()?;
        Ok(Self::new(pid, name, limits))
    }

    /// Validate a process capability record loaded from external state.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.pid > 0, "process id must be greater than zero");
        validate_process_name(&self.name)?;
        self.limits.validate()?;
        anyhow::ensure!(
            self.violations.len() <= MAX_PROCESS_VIOLATIONS,
            "process violation history exceeds {MAX_PROCESS_VIOLATIONS} entries"
        );
        for violation in &self.violations {
            anyhow::ensure!(
                !violation.is_empty() && violation.len() <= MAX_PROCESS_VIOLATION_BYTES,
                "process violation description is empty or too large"
            );
            anyhow::ensure!(
                !violation.chars().any(char::is_control),
                "process violation description contains control characters"
            );
        }
        Ok(())
    }

    /// Record a violation against this process.
    pub fn record_violation(&mut self, description: impl Into<String>) {
        if self.violations.len() >= MAX_PROCESS_VIOLATIONS {
            return;
        }
        let mut description: String = description
            .into()
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect();
        if description.trim().is_empty() {
            description = "unspecified violation".to_string();
        }
        self.violations.push(truncate_security_text(
            description,
            MAX_PROCESS_VIOLATION_BYTES,
        ));
    }

    /// Return the number of violations recorded.
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// Check whether a memory usage value exceeds the configured limit.
    pub fn check_memory(&mut self, used_bytes: u64) -> bool {
        if self.limits.validate().is_err() {
            self.record_violation("invalid process resource limits");
            return false;
        }
        if let Some(max) = self.limits.max_memory_bytes {
            if used_bytes > max {
                self.record_violation(format!("memory limit exceeded: {used_bytes} > {max}"));
                return false;
            }
        }
        true
    }

    /// Check whether a CPU usage value exceeds the configured limit.
    pub fn check_cpu(&mut self, cpu_percent: f64) -> bool {
        if self.limits.validate().is_err() {
            self.record_violation("invalid process resource limits");
            return false;
        }
        if !cpu_percent.is_finite() || cpu_percent < 0.0 {
            self.record_violation("invalid CPU usage measurement");
            return false;
        }
        if let Some(max) = self.limits.max_cpu_percent {
            if cpu_percent > max {
                self.record_violation(format!("CPU limit exceeded: {cpu_percent:.1}% > {max:.1}%"));
                return false;
            }
        }
        true
    }

    /// Check whether an open-file count exceeds the configured limit.
    pub fn check_open_files(&mut self, open_files: u32) -> bool {
        if self.limits.validate().is_err() {
            self.record_violation("invalid process resource limits");
            return false;
        }
        if let Some(max) = self.limits.max_open_files {
            if open_files > max {
                self.record_violation(format!("open-file limit exceeded: {open_files} > {max}"));
                return false;
            }
        }
        true
    }

    /// Check whether a network request is allowed.
    pub fn check_network(&mut self) -> bool {
        if self.limits.validate().is_err() {
            self.record_violation("invalid process resource limits");
            return false;
        }
        if !self.limits.network_allowed {
            self.record_violation("network access denied".to_owned());
            return false;
        }
        true
    }
}

// ===========================================================================
// Phase 11: Network Permission Policy
// ===========================================================================

/// A policy governing which network hosts a process may contact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NetworkPolicy {
    /// All hosts are permitted.
    AllowAll,
    /// No hosts are permitted.
    #[default]
    DenyAll,
    /// Only the listed hosts are permitted.
    AllowList(Vec<String>),
    /// All hosts except the listed ones are permitted.
    DenyList(Vec<String>),
}

impl NetworkPolicy {
    /// Check whether a given host is permitted under this policy.
    pub fn check(&self, host: &str) -> bool {
        if self.validate().is_err() || validate_network_host(host).is_err() {
            return false;
        }
        match self {
            NetworkPolicy::AllowAll => true,
            NetworkPolicy::DenyAll => false,
            NetworkPolicy::AllowList(hosts) => {
                hosts.iter().any(|listed| listed.eq_ignore_ascii_case(host))
            }
            NetworkPolicy::DenyList(hosts) => {
                !hosts.iter().any(|listed| listed.eq_ignore_ascii_case(host))
            }
        }
    }

    /// Check whether a full HTTP(S) URL is permitted under this policy.
    pub fn check_url(&self, url: &str) -> Result<bool> {
        Ok(self.check(&network_url_host(url)?))
    }

    /// Return the hosts explicitly listed by this policy.
    pub fn hosts(&self) -> &[String] {
        match self {
            NetworkPolicy::AllowList(hosts) | NetworkPolicy::DenyList(hosts) => hosts,
            NetworkPolicy::AllowAll | NetworkPolicy::DenyAll => &[],
        }
    }

    /// Validate policy host lists before installing the policy.
    pub fn validate(&self) -> Result<()> {
        match self {
            NetworkPolicy::AllowAll | NetworkPolicy::DenyAll => Ok(()),
            NetworkPolicy::AllowList(hosts) | NetworkPolicy::DenyList(hosts) => {
                validate_network_hosts(hosts)
            }
        }
    }
}

impl kael_net::WebSocketHostPolicy for NetworkPolicy {
    fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    fn allows_host(&self, host: &str) -> bool {
        self.check(host)
    }
}

/// Builder for checked outbound network policies.
#[derive(Debug, Clone, Default)]
pub struct NetworkPolicyBuilder {
    allowed_hosts: Vec<String>,
    denied_hosts: Vec<String>,
    allow_all: bool,
    deny_all: bool,
}

impl NetworkPolicyBuilder {
    /// Create an empty network policy builder. Empty builders produce `DenyAll`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allow every host.
    pub fn allow_all(mut self) -> Self {
        self.allow_all = true;
        self.deny_all = false;
        self.allowed_hosts.clear();
        self.denied_hosts.clear();
        self
    }

    /// Deny every host.
    pub fn deny_all(mut self) -> Self {
        self.deny_all = true;
        self.allow_all = false;
        self.allowed_hosts.clear();
        self.denied_hosts.clear();
        self
    }

    /// Add an allowed host.
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        let host = host.into().to_ascii_lowercase();
        if !self.allowed_hosts.iter().any(|existing| existing == &host) {
            self.allowed_hosts.push(host);
        }
        self
    }

    /// Add multiple allowed hosts.
    pub fn allow_hosts(mut self, hosts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for host in hosts {
            self = self.allow_host(host);
        }
        self
    }

    /// Add an allowed host extracted from a URL.
    pub fn allow_url(mut self, url: &str) -> Result<Self> {
        self = self.allow_host(network_url_host(url)?);
        Ok(self)
    }

    /// Add a denied host.
    pub fn deny_host(mut self, host: impl Into<String>) -> Self {
        let host = host.into().to_ascii_lowercase();
        if !self.denied_hosts.iter().any(|existing| existing == &host) {
            self.denied_hosts.push(host);
        }
        self
    }

    /// Add multiple denied hosts.
    pub fn deny_hosts(mut self, hosts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for host in hosts {
            self = self.deny_host(host);
        }
        self
    }

    /// Add a denied host extracted from a URL.
    pub fn deny_url(mut self, url: &str) -> Result<Self> {
        self = self.deny_host(network_url_host(url)?);
        Ok(self)
    }

    /// Validate and build the policy.
    pub fn build_checked(self) -> Result<NetworkPolicy> {
        anyhow::ensure!(
            !(self.allow_all && self.deny_all),
            "network policy cannot be both allow-all and deny-all"
        );
        anyhow::ensure!(
            self.allowed_hosts.is_empty() || self.denied_hosts.is_empty(),
            "network policy cannot mix allow-list and deny-list hosts"
        );

        if self.allow_all {
            return Ok(NetworkPolicy::AllowAll);
        }
        if self.deny_all || (self.allowed_hosts.is_empty() && self.denied_hosts.is_empty()) {
            return Ok(NetworkPolicy::DenyAll);
        }
        if !self.allowed_hosts.is_empty() {
            validate_network_hosts(&self.allowed_hosts)?;
            return Ok(NetworkPolicy::AllowList(self.allowed_hosts));
        }

        validate_network_hosts(&self.denied_hosts)?;
        Ok(NetworkPolicy::DenyList(self.denied_hosts))
    }
}

/// HTTP method for an app-owned outbound request descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppNetworkMethod {
    /// HTTP GET.
    Get,
    /// HTTP POST.
    Post,
    /// HTTP PUT.
    Put,
    /// HTTP PATCH.
    Patch,
    /// HTTP DELETE.
    Delete,
    /// HTTP HEAD.
    Head,
}

impl AppNetworkMethod {
    /// Stable method string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }

    /// Whether this method normally carries a request body.
    pub fn allows_body(self) -> bool {
        matches!(self, Self::Post | Self::Put | Self::Patch | Self::Delete)
    }
}

/// Checked descriptor for app-owned HTTP requests before handing them to a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppNetworkRequest {
    method: AppNetworkMethod,
    url: String,
    headers: Vec<(String, String)>,
    body_size_bytes: Option<u64>,
    network_policy: Option<NetworkPolicy>,
}

impl AppNetworkRequest {
    /// Create a checked request builder.
    pub fn builder(method: AppNetworkMethod, url: impl Into<String>) -> AppNetworkRequestBuilder {
        AppNetworkRequestBuilder::new(method, url)
    }

    /// HTTP method.
    pub fn method(&self) -> AppNetworkMethod {
        self.method
    }

    /// Request URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Checked headers in insertion order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Optional declared body size.
    pub fn body_size_bytes(&self) -> Option<u64> {
        self.body_size_bytes
    }

    /// Optional outbound network policy.
    pub fn network_policy(&self) -> Option<&NetworkPolicy> {
        self.network_policy.as_ref()
    }

    /// Number of checked headers on this request.
    pub fn header_count(&self) -> usize {
        self.headers.len()
    }

    /// Whether this request declares outbound body bytes.
    pub fn has_body(&self) -> bool {
        self.body_size_bytes.is_some()
    }

    /// Whether a network policy will be checked before sending.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// URL host, normalized to lowercase.
    pub fn host(&self) -> Result<String> {
        network_url_host(&self.url)
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        let url = network_url_summary(&self.url);
        let body = self
            .body_size_bytes
            .map(|bytes| format!("{bytes} bytes"))
            .unwrap_or_else(|| "none".to_string());

        format!(
            "app network request {} {url}, {} headers, body {body}, network policy {}",
            self.method.as_str(),
            self.header_count(),
            if self.has_network_policy() {
                "present"
            } else {
                "none"
            }
        )
    }

    /// Returns a host/body-size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "app network request {}: headers {}, body {}, network policy {}",
            self.method.as_str(),
            self.header_count(),
            self.has_body(),
            self.has_network_policy()
        )
    }

    /// Validate URL, headers, method/body shape, and network policy.
    pub fn validate(&self) -> Result<()> {
        network_url_host(&self.url)?;
        validate_network_headers(&self.headers)?;
        if let Some(size) = self.body_size_bytes {
            anyhow::ensure!(
                size > 0,
                "network request body size must be greater than zero"
            );
            anyhow::ensure!(
                self.method.allows_body(),
                "network request method {} cannot declare a body size",
                self.method.as_str()
            );
            anyhow::ensure!(
                size <= MAX_NETWORK_BODY_BYTES,
                "network request body size exceeds {MAX_NETWORK_BODY_BYTES} bytes"
            );
        }
        if let Some(policy) = &self.network_policy {
            policy.validate()?;
            anyhow::ensure!(
                policy.check_url(&self.url)?,
                "network request URL is denied by network policy"
            );
        }
        Ok(())
    }
}

/// Builder for checked app-owned HTTP request descriptors.
#[derive(Debug, Clone)]
pub struct AppNetworkRequestBuilder {
    method: AppNetworkMethod,
    url: String,
    headers: Vec<(String, String)>,
    body_size_bytes: Option<u64>,
    network_policy: Option<NetworkPolicy>,
}

impl AppNetworkRequestBuilder {
    /// Create a builder from a method and URL.
    pub fn new(method: AppNetworkMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body_size_bytes: None,
            network_policy: None,
        }
    }

    /// Create a GET request builder.
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Get, url)
    }

    /// Create a POST request builder.
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Post, url)
    }

    /// Create a PUT request builder.
    pub fn put(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Put, url)
    }

    /// Create a PATCH request builder.
    pub fn patch(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Patch, url)
    }

    /// Create a DELETE request builder.
    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Delete, url)
    }

    /// Create a HEAD request builder.
    pub fn head(url: impl Into<String>) -> Self {
        Self::new(AppNetworkMethod::Head, url)
    }

    /// Add a checked request header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Add multiple checked request headers.
    pub fn headers(
        mut self,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        for (name, value) in headers {
            self = self.header(name, value);
        }
        self
    }

    /// Declare expected request body size.
    pub fn body_size_bytes(mut self, size: u64) -> Self {
        self.body_size_bytes = Some(size);
        self
    }

    /// Attach an outbound network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// HTTP method configured on this builder.
    pub fn method(&self) -> AppNetworkMethod {
        self.method
    }

    /// Number of configured headers.
    pub fn header_count(&self) -> usize {
        self.headers.len()
    }

    /// Whether this builder declares outbound body bytes.
    pub fn has_body(&self) -> bool {
        self.body_size_bytes.is_some()
    }

    /// Whether a network policy will be checked before sending.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// Validate the planned request without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.as_request().validate()
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        self.as_request().to_text()
    }

    /// Returns a host/body-size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        self.as_request().to_safe_text()
    }

    /// Validate and build the request descriptor.
    pub fn build_checked(self) -> Result<AppNetworkRequest> {
        let request = self.as_request();
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> AppNetworkRequest {
        AppNetworkRequest {
            method: self.method,
            url: self.url.clone(),
            headers: self.headers.clone(),
            body_size_bytes: self.body_size_bytes,
            network_policy: self.network_policy.clone(),
        }
    }
}

/// Long-lived app-owned realtime transport kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppRealtimeConnectionKind {
    /// WebSocket (`ws://` or `wss://`).
    WebSocket,
    /// Server-sent events / EventSource (`http://` or `https://`).
    ServerSentEvents,
}

impl AppRealtimeConnectionKind {
    /// Stable transport key.
    pub fn key(self) -> &'static str {
        match self {
            Self::WebSocket => "websocket",
            Self::ServerSentEvents => "server-sent-events",
        }
    }

    fn allows_scheme(self, scheme: &str) -> bool {
        match self {
            Self::WebSocket => matches!(scheme, "ws" | "wss"),
            Self::ServerSentEvents => matches!(scheme, "http" | "https"),
        }
    }
}

/// Checked reconnect/backoff policy for long-lived realtime transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRealtimeReconnectPolicy {
    max_attempts: u8,
    initial_delay: Duration,
    max_delay: Duration,
}

impl AppRealtimeReconnectPolicy {
    /// Create a reconnect policy with explicit attempts and backoff bounds.
    pub fn new(max_attempts: u8, initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_attempts,
            initial_delay,
            max_delay,
        }
    }

    /// Common conservative reconnect policy for chat, presence, and collaboration.
    pub fn conservative() -> Self {
        Self::new(5, Duration::from_secs(1), Duration::from_secs(30))
    }

    /// Common reconnect policy for critical background sync and notifications.
    pub fn persistent() -> Self {
        Self::new(10, Duration::from_secs(1), Duration::from_secs(60))
    }

    /// Maximum reconnect attempts after the initial connection attempt.
    pub fn max_attempts(&self) -> u8 {
        self.max_attempts
    }

    /// Initial reconnect delay.
    pub fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    /// Maximum reconnect delay.
    pub fn max_delay(&self) -> Duration {
        self.max_delay
    }

    /// Whether reconnect is enabled.
    pub fn reconnects(&self) -> bool {
        self.max_attempts > 0
    }

    /// Validate reconnect bounds before a worker starts its loop.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.max_attempts <= 100,
            "realtime reconnect attempts cannot exceed 100"
        );
        if self.max_attempts == 0 {
            anyhow::ensure!(
                self.initial_delay == Duration::ZERO && self.max_delay == Duration::ZERO,
                "disabled realtime reconnect policy must use zero delays"
            );
            return Ok(());
        }
        anyhow::ensure!(
            self.initial_delay >= Duration::from_millis(100),
            "realtime reconnect initial delay must be at least 100ms"
        );
        anyhow::ensure!(
            self.max_delay >= self.initial_delay,
            "realtime reconnect max delay cannot be less than initial delay"
        );
        anyhow::ensure!(
            self.max_delay <= Duration::from_secs(60 * 60),
            "realtime reconnect max delay cannot exceed 1 hour"
        );
        Ok(())
    }

    /// Return the capped exponential delay for a one-based retry attempt.
    /// Returns `None` when reconnect is disabled or the attempt is outside the
    /// configured retry budget.
    pub fn delay_for_attempt(&self, attempt: u8) -> Option<Duration> {
        if self.validate().is_err() || attempt == 0 || attempt > self.max_attempts {
            return None;
        }
        let mut delay = self.initial_delay;
        for _ in 1..attempt {
            delay = delay.checked_mul(2).unwrap_or(self.max_delay);
            if delay >= self.max_delay {
                return Some(self.max_delay);
            }
        }
        Some(delay.min(self.max_delay))
    }

    /// Content-safe summary for realtime reconnect policy.
    pub fn to_text(&self) -> String {
        format!(
            "realtime reconnect policy: enabled {}, attempts {}, initial delay {}, max delay {}",
            self.reconnects(),
            self.max_attempts,
            self.initial_delay.as_secs_f64(),
            self.max_delay.as_secs_f64()
        )
    }

    /// Timing-safe summary for privacy-sensitive traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "realtime reconnect policy: enabled {}, attempts set {}, initial delay set {}, max delay set {}",
            self.reconnects(),
            self.max_attempts > 0,
            self.initial_delay > Duration::ZERO,
            self.max_delay > Duration::ZERO
        )
    }
}

/// Checked descriptor for app-owned realtime network connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRealtimeConnection {
    kind: AppRealtimeConnectionKind,
    url: String,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
    heartbeat_interval: Option<Duration>,
    max_message_bytes: Option<u64>,
    reconnect_policy: Option<AppRealtimeReconnectPolicy>,
    network_policy: Option<NetworkPolicy>,
}

/// Failure to turn a checked realtime descriptor into a portable live transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AppRealtimeTransportError {
    /// The descriptor itself did not pass its security and bounds checks.
    #[error("invalid realtime connection descriptor")]
    InvalidDescriptor,
    /// Server-sent events remain a descriptor-only boundary in this release.
    #[error("server-sent events do not yet have a portable Kael transport")]
    ServerSentEventsUnsupported,
    /// Browser WebSockets cannot attach application-controlled handshake headers.
    #[error("portable WebSocket transports cannot attach custom handshake headers")]
    CustomHeadersUnsupported,
    /// Browser WebSockets cannot originate protocol ping frames.
    #[error("portable WebSocket transports cannot schedule protocol heartbeat frames")]
    ProtocolHeartbeatUnsupported,
    /// A live transport must have an explicit checked host policy.
    #[error("portable WebSocket transports require an explicit network policy")]
    MissingNetworkPolicy,
    /// The bounded transport configuration could not be built.
    #[error("invalid portable WebSocket transport configuration")]
    InvalidTransportConfig,
    /// The policy or platform rejected transport startup.
    #[error("portable WebSocket transport could not be started")]
    TransportStartRejected,
}

impl AppRealtimeConnection {
    /// Start a WebSocket descriptor.
    pub fn websocket(url: impl Into<String>) -> AppRealtimeConnectionBuilder {
        AppRealtimeConnectionBuilder::websocket(url)
    }

    /// Start a server-sent events descriptor.
    pub fn server_sent_events(url: impl Into<String>) -> AppRealtimeConnectionBuilder {
        AppRealtimeConnectionBuilder::server_sent_events(url)
    }

    /// Realtime transport kind.
    pub fn kind(&self) -> AppRealtimeConnectionKind {
        self.kind
    }

    /// Connection URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// WebSocket subprotocols in preference order.
    pub fn protocols(&self) -> &[String] {
        &self.protocols
    }

    /// Checked headers in insertion order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Optional heartbeat/ping interval expected by the app.
    pub fn heartbeat_interval(&self) -> Option<Duration> {
        self.heartbeat_interval
    }

    /// Optional maximum inbound message/event size.
    pub fn max_message_bytes(&self) -> Option<u64> {
        self.max_message_bytes
    }

    /// Optional reconnect/backoff policy expected by the app worker.
    pub fn reconnect_policy(&self) -> Option<AppRealtimeReconnectPolicy> {
        self.reconnect_policy
    }

    /// Optional outbound network policy.
    pub fn network_policy(&self) -> Option<&NetworkPolicy> {
        self.network_policy.as_ref()
    }

    /// Number of checked headers on this connection.
    pub fn header_count(&self) -> usize {
        self.headers.len()
    }

    /// Number of WebSocket subprotocols on this connection.
    pub fn protocol_count(&self) -> usize {
        self.protocols.len()
    }

    /// Whether this connection declares a heartbeat interval.
    pub fn has_heartbeat_interval(&self) -> bool {
        self.heartbeat_interval.is_some()
    }

    /// Whether this connection declares a maximum inbound message size.
    pub fn has_max_message_bytes(&self) -> bool {
        self.max_message_bytes.is_some()
    }

    /// Whether this connection declares reconnect/backoff behavior.
    pub fn has_reconnect_policy(&self) -> bool {
        self.reconnect_policy.is_some()
    }

    /// Whether a network policy will be checked before connecting.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// URL host, normalized to lowercase.
    pub fn host(&self) -> Result<String> {
        realtime_url_host(&self.url, self.kind)
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        let url = network_url_summary(&self.url);
        let heartbeat = self
            .heartbeat_interval
            .map(|interval| format!("{}s", interval.as_secs()))
            .unwrap_or_else(|| "none".to_string());
        let max_message = self
            .max_message_bytes
            .map(|bytes| format!("{bytes} bytes"))
            .unwrap_or_else(|| "unknown".to_string());
        let reconnect = self
            .reconnect_policy
            .map(|policy| {
                format!(
                    "attempts {}, initial {}s, max {}s",
                    policy.max_attempts(),
                    policy.initial_delay().as_secs_f64(),
                    policy.max_delay().as_secs_f64()
                )
            })
            .unwrap_or_else(|| "none".to_string());

        format!(
            "app realtime {} connection to {url}, {} protocols, {} headers, heartbeat {heartbeat}, max message {max_message}, reconnect {reconnect}, network policy {}",
            self.kind.key(),
            self.protocol_count(),
            self.header_count(),
            if self.has_network_policy() {
                "present"
            } else {
                "none"
            }
        )
    }

    /// Returns a host/timing/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "app realtime {} connection: protocols {}, headers {}, heartbeat {}, max message {}, reconnect {}, network policy {}",
            self.kind.key(),
            self.protocol_count(),
            self.header_count(),
            self.has_heartbeat_interval(),
            self.has_max_message_bytes(),
            self.has_reconnect_policy(),
            self.has_network_policy()
        )
    }

    /// Runtime capability required to open this connection.
    pub fn required_capability(&self) -> Result<Capability> {
        Ok(Capability::Network {
            hosts: vec![self.host()?],
        })
    }

    /// Validate URL, headers, transport options, and network policy.
    pub fn validate(&self) -> Result<()> {
        realtime_url_host(&self.url, self.kind)?;
        validate_network_headers(&self.headers)?;
        validate_realtime_protocols(self.kind, &self.protocols)?;
        if let Some(interval) = self.heartbeat_interval {
            anyhow::ensure!(
                interval >= Duration::from_secs(1),
                "realtime connection heartbeat interval must be at least 1 second"
            );
            anyhow::ensure!(
                interval <= Duration::from_secs(60 * 60),
                "realtime connection heartbeat interval cannot exceed 1 hour"
            );
        }
        if let Some(max_message_bytes) = self.max_message_bytes {
            anyhow::ensure!(
                max_message_bytes > 0,
                "realtime connection max message bytes must be greater than zero"
            );
            anyhow::ensure!(
                max_message_bytes <= 128 * 1024 * 1024,
                "realtime connection max message bytes cannot exceed 134217728"
            );
        }
        if let Some(policy) = self.reconnect_policy {
            policy.validate()?;
        }
        if let Some(policy) = &self.network_policy {
            policy.validate()?;
            anyhow::ensure!(
                policy.check(&self.host()?),
                "realtime connection URL is denied by network policy"
            );
        }
        Ok(())
    }

    /// Build the bounded, cross-platform WebSocket configuration represented by
    /// this checked descriptor.
    ///
    /// Custom handshake headers are rejected because browser WebSockets do not
    /// expose them. This preserves one-codebase behavior instead of silently
    /// applying headers on native only. Protocol heartbeat schedules are also
    /// rejected because browser WebSockets cannot originate ping frames.
    /// Server-sent events remain an explicit typed unsupported boundary.
    pub fn websocket_transport_config(
        &self,
    ) -> std::result::Result<kael_net::WebSocketConfig, AppRealtimeTransportError> {
        self.validate()
            .map_err(|_| AppRealtimeTransportError::InvalidDescriptor)?;
        if self.kind != AppRealtimeConnectionKind::WebSocket {
            return Err(AppRealtimeTransportError::ServerSentEventsUnsupported);
        }
        if !self.headers.is_empty() {
            return Err(AppRealtimeTransportError::CustomHeadersUnsupported);
        }
        if self.heartbeat_interval.is_some() {
            return Err(AppRealtimeTransportError::ProtocolHeartbeatUnsupported);
        }

        let mut builder =
            kael_net::WebSocketConfig::builder(self.url.clone()).protocols(self.protocols.clone());
        if let Some(max_message_bytes) = self.max_message_bytes {
            let max_message_bytes = usize::try_from(max_message_bytes)
                .map_err(|_| AppRealtimeTransportError::InvalidTransportConfig)?;
            let aggregate_bytes = max_message_bytes.saturating_mul(2).min(512 * 1024 * 1024);
            builder = builder
                .max_message_bytes(max_message_bytes)
                .max_inbound_bytes(aggregate_bytes)
                .max_outbound_bytes(aggregate_bytes);
        }
        if let Some(policy) = self.reconnect_policy {
            if policy.reconnects() {
                let reconnect = kael_net::WebSocketReconnectPolicy::new(
                    u16::from(policy.max_attempts()),
                    policy.initial_delay(),
                    policy.max_delay(),
                )
                .map_err(|_| AppRealtimeTransportError::InvalidTransportConfig)?;
                builder = builder.reconnect_policy(reconnect);
            }
        }
        builder
            .build()
            .map_err(|_| AppRealtimeTransportError::InvalidTransportConfig)
    }

    /// Open this descriptor through Kael's real native/browser WebSocket client.
    ///
    /// A descriptor-local [`NetworkPolicy`] is mandatory at the side-effect
    /// boundary even when descriptor validation was performed earlier.
    pub fn open_websocket_transport(
        &self,
    ) -> std::result::Result<kael_net::WebSocketClient, AppRealtimeTransportError> {
        let policy = self
            .network_policy
            .as_ref()
            .ok_or(AppRealtimeTransportError::MissingNetworkPolicy)?;
        let config = self.websocket_transport_config()?;
        kael_net::WebSocketClient::connect(config, policy)
            .map_err(|_| AppRealtimeTransportError::TransportStartRejected)
    }
}

/// Builder for checked app-owned realtime network descriptors.
#[derive(Debug, Clone)]
pub struct AppRealtimeConnectionBuilder {
    kind: AppRealtimeConnectionKind,
    url: String,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
    heartbeat_interval: Option<Duration>,
    max_message_bytes: Option<u64>,
    reconnect_policy: Option<AppRealtimeReconnectPolicy>,
    network_policy: Option<NetworkPolicy>,
}

impl AppRealtimeConnectionBuilder {
    /// Create a builder from a realtime kind and URL.
    pub fn new(kind: AppRealtimeConnectionKind, url: impl Into<String>) -> Self {
        Self {
            kind,
            url: url.into(),
            protocols: Vec::new(),
            headers: Vec::new(),
            heartbeat_interval: None,
            max_message_bytes: None,
            reconnect_policy: None,
            network_policy: None,
        }
    }

    /// Create a WebSocket connection builder.
    pub fn websocket(url: impl Into<String>) -> Self {
        Self::new(AppRealtimeConnectionKind::WebSocket, url)
    }

    /// Create a server-sent events connection builder.
    pub fn server_sent_events(url: impl Into<String>) -> Self {
        Self::new(AppRealtimeConnectionKind::ServerSentEvents, url)
    }

    /// Add a WebSocket subprotocol.
    pub fn protocol(mut self, protocol: impl Into<String>) -> Self {
        self.protocols.push(protocol.into());
        self
    }

    /// Add WebSocket subprotocols.
    pub fn protocols(mut self, protocols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.protocols.extend(protocols.into_iter().map(Into::into));
        self
    }

    /// Add a checked connection header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Add multiple checked connection headers.
    pub fn headers(
        mut self,
        headers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        for (name, value) in headers {
            self = self.header(name, value);
        }
        self
    }

    /// Declare the expected heartbeat/ping interval.
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    /// Declare maximum inbound message/event size.
    pub fn max_message_bytes(mut self, max_message_bytes: u64) -> Self {
        self.max_message_bytes = Some(max_message_bytes);
        self
    }

    /// Attach a reconnect/backoff policy for the app realtime worker.
    pub fn reconnect_policy(mut self, policy: AppRealtimeReconnectPolicy) -> Self {
        self.reconnect_policy = Some(policy);
        self
    }

    /// Use the common conservative reconnect policy.
    pub fn reconnect_conservative(self) -> Self {
        self.reconnect_policy(AppRealtimeReconnectPolicy::conservative())
    }

    /// Use the common persistent reconnect policy.
    pub fn reconnect_persistent(self) -> Self {
        self.reconnect_policy(AppRealtimeReconnectPolicy::persistent())
    }

    /// Disable reconnect behavior explicitly.
    pub fn without_reconnect(mut self) -> Self {
        self.reconnect_policy = None;
        self
    }

    /// Attach an outbound network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Realtime transport kind configured on this builder.
    pub fn kind(&self) -> AppRealtimeConnectionKind {
        self.kind
    }

    /// Number of configured WebSocket subprotocols.
    pub fn protocol_count(&self) -> usize {
        self.protocols.len()
    }

    /// Number of configured headers.
    pub fn header_count(&self) -> usize {
        self.headers.len()
    }

    /// Whether this builder declares a heartbeat interval.
    pub fn has_heartbeat_interval(&self) -> bool {
        self.heartbeat_interval.is_some()
    }

    /// Whether this builder declares a maximum inbound message size.
    pub fn has_max_message_bytes(&self) -> bool {
        self.max_message_bytes.is_some()
    }

    /// Whether this builder declares reconnect/backoff behavior.
    pub fn has_reconnect_policy(&self) -> bool {
        self.reconnect_policy.is_some()
    }

    /// Whether a network policy will be checked before connecting.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// Validate the planned connection without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.as_connection().validate()
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        self.as_connection().to_text()
    }

    /// Returns a host/timing/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        self.as_connection().to_safe_text()
    }

    /// Validate and build the realtime descriptor.
    pub fn build_checked(self) -> Result<AppRealtimeConnection> {
        let connection = self.as_connection();
        connection.validate()?;
        Ok(connection)
    }

    fn as_connection(&self) -> AppRealtimeConnection {
        AppRealtimeConnection {
            kind: self.kind,
            url: self.url.clone(),
            protocols: self.protocols.clone(),
            headers: self.headers.clone(),
            heartbeat_interval: self.heartbeat_interval,
            max_message_bytes: self.max_message_bytes,
            reconnect_policy: self.reconnect_policy,
            network_policy: self.network_policy.clone(),
        }
    }
}

/// A checked group of app-owned realtime connections that should be opened together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRealtimeConnectionSet {
    connections: Vec<AppRealtimeConnection>,
}

impl AppRealtimeConnectionSet {
    /// Create a realtime connection set builder.
    pub fn builder() -> AppRealtimeConnectionSetBuilder {
        AppRealtimeConnectionSetBuilder::new()
    }

    /// Checked connections in declaration order.
    pub fn connections(&self) -> &[AppRealtimeConnection] {
        &self.connections
    }

    /// Consume the set and return its checked connections.
    pub fn into_connections(self) -> Vec<AppRealtimeConnection> {
        self.connections
    }

    /// Number of configured realtime connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Number of WebSocket connections.
    pub fn websocket_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.kind == AppRealtimeConnectionKind::WebSocket)
            .count()
    }

    /// Number of server-sent event connections.
    pub fn server_sent_events_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.kind == AppRealtimeConnectionKind::ServerSentEvents)
            .count()
    }

    /// Total configured header count across connections.
    pub fn header_count(&self) -> usize {
        self.connections
            .iter()
            .map(AppRealtimeConnection::header_count)
            .sum()
    }

    /// Total configured WebSocket subprotocol count across connections.
    pub fn protocol_count(&self) -> usize {
        self.connections
            .iter()
            .map(AppRealtimeConnection::protocol_count)
            .sum()
    }

    /// Number of connections with heartbeat intervals.
    pub fn heartbeat_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.has_heartbeat_interval())
            .count()
    }

    /// Number of connections with inbound message/event budgets.
    pub fn max_message_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.has_max_message_bytes())
            .count()
    }

    /// Number of connections with reconnect/backoff policies.
    pub fn reconnect_policy_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.has_reconnect_policy())
            .count()
    }

    /// Number of connections checked against an outbound network policy.
    pub fn network_policy_count(&self) -> usize {
        self.connections
            .iter()
            .filter(|connection| connection.has_network_policy())
            .count()
    }

    /// Whether the set contains no connections.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Validate every connection in the set.
    pub fn validate(&self) -> Result<()> {
        validate_realtime_connection_set(&self.connections)
    }

    /// Content-safe summary for realtime connection plans.
    pub fn to_text(&self) -> String {
        realtime_connection_set_summary("app realtime connection set", &self.connections)
    }

    /// Host/timing/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "app realtime connection set: connections {}, websockets {}, server sent events {}, protocols {}, headers {}, heartbeats {}, max messages {}, reconnect policies {}, network policies {}",
            self.connection_count(),
            self.websocket_count(),
            self.server_sent_events_count(),
            self.protocol_count(),
            self.header_count(),
            self.heartbeat_count(),
            self.max_message_count(),
            self.reconnect_policy_count(),
            self.network_policy_count()
        )
    }
}

/// Builder for checked app-owned realtime connection sets.
#[derive(Debug, Clone, Default)]
pub struct AppRealtimeConnectionSetBuilder {
    connections: Vec<AppRealtimeConnection>,
}

impl AppRealtimeConnectionSetBuilder {
    /// Create an empty connection set builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a prebuilt checked connection.
    pub fn connection(mut self, connection: AppRealtimeConnection) -> Self {
        self.connections.push(connection);
        self
    }

    /// Add a connection builder after checking it.
    pub fn connection_builder(mut self, connection: AppRealtimeConnectionBuilder) -> Result<Self> {
        self.connections.push(connection.build_checked()?);
        Ok(self)
    }

    /// Add multiple prebuilt checked connections.
    pub fn connections(
        mut self,
        connections: impl IntoIterator<Item = AppRealtimeConnection>,
    ) -> Self {
        self.connections.extend(connections);
        self
    }

    /// Number of configured realtime connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Whether this builder has no configured connections.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Validate the set without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        validate_realtime_connection_set(&self.connections)
    }

    /// Content-safe summary before build.
    pub fn to_text(&self) -> String {
        realtime_connection_set_summary("app realtime connection set builder", &self.connections)
    }

    /// Host/timing/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        AppRealtimeConnectionSet {
            connections: self.connections.clone(),
        }
        .to_safe_text()
    }

    /// Validate and build the connection set.
    pub fn build_checked(self) -> Result<AppRealtimeConnectionSet> {
        validate_realtime_connection_set(&self.connections)?;
        Ok(AppRealtimeConnectionSet {
            connections: self.connections,
        })
    }
}

/// Unit of work covered by a checked network/realtime handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkRealtimeRequest {
    /// Validate an app-owned HTTP request descriptor.
    Request(AppNetworkRequest),
    /// Validate one app-owned realtime connection descriptor.
    RealtimeConnection(AppRealtimeConnection),
    /// Validate a group of app-owned realtime connections.
    RealtimeConnectionSet(AppRealtimeConnectionSet),
    /// Validate an outbound network policy.
    NetworkPolicy(NetworkPolicy),
    /// Route browser-owned fetch/XHR/resource behavior to a hosted surface.
    HostedNetworkBridge {
        /// Hosted surface id.
        surface_id: String,
    },
}

impl NetworkRealtimeRequest {
    /// Validate one network/realtime handoff request.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Request(request) => request.validate(),
            Self::RealtimeConnection(connection) => connection.validate(),
            Self::RealtimeConnectionSet(set) => set.validate(),
            Self::NetworkPolicy(policy) => policy.validate(),
            Self::HostedNetworkBridge { surface_id } => {
                validate_network_bridge_surface_id(surface_id)
            }
        }
    }

    /// Privacy-preserving request kind for summaries.
    pub fn summary_kind(&self) -> &'static str {
        match self {
            Self::Request(_) => "request",
            Self::RealtimeConnection(_) => "realtime-connection",
            Self::RealtimeConnectionSet(_) => "realtime-connection-set",
            Self::NetworkPolicy(_) => "network-policy",
            Self::HostedNetworkBridge { .. } => "hosted-network-bridge",
        }
    }
}

/// Recommended next implementation action for a network/realtime handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkRealtimeNextAction {
    /// Dispatch app-owned HTTP requests through the native HTTP client.
    DispatchNativeRequest,
    /// Open app-owned realtime transports through native workers.
    OpenRealtimeTransport,
    /// Install or evaluate outbound network policy first.
    ApplyNetworkPolicy,
    /// Use hosted WebView network/resource bridges.
    UseHostedNetworkBridge,
}

/// Checked handoff for native HTTP requests, realtime transports, network policy, and hosted fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRealtimeHandoff {
    requests: Vec<NetworkRealtimeRequest>,
}

impl NetworkRealtimeHandoff {
    /// Start building a network/realtime handoff.
    pub fn builder() -> NetworkRealtimeHandoffBuilder {
        NetworkRealtimeHandoffBuilder::new()
    }

    /// Requests covered by this handoff.
    pub fn requests(&self) -> &[NetworkRealtimeRequest] {
        &self.requests
    }

    /// Number of requests in the handoff.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Whether this handoff includes app-owned HTTP requests.
    pub fn has_native_requests(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NetworkRealtimeRequest::Request(_)))
    }

    /// Whether this handoff includes app-owned realtime transports.
    pub fn has_realtime_transports(&self) -> bool {
        self.requests.iter().any(|request| {
            matches!(
                request,
                NetworkRealtimeRequest::RealtimeConnection(_)
                    | NetworkRealtimeRequest::RealtimeConnectionSet(_)
            )
        })
    }

    /// Whether this handoff includes network policy.
    pub fn has_network_policy(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NetworkRealtimeRequest::NetworkPolicy(_)))
    }

    /// Whether this handoff includes a hosted network bridge.
    pub fn has_hosted_network_bridge(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NetworkRealtimeRequest::HostedNetworkBridge { .. }))
    }

    /// First recommended action for a builder or AI agent.
    pub fn next_action(&self) -> NetworkRealtimeNextAction {
        if self.has_native_requests() {
            NetworkRealtimeNextAction::DispatchNativeRequest
        } else if self.has_realtime_transports() {
            NetworkRealtimeNextAction::OpenRealtimeTransport
        } else if self.has_network_policy() {
            NetworkRealtimeNextAction::ApplyNetworkPolicy
        } else {
            NetworkRealtimeNextAction::UseHostedNetworkBridge
        }
    }

    /// Validate all network/realtime handoff requests.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.requests.is_empty(),
            "network/realtime handoff must include at least one request"
        );
        anyhow::ensure!(
            self.requests.len() <= 32,
            "network/realtime handoff cannot include more than 32 requests"
        );
        for request in &self.requests {
            request.validate()?;
        }
        Ok(())
    }

    /// Privacy-preserving summary for logs, tests, and AI-agent traces.
    pub fn to_text(&self) -> String {
        let kinds = self
            .requests
            .iter()
            .map(NetworkRealtimeRequest::summary_kind)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "network/realtime handoff: requests={} next_action={:?} kinds=[{}]",
            self.request_count(),
            self.next_action(),
            kinds
        )
    }
}

/// Builder for checked network/realtime handoffs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkRealtimeHandoffBuilder {
    requests: Vec<NetworkRealtimeRequest>,
}

impl NetworkRealtimeHandoffBuilder {
    /// Create an empty network/realtime handoff builder.
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    /// Add a checked app-owned HTTP request descriptor.
    pub fn request(mut self, request: AppNetworkRequest) -> Self {
        self.requests.push(NetworkRealtimeRequest::Request(request));
        self
    }

    /// Add a request builder after checking it.
    pub fn request_builder(mut self, request: AppNetworkRequestBuilder) -> Result<Self> {
        self.requests
            .push(NetworkRealtimeRequest::Request(request.build_checked()?));
        Ok(self)
    }

    /// Add a checked realtime connection descriptor.
    pub fn realtime_connection(mut self, connection: AppRealtimeConnection) -> Self {
        self.requests
            .push(NetworkRealtimeRequest::RealtimeConnection(connection));
        self
    }

    /// Add a realtime connection builder after checking it.
    pub fn realtime_connection_builder(
        mut self,
        connection: AppRealtimeConnectionBuilder,
    ) -> Result<Self> {
        self.requests
            .push(NetworkRealtimeRequest::RealtimeConnection(
                connection.build_checked()?,
            ));
        Ok(self)
    }

    /// Add a checked realtime connection set.
    pub fn realtime_connection_set(mut self, set: AppRealtimeConnectionSet) -> Self {
        self.requests
            .push(NetworkRealtimeRequest::RealtimeConnectionSet(set));
        self
    }

    /// Add a checked network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.requests
            .push(NetworkRealtimeRequest::NetworkPolicy(policy));
        self
    }

    /// Add a network policy builder after checking it.
    pub fn network_policy_builder(mut self, policy: NetworkPolicyBuilder) -> Result<Self> {
        self.requests.push(NetworkRealtimeRequest::NetworkPolicy(
            policy.build_checked()?,
        ));
        Ok(self)
    }

    /// Add a hosted WebView network/resource bridge request.
    pub fn hosted_network_bridge(mut self, surface_id: impl Into<String>) -> Self {
        self.requests
            .push(NetworkRealtimeRequest::HostedNetworkBridge {
                surface_id: surface_id.into(),
            });
        self
    }

    /// Validate the handoff without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.as_handoff().validate()
    }

    /// Build a checked network/realtime handoff.
    pub fn build_checked(self) -> Result<NetworkRealtimeHandoff> {
        let handoff = self.as_handoff();
        handoff.validate()?;
        Ok(handoff)
    }

    fn as_handoff(&self) -> NetworkRealtimeHandoff {
        NetworkRealtimeHandoff {
            requests: self.requests.clone(),
        }
    }
}

fn network_url_host(url: &str) -> Result<String> {
    validate_network_url_text(url)?;
    let parsed = http_client::Url::parse(url).map_err(|error| anyhow::anyhow!("{error}"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "network policy URL must use http or https: {url}"
    );
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("network policy URL must include a host: {url}"))?;
    validate_network_host(host)?;
    Ok(host.to_ascii_lowercase())
}

fn validate_network_bridge_surface_id(surface_id: &str) -> Result<()> {
    anyhow::ensure!(
        !surface_id.trim().is_empty(),
        "hosted network bridge surface id cannot be empty"
    );
    anyhow::ensure!(
        surface_id == surface_id.trim(),
        "hosted network bridge surface id cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        surface_id.chars().count() <= 64,
        "hosted network bridge surface id cannot be longer than 64 characters"
    );
    anyhow::ensure!(
        surface_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')),
        "hosted network bridge surface id must contain only ASCII letters, digits, '.', '-', or '_'"
    );
    Ok(())
}

fn realtime_url_host(url: &str, kind: AppRealtimeConnectionKind) -> Result<String> {
    validate_network_url_text(url)?;
    let parsed = http_client::Url::parse(url).map_err(|error| anyhow::anyhow!("{error}"))?;
    anyhow::ensure!(
        kind.allows_scheme(parsed.scheme()),
        "realtime connection URL for {} must use {}: {url}",
        kind.key(),
        match kind {
            AppRealtimeConnectionKind::WebSocket => "ws or wss",
            AppRealtimeConnectionKind::ServerSentEvents => "http or https",
        }
    );
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("realtime connection URL must include a host: {url}"))?;
    validate_network_host(host)?;
    Ok(host.to_ascii_lowercase())
}

fn network_url_summary(url: &str) -> String {
    let Ok(parsed) = http_client::Url::parse(url) else {
        return "invalid url".to_string();
    };

    let host = parsed.host_str().unwrap_or("unknown-host");
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();

    format!("{}://{}{}", parsed.scheme(), host, port)
}

fn validate_network_headers(headers: &[(String, String)]) -> Result<()> {
    anyhow::ensure!(
        headers.len() <= MAX_NETWORK_HEADERS,
        "network request cannot contain more than {MAX_NETWORK_HEADERS} headers"
    );
    let mut seen = HashSet::new();
    for (name, value) in headers {
        validate_network_header_name(name)?;
        validate_network_header_value(name, value)?;
        let normalized = name.to_ascii_lowercase();
        anyhow::ensure!(
            seen.insert(normalized.clone()),
            "network request header declared more than once: {normalized}"
        );
    }
    Ok(())
}

fn validate_realtime_protocols(
    kind: AppRealtimeConnectionKind,
    protocols: &[String],
) -> Result<()> {
    anyhow::ensure!(
        kind == AppRealtimeConnectionKind::WebSocket || protocols.is_empty(),
        "server-sent events cannot declare websocket subprotocols"
    );
    anyhow::ensure!(
        protocols.len() <= 16,
        "realtime connection cannot declare more than 16 subprotocols"
    );

    let mut seen = HashSet::new();
    for protocol in protocols {
        anyhow::ensure!(
            !protocol.is_empty(),
            "realtime connection subprotocol cannot be empty"
        );
        anyhow::ensure!(
            protocol.trim() == protocol,
            "realtime connection subprotocol cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            protocol.len() <= 128,
            "realtime connection subprotocol cannot be longer than 128 bytes"
        );
        anyhow::ensure!(
            protocol.bytes().all(is_http_token_byte),
            "realtime connection subprotocol contains invalid characters: {protocol}"
        );
        anyhow::ensure!(
            seen.insert(protocol.to_ascii_lowercase()),
            "realtime connection subprotocol declared more than once: {protocol}"
        );
    }
    Ok(())
}

fn validate_realtime_connection_set(connections: &[AppRealtimeConnection]) -> Result<()> {
    anyhow::ensure!(
        !connections.is_empty(),
        "app realtime connection set must contain at least one connection"
    );
    anyhow::ensure!(
        connections.len() <= MAX_REALTIME_CONNECTIONS,
        "app realtime connection set cannot contain more than {MAX_REALTIME_CONNECTIONS} connections"
    );

    for (index, connection) in connections.iter().enumerate() {
        connection.validate()?;
        anyhow::ensure!(
            !connections[..index].contains(connection),
            "app realtime connection set contains duplicate connections"
        );
    }

    Ok(())
}

fn realtime_connection_set_summary(label: &str, connections: &[AppRealtimeConnection]) -> String {
    let websocket_count = connections
        .iter()
        .filter(|connection| connection.kind == AppRealtimeConnectionKind::WebSocket)
        .count();
    let sse_count = connections
        .iter()
        .filter(|connection| connection.kind == AppRealtimeConnectionKind::ServerSentEvents)
        .count();
    let protocol_count: usize = connections
        .iter()
        .map(AppRealtimeConnection::protocol_count)
        .sum();
    let header_count: usize = connections
        .iter()
        .map(AppRealtimeConnection::header_count)
        .sum();
    let heartbeat_count = connections
        .iter()
        .filter(|connection| connection.has_heartbeat_interval())
        .count();
    let max_message_count = connections
        .iter()
        .filter(|connection| connection.has_max_message_bytes())
        .count();
    let reconnect_policy_count = connections
        .iter()
        .filter(|connection| connection.has_reconnect_policy())
        .count();
    let network_policy_count = connections
        .iter()
        .filter(|connection| connection.has_network_policy())
        .count();

    format!(
        "{label}: connections {}, websockets {}, server sent events {}, protocols {}, headers {}, heartbeats {}, max messages {}, reconnect policies {}, network policies {}",
        connections.len(),
        websocket_count,
        sse_count,
        protocol_count,
        header_count,
        heartbeat_count,
        max_message_count,
        reconnect_policy_count,
        network_policy_count
    )
}

fn validate_network_header_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty(),
        "network request header name cannot be empty"
    );
    anyhow::ensure!(
        name.trim() == name,
        "network request header name cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        name.len() <= MAX_NETWORK_HEADER_NAME_BYTES,
        "network request header name exceeds {MAX_NETWORK_HEADER_NAME_BYTES} bytes"
    );
    anyhow::ensure!(
        name.bytes().all(is_http_token_byte),
        "network request header name contains invalid characters: {name}"
    );
    Ok(())
}

fn validate_network_header_value(name: &str, value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.contains('\r') && !value.contains('\n'),
        "network request header value for {name} cannot contain CR or LF"
    );
    anyhow::ensure!(
        value.len() <= 16 * 1024,
        "network request header value for {name} is too long"
    );
    Ok(())
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn validate_network_hosts(hosts: &[String]) -> Result<()> {
    anyhow::ensure!(
        !hosts.is_empty(),
        "network policy host list cannot be empty"
    );
    anyhow::ensure!(
        hosts.len() <= MAX_NETWORK_POLICY_HOSTS,
        "network policy cannot contain more than {MAX_NETWORK_POLICY_HOSTS} hosts"
    );
    let mut seen = HashSet::new();
    for host in hosts {
        validate_network_host(host)?;
        anyhow::ensure!(
            seen.insert(host.to_ascii_lowercase()),
            "network policy host is duplicated: {host}"
        );
    }
    Ok(())
}

fn validate_network_host(host: &str) -> Result<()> {
    anyhow::ensure!(
        !host.trim().is_empty(),
        "network policy host cannot be empty"
    );
    anyhow::ensure!(
        host == host.trim(),
        "network policy host cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        host.len() <= 253,
        "network policy host cannot be longer than 253 bytes"
    );
    anyhow::ensure!(
        !host.contains('/') && !host.contains('[') && !host.contains(']'),
        "network policy host must not include a URL scheme, path, or IPv6 brackets"
    );
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    anyhow::ensure!(
        !host
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "network policy numeric host must be a valid IP address"
    );
    anyhow::ensure!(
        !host.contains(':') && host.is_ascii(),
        "network policy host must be a valid IP address or ASCII DNS name"
    );
    let labels = host.split('.').collect::<Vec<_>>();
    anyhow::ensure!(
        !labels.is_empty() && labels.iter().all(|label| !label.is_empty()),
        "network policy DNS name cannot contain empty labels"
    );
    for label in labels {
        anyhow::ensure!(
            label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric),
            "network policy DNS label is invalid: {label}"
        );
    }
    Ok(())
}

fn validate_process_name(name: &str) -> Result<()> {
    anyhow::ensure!(!name.trim().is_empty(), "process name cannot be empty");
    anyhow::ensure!(
        name == name.trim(),
        "process name cannot have surrounding whitespace"
    );
    anyhow::ensure!(
        name.len() <= MAX_PROCESS_NAME_BYTES,
        "process name exceeds {MAX_PROCESS_NAME_BYTES} bytes"
    );
    anyhow::ensure!(
        !name.chars().any(char::is_control),
        "process name cannot contain control characters"
    );
    Ok(())
}

fn validate_network_url_text(url: &str) -> Result<()> {
    anyhow::ensure!(!url.trim().is_empty(), "network URL cannot be empty");
    anyhow::ensure!(
        url == url.trim(),
        "network URL cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        url.len() <= MAX_NETWORK_URL_BYTES,
        "network URL exceeds {MAX_NETWORK_URL_BYTES} bytes"
    );
    anyhow::ensure!(
        !url.chars().any(char::is_control),
        "network URL cannot contain control characters"
    );
    Ok(())
}

fn truncate_security_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text
}

// ===========================================================================
// Phase 11: IPC Schema Versioning
// ===========================================================================

/// Version metadata for an IPC message schema, enabling forward/backward
/// compatibility negotiation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcSchema {
    /// The current schema version.
    pub version: u32,
    /// The minimum version this schema is compatible with.
    pub min_compatible: u32,
    /// The message types supported by this schema.
    pub message_types: Vec<String>,
}

impl IpcSchema {
    /// Create a new IPC schema.
    pub fn new(version: u32, min_compatible: u32, message_types: Vec<String>) -> Self {
        Self {
            version,
            min_compatible,
            message_types,
        }
    }

    /// Create and validate an IPC schema.
    pub fn new_checked(
        version: u32,
        min_compatible: u32,
        message_types: Vec<String>,
    ) -> Result<Self> {
        let schema = Self::new(version, min_compatible, message_types);
        schema.validate()?;
        Ok(schema)
    }

    /// Validate the version range and message identifiers.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.version > 0,
            "IPC schema version must be greater than zero"
        );
        anyhow::ensure!(
            self.min_compatible > 0 && self.min_compatible <= self.version,
            "IPC schema minimum compatible version must be between 1 and the current version"
        );
        anyhow::ensure!(
            self.message_types.len() <= MAX_IPC_MESSAGE_TYPES,
            "IPC schema cannot contain more than {MAX_IPC_MESSAGE_TYPES} message types"
        );
        let mut seen = HashSet::new();
        for message_type in &self.message_types {
            anyhow::ensure!(
                !message_type.is_empty() && message_type.len() <= MAX_IPC_MESSAGE_TYPE_BYTES,
                "IPC message type is empty or too long"
            );
            anyhow::ensure!(
                message_type.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':' | b'/')
                }),
                "IPC message type contains invalid characters: {message_type}"
            );
            anyhow::ensure!(
                seen.insert(message_type),
                "IPC schema contains duplicate message type: {message_type}"
            );
        }
        Ok(())
    }

    /// Check whether this schema is compatible with another schema.
    ///
    /// Two schemas are compatible when each schema's version falls within the
    /// other's supported range (i.e. `>= min_compatible`).
    pub fn is_compatible(&self, other: &IpcSchema) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && self.version >= other.min_compatible
            && other.version >= self.min_compatible
    }

    /// Negotiate a common schema version between `self` and `other`.
    ///
    /// Returns the lower of the two versions if compatible, or `None`.
    pub fn negotiate(&self, other: &IpcSchema) -> Option<u32> {
        if self.is_compatible(other) {
            Some(self.version.min(other.version))
        } else {
            None
        }
    }

    /// Return the intersection of message types supported by both schemas.
    pub fn common_message_types(&self, other: &IpcSchema) -> Vec<String> {
        if self.validate().is_err() || other.validate().is_err() {
            return Vec::new();
        }
        let other_set: HashSet<&String> = other.message_types.iter().collect();
        self.message_types
            .iter()
            .filter(|mt| other_set.contains(mt))
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // === Capability & PermissionBroker (existing) ===

    #[test]
    fn test_capability_high_risk() {
        assert!(Capability::ShellExecute.is_high_risk());
        assert!(Capability::ClipboardRead.is_high_risk());
        assert!(
            Capability::Network {
                hosts: vec!["example.com".to_string()]
            }
            .is_high_risk()
        );
        assert!(Capability::Location.is_high_risk());
        assert!(Capability::UsbDevice.is_high_risk());
        assert!(Capability::HidDevice.is_high_risk());
        assert!(Capability::SerialPort.is_high_risk());
        assert!(Capability::Bluetooth.is_high_risk());
        assert!(!Capability::Notification.is_high_risk());
        assert!(!Capability::OpenExternalUrl.is_high_risk());
    }

    #[test]
    fn test_permission_broker_grant_and_check() {
        let mut broker = PermissionBroker::new();
        let process = ProcessId(1);

        assert_eq!(
            broker.check(process, &Capability::OpenExternalUrl),
            PermissionResult::Denied
        );

        broker.grant(process, Capability::OpenExternalUrl);
        assert_eq!(
            broker.check(process, &Capability::OpenExternalUrl),
            PermissionResult::Granted
        );
    }

    #[test]
    fn test_permission_broker_class_defaults() {
        let process = ProcessId(2);
        let utility = ProcessId(3);
        let mut broker = PermissionBroker::new();
        broker.register_process(process, ProcessClass::Worker);
        broker.register_process(utility, ProcessClass::Utility);
        broker.set_default_capabilities(
            ProcessClass::Worker,
            [Capability::FilesystemRead {
                scope: PathScope::AppData,
            }],
        );
        broker.set_default_capabilities(ProcessClass::Utility, [Capability::OpenExternalUrl]);

        assert_eq!(
            broker.check(
                process,
                &Capability::FilesystemRead {
                    scope: PathScope::AppData,
                },
            ),
            PermissionResult::Granted
        );
        assert_eq!(
            broker.check(utility, &Capability::OpenExternalUrl),
            PermissionResult::Granted
        );
    }

    #[test]
    fn test_permission_broker_revoke() {
        let mut broker = PermissionBroker::new();
        let process = ProcessId(1);

        broker.grant(process, Capability::ClipboardRead);
        broker.revoke(process, &Capability::ClipboardRead);

        assert_eq!(
            broker.check(process, &Capability::ClipboardRead),
            PermissionResult::Denied
        );
    }

    #[test]
    fn test_permission_broker_check_any() {
        let mut broker = PermissionBroker::new();
        let process = ProcessId(1);

        broker.grant(process, Capability::Notification);
        assert_eq!(
            broker.check_any(
                process,
                &[Capability::ShellExecute, Capability::Notification]
            ),
            PermissionResult::Granted
        );
        assert_eq!(
            broker.check_any(process, &[Capability::ShellExecute, Capability::Camera]),
            PermissionResult::Denied
        );
    }

    #[test]
    fn test_threat_model_defaults() {
        let model = ThreatModel::new();
        assert!(!model.ui_defaults.is_empty());
        assert!(!model.worker_defaults.is_empty());
        assert!(!model.media_defaults.is_empty());
        assert!(model.extension_defaults.is_empty());

        assert!(model.ui_defaults.contains(&Capability::FilesystemRead {
            scope: PathScope::UserSelected,
        }));
        assert!(model.ui_defaults.contains(&Capability::FilesystemWrite {
            scope: PathScope::UserSelected,
        }));
        assert!(!model.ui_defaults.contains(&Capability::FilesystemRead {
            scope: PathScope::Any,
        }));
        assert!(!model.ui_defaults.contains(&Capability::FilesystemWrite {
            scope: PathScope::Any,
        }));

        let strict = ThreatModel::strict();
        assert!(
            strict
                .ui_defaults
                .iter()
                .chain(&strict.worker_defaults)
                .chain(&strict.utility_defaults)
                .chain(&strict.media_defaults)
                .chain(&strict.extension_defaults)
                .all(|capability| !capability.is_high_risk())
        );
    }

    #[test]
    fn test_threat_model_serialization() {
        let model = ThreatModel::new().add_threat(Threat {
            category: ThreatCategory::UntrustedContent,
            description: "User-generated HTML may contain scripts".to_string(),
            surfaces: vec!["webview".to_string()],
            mitigations: vec!["sanitize input".to_string()],
        });

        let json = serde_json::to_string(&model).unwrap();
        let decoded: ThreatModel = serde_json::from_str(&json).unwrap();
        assert_eq!(model, decoded);
    }

    #[test]
    fn test_permission_broker_apply_threat_model() {
        let process = ProcessId(3);
        let mut broker = PermissionBroker::new();
        broker.register_process(process, ProcessClass::Ui);
        broker.apply_threat_model(&ThreatModel::new());

        assert_eq!(
            broker.check(process, &Capability::Notification),
            PermissionResult::Granted
        );
    }

    #[test]
    fn test_permission_broker_prompt_handler() {
        let process = ProcessId(4);
        let broker = PermissionBroker::new().with_prompt_handler(|_, capability| {
            if capability.is_high_risk() {
                PermissionResult::Prompt
            } else {
                PermissionResult::Denied
            }
        });

        assert_eq!(
            broker.check(process, &Capability::ShellExecute),
            PermissionResult::Prompt
        );
    }

    #[test]
    fn panicking_permission_prompt_handlers_fail_closed() {
        let process = ProcessId(5);
        let broker = PermissionBroker::new().with_prompt_handler(|_, _| panic!("prompt failed"));

        assert_eq!(
            broker.prompt(process, &Capability::Camera),
            PermissionResult::Denied
        );
        assert_eq!(
            broker.check(process, &Capability::Camera),
            PermissionResult::Denied
        );
    }

    // === PermissionManager ===

    #[test]
    fn test_permission_manager_default_not_determined() {
        let mgr = PermissionManager::new();
        assert_eq!(
            mgr.status(PermissionKind::Camera),
            PermissionStatus::NotDetermined
        );
    }

    #[test]
    fn test_permission_manager_request_grants() {
        let mut mgr = PermissionManager::new();
        let req = PermissionRequest {
            kind: PermissionKind::Camera,
            reason: "Video call".to_string(),
        };
        let status = mgr.request(&req, |_| PermissionStatus::Granted);
        assert_eq!(status, PermissionStatus::Granted);
        assert_eq!(
            mgr.status(PermissionKind::Camera),
            PermissionStatus::Granted
        );
    }

    #[test]
    fn test_permission_manager_request_denied() {
        let mut mgr = PermissionManager::new();
        let req = PermissionRequest {
            kind: PermissionKind::Microphone,
            reason: "Audio recording".to_string(),
        };
        let status = mgr.request(&req, |_| PermissionStatus::Denied);
        assert_eq!(status, PermissionStatus::Denied);
    }

    #[test]
    fn test_permission_manager_does_not_re_prompt() {
        let mut mgr = PermissionManager::new();
        mgr.set_status(PermissionKind::Location, PermissionStatus::Denied);
        let req = PermissionRequest {
            kind: PermissionKind::Location,
            reason: "Map".to_string(),
        };
        let status = mgr.request(&req, |_| PermissionStatus::Granted);
        assert_eq!(status, PermissionStatus::Denied);
    }

    #[test]
    fn test_permission_manager_revoke() {
        let mut mgr = PermissionManager::new();
        mgr.set_status(PermissionKind::Network, PermissionStatus::Granted);
        mgr.revoke(PermissionKind::Network);
        assert_eq!(
            mgr.status(PermissionKind::Network),
            PermissionStatus::Denied
        );
    }

    #[test]
    fn test_permission_manager_all_statuses() {
        let mut mgr = PermissionManager::new();
        mgr.set_status(PermissionKind::Camera, PermissionStatus::Granted);
        mgr.set_status(PermissionKind::Microphone, PermissionStatus::Denied);
        assert_eq!(mgr.all_statuses().len(), 2);
    }

    #[test]
    fn test_permission_manager_restricted_status() {
        let mut mgr = PermissionManager::new();
        mgr.set_status(PermissionKind::Camera, PermissionStatus::Restricted);
        assert_eq!(
            mgr.status(PermissionKind::Camera),
            PermissionStatus::Restricted
        );
    }

    #[test]
    fn permission_requests_validate_text_and_contain_decider_panics() {
        let mut manager = PermissionManager::new();
        let invalid = PermissionRequest {
            kind: PermissionKind::Camera,
            reason: " bad\nreason".to_string(),
        };
        assert!(invalid.validate().is_err());
        assert_eq!(
            manager.request(&invalid, |_| PermissionStatus::Granted),
            PermissionStatus::Denied
        );
        assert_eq!(
            manager.status(PermissionKind::Camera),
            PermissionStatus::NotDetermined
        );

        let valid = PermissionRequest {
            kind: PermissionKind::Camera,
            reason: "Join a video call".to_string(),
        };
        assert_eq!(
            manager.request(&valid, |_| panic!("platform prompt failed")),
            PermissionStatus::Denied
        );
        assert_eq!(
            manager.status(PermissionKind::Camera),
            PermissionStatus::Denied
        );
    }

    // === KeychainStore ===

    #[test]
    fn test_keychain_store_and_retrieve() {
        let mut store = KeychainStore::new();
        store
            .store(CredentialEntry {
                service: "github".to_string(),
                account: "user1".to_string(),
                secret: b"token123".to_vec(),
            })
            .unwrap();
        let entry = store.retrieve("github", "user1").unwrap();
        assert_eq!(entry.secret, b"token123");
    }

    #[test]
    fn test_keychain_retrieve_missing() {
        let store = KeychainStore::new();
        assert!(store.retrieve("nope", "nada").is_none());
    }

    #[test]
    fn test_keychain_delete() {
        let mut store = KeychainStore::new();
        store
            .store(CredentialEntry {
                service: "svc".to_string(),
                account: "acct".to_string(),
                secret: vec![1, 2, 3],
            })
            .unwrap();
        assert!(store.delete("svc", "acct"));
        assert!(!store.delete("svc", "acct"));
        assert!(store.is_empty());
    }

    #[test]
    fn test_keychain_list() {
        let mut store = KeychainStore::new();
        store
            .store(CredentialEntry {
                service: "a".to_string(),
                account: "b".to_string(),
                secret: vec![1],
            })
            .unwrap();
        store
            .store(CredentialEntry {
                service: "c".to_string(),
                account: "d".to_string(),
                secret: vec![2],
            })
            .unwrap();
        assert_eq!(store.len(), 2);
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn test_keychain_overwrite() {
        let mut store = KeychainStore::new();
        store
            .store(CredentialEntry {
                service: "svc".to_string(),
                account: "acct".to_string(),
                secret: b"old".to_vec(),
            })
            .unwrap();
        store
            .store(CredentialEntry {
                service: "svc".to_string(),
                account: "acct".to_string(),
                secret: b"new".to_vec(),
            })
            .unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.retrieve("svc", "acct").unwrap().secret, b"new");
    }

    #[test]
    fn credential_validation_and_debug_output_protect_secrets() {
        let entry = CredentialEntry {
            service: "service".to_string(),
            account: "account".to_string(),
            secret: b"super-secret-token".to_vec(),
        };
        assert!(entry.validate().is_ok());
        let debug = format!("{entry:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret-token"));

        let mut store = KeychainStore::new();
        assert!(store.store(entry).is_ok());
        assert!(!format!("{store:?}").contains("super-secret-token"));
        assert!(
            store
                .store(CredentialEntry {
                    service: "service".to_string(),
                    account: "account-2".to_string(),
                    secret: Vec::new(),
                })
                .is_err()
        );
    }

    // === AccessTokenStore ===

    #[test]
    fn test_access_token_issue_and_validate() {
        let mut store = AccessTokenStore::new();
        let token = store
            .issue(PathBuf::from("/tmp/file.txt"), 1000, Some(3600))
            .unwrap();
        assert!(store.validate(&token, 1000).is_some());
        assert_eq!(
            store.validate(&token, 1000).unwrap(),
            &PathBuf::from("/tmp/file.txt")
        );
    }

    #[test]
    fn test_access_token_expired() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 1000, Some(60)).unwrap();
        assert!(store.validate(&token, 1059).is_some());
        assert!(store.validate(&token, 1060).is_none());
    }

    #[test]
    fn test_access_token_no_expiry() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 0, None).unwrap();
        assert!(store.validate(&token, u64::MAX - 1).is_some());
    }

    #[test]
    fn test_access_token_revoke() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 0, None).unwrap();
        assert!(store.revoke(&token));
        assert!(store.validate(&token, 0).is_none());
        assert!(!store.revoke(&token));
    }

    #[test]
    fn test_access_token_list() {
        let mut store = AccessTokenStore::new();
        store.issue(PathBuf::from("/a"), 0, None).unwrap();
        store.issue(PathBuf::from("/b"), 0, None).unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn test_access_token_purge_expired() {
        let mut store = AccessTokenStore::new();
        store.issue(PathBuf::from("/a"), 0, Some(10)).unwrap();
        store.issue(PathBuf::from("/b"), 0, Some(20)).unwrap();
        store.issue(PathBuf::from("/c"), 0, None).unwrap();
        let purged = store.purge_expired(15);
        assert_eq!(purged, 1);
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn access_token_issuance_is_opaque_checked_and_redacted() {
        let mut store = AccessTokenStore::new();
        assert!(
            store
                .issue(PathBuf::from("relative/file"), 0, None)
                .is_err()
        );
        assert!(
            store
                .issue(PathBuf::from("/file"), u64::MAX, Some(1))
                .is_err()
        );
        assert!(store.issue(PathBuf::from("/file"), 0, Some(0)).is_err());

        let token = store
            .issue(PathBuf::from("/private/customer/file"), 10, Some(20))
            .unwrap();
        assert!(token.starts_with("kat_"));
        assert_eq!(token.len(), 36);
        assert!(!token.contains("private"));
        assert!(!token.contains("customer"));
        let entry_debug = format!("{:?}", store.list()[0]);
        assert!(entry_debug.contains("[REDACTED]"));
        assert!(!entry_debug.contains(&token));
        assert!(!entry_debug.contains("customer"));
        assert!(!format!("{store:?}").contains(&token));
    }

    // === PluginPermissionManifest ===

    #[test]
    fn test_plugin_manifest_validate_ok() {
        let manifest = PluginPermissionManifest {
            plugin_id: "my-plugin".to_string(),
            required: vec![PermissionKind::Network, PermissionKind::FileAccess],
            optional: vec![PermissionKind::Notifications],
        };
        let granted: HashSet<PermissionKind> =
            [PermissionKind::Network, PermissionKind::FileAccess]
                .into_iter()
                .collect();
        assert!(manifest.validate(&granted).is_ok());
    }

    #[test]
    fn test_plugin_manifest_validate_missing() {
        let manifest = PluginPermissionManifest {
            plugin_id: "p".to_string(),
            required: vec![PermissionKind::Camera, PermissionKind::Microphone],
            optional: vec![],
        };
        let granted: HashSet<PermissionKind> = [PermissionKind::Camera].into_iter().collect();
        let err = manifest.validate(&granted).unwrap_err();
        assert_eq!(err, vec![PermissionKind::Microphone]);
    }

    #[test]
    fn test_plugin_manifest_check_permission() {
        let manifest = PluginPermissionManifest {
            plugin_id: "p".to_string(),
            required: vec![PermissionKind::Camera],
            optional: vec![PermissionKind::Location],
        };
        assert!(manifest.check_permission(PermissionKind::Camera));
        assert!(manifest.check_permission(PermissionKind::Location));
        assert!(!manifest.check_permission(PermissionKind::Network));
    }

    #[test]
    fn test_plugin_manifest_has_required() {
        let empty = PluginPermissionManifest::new("e");
        assert!(!empty.has_required());

        let with_req = PluginPermissionManifest {
            plugin_id: "p".to_string(),
            required: vec![PermissionKind::Camera],
            optional: vec![],
        };
        assert!(with_req.has_required());
    }

    #[test]
    fn test_plugin_manifest_all_permissions() {
        let manifest = PluginPermissionManifest {
            plugin_id: "p".to_string(),
            required: vec![PermissionKind::Camera],
            optional: vec![PermissionKind::Camera, PermissionKind::Network],
        };
        let all = manifest.all_permissions();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_plugin_manifest_serialization() {
        let manifest = PluginPermissionManifest {
            plugin_id: "test".to_string(),
            required: vec![PermissionKind::Camera],
            optional: vec![PermissionKind::Network],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: PluginPermissionManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn plugin_permission_manifests_reject_ambiguous_declarations() {
        let duplicate = PluginPermissionManifest {
            plugin_id: "plugin".to_string(),
            required: vec![PermissionKind::Camera, PermissionKind::Camera],
            optional: vec![],
        };
        assert!(duplicate.validate_declaration().is_err());
        assert!(duplicate.validate(&HashSet::new()).is_err());

        let overlap = PluginPermissionManifest {
            plugin_id: "plugin".to_string(),
            required: vec![PermissionKind::Camera],
            optional: vec![PermissionKind::Camera],
        };
        assert!(overlap.validate_declaration().is_err());
        assert!(
            PluginPermissionManifest::new("bad plugin")
                .validate_declaration()
                .is_err()
        );

        let ordered = PluginPermissionManifest {
            plugin_id: "plugin".to_string(),
            required: vec![PermissionKind::Network, PermissionKind::Camera],
            optional: vec![PermissionKind::Notifications],
        };
        assert_eq!(
            ordered.all_permissions(),
            vec![
                PermissionKind::Network,
                PermissionKind::Camera,
                PermissionKind::Notifications
            ]
        );
    }

    // === ProcessCapability ===

    #[test]
    fn test_process_capability_memory_ok() {
        let limits = ProcessLimits {
            max_memory_bytes: Some(1024 * 1024),
            ..Default::default()
        };
        let mut cap = ProcessCapability::new(100, "worker", limits);
        assert!(cap.check_memory(512 * 1024));
        assert_eq!(cap.violation_count(), 0);
    }

    #[test]
    fn test_process_capability_memory_exceeded() {
        let limits = ProcessLimits {
            max_memory_bytes: Some(1024),
            ..Default::default()
        };
        let mut cap = ProcessCapability::new(101, "worker", limits);
        assert!(!cap.check_memory(2048));
        assert_eq!(cap.violation_count(), 1);
    }

    #[test]
    fn test_process_capability_cpu_exceeded() {
        let limits = ProcessLimits {
            max_cpu_percent: Some(50.0),
            ..Default::default()
        };
        let mut cap = ProcessCapability::new(102, "renderer", limits);
        assert!(cap.check_cpu(49.9));
        assert!(!cap.check_cpu(75.0));
        assert_eq!(cap.violation_count(), 1);
    }

    #[test]
    fn test_process_capability_network_denied() {
        let limits = ProcessLimits {
            network_allowed: false,
            ..Default::default()
        };
        let mut cap = ProcessCapability::new(103, "sandbox", limits);
        assert!(!cap.check_network());
        assert_eq!(cap.violation_count(), 1);
    }

    #[test]
    fn test_process_capability_network_allowed() {
        let limits = ProcessLimits::default();
        let mut cap = ProcessCapability::new(104, "app", limits);
        assert!(cap.check_network());
        assert_eq!(cap.violation_count(), 0);
    }

    #[test]
    fn test_process_limits_serialization() {
        let limits = ProcessLimits {
            max_memory_bytes: Some(1_000_000),
            max_cpu_percent: Some(80.0),
            max_open_files: Some(256),
            network_allowed: false,
        };
        let json = serde_json::to_string(&limits).unwrap();
        let decoded: ProcessLimits = serde_json::from_str(&json).unwrap();
        assert_eq!(limits, decoded);
    }

    #[test]
    fn process_limits_reject_invalid_numbers_and_enforce_open_files() {
        assert!(
            ProcessLimits {
                max_cpu_percent: Some(f64::NAN),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ProcessLimits {
                max_memory_bytes: Some(0),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(ProcessCapability::try_new(1, " bad", ProcessLimits::default()).is_err());
        assert!(ProcessCapability::try_new(0, "worker", ProcessLimits::default()).is_err());

        let mut capability = ProcessCapability::try_new(
            1,
            "worker",
            ProcessLimits {
                max_cpu_percent: Some(50.0),
                max_open_files: Some(10),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!capability.check_cpu(f64::NAN));
        assert!(!capability.check_cpu(-1.0));
        assert!(capability.check_open_files(10));
        assert!(!capability.check_open_files(11));
        assert!(capability.validate().is_ok());
    }

    #[test]
    fn process_violation_history_is_bounded_and_utf8_safe() {
        let mut capability = ProcessCapability::new(1, "worker", ProcessLimits::default());
        for _ in 0..(MAX_PROCESS_VIOLATIONS + 10) {
            capability.record_violation("🙂".repeat(MAX_PROCESS_VIOLATION_BYTES));
        }
        assert_eq!(capability.violation_count(), MAX_PROCESS_VIOLATIONS);
        assert!(
            capability
                .violations
                .iter()
                .all(|violation| violation.len() <= MAX_PROCESS_VIOLATION_BYTES)
        );
        capability.violations.clear();
        capability.record_violation("bad\nlog\rentry");
        assert_eq!(capability.violations[0], "bad log entry");
        assert!(capability.validate().is_ok());
    }

    // === NetworkPolicy ===

    #[test]
    fn test_network_policy_allow_all() {
        let policy = NetworkPolicy::AllowAll;
        assert!(policy.check("anything.com"));
    }

    #[test]
    fn test_network_policy_deny_all() {
        let policy = NetworkPolicy::DenyAll;
        assert!(!policy.check("anything.com"));
    }

    #[test]
    fn test_network_policy_allow_list() {
        let policy = NetworkPolicy::AllowList(vec!["api.example.com".to_string()]);
        assert!(policy.check("api.example.com"));
        assert!(!policy.check("evil.com"));
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_network_policy_deny_list() {
        let policy = NetworkPolicy::DenyList(vec!["evil.com".to_string()]);
        assert!(!policy.check("evil.com"));
        assert!(policy.check("good.com"));
    }

    #[test]
    fn test_network_policy_default_is_deny_all() {
        let policy = NetworkPolicy::default();
        assert!(!policy.check("anything.com"));
    }

    #[test]
    fn test_network_policy_serialization() {
        let policy = NetworkPolicy::AllowList(vec!["a.com".to_string(), "b.com".to_string()]);
        let json = serde_json::to_string(&policy).unwrap();
        let decoded: NetworkPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, decoded);
    }

    #[test]
    fn network_policy_builder_builds_checked_allow_list() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("api.example.com")
            .allow_url("https://cdn.example.com/assets/app.js")
            .unwrap()
            .build_checked()
            .unwrap();

        assert_eq!(
            policy,
            NetworkPolicy::AllowList(vec![
                "api.example.com".to_string(),
                "cdn.example.com".to_string(),
            ])
        );
        assert!(policy.check_url("https://api.example.com/v1").unwrap());
        assert!(!policy.check_url("https://evil.example.com/v1").unwrap());
    }

    #[test]
    fn network_policy_builder_builds_checked_deny_list() {
        let policy = NetworkPolicyBuilder::new()
            .deny_host("tracking.example.com")
            .deny_url("https://ads.example.com/pixel")
            .unwrap()
            .build_checked()
            .unwrap();

        assert_eq!(
            policy.hosts(),
            &[
                "tracking.example.com".to_string(),
                "ads.example.com".to_string()
            ]
        );
        assert!(!policy.check_url("https://ads.example.com/pixel").unwrap());
        assert!(policy.check_url("https://api.example.com/v1").unwrap());
    }

    #[test]
    fn network_policy_builder_rejects_generated_bad_inputs() {
        assert!(
            NetworkPolicyBuilder::new()
                .allow_host(" https://example.com")
                .build_checked()
                .is_err()
        );
        assert!(
            NetworkPolicyBuilder::new()
                .allow_host("example.com/path")
                .build_checked()
                .is_err()
        );
        assert!(
            NetworkPolicyBuilder::new()
                .allow_host("api.example.com")
                .deny_host("evil.example.com")
                .build_checked()
                .is_err()
        );
        assert!(
            NetworkPolicyBuilder::new()
                .allow_url("file:///tmp/data.json")
                .is_err()
        );
        assert!(
            NetworkPolicy::AllowList(vec![
                "api.example.com".to_string(),
                "API.example.com".to_string()
            ])
            .validate()
            .is_err()
        );
        for invalid in [
            "-bad.example",
            "bad-.example",
            "bad..example",
            "999.999.999.999",
        ] {
            assert!(
                NetworkPolicyBuilder::new()
                    .allow_host(invalid)
                    .build_checked()
                    .is_err(),
                "accepted invalid host {invalid}"
            );
        }
        assert!(
            NetworkPolicyBuilder::new()
                .allow_hosts((0..=MAX_NETWORK_POLICY_HOSTS).map(|index| format!("h{index}.test")))
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn network_policy_checks_are_case_insensitive_ip_aware_and_fail_closed() {
        let allowed = NetworkPolicy::AllowList(vec!["Api.Example.COM".to_string()]);
        assert!(allowed.check("api.example.com"));
        assert!(allowed.check("API.EXAMPLE.COM"));

        let ipv6 = NetworkPolicyBuilder::new()
            .allow_host("2001:db8::1")
            .build_checked()
            .unwrap();
        assert!(ipv6.check("2001:DB8::1"));

        let malformed_deny = NetworkPolicy::DenyList(vec!["bad..host".to_string()]);
        assert!(!malformed_deny.check("good.example"));
        assert!(!NetworkPolicy::AllowAll.check("bad..host"));
    }

    #[test]
    fn app_network_request_builder_validates_checked_requests() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("api.example.com")
            .build_checked()
            .unwrap();

        let builder = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
            .header("Content-Type", "application/json")
            .header("X-Trace-Id", "abc123")
            .body_size_bytes(128)
            .network_policy(policy);

        assert!(builder.validate().is_ok());
        assert_eq!(builder.method(), AppNetworkMethod::Post);
        assert_eq!(builder.header_count(), 2);
        assert!(builder.has_body());
        assert!(builder.has_network_policy());
        assert!(builder.to_text().contains("app network request POST"));
        assert!(!builder.to_text().contains("/v1/sync"));
        assert_eq!(
            builder.to_safe_text(),
            "app network request POST: headers 2, body true, network policy true"
        );

        let request = builder.build_checked().unwrap();

        assert_eq!(request.method(), AppNetworkMethod::Post);
        assert_eq!(request.method().as_str(), "POST");
        assert_eq!(request.url(), "https://api.example.com/v1/sync");
        assert_eq!(request.host().unwrap(), "api.example.com");
        assert_eq!(request.headers().len(), 2);
        assert_eq!(request.header_count(), 2);
        assert_eq!(request.body_size_bytes(), Some(128));
        assert!(request.has_body());
        assert!(request.has_network_policy());
        assert!(request.network_policy().is_some());
    }

    #[test]
    fn app_network_request_summary_is_agent_readable_and_credential_safe() {
        let request = AppNetworkRequestBuilder::post(
            "https://user:secret@api.example.com:8443/v1/sync?token=sensitive#frag",
        )
        .header("Authorization", "Bearer sensitive")
        .header("Cookie", "session=sensitive")
        .body_size_bytes(128)
        .build_checked()
        .unwrap();

        let summary = request.to_text();

        assert!(summary.contains("app network request POST https://api.example.com:8443"));
        assert!(summary.contains("2 headers"));
        assert!(summary.contains("body 128 bytes"));
        assert!(summary.contains("network policy none"));
        assert!(!summary.contains("user"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("sensitive"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("Cookie"));
        assert!(!summary.contains("sync"));
        assert!(!summary.contains("frag"));

        let safe_summary = request.to_safe_text();

        assert_eq!(
            safe_summary,
            "app network request POST: headers 2, body true, network policy false"
        );
        assert!(!safe_summary.contains("api.example.com"));
        assert!(!safe_summary.contains("128"));
        assert!(!safe_summary.contains("Authorization"));
    }

    #[test]
    fn app_network_request_builder_summary_is_available_before_build() {
        let builder = AppNetworkRequestBuilder::put(
            "https://user:secret@api.example.com/private/upload?token=sensitive#frag",
        )
        .header("Authorization", "Bearer sensitive")
        .body_size_bytes(256);

        let summary = builder.to_text();

        assert!(builder.validate().is_ok());
        assert_eq!(builder.method(), AppNetworkMethod::Put);
        assert_eq!(builder.header_count(), 1);
        assert!(builder.has_body());
        assert!(!builder.has_network_policy());
        assert!(summary.contains("app network request PUT https://api.example.com"));
        assert!(summary.contains("1 headers"));
        assert!(summary.contains("body 256 bytes"));
        assert!(summary.contains("network policy none"));
        assert!(!summary.contains("user"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("sensitive"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("upload"));
        assert!(!summary.contains("frag"));

        let safe_summary = builder.to_safe_text();

        assert_eq!(
            safe_summary,
            "app network request PUT: headers 1, body true, network policy false"
        );
        assert!(!safe_summary.contains("api.example.com"));
        assert!(!safe_summary.contains("256"));
        assert!(!safe_summary.contains("Authorization"));
    }

    #[test]
    fn app_network_request_builder_rejects_generated_footguns() {
        assert!(
            AppNetworkRequestBuilder::get("file:///tmp/data.json")
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::get("https://example.com/data.json")
                .header("Bad Header", "value")
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::get("https://example.com/data.json")
                .header("X-Test", "good")
                .header("x-test", "duplicate")
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::get("https://example.com/data.json")
                .header("X-Test", "bad\r\nInjected: yes")
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::get("https://example.com/data.json")
                .body_size_bytes(1)
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::post("https://example.com/data.json")
                .body_size_bytes(0)
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::post("https://example.com/data.json")
                .body_size_bytes(MAX_NETWORK_BODY_BYTES + 1)
                .build_checked()
                .is_err()
        );
        assert!(
            AppNetworkRequestBuilder::get(format!(
                "https://example.com/{}",
                "a".repeat(MAX_NETWORK_URL_BYTES)
            ))
            .build_checked()
            .is_err()
        );
        let mut too_many_headers = AppNetworkRequestBuilder::get("https://example.com/data.json");
        for index in 0..=MAX_NETWORK_HEADERS {
            too_many_headers = too_many_headers.header(format!("X-Test-{index}"), "value");
        }
        assert!(too_many_headers.build_checked().is_err());
        assert!(
            AppNetworkRequestBuilder::get("https://blocked.example.com/data.json")
                .network_policy(
                    NetworkPolicyBuilder::new()
                        .allow_host("api.example.com")
                        .build_checked()
                        .unwrap(),
                )
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn app_realtime_connection_builder_validates_websocket_policy() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("events.example.com")
            .build_checked()
            .unwrap();

        let builder = AppRealtimeConnection::websocket("wss://events.example.com/socket")
            .protocol("kael.v1")
            .header("Authorization", "Bearer token")
            .heartbeat_interval(Duration::from_secs(30))
            .max_message_bytes(65_536)
            .network_policy(policy);

        assert!(builder.validate().is_ok());
        assert_eq!(builder.kind(), AppRealtimeConnectionKind::WebSocket);
        assert_eq!(builder.protocol_count(), 1);
        assert_eq!(builder.header_count(), 1);
        assert!(builder.has_heartbeat_interval());
        assert!(builder.has_max_message_bytes());
        assert!(!builder.has_reconnect_policy());
        assert!(builder.has_network_policy());
        assert!(builder.to_text().contains("app realtime websocket"));
        assert!(!builder.to_text().contains("/socket"));
        assert_eq!(
            builder.to_safe_text(),
            "app realtime websocket connection: protocols 1, headers 1, heartbeat true, max message true, reconnect false, network policy true"
        );

        let connection = builder.build_checked().unwrap();

        assert_eq!(connection.kind(), AppRealtimeConnectionKind::WebSocket);
        assert_eq!(connection.kind().key(), "websocket");
        assert_eq!(connection.url(), "wss://events.example.com/socket");
        assert_eq!(connection.host().unwrap(), "events.example.com");
        assert_eq!(connection.protocols(), &["kael.v1".to_string()]);
        assert_eq!(connection.headers().len(), 1);
        assert_eq!(connection.header_count(), 1);
        assert_eq!(connection.protocol_count(), 1);
        assert_eq!(
            connection.heartbeat_interval(),
            Some(Duration::from_secs(30))
        );
        assert!(connection.has_heartbeat_interval());
        assert_eq!(connection.max_message_bytes(), Some(65_536));
        assert!(connection.has_max_message_bytes());
        assert_eq!(connection.reconnect_policy(), None);
        assert!(!connection.has_reconnect_policy());
        assert_eq!(
            connection.required_capability().unwrap(),
            Capability::Network {
                hosts: vec!["events.example.com".to_string()]
            }
        );
        assert!(connection.has_network_policy());
        assert!(connection.network_policy().is_some());
    }

    #[test]
    fn realtime_descriptor_builds_real_portable_websocket_transport_config() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("127.0.0.1")
            .build_checked()
            .unwrap();
        let connection = AppRealtimeConnection::websocket("ws://127.0.0.1:8128/collab")
            .protocol("kael.collab.v1")
            .max_message_bytes(1_024 * 1_024)
            .reconnect_policy(AppRealtimeReconnectPolicy::conservative())
            .network_policy(policy.clone())
            .build_checked()
            .unwrap();

        let config = connection.websocket_transport_config().unwrap();
        assert_eq!(config.host(), "127.0.0.1");
        assert_eq!(config.protocols(), &["kael.collab.v1"]);
        assert_eq!(config.max_message_bytes(), 1_024 * 1_024);
        assert_eq!(config.max_inbound_bytes(), 2 * 1_024 * 1_024);
        assert_eq!(config.max_outbound_bytes(), 2 * 1_024 * 1_024);
        assert_eq!(config.reconnect_policy().unwrap().max_attempts(), 5);
        assert!(kael_net::WebSocketHostPolicy::is_valid(&policy));
        assert!(kael_net::WebSocketHostPolicy::allows_host(
            &policy,
            config.host()
        ));
    }

    #[test]
    fn realtime_transport_keeps_browser_parity_and_sse_boundaries_typed() {
        let policy = NetworkPolicy::AllowAll;
        let headers = AppRealtimeConnection::websocket("wss://events.example.com/collab")
            .header("Authorization", "Bearer secret")
            .network_policy(policy.clone())
            .build_checked()
            .unwrap();
        assert_eq!(
            headers.websocket_transport_config().unwrap_err(),
            AppRealtimeTransportError::CustomHeadersUnsupported
        );

        let heartbeat = AppRealtimeConnection::websocket("wss://events.example.com/collab")
            .heartbeat_interval(Duration::from_secs(30))
            .network_policy(NetworkPolicy::AllowAll)
            .build_checked()
            .unwrap();
        assert_eq!(
            heartbeat.websocket_transport_config().unwrap_err(),
            AppRealtimeTransportError::ProtocolHeartbeatUnsupported
        );

        let sse =
            AppRealtimeConnection::server_sent_events("https://events.example.com/collab/events")
                .network_policy(policy)
                .build_checked()
                .unwrap();
        assert_eq!(
            sse.websocket_transport_config().unwrap_err(),
            AppRealtimeTransportError::ServerSentEventsUnsupported
        );

        let missing_policy = AppRealtimeConnection::websocket("wss://events.example.com/collab")
            .build_checked()
            .unwrap();
        assert_eq!(
            missing_policy.open_websocket_transport().unwrap_err(),
            AppRealtimeTransportError::MissingNetworkPolicy
        );
    }

    #[test]
    fn app_realtime_connection_summary_is_agent_readable_and_credential_safe() {
        let connection = AppRealtimeConnection::websocket(
            "wss://user:secret@events.example.com:9443/socket?token=sensitive#frag",
        )
        .protocol("kael.v1")
        .header("Authorization", "Bearer sensitive")
        .heartbeat_interval(Duration::from_secs(30))
        .max_message_bytes(65_536)
        .build_checked()
        .unwrap();

        let summary = connection.to_text();

        assert!(
            summary.contains("app realtime websocket connection to wss://events.example.com:9443")
        );
        assert!(summary.contains("1 protocols"));
        assert!(summary.contains("1 headers"));
        assert!(summary.contains("heartbeat 30s"));
        assert!(summary.contains("max message 65536 bytes"));
        assert!(summary.contains("reconnect none"));
        assert!(summary.contains("network policy none"));
        assert!(!summary.contains("user"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("sensitive"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("/socket"));
        assert!(!summary.contains("frag"));

        let safe_summary = connection.to_safe_text();

        assert_eq!(
            safe_summary,
            "app realtime websocket connection: protocols 1, headers 1, heartbeat true, max message true, reconnect false, network policy false"
        );
        assert!(!safe_summary.contains("events.example.com"));
        assert!(!safe_summary.contains("30"));
        assert!(!safe_summary.contains("65536"));
        assert!(!safe_summary.contains("Authorization"));
    }

    #[test]
    fn app_realtime_connection_builder_summary_is_available_before_build() {
        let builder = AppRealtimeConnection::websocket(
            "wss://user:secret@events.example.com/private/socket?token=sensitive#frag",
        )
        .protocol("kael.v1")
        .header("Authorization", "Bearer sensitive")
        .heartbeat_interval(Duration::from_secs(30))
        .max_message_bytes(65_536);

        let summary = builder.to_text();

        assert!(builder.validate().is_ok());
        assert_eq!(builder.kind(), AppRealtimeConnectionKind::WebSocket);
        assert_eq!(builder.protocol_count(), 1);
        assert_eq!(builder.header_count(), 1);
        assert!(builder.has_heartbeat_interval());
        assert!(builder.has_max_message_bytes());
        assert!(!builder.has_reconnect_policy());
        assert!(!builder.has_network_policy());
        assert!(summary.contains("app realtime websocket connection to wss://events.example.com"));
        assert!(summary.contains("1 protocols"));
        assert!(summary.contains("1 headers"));
        assert!(summary.contains("heartbeat 30s"));
        assert!(summary.contains("max message 65536 bytes"));
        assert!(summary.contains("reconnect none"));
        assert!(summary.contains("network policy none"));
        assert!(!summary.contains("user"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("sensitive"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("socket?"));
        assert!(!summary.contains("frag"));

        let safe_summary = builder.to_safe_text();

        assert_eq!(
            safe_summary,
            "app realtime websocket connection: protocols 1, headers 1, heartbeat true, max message true, reconnect false, network policy false"
        );
        assert!(!safe_summary.contains("events.example.com"));
        assert!(!safe_summary.contains("30"));
        assert!(!safe_summary.contains("65536"));
        assert!(!safe_summary.contains("Authorization"));
    }

    #[test]
    fn app_realtime_reconnect_policy_validates_and_summarizes() {
        let conservative = AppRealtimeReconnectPolicy::conservative();
        assert!(conservative.validate().is_ok());
        assert_eq!(conservative.max_attempts(), 5);
        assert_eq!(conservative.initial_delay(), Duration::from_secs(1));
        assert_eq!(conservative.max_delay(), Duration::from_secs(30));
        assert!(conservative.reconnects());
        assert_eq!(
            conservative.to_text(),
            "realtime reconnect policy: enabled true, attempts 5, initial delay 1, max delay 30"
        );
        assert_eq!(
            conservative.to_safe_text(),
            "realtime reconnect policy: enabled true, attempts set true, initial delay set true, max delay set true"
        );

        let persistent = AppRealtimeReconnectPolicy::persistent();
        assert!(persistent.validate().is_ok());
        assert_eq!(persistent.max_attempts(), 10);
        assert_eq!(persistent.max_delay(), Duration::from_secs(60));
        assert_eq!(
            persistent.delay_for_attempt(1),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            persistent.delay_for_attempt(2),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            persistent.delay_for_attempt(7),
            Some(Duration::from_secs(60))
        );
        assert_eq!(persistent.delay_for_attempt(11), None);
        assert_eq!(persistent.delay_for_attempt(0), None);

        let disabled = AppRealtimeReconnectPolicy::new(0, Duration::ZERO, Duration::ZERO);
        assert!(disabled.validate().is_ok());
        assert!(!disabled.reconnects());
        assert_eq!(disabled.delay_for_attempt(1), None);
    }

    #[test]
    fn app_realtime_reconnect_policy_rejects_generated_footguns() {
        assert!(
            AppRealtimeReconnectPolicy::new(101, Duration::from_secs(1), Duration::from_secs(30))
                .validate()
                .is_err()
        );
        assert!(
            AppRealtimeReconnectPolicy::new(0, Duration::from_secs(1), Duration::from_secs(1))
                .validate()
                .is_err()
        );
        assert!(
            AppRealtimeReconnectPolicy::new(1, Duration::from_millis(99), Duration::from_secs(30))
                .validate()
                .is_err()
        );
        assert!(
            AppRealtimeReconnectPolicy::new(1, Duration::from_secs(30), Duration::from_secs(1))
                .validate()
                .is_err()
        );
        assert!(
            AppRealtimeReconnectPolicy::new(
                1,
                Duration::from_secs(1),
                Duration::from_secs(60 * 60 + 1)
            )
            .validate()
            .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .reconnect_policy(AppRealtimeReconnectPolicy::new(
                    1,
                    Duration::from_secs(30),
                    Duration::from_secs(1)
                ))
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn app_realtime_reconnect_policy_summary_is_content_safe() {
        let connection = AppRealtimeConnection::websocket(
            "wss://user:secret@events.example.com/socket?token=sensitive",
        )
        .header("Authorization", "Bearer sensitive")
        .reconnect_policy(AppRealtimeReconnectPolicy::new(
            3,
            Duration::from_millis(250),
            Duration::from_secs(5),
        ))
        .build_checked()
        .unwrap();

        assert!(connection.has_reconnect_policy());
        assert_eq!(
            connection.reconnect_policy().unwrap().to_text(),
            "realtime reconnect policy: enabled true, attempts 3, initial delay 0.25, max delay 5"
        );
        assert!(connection.to_text().contains("reconnect attempts 3"));
        assert!(!connection.to_text().contains("secret"));
        assert!(!connection.to_text().contains("token"));
        assert!(!connection.to_text().contains("Authorization"));

        let safe = connection.to_safe_text();
        assert!(safe.contains("reconnect true"));
        assert!(!safe.contains("events.example.com"));
        assert!(!safe.contains("0.25"));
        assert!(!safe.contains("5"));
    }

    #[test]
    fn app_realtime_connection_builder_validates_server_sent_events() {
        let connection =
            AppRealtimeConnection::server_sent_events("https://events.example.com/stream")
                .header("Accept", "text/event-stream")
                .build_checked()
                .unwrap();

        assert_eq!(
            connection.kind(),
            AppRealtimeConnectionKind::ServerSentEvents
        );
        assert_eq!(connection.kind().key(), "server-sent-events");
        assert_eq!(connection.host().unwrap(), "events.example.com");
        assert!(connection.protocols().is_empty());
    }

    #[test]
    fn app_realtime_connection_set_validates_and_summarizes_plan() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("events.example.com")
            .allow_host("stream.example.com")
            .build_checked()
            .unwrap();
        let set = AppRealtimeConnectionSet::builder()
            .connection_builder(
                AppRealtimeConnection::websocket(
                    "wss://user:secret@events.example.com/private/socket?token=sensitive",
                )
                .protocol("kael.v1")
                .header("Authorization", "Bearer sensitive")
                .heartbeat_interval(Duration::from_secs(30))
                .max_message_bytes(65_536)
                .reconnect_conservative()
                .network_policy(policy.clone()),
            )
            .unwrap()
            .connection_builder(
                AppRealtimeConnection::server_sent_events(
                    "https://stream.example.com/events?cursor=secret",
                )
                .header("Accept", "text/event-stream")
                .reconnect_persistent()
                .network_policy(policy),
            )
            .unwrap();

        assert_eq!(set.connection_count(), 2);
        assert!(!set.is_empty());
        assert!(set.validate().is_ok());

        let summary = set.to_text();
        assert!(summary.contains("app realtime connection set builder"));
        assert!(summary.contains("connections 2"));
        assert!(summary.contains("websockets 1"));
        assert!(summary.contains("server sent events 1"));
        assert!(summary.contains("protocols 1"));
        assert!(summary.contains("headers 2"));
        assert!(summary.contains("heartbeats 1"));
        assert!(summary.contains("max messages 1"));
        assert!(summary.contains("reconnect policies 2"));
        assert!(summary.contains("network policies 2"));
        assert!(!summary.contains("events.example.com"));
        assert!(!summary.contains("stream.example.com"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("65536"));
        assert!(!summary.contains("30"));

        let set = set.build_checked().unwrap();
        assert_eq!(set.connection_count(), 2);
        assert_eq!(set.websocket_count(), 1);
        assert_eq!(set.server_sent_events_count(), 1);
        assert_eq!(set.protocol_count(), 1);
        assert_eq!(set.header_count(), 2);
        assert_eq!(set.heartbeat_count(), 1);
        assert_eq!(set.max_message_count(), 1);
        assert_eq!(set.reconnect_policy_count(), 2);
        assert_eq!(set.network_policy_count(), 2);
        assert_eq!(set.connections().len(), 2);

        let safe_summary = set.to_safe_text();
        assert_eq!(
            safe_summary,
            "app realtime connection set: connections 2, websockets 1, server sent events 1, protocols 1, headers 2, heartbeats 1, max messages 1, reconnect policies 2, network policies 2"
        );
        assert!(!safe_summary.contains("events.example.com"));
        assert!(!safe_summary.contains("stream.example.com"));
        assert!(!safe_summary.contains("Authorization"));
        assert!(!safe_summary.contains("65536"));
    }

    #[test]
    fn app_realtime_connection_set_rejects_empty_and_duplicates() {
        assert!(AppRealtimeConnectionSet::builder().build_checked().is_err());

        let connection = AppRealtimeConnection::websocket("wss://events.example.com/socket")
            .protocol("kael.v1")
            .build_checked()
            .unwrap();

        assert!(
            AppRealtimeConnectionSet::builder()
                .connection(connection.clone())
                .connection(connection)
                .build_checked()
                .is_err()
        );

        let connections = (0..=MAX_REALTIME_CONNECTIONS)
            .map(|index| {
                AppRealtimeConnection::websocket(format!("wss://events.example.com/socket/{index}"))
                    .build_checked()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(
            AppRealtimeConnectionSet::builder()
                .connections(connections)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn network_realtime_handoff_builder_validates_native_network_surface() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("api.example.com")
            .allow_host("events.example.com")
            .build_checked()
            .unwrap();
        let request = AppNetworkRequestBuilder::post(
            "https://user:secret@api.example.com/v1/sync?token=sensitive",
        )
        .header("Authorization", "Bearer sensitive")
        .body_size_bytes(128)
        .network_policy(policy.clone())
        .build_checked()
        .unwrap();
        let connection = AppRealtimeConnection::websocket(
            "wss://user:secret@events.example.com/socket?token=sensitive",
        )
        .protocol("kael.v1")
        .header("Authorization", "Bearer sensitive")
        .reconnect_conservative()
        .network_policy(policy.clone())
        .build_checked()
        .unwrap();
        let set = AppRealtimeConnectionSet::builder()
            .connection(connection.clone())
            .build_checked()
            .unwrap();

        let handoff = NetworkRealtimeHandoff::builder()
            .request(request)
            .realtime_connection(connection)
            .realtime_connection_set(set)
            .network_policy(policy)
            .hosted_network_bridge("checkout")
            .build_checked()
            .unwrap();

        assert_eq!(handoff.request_count(), 5);
        assert!(handoff.has_native_requests());
        assert!(handoff.has_realtime_transports());
        assert!(handoff.has_network_policy());
        assert!(handoff.has_hosted_network_bridge());
        assert_eq!(
            handoff.next_action(),
            NetworkRealtimeNextAction::DispatchNativeRequest
        );

        let summary = handoff.to_text();
        assert!(summary.contains("network/realtime handoff"));
        assert!(summary.contains("hosted-network-bridge"));
        assert!(!summary.contains("api.example.com"));
        assert!(!summary.contains("events.example.com"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("Authorization"));
        assert!(!summary.contains("checkout"));
    }

    #[test]
    fn network_realtime_handoff_builder_rejects_unsafe_shapes() {
        assert!(NetworkRealtimeHandoffBuilder::new().validate().is_err());
        assert!(
            NetworkRealtimeHandoffBuilder::new()
                .hosted_network_bridge("bad surface")
                .validate()
                .is_err()
        );
        assert!(
            NetworkRealtimeHandoffBuilder::new()
                .request_builder(AppNetworkRequestBuilder::get("file:///tmp/data.json"))
                .is_err()
        );
        assert!(
            NetworkRealtimeHandoffBuilder::new()
                .realtime_connection_builder(AppRealtimeConnection::websocket(
                    "https://events.example.com/socket",
                ))
                .is_err()
        );
    }

    #[test]
    fn network_realtime_handoff_next_action_prioritizes_realtime_policy_and_hosted() {
        let realtime = NetworkRealtimeHandoffBuilder::new()
            .realtime_connection_builder(AppRealtimeConnection::websocket(
                "wss://events.example.com/socket",
            ))
            .unwrap()
            .build_checked()
            .unwrap();
        assert_eq!(
            realtime.next_action(),
            NetworkRealtimeNextAction::OpenRealtimeTransport
        );

        let policy = NetworkRealtimeHandoffBuilder::new()
            .network_policy_builder(NetworkPolicyBuilder::new().allow_host("api.example.com"))
            .unwrap()
            .build_checked()
            .unwrap();
        assert_eq!(
            policy.next_action(),
            NetworkRealtimeNextAction::ApplyNetworkPolicy
        );

        let hosted = NetworkRealtimeHandoffBuilder::new()
            .hosted_network_bridge("checkout")
            .build_checked()
            .unwrap();
        assert_eq!(
            hosted.next_action(),
            NetworkRealtimeNextAction::UseHostedNetworkBridge
        );
    }

    #[test]
    fn app_realtime_connection_builder_rejects_generated_footguns() {
        assert!(
            AppRealtimeConnection::websocket("https://events.example.com/socket")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::server_sent_events("wss://events.example.com/stream")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::server_sent_events("https://events.example.com/stream")
                .protocol("kael.v1")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .protocol("bad protocol")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .protocol("kael.v1")
                .protocol("KAEL.V1")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .header("X-Test", "one")
                .header("x-test", "two")
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .heartbeat_interval(Duration::ZERO)
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .heartbeat_interval(Duration::from_secs(60 * 60 + 1))
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .max_message_bytes(0)
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://events.example.com/socket")
                .max_message_bytes(128 * 1024 * 1024 + 1)
                .build_checked()
                .is_err()
        );
        assert!(
            AppRealtimeConnection::websocket("wss://blocked.example.com/socket")
                .network_policy(
                    NetworkPolicyBuilder::new()
                        .allow_host("events.example.com")
                        .build_checked()
                        .unwrap(),
                )
                .build_checked()
                .is_err()
        );
    }

    // === IpcSchema ===

    #[test]
    fn test_ipc_schema_compatible() {
        let a = IpcSchema::new(3, 1, vec!["ping".to_string()]);
        let b = IpcSchema::new(2, 1, vec!["pong".to_string()]);
        assert!(a.is_compatible(&b));
        assert!(b.is_compatible(&a));
    }

    #[test]
    fn test_ipc_schema_incompatible() {
        let a = IpcSchema::new(3, 3, vec![]);
        let b = IpcSchema::new(1, 1, vec![]);
        assert!(!a.is_compatible(&b));
    }

    #[test]
    fn test_ipc_schema_negotiate() {
        let a = IpcSchema::new(5, 2, vec![]);
        let b = IpcSchema::new(3, 1, vec![]);
        assert_eq!(a.negotiate(&b), Some(3));
    }

    #[test]
    fn test_ipc_schema_negotiate_incompatible() {
        let a = IpcSchema::new(5, 4, vec![]);
        let b = IpcSchema::new(3, 1, vec![]);
        assert_eq!(a.negotiate(&b), None);
    }

    #[test]
    fn test_ipc_schema_common_message_types() {
        let a = IpcSchema::new(1, 1, vec!["ping".to_string(), "data".to_string()]);
        let b = IpcSchema::new(1, 1, vec!["data".to_string(), "pong".to_string()]);
        let common = a.common_message_types(&b);
        assert_eq!(common, vec!["data".to_string()]);
    }

    #[test]
    fn test_ipc_schema_serialization() {
        let schema = IpcSchema::new(2, 1, vec!["hello".to_string()]);
        let json = serde_json::to_string(&schema).unwrap();
        let decoded: IpcSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, decoded);
    }

    #[test]
    fn test_ipc_schema_self_compatible() {
        let schema = IpcSchema::new(1, 1, vec!["msg".to_string()]);
        assert!(schema.is_compatible(&schema));
        assert_eq!(schema.negotiate(&schema), Some(1));
    }

    #[test]
    fn ipc_schema_validation_bounds_negotiation_and_identifiers() {
        assert!(IpcSchema::new_checked(3, 1, vec!["worker.progress/v1".to_string()]).is_ok());
        for schema in [
            IpcSchema::new(0, 0, vec![]),
            IpcSchema::new(1, 2, vec![]),
            IpcSchema::new(1, 1, vec!["bad message".to_string()]),
            IpcSchema::new(1, 1, vec!["ping".to_string(), "ping".to_string()]),
        ] {
            assert!(schema.validate().is_err());
            assert!(!schema.is_compatible(&IpcSchema::new(1, 1, vec![])));
            assert_eq!(schema.negotiate(&IpcSchema::new(1, 1, vec![])), None);
            assert!(
                schema
                    .common_message_types(&IpcSchema::new(1, 1, vec![]))
                    .is_empty()
            );
        }

        assert!(
            IpcSchema::new(
                1,
                1,
                (0..=MAX_IPC_MESSAGE_TYPES)
                    .map(|index| format!("message.{index}"))
                    .collect(),
            )
            .validate()
            .is_err()
        );
    }

    // === Additional edge-case tests ===

    #[test]
    fn test_permission_broker_unregister() {
        let mut broker = PermissionBroker::new();
        let pid = ProcessId(10);
        broker.register_process(pid, ProcessClass::Extension);
        broker.grant(pid, Capability::Notification);
        broker.unregister_process(pid);
        assert_eq!(
            broker.check(pid, &Capability::Notification),
            PermissionResult::Denied
        );
    }

    #[test]
    fn test_permission_broker_revoke_all() {
        let mut broker = PermissionBroker::new();
        let pid = ProcessId(11);
        broker.grant(pid, Capability::Camera);
        broker.grant(pid, Capability::Microphone);
        broker.revoke_all(pid);
        assert!(broker.capabilities(pid).is_empty());
    }

    #[test]
    fn test_credential_entry_clone() {
        let entry = CredentialEntry {
            service: "s".to_string(),
            account: "a".to_string(),
            secret: vec![42],
        };
        let cloned = entry.clone();
        assert_eq!(cloned.secret, vec![42]);
    }

    #[test]
    fn test_access_token_unique_ids() {
        let mut store = AccessTokenStore::new();
        let t1 = store.issue(PathBuf::from("/a"), 0, None).unwrap();
        let t2 = store.issue(PathBuf::from("/a"), 0, None).unwrap();
        assert_ne!(t1, t2);
    }

    #[test]
    fn file_access_bookmark_builder_validates_generated_inputs() {
        assert!(
            FileAccessBookmark::builder("project.main", "/tmp/project")
                .read_only()
                .ttl_seconds(60)
                .build_checked()
                .is_ok()
        );
        assert!(
            FileAccessBookmark::builder("", "/tmp/project")
                .build_checked()
                .is_err()
        );
        assert!(
            FileAccessBookmark::builder("bad id", "/tmp/project")
                .build_checked()
                .is_err()
        );
        assert!(
            FileAccessBookmark::builder("project", "")
                .build_checked()
                .is_err()
        );
        assert!(
            FileAccessBookmark::builder("project", "/tmp/project")
                .ttl_seconds(0)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn file_access_bookmark_maps_capabilities_and_issues_tokens() {
        let bookmark = FileAccessBookmark::builder("project.main", "/tmp/project")
            .read_write()
            .scope(PathScope::UserSelected)
            .ttl_seconds(10)
            .build_checked()
            .unwrap();

        assert_eq!(
            bookmark.capabilities(),
            vec![
                Capability::FilesystemRead {
                    scope: PathScope::UserSelected
                },
                Capability::FilesystemWrite {
                    scope: PathScope::UserSelected
                },
            ]
        );

        let mut store = AccessTokenStore::new();
        let token = bookmark.issue_token(&mut store, 100).unwrap();
        assert_eq!(
            store.validate(&token, 105),
            Some(&PathBuf::from("/tmp/project"))
        );
        assert_eq!(store.validate(&token, 110), None);
    }

    #[test]
    fn file_access_bookmark_requires_existing_and_canonicalizes_paths() {
        let dir =
            std::env::temp_dir().join(format!("kael-file-bookmark-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("document.txt");
        std::fs::write(&file, "hello").unwrap();

        let bookmark = FileAccessBookmark::builder("doc", &file)
            .require_existing_path()
            .canonicalize_path()
            .read_only()
            .build_checked()
            .unwrap();

        assert_eq!(bookmark.path, file.canonicalize().unwrap());
        assert_eq!(
            bookmark.capabilities(),
            vec![Capability::FilesystemRead {
                scope: PathScope::UserSelected
            }]
        );

        assert!(
            FileAccessBookmark::builder("missing", dir.join("missing.txt"))
                .require_existing_path()
                .build_checked()
                .is_err()
        );
        std::fs::remove_file(&file).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn test_process_capability_multiple_violations() {
        let limits = ProcessLimits {
            max_memory_bytes: Some(100),
            max_cpu_percent: Some(10.0),
            network_allowed: false,
            ..Default::default()
        };
        let mut cap = ProcessCapability::new(200, "test", limits);
        cap.check_memory(200);
        cap.check_cpu(50.0);
        cap.check_network();
        assert_eq!(cap.violation_count(), 3);
    }
}
