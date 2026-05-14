use crate::plugin::Contributions;
use crate::{
    Capability, ExecutionModel, ExtensionHostRuntime, PermissionBroker, PermissionResult,
    PluginManifest, ProcessClass, ProcessId, ThreatModel,
};

#[test]
fn test_denied_capability_returns_error() {
    let mut broker = PermissionBroker::new();
    let process = ProcessId(1);
    broker.register_process(process, ProcessClass::Ui);

    assert_eq!(
        broker.check(process, &Capability::OpenExternalUrl),
        PermissionResult::Denied
    );
}

#[test]
fn test_ui_class_defaults_applied() {
    let process = ProcessId(0);
    let mut broker = PermissionBroker::new();
    broker.register_process(process, ProcessClass::Ui);
    broker.apply_threat_model(&ThreatModel::new());

    assert_eq!(
        broker.check(process, &Capability::OpenExternalUrl),
        PermissionResult::Granted
    );
    assert_eq!(
        broker.check(process, &Capability::ClipboardRead),
        PermissionResult::Granted
    );
    assert_eq!(
        broker.check(process, &Capability::Notification),
        PermissionResult::Granted
    );
    assert_eq!(
        broker.check(process, &Capability::ShellExecute),
        PermissionResult::Denied
    );
}

#[test]
fn test_plugin_activation_respects_broker() {
    let tmp = std::env::temp_dir().join(format!("gpui-sec-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut host = ExtensionHostRuntime::new(&tmp, "test-app");
    let broker = PermissionBroker::new();

    let manifest = PluginManifest {
        id: "ext-1".to_string(),
        name: "Extension 1".to_string(),
        version: "1.0.0".to_string(),
        api_version: "1.0.0".to_string(),
        description: None,
        author: None,
        entry_point: "ext.wasm".to_string(),
        execution_model: ExecutionModel::Wasm,
        capabilities: vec![Capability::ShellExecute],
        args: Vec::new(),
        contributions: Contributions::default(),
    };

    host.load(manifest).unwrap();
    assert!(host.activate_with_broker("ext-1", &broker).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}
