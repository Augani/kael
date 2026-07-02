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
            handler(process, capability)
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
            return prompt_handler(process, capability);
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
        let result = decide(request);
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    /// The service or application the credential belongs to.
    pub service: String,
    /// The account or username.
    pub account: String,
    /// The secret data (password, token, etc.).
    pub secret: Vec<u8>,
}

/// An in-memory credential store that mimics system keychain semantics.
///
/// In production, callers should back this with a platform keychain
/// (macOS Keychain, Windows Credential Manager, libsecret on Linux).
#[derive(Debug, Clone, Default)]
pub struct KeychainStore {
    entries: HashMap<(String, String), CredentialEntry>,
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
    pub fn store(&mut self, entry: CredentialEntry) {
        let key = (entry.service.clone(), entry.account.clone());
        self.entries.insert(key, entry);
    }

    /// Retrieve a credential by service and account.
    pub fn retrieve(&self, service: &str, account: &str) -> Option<&CredentialEntry> {
        self.entries.get(&(service.to_owned(), account.to_owned()))
    }

    /// Delete a credential by service and account. Returns `true` if removed.
    pub fn delete(&mut self, service: &str, account: &str) -> bool {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Manages file-scoped access tokens with optional expiry.
#[derive(Debug, Clone, Default)]
pub struct AccessTokenStore {
    tokens: HashMap<String, AccessToken>,
    next_id: u64,
}

impl AccessTokenStore {
    /// Create an empty token store.
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
            next_id: 0,
        }
    }

    /// Issue a new access token for the given path. Returns the token string.
    pub fn issue(&mut self, path: PathBuf, now: u64, ttl_seconds: Option<u64>) -> String {
        self.next_id += 1;
        let token_str = format!(
            "kat_{:016x}_{}",
            self.next_id,
            seahash::hash(path.to_string_lossy().as_bytes())
        );
        let access_token = AccessToken {
            path,
            token: token_str.clone(),
            created_at: now,
            expires_at: ttl_seconds.map(|ttl| now + ttl),
        };
        self.tokens.insert(token_str.clone(), access_token);
        token_str
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
        Ok(store.issue(self.path.clone(), now, self.ttl_seconds))
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
        let mut set = HashSet::new();
        for perm in &self.required {
            set.insert(*perm);
        }
        for perm in &self.optional {
            set.insert(*perm);
        }
        set.into_iter().collect()
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

    /// Record a violation against this process.
    pub fn record_violation(&mut self, description: impl Into<String>) {
        self.violations.push(description.into());
    }

    /// Return the number of violations recorded.
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// Check whether a memory usage value exceeds the configured limit.
    pub fn check_memory(&mut self, used_bytes: u64) -> bool {
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
        if let Some(max) = self.limits.max_cpu_percent {
            if cpu_percent > max {
                self.record_violation(format!("CPU limit exceeded: {cpu_percent:.1}% > {max:.1}%"));
                return false;
            }
        }
        true
    }

    /// Check whether a network request is allowed.
    pub fn check_network(&mut self) -> bool {
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
        match self {
            NetworkPolicy::AllowAll => true,
            NetworkPolicy::DenyAll => false,
            NetworkPolicy::AllowList(hosts) => hosts.iter().any(|h| h == host),
            NetworkPolicy::DenyList(hosts) => !hosts.iter().any(|h| h == host),
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

    /// URL host, normalized to lowercase.
    pub fn host(&self) -> Result<String> {
        network_url_host(&self.url)
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

    /// Validate and build the request descriptor.
    pub fn build_checked(self) -> Result<AppNetworkRequest> {
        let request = AppNetworkRequest {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body_size_bytes: self.body_size_bytes,
            network_policy: self.network_policy,
        };
        request.validate()?;
        Ok(request)
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

/// Checked descriptor for app-owned realtime network connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRealtimeConnection {
    kind: AppRealtimeConnectionKind,
    url: String,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
    heartbeat_interval: Option<Duration>,
    max_message_bytes: Option<u64>,
    network_policy: Option<NetworkPolicy>,
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

    /// Optional outbound network policy.
    pub fn network_policy(&self) -> Option<&NetworkPolicy> {
        self.network_policy.as_ref()
    }

    /// URL host, normalized to lowercase.
    pub fn host(&self) -> Result<String> {
        realtime_url_host(&self.url, self.kind)
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
        if let Some(policy) = &self.network_policy {
            policy.validate()?;
            anyhow::ensure!(
                policy.check(&self.host()?),
                "realtime connection URL is denied by network policy"
            );
        }
        Ok(())
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

    /// Attach an outbound network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Validate and build the realtime descriptor.
    pub fn build_checked(self) -> Result<AppRealtimeConnection> {
        let connection = AppRealtimeConnection {
            kind: self.kind,
            url: self.url,
            protocols: self.protocols,
            headers: self.headers,
            heartbeat_interval: self.heartbeat_interval,
            max_message_bytes: self.max_message_bytes,
            network_policy: self.network_policy,
        };
        connection.validate()?;
        Ok(connection)
    }
}

fn network_url_host(url: &str) -> Result<String> {
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

fn realtime_url_host(url: &str, kind: AppRealtimeConnectionKind) -> Result<String> {
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

fn validate_network_headers(headers: &[(String, String)]) -> Result<()> {
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
        !host.contains("://") && !host.contains('/'),
        "network policy host must not include a URL scheme or path"
    );
    anyhow::ensure!(
        host.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-')),
        "network policy host must contain only ASCII letters, numbers, '.' or '-'"
    );
    Ok(())
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

    /// Check whether this schema is compatible with another schema.
    ///
    /// Two schemas are compatible when each schema's version falls within the
    /// other's supported range (i.e. `>= min_compatible`).
    pub fn is_compatible(&self, other: &IpcSchema) -> bool {
        self.version >= other.min_compatible && other.version >= self.min_compatible
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

    // === KeychainStore ===

    #[test]
    fn test_keychain_store_and_retrieve() {
        let mut store = KeychainStore::new();
        store.store(CredentialEntry {
            service: "github".to_string(),
            account: "user1".to_string(),
            secret: b"token123".to_vec(),
        });
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
        store.store(CredentialEntry {
            service: "svc".to_string(),
            account: "acct".to_string(),
            secret: vec![1, 2, 3],
        });
        assert!(store.delete("svc", "acct"));
        assert!(!store.delete("svc", "acct"));
        assert!(store.is_empty());
    }

    #[test]
    fn test_keychain_list() {
        let mut store = KeychainStore::new();
        store.store(CredentialEntry {
            service: "a".to_string(),
            account: "b".to_string(),
            secret: vec![],
        });
        store.store(CredentialEntry {
            service: "c".to_string(),
            account: "d".to_string(),
            secret: vec![],
        });
        assert_eq!(store.len(), 2);
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn test_keychain_overwrite() {
        let mut store = KeychainStore::new();
        store.store(CredentialEntry {
            service: "svc".to_string(),
            account: "acct".to_string(),
            secret: b"old".to_vec(),
        });
        store.store(CredentialEntry {
            service: "svc".to_string(),
            account: "acct".to_string(),
            secret: b"new".to_vec(),
        });
        assert_eq!(store.len(), 1);
        assert_eq!(store.retrieve("svc", "acct").unwrap().secret, b"new");
    }

    // === AccessTokenStore ===

    #[test]
    fn test_access_token_issue_and_validate() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/tmp/file.txt"), 1000, Some(3600));
        assert!(store.validate(&token, 1000).is_some());
        assert_eq!(
            store.validate(&token, 1000).unwrap(),
            &PathBuf::from("/tmp/file.txt")
        );
    }

    #[test]
    fn test_access_token_expired() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 1000, Some(60));
        assert!(store.validate(&token, 1059).is_some());
        assert!(store.validate(&token, 1060).is_none());
    }

    #[test]
    fn test_access_token_no_expiry() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 0, None);
        assert!(store.validate(&token, u64::MAX - 1).is_some());
    }

    #[test]
    fn test_access_token_revoke() {
        let mut store = AccessTokenStore::new();
        let token = store.issue(PathBuf::from("/f"), 0, None);
        assert!(store.revoke(&token));
        assert!(store.validate(&token, 0).is_none());
        assert!(!store.revoke(&token));
    }

    #[test]
    fn test_access_token_list() {
        let mut store = AccessTokenStore::new();
        store.issue(PathBuf::from("/a"), 0, None);
        store.issue(PathBuf::from("/b"), 0, None);
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn test_access_token_purge_expired() {
        let mut store = AccessTokenStore::new();
        store.issue(PathBuf::from("/a"), 0, Some(10));
        store.issue(PathBuf::from("/b"), 0, Some(20));
        store.issue(PathBuf::from("/c"), 0, None);
        let purged = store.purge_expired(15);
        assert_eq!(purged, 1);
        assert_eq!(store.list().len(), 2);
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
    }

    #[test]
    fn app_network_request_builder_validates_checked_requests() {
        let policy = NetworkPolicyBuilder::new()
            .allow_host("api.example.com")
            .build_checked()
            .unwrap();

        let request = AppNetworkRequestBuilder::post("https://api.example.com/v1/sync")
            .header("Content-Type", "application/json")
            .header("X-Trace-Id", "abc123")
            .body_size_bytes(128)
            .network_policy(policy)
            .build_checked()
            .unwrap();

        assert_eq!(request.method(), AppNetworkMethod::Post);
        assert_eq!(request.method().as_str(), "POST");
        assert_eq!(request.url(), "https://api.example.com/v1/sync");
        assert_eq!(request.host().unwrap(), "api.example.com");
        assert_eq!(request.headers().len(), 2);
        assert_eq!(request.body_size_bytes(), Some(128));
        assert!(request.network_policy().is_some());
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

        let connection = AppRealtimeConnection::websocket("wss://events.example.com/socket")
            .protocol("kael.v1")
            .header("Authorization", "Bearer token")
            .heartbeat_interval(Duration::from_secs(30))
            .max_message_bytes(65_536)
            .network_policy(policy)
            .build_checked()
            .unwrap();

        assert_eq!(connection.kind(), AppRealtimeConnectionKind::WebSocket);
        assert_eq!(connection.kind().key(), "websocket");
        assert_eq!(connection.url(), "wss://events.example.com/socket");
        assert_eq!(connection.host().unwrap(), "events.example.com");
        assert_eq!(connection.protocols(), &["kael.v1".to_string()]);
        assert_eq!(connection.headers().len(), 1);
        assert_eq!(
            connection.heartbeat_interval(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(connection.max_message_bytes(), Some(65_536));
        assert_eq!(
            connection.required_capability().unwrap(),
            Capability::Network {
                hosts: vec!["events.example.com".to_string()]
            }
        );
        assert!(connection.network_policy().is_some());
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
        let t1 = store.issue(PathBuf::from("/a"), 0, None);
        let t2 = store.issue(PathBuf::from("/a"), 0, None);
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
