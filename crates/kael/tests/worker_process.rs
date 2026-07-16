use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kael::{ProcessClass, ProcessId, ProcessInfo, SupervisorEvent, WorkerHost};

fn worker_child_path() -> &'static str {
    env!("CARGO_BIN_EXE_kael-worker-child")
}

#[test]
fn worker_process_handles_requests_and_health_checks() {
    let mut host = WorkerHost::with_temp_dir();
    let info =
        ProcessInfo::worker(ProcessId(0), "integration-worker").executable(worker_child_path());

    let worker = host
        .spawn_worker(ProcessClass::Worker, info)
        .expect("spawn worker child");

    worker.health_check().expect("worker responds to ping");

    let response: serde_json::Value = worker
        .request(serde_json::json!({
            "op": "echo",
            "message": "hello from host"
        }))
        .expect("worker echo response");

    assert_eq!(response["message"], "hello from host");
}

#[test]
fn worker_process_streams_progress_and_closes_after_success() {
    let mut host = WorkerHost::with_temp_dir();
    let info = ProcessInfo::worker(ProcessId(0), "progress-worker").executable(worker_child_path());
    let worker = host
        .spawn_worker(ProcessClass::Worker, info)
        .expect("spawn worker child");

    let progress = worker
        .stream_progress::<_, serde_json::Value>(serde_json::json!({ "op": "progress" }))
        .expect("start progress stream");
    assert_eq!(
        progress
            .recv_timeout(Duration::from_secs(2))
            .expect("first progress update")
            .expect("valid first progress update"),
        serde_json::json!({ "step": 1 })
    );
    assert_eq!(
        progress
            .recv_timeout(Duration::from_secs(2))
            .expect("second progress update")
            .expect("valid second progress update"),
        serde_json::json!({ "step": 2 })
    );
    assert!(
        progress.recv_timeout(Duration::from_secs(2)).is_err(),
        "successful terminal response should close the progress stream"
    );
}

#[cfg(unix)]
#[test]
fn worker_bootstrap_failure_stops_child_and_removes_socket() {
    let socket_dir =
        std::env::temp_dir().join(format!("kael-worker-test-{}", uuid::Uuid::new_v4()));
    let mut host = WorkerHost::new(&socket_dir);
    let info = ProcessInfo::worker(ProcessId(0), "non-connecting-worker")
        .executable(std::path::Path::new("/bin/true"));

    let started = Instant::now();
    assert!(host.spawn_worker(ProcessClass::Worker, info).is_err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "an exited worker should fail bootstrap without waiting for the full timeout"
    );
    assert_eq!(
        std::fs::read_dir(&socket_dir).unwrap().count(),
        0,
        "failed bootstrap should remove its socket"
    );
    std::fs::remove_dir(&socket_dir).unwrap();
}

#[test]
fn worker_process_crash_is_reported_without_panicking_host() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = WorkerHost::with_temp_dir();
    host.on_event({
        let events = Arc::clone(&events);
        move |event| events.lock().unwrap().push(event)
    });

    let info = ProcessInfo::worker(ProcessId(0), "crashing-worker").executable(worker_child_path());
    let worker = host
        .spawn_worker(ProcessClass::Worker, info)
        .expect("spawn worker child");

    worker.health_check().expect("worker responds before crash");
    worker
        .fire_and_forget(serde_json::json!({ "op": "exit" }))
        .expect("send crash command");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, SupervisorEvent::Exited { id, .. } if *id == worker.id()))
        {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "worker exit event was not observed"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
