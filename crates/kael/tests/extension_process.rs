use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kael::{ExecutionModel, ExtensionHostRuntime, PluginManifestBuilder, SupervisorEvent};

fn extension_child_path() -> &'static str {
    env!("CARGO_BIN_EXE_kael-extension-child")
}

#[test]
fn external_process_extension_activates_and_deactivates() {
    let tmp = std::env::temp_dir().join(format!(
        "kael-extension-process-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let manifest = PluginManifestBuilder::new(
        "com.kael.integration.extension",
        "Integration Extension",
        "1.0.0",
        "1.0.0",
        extension_child_path(),
        ExecutionModel::ExternalProcess,
    )
    .build()
    .unwrap();

    let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app");
    runtime.load(manifest).unwrap();
    runtime
        .activate("com.kael.integration.extension")
        .expect("extension activates through real child process");

    assert!(
        runtime
            .get("com.kael.integration.extension")
            .unwrap()
            .is_active
    );
    assert_eq!(runtime.active().len(), 1);

    runtime
        .deactivate("com.kael.integration.extension")
        .expect("extension deactivates cleanly");
    assert!(
        !runtime
            .get("com.kael.integration.extension")
            .unwrap()
            .is_active
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn external_process_extension_handles_commands_and_contributions() {
    let tmp = std::env::temp_dir().join(format!("kael-extension-rpc-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let manifest = PluginManifestBuilder::new(
        "com.kael.integration.rpc",
        "Integration RPC Extension",
        "1.0.0",
        "1.0.0",
        extension_child_path(),
        ExecutionModel::ExternalProcess,
    )
    .build()
    .unwrap();

    let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app-rpc");
    runtime.load(manifest).unwrap();
    runtime.activate("com.kael.integration.rpc").unwrap();

    runtime
        .send_command("com.kael.integration.rpc", "integration.echo", None)
        .expect("extension command returns ack");

    let contributions = runtime
        .request_contributions("com.kael.integration.rpc")
        .expect("extension returns contributions over RPC");
    assert_eq!(contributions.commands.len(), 1);
    assert_eq!(contributions.commands[0].id, "integration.echo");

    runtime.deactivate("com.kael.integration.rpc").unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn external_process_extension_crash_is_reported() {
    let tmp =
        std::env::temp_dir().join(format!("kael-extension-crash-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = ExtensionHostRuntime::new(tmp.join("extensions"), "test-app-crash");
    runtime.supervisor_mut().on_event({
        let events = Arc::clone(&events);
        move |event| events.lock().unwrap().push(event)
    });

    let manifest = PluginManifestBuilder::new(
        "com.kael.integration.crash",
        "Integration Crash Extension",
        "1.0.0",
        "1.0.0",
        extension_child_path(),
        ExecutionModel::ExternalProcess,
    )
    .build()
    .unwrap();

    runtime.load(manifest).unwrap();
    runtime.activate("com.kael.integration.crash").unwrap();
    let process_id = runtime
        .get("com.kael.integration.crash")
        .and_then(|info| info.process_id)
        .expect("extension has process id");

    let result = runtime.send_command("com.kael.integration.crash", "integration.crash", None);
    assert!(result.is_err(), "crashing command should break RPC");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, SupervisorEvent::Exited { id, .. } if *id == process_id))
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "extension exit event was not observed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
