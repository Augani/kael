//! Process supervisor for the GPUI process-isolation model.
//!
//! The supervisor launches child processes, monitors their health via
//! heartbeats, and applies restart policies on failure. It integrates with
//! [`Tracer`] for observability and emits structured events that applications
//! can subscribe to.

use std::{
    collections::HashMap,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};

use crate::{
    TracePhase, Tracer,
    process_model::{
        ProcessHealth, ProcessId, ProcessInfo, ProcessSpawnOptions, RestartPolicy, Supervisor,
        SupervisorEvent,
    },
};

static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

type SharedHealthChangeCallback = Arc<Mutex<Box<dyn FnMut(ProcessId, ProcessHealth) + Send>>>;
type SharedSupervisorEventCallback = Arc<Mutex<Box<dyn FnMut(SupervisorEvent) + Send>>>;

/// Strategy used to spawn child processes for supervision.
pub trait ProcessSpawner: Send + Sync {
    /// Launch the given process info and return the child handle.
    fn spawn(&self, info: &ProcessInfo) -> Result<Child>;
}

/// Default process spawner backed by [`std::process::Command`].
#[derive(Debug, Default)]
pub struct CommandProcessSpawner;

impl ProcessSpawner for CommandProcessSpawner {
    fn spawn(&self, info: &ProcessInfo) -> Result<Child> {
        let mut cmd = Command::new(&info.executable);
        cmd.envs(&info.env)
            .args(&info.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Some(ref dir) = info.working_dir {
            cmd.current_dir(dir);
        }

        cmd.spawn()
            .with_context(|| format!("failed to spawn process: {}", info.executable.display()))
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Manages a collection of child processes with health monitoring and
/// automatic restart.
pub struct ProcessSupervisor {
    processes: Arc<Mutex<HashMap<ProcessId, SupervisedProcess>>>,
    event_callbacks: Arc<Mutex<Vec<SharedSupervisorEventCallback>>>,
    spawner: Arc<dyn ProcessSpawner>,
    tracer: Option<Tracer>,
    monitor_signal: Arc<(Mutex<bool>, Condvar)>,
}

fn lock_recover<'a, T>(mutex: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        log::error!("recovering poisoned process-supervisor {name} lock");
        poisoned.into_inner()
    })
}

fn wait_for_monitor_signal(signal: &Arc<(Mutex<bool>, Condvar)>, duration: Duration) -> bool {
    let (shutdown, wake) = &**signal;
    let shutdown = lock_recover(shutdown, "shutdown");
    if *shutdown {
        return true;
    }
    let (shutdown, _) = wake
        .wait_timeout(shutdown, duration)
        .unwrap_or_else(|poisoned| {
            log::error!("recovering poisoned process-supervisor shutdown wait");
            poisoned.into_inner()
        });
    *shutdown
}

struct SupervisedProcess {
    info: ProcessInfo,
    options: ProcessSpawnOptions,
    health: ProcessHealth,
    restart_count: u32,
    last_heartbeat: Instant,
    child: Option<Child>,
    on_health_change: Option<SharedHealthChangeCallback>,
}

impl ProcessSupervisor {
    /// Create a new supervisor.
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            event_callbacks: Arc::new(Mutex::new(Vec::new())),
            spawner: Arc::new(CommandProcessSpawner),
            tracer: None,
            monitor_signal: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Replace the process spawner implementation.
    pub fn with_spawner(mut self, spawner: Arc<dyn ProcessSpawner>) -> Self {
        self.spawner = spawner;
        self
    }

    /// Attach a tracer for observability.
    pub fn with_tracer(mut self, tracer: Tracer) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// Spawn a new child process with the given info and restart policy.
    pub fn spawn(&mut self, mut info: ProcessInfo, policy: RestartPolicy) -> Result<ProcessId> {
        self.spawn_with_options(info, ProcessSpawnOptions::default().restart_policy(policy))
    }

    /// Spawn a new child process with explicit process options.
    pub fn spawn_with_options(
        &mut self,
        mut info: ProcessInfo,
        options: ProcessSpawnOptions,
    ) -> Result<ProcessId> {
        info.validate()?;
        options.validate()?;
        let raw_id = NEXT_PROCESS_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow!("process supervisor identifier space exhausted"))?;
        let id = ProcessId(raw_id);
        anyhow::ensure!(
            !lock_recover(&self.processes, "processes").contains_key(&id),
            "process supervisor identifier collision"
        );
        info.id = id;
        let child = match self.spawner.spawn(&info) {
            Ok(child) => child,
            Err(error) => {
                self.emit_event(SupervisorEvent::SpawnFailed {
                    info,
                    error: error.to_string(),
                });
                return Err(error);
            }
        };

        if let Some(ref tracer) = self.tracer {
            tracer.record(
                format!("process_spawn/{}", info.class.label()),
                "supervisor",
                TracePhase::Instant,
            );
        }

        let now = Instant::now();

        let supervised = SupervisedProcess {
            info: info.clone(),
            options,
            health: ProcessHealth::Starting,
            restart_count: 0,
            last_heartbeat: now,
            child: Some(child),
            on_health_change: None,
        };

        lock_recover(&self.processes, "processes").insert(id, supervised);
        if let Err(error) = self.start_health_monitor(id) {
            if let Some(mut process) = lock_recover(&self.processes, "processes").remove(&id)
                && let Some(mut child) = process.child.take()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.emit_event(SupervisorEvent::SpawnFailed {
                info,
                error: error.to_string(),
            });
            return Err(error);
        }
        self.emit_event(SupervisorEvent::Spawned { info });

        Ok(id)
    }

    /// Stop a supervised process.
    pub fn stop(&mut self, id: ProcessId) -> Result<()> {
        let mut processes = lock_recover(&self.processes, "processes");
        let proc = processes
            .get_mut(&id)
            .ok_or_else(|| anyhow!("process not found: {:?}", id))?;

        if let Some(ref mut child) = proc.child {
            if let Err(error) = child.kill()
                && error.kind() != std::io::ErrorKind::InvalidInput
            {
                return Err(error).context("failed to terminate supervised process");
            }
            child
                .wait()
                .context("failed to reap supervised process after termination")?;
        }

        let old = proc.health;
        proc.health = ProcessHealth::Stopped;
        proc.child = None;

        if let Some(ref tracer) = self.tracer {
            tracer.record(
                format!("process_stop/{}", proc.info.class.label()),
                "supervisor",
                TracePhase::Instant,
            );
        }

        drop(processes);
        self.monitor_signal.1.notify_all();
        self.emit_health_change(id, old, ProcessHealth::Stopped);
        self.emit_event(SupervisorEvent::Stopped { id });

        Ok(())
    }

    /// Get the current health of a supervised process.
    pub fn health(&self, id: ProcessId) -> Option<ProcessHealth> {
        lock_recover(&self.processes, "processes")
            .get(&id)
            .map(|p| p.health)
    }

    /// Return the IDs of all currently supervised processes.
    pub fn processes(&self) -> Vec<ProcessId> {
        lock_recover(&self.processes, "processes")
            .keys()
            .cloned()
            .collect()
    }

    /// Register a callback for health changes on a specific process.
    pub fn on_health_change(
        &mut self,
        id: ProcessId,
        callback: impl FnMut(ProcessId, ProcessHealth) + Send + 'static,
    ) {
        if let Some(proc) = lock_recover(&self.processes, "processes").get_mut(&id) {
            proc.on_health_change = Some(Arc::new(Mutex::new(Box::new(callback))));
        }
    }

    /// Record a heartbeat for a process, resetting the unresponsive timer.
    pub fn record_heartbeat(&mut self, id: ProcessId) -> Result<()> {
        let mut processes = lock_recover(&self.processes, "processes");
        let proc = processes
            .get_mut(&id)
            .ok_or_else(|| anyhow!("process not found: {:?}", id))?;

        proc.last_heartbeat = Instant::now();
        if proc.health != ProcessHealth::Healthy {
            let old = proc.health;
            proc.health = ProcessHealth::Healthy;
            proc.restart_count = 0;
            drop(processes);
            self.emit_health_change(id, old, ProcessHealth::Healthy);
        }

        Ok(())
    }

    /// Register a callback for supervisor-wide events.
    pub fn on_event(&mut self, callback: impl FnMut(SupervisorEvent) + Send + 'static) {
        lock_recover(&self.event_callbacks, "event callbacks")
            .push(Arc::new(Mutex::new(Box::new(callback))));
    }

    fn emit_event(&self, event: SupervisorEvent) {
        Self::emit_event_static(&self.event_callbacks, event);
    }

    fn emit_event_static(
        event_callbacks: &Arc<Mutex<Vec<SharedSupervisorEventCallback>>>,
        event: SupervisorEvent,
    ) {
        let callbacks = lock_recover(event_callbacks, "event callbacks").clone();
        for callback in callbacks {
            let mut callback = lock_recover(&callback, "event callback");
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback(event.clone());
            }));
        }
    }

    fn start_health_monitor(&self, id: ProcessId) -> Result<()> {
        let processes = self.processes.clone();
        let event_callbacks = self.event_callbacks.clone();
        let spawner = self.spawner.clone();
        let tracer = self.tracer.clone();
        let monitor_signal = self.monitor_signal.clone();

        thread::Builder::new()
            .name(format!("process-monitor-{}", id.0))
            .spawn(move || {
                loop {
                    if wait_for_monitor_signal(&monitor_signal, Duration::from_secs(1)) {
                        break;
                    }

                    let mut procs = lock_recover(&processes, "processes");
                    let proc = match procs.get_mut(&id) {
                        Some(p) => p,
                        None => break,
                    };

                    if proc.health == ProcessHealth::Stopped || proc.child.is_none() {
                        break;
                    }

                    let elapsed = proc.last_heartbeat.elapsed();
                    let threshold = proc.options.health_check.heartbeat_interval.saturating_mul(
                        proc.options.health_check.missed_heartbeats_before_unhealthy,
                    );

                    if elapsed > threshold && proc.health == ProcessHealth::Healthy {
                        let old = proc.health;
                        proc.health = ProcessHealth::Unresponsive;
                        let label = proc.info.class.label().to_string();
                        drop(procs);
                        Self::emit_health_change_static(
                            &processes,
                            &event_callbacks,
                            id,
                            old,
                            ProcessHealth::Unresponsive,
                        );
                        if let Some(ref tracer) = tracer {
                            tracer.record(
                                format!("process_unresponsive/{label}"),
                                "supervisor",
                                TracePhase::Instant,
                            );
                        }
                        continue;
                    }

                    // Check if child has exited
                    let mut should_restart = false;
                    let exit_status: Option<i32> = if let Some(ref mut child) = proc.child {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                let old = proc.health;
                                proc.health = ProcessHealth::Dead;
                                should_restart = match proc.options.restart_policy {
                                    RestartPolicy::Never => false,
                                    RestartPolicy::OnFailure { .. } => !status.success(),
                                    RestartPolicy::Always { .. } => true,
                                };
                                proc.child = None;
                                let code = status.code().unwrap_or(-1);
                                let label = proc.info.class.label().to_string();
                                drop(procs);
                                Self::emit_health_change_static(
                                    &processes,
                                    &event_callbacks,
                                    id,
                                    old,
                                    ProcessHealth::Dead,
                                );
                                Self::emit_event_static(
                                    &event_callbacks,
                                    SupervisorEvent::Exited {
                                        id,
                                        exit_code: status.code(),
                                        will_restart: should_restart,
                                    },
                                );
                                if let Some(ref tracer) = tracer {
                                    tracer.record(
                                        format!("process_exit/{label}?status={code}"),
                                        "supervisor",
                                        TracePhase::Instant,
                                    );
                                }
                                Some(code)
                            }
                            Ok(None) => None,
                        Err(error) => {
                            log::warn!("failed to query supervised process state: {error}");
                            if let Err(kill_error) = child.kill()
                                && kill_error.kind() != std::io::ErrorKind::InvalidInput
                            {
                                log::warn!(
                                    "failed to terminate process after status-query failure: {kill_error}"
                                );
                            }
                            if let Err(wait_error) = child.wait() {
                                log::warn!(
                                    "failed to reap process after status-query failure: {wait_error}"
                                );
                            }
                            let old = proc.health;
                                proc.health = ProcessHealth::Dead;
                                should_restart =
                                    !matches!(proc.options.restart_policy, RestartPolicy::Never);
                                proc.child = None;
                                drop(procs);
                                Self::emit_health_change_static(
                                    &processes,
                                    &event_callbacks,
                                    id,
                                    old,
                                    ProcessHealth::Dead,
                                );
                                Self::emit_event_static(
                                    &event_callbacks,
                                    SupervisorEvent::Exited {
                                        id,
                                        exit_code: None,
                                        will_restart: should_restart,
                                    },
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };

                    let _ = exit_status;

                    if should_restart {
                        let mut procs = lock_recover(&processes, "processes");
                        let Some(proc) = procs.get_mut(&id) else {
                            break;
                        };
                        if proc.health == ProcessHealth::Stopped {
                            break;
                        }
                        match &proc.options.restart_policy {
                            RestartPolicy::Never => {
                                proc.child = None;
                            }
                            RestartPolicy::OnFailure {
                                max_restarts,
                                backoff,
                            } => {
                                if proc.restart_count >= *max_restarts {
                                    proc.child = None;
                                } else {
                                    proc.restart_count = proc.restart_count.saturating_add(1);
                                    let wait = backoff
                                        .saturating_mul(proc.restart_count)
                                        .min(Duration::from_secs(86_400));
                                    let info = proc.info.clone();
                                    let attempt = proc.restart_count;
                                    drop(procs);
                                    Self::emit_event_static(
                                        &event_callbacks,
                                        SupervisorEvent::Restarting {
                                            id,
                                            attempt,
                                            backoff: wait,
                                        },
                                    );
                                    if wait_for_monitor_signal(&monitor_signal, wait) {
                                        break;
                                    }
                                    {
                                        let procs = lock_recover(&processes, "processes");
                                        let Some(proc) = procs.get(&id) else {
                                            break;
                                        };
                                        if proc.health == ProcessHealth::Stopped
                                            || proc.child.is_some()
                                        {
                                            break;
                                        }
                                    }
                                    let spawn_result = spawner.spawn(&info);
                                    let mut procs = lock_recover(&processes, "processes");
                                    let Some(proc) = procs.get_mut(&id) else {
                                        if let Ok(mut child) = spawn_result {
                                            let _ = child.kill();
                                            let _ = child.wait();
                                        }
                                        break;
                                    };
                                    if proc.health == ProcessHealth::Stopped {
                                        if let Ok(mut child) = spawn_result {
                                            let _ = child.kill();
                                            let _ = child.wait();
                                        }
                                        break;
                                    }
                                    match spawn_result {
                                        Ok(new_child) => {
                                            proc.child = Some(new_child);
                                            proc.health = ProcessHealth::Starting;
                                            proc.last_heartbeat = Instant::now();
                                            drop(procs);
                                            Self::emit_event_static(
                                                &event_callbacks,
                                                SupervisorEvent::Restarted { info },
                                            );
                                        }
                                        Err(error) => {
                                            proc.child = None;
                                            drop(procs);
                                            Self::emit_event_static(
                                                &event_callbacks,
                                                SupervisorEvent::SpawnFailed {
                                                    info,
                                                    error: error.to_string(),
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                            RestartPolicy::Always { backoff } => {
                                proc.restart_count = proc.restart_count.saturating_add(1);
                                let wait = backoff
                                    .saturating_mul(proc.restart_count)
                                    .min(Duration::from_secs(86_400));
                                let info = proc.info.clone();
                                let attempt = proc.restart_count;
                                drop(procs);
                                Self::emit_event_static(
                                    &event_callbacks,
                                    SupervisorEvent::Restarting {
                                        id,
                                        attempt,
                                        backoff: wait,
                                    },
                                );
                                if wait_for_monitor_signal(&monitor_signal, wait) {
                                    break;
                                }
                                {
                                    let procs = lock_recover(&processes, "processes");
                                    let Some(proc) = procs.get(&id) else {
                                        break;
                                    };
                                    if proc.health == ProcessHealth::Stopped || proc.child.is_some()
                                    {
                                        break;
                                    }
                                }
                                let spawn_result = spawner.spawn(&info);
                                let mut procs = lock_recover(&processes, "processes");
                                let Some(proc) = procs.get_mut(&id) else {
                                    if let Ok(mut child) = spawn_result {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                    }
                                    break;
                                };
                                if proc.health == ProcessHealth::Stopped {
                                    if let Ok(mut child) = spawn_result {
                                        let _ = child.kill();
                                        let _ = child.wait();
                                    }
                                    break;
                                }
                                match spawn_result {
                                    Ok(new_child) => {
                                        proc.child = Some(new_child);
                                        proc.health = ProcessHealth::Starting;
                                        proc.last_heartbeat = Instant::now();
                                        drop(procs);
                                        Self::emit_event_static(
                                            &event_callbacks,
                                            SupervisorEvent::Restarted { info },
                                        );
                                    }
                                    Err(error) => {
                                        proc.child = None;
                                        drop(procs);
                                        Self::emit_event_static(
                                            &event_callbacks,
                                            SupervisorEvent::SpawnFailed {
                                                info,
                                                error: error.to_string(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            })
            .map(|_| ())
            .context("failed to spawn process health monitor")
    }

    fn emit_health_change(&self, id: ProcessId, _old: ProcessHealth, new: ProcessHealth) {
        Self::emit_health_change_static(&self.processes, &self.event_callbacks, id, _old, new);
    }

    fn emit_health_change_static(
        processes: &Arc<Mutex<HashMap<ProcessId, SupervisedProcess>>>,
        event_callbacks: &Arc<Mutex<Vec<SharedSupervisorEventCallback>>>,
        id: ProcessId,
        old: ProcessHealth,
        new: ProcessHealth,
    ) {
        let callback = lock_recover(processes, "processes")
            .get(&id)
            .and_then(|proc| proc.on_health_change.clone());
        if let Some(callback) = callback {
            let mut callback = lock_recover(&callback, "health callback");
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(id, new)));
        }
        Self::emit_event_static(
            event_callbacks,
            SupervisorEvent::HealthChanged { id, old, new },
        );
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        let (shutdown, wake) = &*self.monitor_signal;
        *lock_recover(shutdown, "shutdown") = true;
        wake.notify_all();

        let mut processes = lock_recover(&self.processes, "processes");
        for process in processes.values_mut() {
            process.health = ProcessHealth::Stopped;
            if let Some(mut child) = process.child.take() {
                if let Err(error) = child.kill()
                    && error.kind() != std::io::ErrorKind::InvalidInput
                {
                    log::warn!("failed to terminate supervised process during shutdown: {error}");
                }
                if let Err(error) = child.wait() {
                    log::warn!("failed to reap supervised process during shutdown: {error}");
                }
            }
        }
        processes.clear();
    }
}

impl Default for ProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Supervisor for ProcessSupervisor {
    fn spawn(&mut self, info: ProcessInfo, policy: RestartPolicy) -> Result<ProcessId> {
        ProcessSupervisor::spawn(self, info, policy)
    }

    fn spawn_with_options(
        &mut self,
        info: ProcessInfo,
        options: ProcessSpawnOptions,
    ) -> Result<ProcessId> {
        ProcessSupervisor::spawn_with_options(self, info, options)
    }

    fn stop(&mut self, id: ProcessId) -> Result<()> {
        ProcessSupervisor::stop(self, id)
    }

    fn health(&self, id: ProcessId) -> Option<ProcessHealth> {
        ProcessSupervisor::health(self, id)
    }

    fn processes(&self) -> Vec<ProcessId> {
        ProcessSupervisor::processes(self)
    }

    fn on_health_change(
        &mut self,
        id: ProcessId,
        callback: Box<dyn FnMut(ProcessId, ProcessHealth) + Send>,
    ) {
        if let Some(proc) = lock_recover(&self.processes, "processes").get_mut(&id) {
            proc.on_health_change = Some(Arc::new(Mutex::new(callback)));
        }
    }

    fn on_event(&mut self, callback: Box<dyn FnMut(SupervisorEvent) + Send>) {
        lock_recover(&self.event_callbacks, "event callbacks").push(Arc::new(Mutex::new(callback)));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_model::{HealthCheckConfig, ProcessClass};

    struct CountingSpawner {
        spawns: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ProcessSpawner for CountingSpawner {
        fn spawn(&self, info: &ProcessInfo) -> Result<Child> {
            self.spawns
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            CommandProcessSpawner.spawn(info)
        }
    }

    #[cfg(unix)]
    fn short_lived_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "sh")
            .executable("sh")
            .args(["-c", "exit 0"])
    }

    #[cfg(windows)]
    fn short_lived_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "cmd")
            .executable("cmd")
            .args(["/C", "exit 0"])
    }

    #[cfg(unix)]
    fn long_lived_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "sh")
            .executable("sh")
            .args(["-c", "exec sleep 5"])
            .env("DUMMY", "1")
    }

    #[cfg(windows)]
    fn long_lived_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "cmd")
            .executable("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 >NUL"])
            .env("DUMMY", "1")
    }

    #[test]
    fn test_supervisor_spawn_and_health() {
        let mut supervisor = ProcessSupervisor::new();
        let id = supervisor
            .spawn(short_lived_process(), RestartPolicy::Never)
            .unwrap();
        assert_eq!(supervisor.health(id), Some(ProcessHealth::Starting));

        // Clean up
        let _ = supervisor.stop(id);
    }

    #[test]
    fn test_supervisor_stop() {
        let mut supervisor = ProcessSupervisor::new();
        let id = supervisor
            .spawn(long_lived_process(), RestartPolicy::Never)
            .unwrap();
        assert!(supervisor.stop(id).is_ok());
        assert_eq!(supervisor.health(id), Some(ProcessHealth::Stopped));
    }

    #[test]
    fn test_restart_policy_bounds() {
        let policy = RestartPolicy::OnFailure {
            max_restarts: 3,
            backoff: Duration::from_secs(1),
        };
        let json = serde_json::to_string(&policy).unwrap();
        let decoded: RestartPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, decoded);
    }

    #[test]
    fn test_supervisor_emits_lifecycle_events() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor = ProcessSupervisor::new();
        supervisor.on_event({
            let events = Arc::clone(&events);
            move |event| events.lock().unwrap().push(event)
        });

        let id = supervisor
            .spawn(long_lived_process(), RestartPolicy::Never)
            .unwrap();
        supervisor.stop(id).unwrap();

        let events = events.lock().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            SupervisorEvent::Spawned { info } if info.id == id
        )));
        assert!(events.iter().any(
            |event| matches!(event, SupervisorEvent::Stopped { id: event_id } if *event_id == id)
        ));
    }

    #[test]
    fn panicking_event_callback_does_not_abort_supervisor_dispatch() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor = ProcessSupervisor::new();
        supervisor.on_event(|_| panic!("observer failed"));
        supervisor.on_event({
            let events = Arc::clone(&events);
            move |event| events.lock().unwrap().push(event)
        });
        let id = supervisor
            .spawn(long_lived_process(), RestartPolicy::Never)
            .unwrap();
        supervisor.stop(id).unwrap();
        assert!(!events.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_spawn_options_fail_before_process_creation() {
        let mut supervisor = ProcessSupervisor::new();
        let options = ProcessSpawnOptions::default().health_check(HealthCheckConfig {
            heartbeat_interval: Duration::ZERO,
            missed_heartbeats_before_unhealthy: 1,
        });
        assert!(
            supervisor
                .spawn_with_options(long_lived_process(), options)
                .is_err()
        );
        assert!(supervisor.processes().is_empty());
    }

    #[test]
    fn stop_cancels_an_in_progress_restart_backoff() {
        let spawns = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut supervisor = ProcessSupervisor::new().with_spawner(Arc::new(CountingSpawner {
            spawns: spawns.clone(),
        }));
        let (tx, rx) = std::sync::mpsc::channel();
        supervisor.on_event(move |event| {
            if matches!(event, SupervisorEvent::Restarting { .. }) {
                let _ = tx.send(());
            }
        });
        let id = supervisor
            .spawn(
                crashing_process(),
                RestartPolicy::Always {
                    backoff: Duration::from_secs(30),
                },
            )
            .expect("initial process spawn should succeed");

        rx.recv_timeout(Duration::from_secs(3))
            .expect("monitor should enter restart backoff");
        supervisor
            .stop(id)
            .expect("stopping during backoff should succeed");
        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(
            spawns.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a stopped process must not restart after its backoff is interrupted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dropping_supervisor_terminates_and_reaps_children() {
        let mut supervisor = ProcessSupervisor::new();
        let id = supervisor
            .spawn(long_lived_process(), RestartPolicy::Never)
            .expect("long-lived process should spawn");
        let child_id = lock_recover(&supervisor.processes, "test processes")
            .get(&id)
            .and_then(|process| process.child.as_ref())
            .map(Child::id)
            .expect("supervised child should be present");

        drop(supervisor);

        let result = unsafe { libc::kill(child_id as i32, 0) };
        assert_eq!(result, -1, "supervised child should no longer exist");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn poisoned_supervisor_state_is_recovered() {
        let supervisor = ProcessSupervisor::new();
        let processes = supervisor.processes.clone();
        let _ = std::thread::spawn(move || {
            let _guard = processes.lock().expect("test lock should start healthy");
            panic!("poison process state");
        })
        .join();

        assert!(supervisor.processes().is_empty());
    }

    #[cfg(unix)]
    fn crashing_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "sh")
            .executable("sh")
            .args(["-c", "exit 1"])
    }

    #[cfg(windows)]
    fn crashing_process() -> ProcessInfo {
        ProcessInfo::new(ProcessId(0), ProcessClass::Worker, "cmd")
            .executable("cmd")
            .args(["/C", "exit 1"])
    }

    #[test]
    fn test_supervisor_restarts_on_failure() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor = ProcessSupervisor::new();
        supervisor.on_event({
            let events = Arc::clone(&events);
            move |event| events.lock().unwrap().push(event)
        });

        let id = supervisor
            .spawn(
                crashing_process(),
                RestartPolicy::OnFailure {
                    max_restarts: 2,
                    backoff: Duration::from_millis(100),
                },
            )
            .unwrap();

        std::thread::sleep(Duration::from_secs(3));
        supervisor.stop(id).ok();

        let events = events.lock().unwrap();
        let spawned_count = events
            .iter()
            .filter(|e| matches!(e, SupervisorEvent::Spawned { info } if info.id == id))
            .count();
        assert!(
            spawned_count >= 1,
            "expected at least one spawn event, got {}",
            spawned_count
        );

        let exited_events: Vec<_> = events
            .iter()
            .filter(
                |e| matches!(e, SupervisorEvent::Exited { id: event_id, .. } if *event_id == id),
            )
            .collect();
        assert!(
            !exited_events.is_empty(),
            "expected at least one exited event"
        );
    }

    #[test]
    fn test_supervisor_restart_bounded() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut supervisor = ProcessSupervisor::new();
        supervisor.on_event({
            let events = Arc::clone(&events);
            move |event| events.lock().unwrap().push(event)
        });

        let id = supervisor
            .spawn(
                crashing_process(),
                RestartPolicy::OnFailure {
                    max_restarts: 1,
                    backoff: Duration::from_millis(100),
                },
            )
            .unwrap();

        std::thread::sleep(Duration::from_secs(4));

        let events = events.lock().unwrap();
        let restart_count = events
            .iter()
            .filter(|e| matches!(e, SupervisorEvent::Restarting { id: event_id, .. } if *event_id == id))
            .count();
        assert!(
            restart_count <= 1,
            "expected at most 1 restart attempt, got {}",
            restart_count
        );
    }
}
