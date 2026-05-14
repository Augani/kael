//! Security boundary: capability model and permission broker for GPUI.
//!
//! This module provides safe defaults and explicit capability grants for
//! dangerous actions such as opening external URLs, filesystem access, and
//! plugin execution. The model is designed to integrate with the process
//! isolation layer so that child processes receive only the capabilities they
//! need.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

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
            ProcessClass::Media => &self.media_defaults,
            ProcessClass::Extension => &self.extension_defaults,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut broker = PermissionBroker::new();
        broker.register_process(process, ProcessClass::Worker);
        broker.set_default_capabilities(
            ProcessClass::Worker,
            [Capability::FilesystemRead {
                scope: PathScope::AppData,
            }],
        );

        assert_eq!(
            broker.check(
                process,
                &Capability::FilesystemRead {
                    scope: PathScope::AppData,
                },
            ),
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
}
