use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use wasm_bindgen::{JsCast as _, closure::Closure};
use web_sys::{DedicatedWorkerGlobalScope, MessageEvent, Worker, WorkerOptions, WorkerType};

use crate::{
    Tracer,
    process_model::{
        BootstrapMessage, IpcMessage, ProcessClass, ProcessId, ProcessInfo, ProcessSpawnOptions,
        RestartPolicy, SupervisorEvent, WORKER_PROTOCOL_VERSION, WorkerError, WorkerProgress,
        WorkerRequest, WorkerResponse,
    },
    supervisor::ProcessSupervisor,
    worker_api::WorkerHandle,
};

use super::web_protocol::{
    BrowserWorkerWireBody, browser_worker_message_from_js, post_browser_worker_scope_message,
};

const DEFAULT_BROWSER_WORKER_CAPABILITY: &str = "worker:execute";
const MAX_BROWSER_WORKER_SCRIPT_URL_BYTES: usize = 8 * 1_024;
static NEXT_BROWSER_WORKER_ID: AtomicU64 = AtomicU64::new(1);

struct ActiveWorkerClient {
    scope: DedicatedWorkerGlobalScope,
    _message_listener: Closure<dyn FnMut(MessageEvent)>,
}

thread_local! {
    static ACTIVE_WORKER_CLIENT: RefCell<Option<ActiveWorkerClient>> = const { RefCell::new(None) };
}

/// Client runtime installed inside a dedicated browser Web Worker.
///
/// The browser implementation registers an event-driven handler and returns
/// from [`Self::run`]. Native `WorkerClient::run` continues to own its blocking
/// socket loop.
pub struct WorkerClient {
    scope: DedicatedWorkerGlobalScope,
    allowed_capabilities: Vec<String>,
}

impl WorkerClient {
    /// Connect to the hosting page from a dedicated Web Worker global scope.
    pub fn connect_from_env() -> Result<Self> {
        Self::connect_with_capabilities([DEFAULT_BROWSER_WORKER_CAPABILITY])
    }

    /// Connect to the hosting page. Browser workers have one implicit parent,
    /// so `path` must be `self` rather than a filesystem socket name.
    pub fn connect(path: &str) -> Result<Self> {
        anyhow::ensure!(
            path == "self",
            "browser workers only support the implicit 'self' host endpoint"
        );
        Self::connect_from_env()
    }

    /// Connect with the exact capability labels this worker is willing to grant.
    pub fn connect_with_capabilities<I, S>(capabilities: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let allowed_capabilities: Vec<String> = capabilities.into_iter().map(Into::into).collect();
        BootstrapMessage::Handshake {
            version: WORKER_PROTOCOL_VERSION,
            capabilities: allowed_capabilities.clone(),
        }
        .validate()?;
        let scope = js_sys::global()
            .dyn_into::<DedicatedWorkerGlobalScope>()
            .map_err(|_| anyhow!("WorkerClient must run inside a dedicated browser Web Worker"))?;
        Ok(Self {
            scope,
            allowed_capabilities,
        })
    }

    /// Install the typed worker request handler.
    ///
    /// Requests execute on this worker's JavaScript event loop, away from the UI
    /// thread. The handler must return before this worker can observe a cancel
    /// message; long algorithms should cooperate through smaller requests.
    pub fn run<F>(&self, handler: F) -> Result<()>
    where
        F: FnMut(
                WorkerRequest,
                Box<dyn Fn(WorkerProgress) + Send>,
            ) -> Result<WorkerResponse, WorkerError>
            + 'static,
    {
        ACTIVE_WORKER_CLIENT.with(|active| {
            anyhow::ensure!(
                active.borrow().is_none(),
                "a browser WorkerClient handler is already installed"
            );
            Ok::<(), anyhow::Error>(())
        })?;

        let scope = self.scope.clone();
        let callback_scope = scope.clone();
        let allowed_capabilities = self.allowed_capabilities.clone();
        let handshake_complete = Rc::new(Cell::new(false));
        let callback_handshake = Rc::clone(&handshake_complete);
        let handler = Rc::new(RefCell::new(handler));
        let callback_handler = Rc::clone(&handler);

        let listener = Closure::wrap(Box::new(move |event: MessageEvent| {
            let result = browser_worker_message_from_js(&event.data()).and_then(|message| {
                handle_client_message(
                    &callback_scope,
                    &allowed_capabilities,
                    &callback_handshake,
                    &callback_handler,
                    message,
                )
            });
            if let Err(error) = result {
                let _ = post_browser_worker_scope_message(
                    &callback_scope,
                    BrowserWorkerWireBody::Worker(IpcMessage::Response {
                        id: 0,
                        result: Err(WorkerError::Execution(error.to_string())),
                    }),
                );
                callback_scope.close();
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        scope.set_onmessage(Some(listener.as_ref().unchecked_ref()));

        ACTIVE_WORKER_CLIENT.with(|active| {
            *active.borrow_mut() = Some(ActiveWorkerClient {
                scope,
                _message_listener: listener,
            });
        });
        Ok(())
    }

    /// Send a correlated progress update to the hosting page.
    pub fn send_progress(&self, id: u64, progress: WorkerProgress) -> Result<()> {
        anyhow::ensure!(id != 0, "browser worker progress id must be nonzero");
        post_browser_worker_scope_message(
            &self.scope,
            BrowserWorkerWireBody::Worker(IpcMessage::Progress { id, body: progress }),
        )
    }

    /// Send a correlated terminal response to the hosting page.
    pub fn send_response(
        &self,
        id: u64,
        response: Result<WorkerResponse, WorkerError>,
    ) -> Result<()> {
        anyhow::ensure!(id != 0, "browser worker response id must be nonzero");
        post_browser_worker_scope_message(
            &self.scope,
            BrowserWorkerWireBody::Worker(IpcMessage::Response {
                id,
                result: response,
            }),
        )
    }
}

fn handle_client_message<F>(
    scope: &DedicatedWorkerGlobalScope,
    allowed_capabilities: &[String],
    handshake_complete: &Cell<bool>,
    handler: &RefCell<F>,
    message: BrowserWorkerWireBody,
) -> Result<()>
where
    F: FnMut(
        WorkerRequest,
        Box<dyn Fn(WorkerProgress) + Send>,
    ) -> Result<WorkerResponse, WorkerError>,
{
    match message {
        BrowserWorkerWireBody::Bootstrap(message) => {
            let IpcMessage::Request {
                id: 1,
                body:
                    BootstrapMessage::Handshake {
                        version,
                        capabilities,
                    },
            } = message
            else {
                return Err(anyhow!("unexpected browser worker bootstrap message"));
            };
            let handshake = BootstrapMessage::Handshake {
                version,
                capabilities: capabilities.clone(),
            };
            handshake.validate()?;
            anyhow::ensure!(
                version == WORKER_PROTOCOL_VERSION,
                "unsupported worker bootstrap protocol version"
            );
            anyhow::ensure!(
                !handshake_complete.replace(true),
                "duplicate browser worker bootstrap handshake"
            );
            anyhow::ensure!(
                capabilities
                    .iter()
                    .all(|capability| allowed_capabilities.contains(capability)),
                "browser host requested a capability this worker did not allow"
            );
            post_browser_worker_scope_message(
                scope,
                BrowserWorkerWireBody::Bootstrap(IpcMessage::Response {
                    id: 1,
                    result: Ok(BootstrapMessage::HandshakeAck {
                        heartbeat_interval_secs: 5,
                        granted_capabilities: capabilities,
                    }),
                }),
            )
        }
        BrowserWorkerWireBody::Worker(message) => {
            anyhow::ensure!(
                handshake_complete.get(),
                "browser worker request arrived before bootstrap"
            );
            match message {
                IpcMessage::Request { id, body } => {
                    anyhow::ensure!(id != 0, "browser worker request id must be nonzero");
                    let response = match body {
                        WorkerRequest::Ping => Ok(WorkerResponse::Pong),
                        request @ WorkerRequest::Execute { .. } => {
                            let progress: Box<dyn Fn(WorkerProgress) + Send> =
                                Box::new(move |progress| {
                                    let _ = post_active_worker_progress(id, progress);
                                });
                            catch_unwind(AssertUnwindSafe(|| {
                                (handler.borrow_mut())(request, progress)
                            }))
                            .unwrap_or_else(|_| {
                                Err(WorkerError::Execution(
                                    "browser worker handler panicked".to_string(),
                                ))
                            })
                        }
                    };
                    post_browser_worker_scope_message(
                        scope,
                        BrowserWorkerWireBody::Worker(IpcMessage::Response {
                            id,
                            result: response,
                        }),
                    )
                }
                IpcMessage::Cancel { id } => post_browser_worker_scope_message(
                    scope,
                    BrowserWorkerWireBody::Worker(IpcMessage::Response {
                        id,
                        result: Err(WorkerError::Cancelled),
                    }),
                ),
                IpcMessage::Response { .. } | IpcMessage::Progress { .. } => Err(anyhow!(
                    "browser host sent a worker-only response or progress message"
                )),
            }
        }
    }
}

fn post_active_worker_progress(id: u64, progress: WorkerProgress) -> Result<()> {
    ACTIVE_WORKER_CLIENT.with(|active| {
        let active = active.borrow();
        let active = active
            .as_ref()
            .ok_or_else(|| anyhow!("browser WorkerClient is not active"))?;
        post_browser_worker_scope_message(
            &active.scope,
            BrowserWorkerWireBody::Worker(IpcMessage::Progress { id, body: progress }),
        )
    })
}

type BrowserSupervisorCallback = Arc<Mutex<Box<dyn Fn(SupervisorEvent) + Send + Sync>>>;

/// Host-side manager for dedicated browser Web Workers.
pub struct WorkerHost {
    supervisor: ProcessSupervisor,
    _socket_dir: PathBuf,
    capabilities: Vec<String>,
    module_scripts: bool,
    workers: HashMap<ProcessId, WorkerHandle>,
    event_callbacks: Vec<BrowserSupervisorCallback>,
}

impl WorkerHost {
    /// Create a browser worker host. `socket_dir` is retained for source parity
    /// but has no browser filesystem meaning.
    pub fn new(socket_dir: impl Into<PathBuf>) -> Self {
        Self {
            supervisor: ProcessSupervisor::new(),
            _socket_dir: socket_dir.into(),
            capabilities: vec![DEFAULT_BROWSER_WORKER_CAPABILITY.to_string()],
            module_scripts: true,
            workers: HashMap::new(),
            event_callbacks: Vec::new(),
        }
    }

    /// Create a host with the same source-level constructor used on desktop.
    pub fn with_temp_dir() -> Self {
        Self::new(PathBuf::new())
    }

    /// Attach tracing to the compatibility process supervisor.
    pub fn with_tracer(mut self, tracer: Tracer) -> Self {
        self.supervisor = std::mem::take(&mut self.supervisor).with_tracer(tracer);
        self
    }

    /// Set the exact capabilities required during the worker handshake.
    pub fn with_capabilities<I, S>(mut self, capabilities: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = capabilities.into_iter().map(Into::into).collect();
        BootstrapMessage::Handshake {
            version: WORKER_PROTOCOL_VERSION,
            capabilities: self.capabilities.clone(),
        }
        .validate()?;
        Ok(self)
    }

    /// Opt into classic worker scripts. Module workers are the secure default.
    pub fn classic_worker_scripts(mut self) -> Self {
        self.module_scripts = false;
        self
    }

    /// Register a callback for browser-worker lifecycle events.
    pub fn on_event<F>(&mut self, callback: F)
    where
        F: Fn(SupervisorEvent) + Send + Sync + 'static,
    {
        self.event_callbacks
            .push(Arc::new(Mutex::new(Box::new(callback))));
    }

    /// Spawn a dedicated Web Worker from `ProcessInfo::executable`.
    pub fn spawn_worker(&mut self, class: ProcessClass, info: ProcessInfo) -> Result<WorkerHandle> {
        self.spawn_worker_with_options(class, info, ProcessSpawnOptions::default())
    }

    /// Spawn a dedicated Web Worker with checked lifecycle options.
    ///
    /// Browsers do not expose a process restart primitive; non-`Never`
    /// policies are rejected explicitly instead of pretending supervision is
    /// active. Applications can create a replacement worker after an `Exited`
    /// event.
    pub fn spawn_worker_with_options(
        &mut self,
        class: ProcessClass,
        mut info: ProcessInfo,
        options: ProcessSpawnOptions,
    ) -> Result<WorkerHandle> {
        info.validate()?;
        options.validate()?;
        anyhow::ensure!(
            class != ProcessClass::Ui,
            "cannot spawn the UI as a Web Worker"
        );
        anyhow::ensure!(
            matches!(options.restart_policy, RestartPolicy::Never),
            "automatic browser worker restart is unsupported; use RestartPolicy::Never"
        );
        anyhow::ensure!(
            info.args.is_empty(),
            "browser workers do not support process arguments; send a typed request instead"
        );
        anyhow::ensure!(
            info.env.is_empty(),
            "browser workers do not support process environment variables"
        );
        anyhow::ensure!(
            info.working_dir.is_none(),
            "browser workers do not support a process working directory"
        );

        let script_url = info
            .executable
            .to_str()
            .context("browser worker script URL must be valid UTF-8")?;
        anyhow::ensure!(
            script_url.len() <= MAX_BROWSER_WORKER_SCRIPT_URL_BYTES,
            "browser worker script URL is too long"
        );
        let lower_url = script_url.to_ascii_lowercase();
        anyhow::ensure!(
            !lower_url.starts_with("javascript:") && !lower_url.starts_with("data:"),
            "browser worker script URL uses a forbidden scheme"
        );

        let raw_id = NEXT_BROWSER_WORKER_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow!("browser worker identifier space exhausted"))?;
        let id = ProcessId(raw_id);
        info.id = id;
        info.class = class;

        let worker_options = WorkerOptions::new();
        worker_options.set_name(&info.name);
        worker_options.set_type(if self.module_scripts {
            WorkerType::Module
        } else {
            WorkerType::Classic
        });
        let worker = Worker::new_with_options(script_url, &worker_options).map_err(|error| {
            let error = anyhow!("failed to create browser Web Worker: {error:?}");
            self.emit_event(SupervisorEvent::SpawnFailed {
                info: info.clone(),
                error: error.to_string(),
            });
            error
        })?;

        let callbacks = self.event_callbacks.clone();
        let on_failure = Rc::new(move |_message: String| {
            emit_browser_event(
                &callbacks,
                SupervisorEvent::Exited {
                    id,
                    exit_code: None,
                    will_restart: false,
                },
            );
        });
        let handle = WorkerHandle::new_browser(id, worker, self.capabilities.clone(), on_failure)?;
        anyhow::ensure!(
            self.workers.insert(id, handle.clone()).is_none(),
            "duplicate browser worker identifier"
        );
        self.emit_event(SupervisorEvent::Spawned { info });
        Ok(handle)
    }

    /// Terminate one browser worker and close all of its pending channels.
    pub fn terminate_worker(&mut self, id: ProcessId) -> Result<()> {
        let handle = self
            .workers
            .remove(&id)
            .ok_or_else(|| anyhow!("browser worker not found: {}", id.0))?;
        handle.terminate_browser()?;
        self.emit_event(SupervisorEvent::Stopped { id });
        Ok(())
    }

    /// Number of workers currently owned by this host.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Access the compatibility supervisor used by shared diagnostics code.
    /// Browser lifecycle events are emitted through [`Self::on_event`].
    pub fn supervisor(&self) -> &ProcessSupervisor {
        &self.supervisor
    }

    /// Access the compatibility supervisor mutably.
    pub fn supervisor_mut(&mut self) -> &mut ProcessSupervisor {
        &mut self.supervisor
    }

    fn emit_event(&self, event: SupervisorEvent) {
        emit_browser_event(&self.event_callbacks, event);
    }
}

impl Drop for WorkerHost {
    fn drop(&mut self) {
        let workers = std::mem::take(&mut self.workers);
        for (id, worker) in workers {
            if worker.terminate_browser().is_ok() {
                self.emit_event(SupervisorEvent::Stopped { id });
            }
        }
    }
}

fn emit_browser_event(callbacks: &[BrowserSupervisorCallback], event: SupervisorEvent) {
    for callback in callbacks {
        let Ok(callback) = callback.lock() else {
            continue;
        };
        let _ = catch_unwind(AssertUnwindSafe(|| callback(event.clone())));
    }
}
