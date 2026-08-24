use std::{
    cell::RefCell,
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow};
use futures::{
    channel::{mpsc as async_mpsc, oneshot},
    future::{Either, select},
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast as _, closure::Closure};
use web_sys::{ErrorEvent, Event, MessageEvent, Worker};

use crate::{
    Timer,
    process_model::{
        BootstrapMessage, IpcMessage, ProcessId, WorkerError, WorkerProgress, WorkerRequest,
        WorkerResponse,
    },
    runtime::{
        MAX_BROWSER_WORKER_PENDING_REQUESTS,
        web_protocol::{
            BrowserWorkerWireBody, browser_worker_message_from_js, post_browser_worker_message,
        },
    },
};

const MAX_WORKER_POOL_SIZE: usize = 1_024;
const DEFAULT_BROWSER_WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const BROWSER_WORKER_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);

/// Asynchronous progress channel used by the shared desktop/browser worker API.
pub type WorkerProgressStream<T> = async_mpsc::UnboundedReceiver<Result<T, WorkerError>>;

fn serialize_payload<T: Serialize>(value: T) -> Result<serde_json::Value> {
    catch_unwind(AssertUnwindSafe(|| serde_json::to_value(value)))
        .map_err(|_| anyhow!("request serializer panicked"))?
        .context("failed to serialize request")
}

fn deserialize_payload<T>(value: serde_json::Value) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    catch_unwind(AssertUnwindSafe(|| serde_json::from_value(value)))
        .map_err(|_| anyhow!("response deserializer panicked"))?
        .context("failed to deserialize response")
}

#[derive(Debug, Clone)]
enum BrowserWorkerLifecycle {
    Starting,
    Ready,
    Failed(String),
    Terminated,
}

enum PendingRequest {
    Response(oneshot::Sender<Result<WorkerResponse, WorkerError>>),
    Progress {
        deliver: Box<dyn FnMut(Result<WorkerProgress, WorkerError>) -> bool>,
    },
}

impl PendingRequest {
    fn fail(self, message: &str) {
        let error = WorkerError::Execution(message.to_string());
        match self {
            Self::Response(sender) => {
                let _ = sender.send(Err(error));
            }
            Self::Progress { mut deliver } => {
                let _ = deliver(Err(error));
            }
        }
    }
}

struct BrowserWorkerState {
    lifecycle: BrowserWorkerLifecycle,
    requested_capabilities: Vec<String>,
    pending: HashMap<u64, PendingRequest>,
    ready_waiters: Vec<oneshot::Sender<Result<(), String>>>,
}

impl BrowserWorkerState {
    fn fail(&mut self, message: String) -> bool {
        if matches!(
            self.lifecycle,
            BrowserWorkerLifecycle::Failed(_) | BrowserWorkerLifecycle::Terminated
        ) {
            return false;
        }
        self.lifecycle = BrowserWorkerLifecycle::Failed(message.clone());
        for (_, pending) in self.pending.drain() {
            pending.fail(&message);
        }
        for waiter in self.ready_waiters.drain(..) {
            let _ = waiter.send(Err(message.clone()));
        }
        true
    }

    fn terminate(&mut self) {
        if matches!(self.lifecycle, BrowserWorkerLifecycle::Terminated) {
            return;
        }
        let message = "browser worker was terminated".to_string();
        for (_, pending) in self.pending.drain() {
            pending.fail(&message);
        }
        for waiter in self.ready_waiters.drain(..) {
            let _ = waiter.send(Err(message.clone()));
        }
        self.lifecycle = BrowserWorkerLifecycle::Terminated;
    }
}

struct BrowserWorkerEndpoint {
    worker: Worker,
    state: Rc<RefCell<BrowserWorkerState>>,
    message_listener: Closure<dyn FnMut(MessageEvent)>,
    error_listener: Closure<dyn FnMut(ErrorEvent)>,
    message_error_listener: Closure<dyn FnMut(Event)>,
    on_failure: Rc<dyn Fn(String)>,
}

impl BrowserWorkerEndpoint {
    fn terminate(&self) {
        self.state.borrow_mut().terminate();
        self.detach_and_terminate();
    }

    fn fail(&self, message: String) {
        if self.state.borrow_mut().fail(message.clone()) {
            (self.on_failure)(message);
        }
        self.detach_and_terminate();
    }

    fn detach_and_terminate(&self) {
        self.worker.set_onmessage(None);
        self.worker.set_onerror(None);
        self.worker.set_onmessageerror(None);
        self.worker.terminate();
        let _ = (
            &self.message_listener,
            &self.error_listener,
            &self.message_error_listener,
        );
    }
}

impl Drop for BrowserWorkerEndpoint {
    fn drop(&mut self) {
        self.detach_and_terminate();
    }
}

thread_local! {
    static BROWSER_WORKERS: RefCell<HashMap<u64, Rc<BrowserWorkerEndpoint>>> =
        RefCell::new(HashMap::new());
}

fn browser_worker_endpoint(id: ProcessId) -> Result<Rc<BrowserWorkerEndpoint>> {
    BROWSER_WORKERS.with(|workers| {
        workers
            .borrow()
            .get(&id.0)
            .cloned()
            .ok_or_else(|| anyhow!("browser worker {} is not registered", id.0))
    })
}

fn truncate_browser_error(message: &str) -> String {
    const MAX_BROWSER_ERROR_BYTES: usize = 4 * 1_024;
    if message.len() <= MAX_BROWSER_ERROR_BYTES {
        return message.to_string();
    }
    let mut end = MAX_BROWSER_ERROR_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &message[..end])
}

fn fail_browser_worker(id: ProcessId, message: String) {
    let endpoint = BROWSER_WORKERS.with(|workers| workers.borrow().get(&id.0).cloned());
    if let Some(endpoint) = endpoint {
        endpoint.fail(message);
    }
}

fn handle_browser_worker_message(
    state: &Rc<RefCell<BrowserWorkerState>>,
    body: BrowserWorkerWireBody,
) -> Result<Option<IpcMessage<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>>> {
    match body {
        BrowserWorkerWireBody::Bootstrap(message) => {
            let IpcMessage::Response {
                id: 1,
                result:
                    Ok(BootstrapMessage::HandshakeAck {
                        heartbeat_interval_secs,
                        granted_capabilities,
                    }),
            } = message
            else {
                return Err(anyhow!("unexpected browser worker bootstrap response"));
            };
            let ack = BootstrapMessage::HandshakeAck {
                heartbeat_interval_secs,
                granted_capabilities: granted_capabilities.clone(),
            };
            ack.validate()?;

            let mut state = state.borrow_mut();
            anyhow::ensure!(
                matches!(state.lifecycle, BrowserWorkerLifecycle::Starting),
                "browser worker sent a duplicate bootstrap response"
            );
            anyhow::ensure!(
                granted_capabilities
                    .iter()
                    .all(|capability| state.requested_capabilities.contains(capability)),
                "browser worker granted an unrequested capability"
            );
            anyhow::ensure!(
                granted_capabilities.len() == state.requested_capabilities.len(),
                "browser worker did not grant all required capabilities"
            );
            state.lifecycle = BrowserWorkerLifecycle::Ready;
            for waiter in state.ready_waiters.drain(..) {
                let _ = waiter.send(Ok(()));
            }
            Ok(None)
        }
        BrowserWorkerWireBody::Worker(message) => {
            let mut state = state.borrow_mut();
            anyhow::ensure!(
                matches!(state.lifecycle, BrowserWorkerLifecycle::Ready),
                "browser worker message arrived before bootstrap completed"
            );

            match message {
                IpcMessage::Response { id, result } => {
                    anyhow::ensure!(id != 0, "browser worker response id must be nonzero");
                    let Some(pending) = state.pending.remove(&id) else {
                        // Fire-and-forget requests intentionally have no retained
                        // callback. Their terminal responses are safe to ignore.
                        return Ok(None);
                    };
                    match pending {
                        PendingRequest::Response(sender) => {
                            let _ = sender.send(result);
                        }
                        PendingRequest::Progress { mut deliver } => {
                            if let Err(error) = result {
                                let _ = deliver(Err(error));
                            }
                        }
                    }
                    Ok(None)
                }
                IpcMessage::Progress { id, body } => {
                    anyhow::ensure!(id != 0, "browser worker progress id must be nonzero");
                    let keep = match state.pending.get_mut(&id) {
                        Some(PendingRequest::Progress { deliver }) => deliver(Ok(body)),
                        Some(PendingRequest::Response(_)) => {
                            return Err(anyhow!(
                                "browser worker sent progress for a non-streaming request"
                            ));
                        }
                        None => return Ok(None),
                    };
                    if !keep {
                        state.pending.remove(&id);
                        return Ok(Some(IpcMessage::Cancel { id }));
                    }
                    Ok(None)
                }
                IpcMessage::Request { .. } | IpcMessage::Cancel { .. } => {
                    Err(anyhow!("browser worker sent a host-only worker message"))
                }
            }
        }
    }
}

/// A handle to a dedicated browser Web Worker.
#[derive(Clone)]
pub struct WorkerHandle {
    id: ProcessId,
    next_request_id: Arc<AtomicU64>,
}

impl WorkerHandle {
    pub(crate) fn new_browser(
        id: ProcessId,
        worker: Worker,
        requested_capabilities: Vec<String>,
        on_failure: Rc<dyn Fn(String)>,
    ) -> Result<Self> {
        anyhow::ensure!(id.0 != 0, "browser worker identifier must be nonzero");
        let handshake = BootstrapMessage::Handshake {
            version: crate::process_model::WORKER_PROTOCOL_VERSION,
            capabilities: requested_capabilities.clone(),
        };
        handshake.validate()?;

        let state = Rc::new(RefCell::new(BrowserWorkerState {
            lifecycle: BrowserWorkerLifecycle::Starting,
            requested_capabilities,
            pending: HashMap::new(),
            ready_waiters: Vec::new(),
        }));

        let message_state = Rc::clone(&state);
        let message_worker = worker.clone();
        let message_failure = Rc::clone(&on_failure);
        let message_listener = Closure::wrap(Box::new(move |event: MessageEvent| {
            let result = browser_worker_message_from_js(&event.data())
                .and_then(|message| handle_browser_worker_message(&message_state, message));
            match result {
                Ok(Some(cancel)) => {
                    if let Err(error) = post_browser_worker_message(
                        &message_worker,
                        BrowserWorkerWireBody::Worker(cancel),
                    ) {
                        let message = error.to_string();
                        if message_state.borrow_mut().fail(message.clone()) {
                            message_failure(message);
                        }
                        message_worker.terminate();
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    let message = truncate_browser_error(&error.to_string());
                    if message_state.borrow_mut().fail(message.clone()) {
                        message_failure(message);
                    }
                    message_worker.terminate();
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let error_state = Rc::clone(&state);
        let error_worker = worker.clone();
        let error_failure = Rc::clone(&on_failure);
        let error_listener = Closure::wrap(Box::new(move |event: ErrorEvent| {
            event.prevent_default();
            let detail = if event.message().is_empty() {
                "browser worker reported an error".to_string()
            } else {
                format!("browser worker error: {}", event.message())
            };
            let message = truncate_browser_error(&detail);
            if error_state.borrow_mut().fail(message.clone()) {
                error_failure(message);
            }
            error_worker.terminate();
        }) as Box<dyn FnMut(ErrorEvent)>);

        let message_error_state = Rc::clone(&state);
        let message_error_worker = worker.clone();
        let message_error_failure = Rc::clone(&on_failure);
        let message_error_listener = Closure::wrap(Box::new(move |_event: Event| {
            let message = "browser worker message could not be deserialized".to_string();
            if message_error_state.borrow_mut().fail(message.clone()) {
                message_error_failure(message);
            }
            message_error_worker.terminate();
        }) as Box<dyn FnMut(Event)>);

        worker.set_onmessage(Some(message_listener.as_ref().unchecked_ref()));
        worker.set_onerror(Some(error_listener.as_ref().unchecked_ref()));
        worker.set_onmessageerror(Some(message_error_listener.as_ref().unchecked_ref()));

        let endpoint = Rc::new(BrowserWorkerEndpoint {
            worker,
            state,
            message_listener,
            error_listener,
            message_error_listener,
            on_failure,
        });
        BROWSER_WORKERS.with(|workers| {
            let previous = workers.borrow_mut().insert(id.0, Rc::clone(&endpoint));
            anyhow::ensure!(previous.is_none(), "duplicate browser worker identifier");
            Ok::<(), anyhow::Error>(())
        })?;

        if let Err(error) = post_browser_worker_message(
            &endpoint.worker,
            BrowserWorkerWireBody::Bootstrap(IpcMessage::Request {
                id: 1,
                body: handshake,
            }),
        ) {
            BROWSER_WORKERS.with(|workers| {
                workers.borrow_mut().remove(&id.0);
            });
            endpoint.fail(error.to_string());
            return Err(error);
        }

        Ok(Self {
            id,
            next_request_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// The process identifier assigned to this Web Worker.
    pub fn id(&self) -> ProcessId {
        self.id
    }

    /// Browser worker health checks are asynchronous; this method fails
    /// explicitly so callers cannot accidentally block the JavaScript thread.
    pub fn health_check(&self) -> Result<()> {
        Err(anyhow!(
            "browser worker health checks are asynchronous; use health_check_async"
        ))
    }

    /// Verify that the worker completed the versioned handshake and answers a ping.
    pub async fn health_check_async(&self) -> Result<()> {
        match self
            .send_request_async(WorkerRequest::Ping, DEFAULT_BROWSER_WORKER_REQUEST_TIMEOUT)
            .await?
        {
            WorkerResponse::Pong => Ok(()),
            WorkerResponse::Result(_) => Err(anyhow!("unexpected worker result for health check")),
        }
    }

    /// Browser worker responses are asynchronous; this method fails explicitly
    /// so callers cannot accidentally freeze the UI event loop.
    pub fn request<Req, Resp>(&self, _request: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        Err(anyhow!(
            "browser worker requests are asynchronous; use request_async"
        ))
    }

    /// Send a typed request to the Web Worker and await its typed response.
    pub async fn request_async<Req, Resp>(&self, request: Req) -> Result<Resp>
    where
        Req: Serialize + Send + 'static,
        Resp: for<'de> Deserialize<'de> + Send + 'static,
    {
        self.request_async_with_timeout(request, DEFAULT_BROWSER_WORKER_REQUEST_TIMEOUT)
            .await
    }

    /// Send a typed request with an explicit bounded response timeout.
    pub async fn request_async_with_timeout<Req, Resp>(
        &self,
        request: Req,
        timeout: Duration,
    ) -> Result<Resp>
    where
        Req: Serialize + Send + 'static,
        Resp: for<'de> Deserialize<'de> + Send + 'static,
    {
        anyhow::ensure!(
            timeout > Duration::ZERO,
            "worker request timeout must be positive"
        );
        let payload = serialize_payload(request)?;
        match self
            .send_request_async(WorkerRequest::Execute { payload }, timeout)
            .await?
        {
            WorkerResponse::Result(value) => deserialize_payload(value),
            WorkerResponse::Pong => Err(anyhow!("unexpected pong response")),
        }
    }

    async fn send_request_async(
        &self,
        request: WorkerRequest,
        timeout: Duration,
    ) -> Result<WorkerResponse> {
        self.wait_until_ready().await?;
        let id = next_request_id(&self.next_request_id)?;
        let endpoint = browser_worker_endpoint(self.id)?;
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = endpoint.state.borrow_mut();
            ensure_worker_can_send(&state)?;
            anyhow::ensure!(
                state.pending.len() < MAX_BROWSER_WORKER_PENDING_REQUESTS,
                "browser worker pending request limit exceeded"
            );
            state.pending.insert(id, PendingRequest::Response(sender));
        }
        if let Err(error) = post_browser_worker_message(
            &endpoint.worker,
            BrowserWorkerWireBody::Worker(IpcMessage::Request { id, body: request }),
        ) {
            endpoint.state.borrow_mut().pending.remove(&id);
            return Err(error);
        }

        let timeout_future = Timer::after(timeout);
        futures::pin_mut!(timeout_future);
        match select(receiver, timeout_future).await {
            Either::Left((result, _)) => result
                .map_err(|_| anyhow!("browser worker response channel closed"))?
                .map_err(|error| anyhow!("worker error: {}", error.to_text())),
            Either::Right((_deadline, _)) => {
                endpoint.state.borrow_mut().pending.remove(&id);
                let _ = post_browser_worker_message(
                    &endpoint.worker,
                    BrowserWorkerWireBody::Worker(IpcMessage::Cancel { id }),
                );
                Err(anyhow!("browser worker request timed out"))
            }
        }
    }

    /// Send a typed request without retaining a response callback.
    pub fn fire_and_forget<Req>(&self, request: Req) -> Result<()>
    where
        Req: Serialize,
    {
        let payload = serialize_payload(request)?;
        let id = next_request_id(&self.next_request_id)?;
        let endpoint = browser_worker_endpoint(self.id)?;
        ensure_worker_can_send(&endpoint.state.borrow())?;
        post_browser_worker_message(
            &endpoint.worker,
            BrowserWorkerWireBody::Worker(IpcMessage::Request {
                id,
                body: WorkerRequest::Execute { payload },
            }),
        )
    }

    /// Send a typed request and expose progress through the legacy receiver.
    ///
    /// `recv` must not be called on the browser main thread; use `try_recv` or
    /// [`Self::stream_progress_async`] there.
    pub fn stream_progress<Req, Prog>(
        &self,
        request: Req,
    ) -> Result<mpsc::Receiver<Result<Prog, WorkerError>>>
    where
        Req: Serialize + Send + 'static,
        Prog: for<'de> Deserialize<'de> + Send + 'static,
    {
        let payload = serialize_payload(request)?;
        let (sender, receiver) = mpsc::channel();
        self.register_progress_request(payload, move |result| {
            let result = decode_progress(result);
            sender.send(result).is_ok()
        })?;
        Ok(receiver)
    }

    /// Send a typed request and expose progress through an async channel.
    pub fn stream_progress_async<Req, Prog>(
        &self,
        request: Req,
    ) -> Result<WorkerProgressStream<Prog>>
    where
        Req: Serialize + Send + 'static,
        Prog: for<'de> Deserialize<'de> + Send + 'static,
    {
        let payload = serialize_payload(request)?;
        let (sender, receiver) = async_mpsc::unbounded();
        self.register_progress_request(payload, move |result| {
            sender.unbounded_send(decode_progress(result)).is_ok()
        })?;
        Ok(receiver)
    }

    fn register_progress_request(
        &self,
        payload: serde_json::Value,
        deliver: impl FnMut(Result<WorkerProgress, WorkerError>) -> bool + 'static,
    ) -> Result<()> {
        let id = next_request_id(&self.next_request_id)?;
        let endpoint = browser_worker_endpoint(self.id)?;
        {
            let mut state = endpoint.state.borrow_mut();
            ensure_worker_can_send(&state)?;
            anyhow::ensure!(
                state.pending.len() < MAX_BROWSER_WORKER_PENDING_REQUESTS,
                "browser worker pending request limit exceeded"
            );
            state.pending.insert(
                id,
                PendingRequest::Progress {
                    deliver: Box::new(deliver),
                },
            );
        }
        if let Err(error) = post_browser_worker_message(
            &endpoint.worker,
            BrowserWorkerWireBody::Worker(IpcMessage::Request {
                id,
                body: WorkerRequest::Execute { payload },
            }),
        ) {
            endpoint.state.borrow_mut().pending.remove(&id);
            return Err(error);
        }
        Ok(())
    }

    async fn wait_until_ready(&self) -> Result<()> {
        let endpoint = browser_worker_endpoint(self.id)?;
        let receiver = {
            let mut state = endpoint.state.borrow_mut();
            match &state.lifecycle {
                BrowserWorkerLifecycle::Ready => return Ok(()),
                BrowserWorkerLifecycle::Failed(message) => {
                    return Err(anyhow!("browser worker failed: {message}"));
                }
                BrowserWorkerLifecycle::Terminated => {
                    return Err(anyhow!("browser worker was terminated"));
                }
                BrowserWorkerLifecycle::Starting => {
                    let (sender, receiver) = oneshot::channel();
                    state.ready_waiters.push(sender);
                    receiver
                }
            }
        };

        let timeout = Timer::after(BROWSER_WORKER_BOOTSTRAP_TIMEOUT);
        futures::pin_mut!(timeout);
        match select(receiver, timeout).await {
            Either::Left((result, _)) => result
                .map_err(|_| anyhow!("browser worker bootstrap channel closed"))?
                .map_err(|error| anyhow!("browser worker bootstrap failed: {error}")),
            Either::Right((_deadline, _)) => {
                let message = "browser worker bootstrap timed out".to_string();
                fail_browser_worker(self.id, message.clone());
                Err(anyhow!(message))
            }
        }
    }

    /// Whether the worker has completed its versioned bootstrap handshake.
    pub fn is_ready(&self) -> bool {
        browser_worker_endpoint(self.id).is_ok_and(|endpoint| {
            matches!(
                endpoint.state.borrow().lifecycle,
                BrowserWorkerLifecycle::Ready
            )
        })
    }

    /// Last terminal browser-worker error, when one was recorded.
    pub fn last_error(&self) -> Option<String> {
        browser_worker_endpoint(self.id).ok().and_then(|endpoint| {
            match &endpoint.state.borrow().lifecycle {
                BrowserWorkerLifecycle::Failed(message) => Some(message.clone()),
                BrowserWorkerLifecycle::Starting
                | BrowserWorkerLifecycle::Ready
                | BrowserWorkerLifecycle::Terminated => None,
            }
        })
    }

    pub(crate) fn terminate_browser(&self) -> Result<()> {
        let endpoint = BROWSER_WORKERS.with(|workers| workers.borrow_mut().remove(&self.id.0));
        let endpoint =
            endpoint.ok_or_else(|| anyhow!("browser worker {} is not registered", self.id.0))?;
        endpoint.terminate();
        Ok(())
    }
}

fn decode_progress<Prog>(result: Result<WorkerProgress, WorkerError>) -> Result<Prog, WorkerError>
where
    Prog: for<'de> Deserialize<'de>,
{
    match result {
        Ok(WorkerProgress::Update(value)) => {
            deserialize_payload(value).map_err(|error| WorkerError::Execution(error.to_string()))
        }
        Err(error) => Err(error),
    }
}

fn ensure_worker_can_send(state: &BrowserWorkerState) -> Result<()> {
    match &state.lifecycle {
        BrowserWorkerLifecycle::Starting | BrowserWorkerLifecycle::Ready => Ok(()),
        BrowserWorkerLifecycle::Failed(message) => Err(anyhow!("browser worker failed: {message}")),
        BrowserWorkerLifecycle::Terminated => Err(anyhow!("browser worker was terminated")),
    }
}

fn next_request_id(counter: &AtomicU64) -> Result<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| anyhow!("worker request identifier space exhausted"))
}

/// A round-robin pool of browser Web Workers.
pub struct WorkerPool {
    workers: Vec<WorkerHandle>,
    next_index: AtomicUsize,
}

impl WorkerPool {
    /// Create an empty worker pool.
    pub fn new() -> Self {
        Self {
            workers: Vec::new(),
            next_index: AtomicUsize::new(0),
        }
    }

    /// Add a worker, ignoring invalid additions for backward compatibility.
    pub fn add(&mut self, handle: WorkerHandle) {
        let _ = self.try_add(handle);
    }

    /// Add a unique, nonzero worker within the bounded pool capacity.
    pub fn try_add(&mut self, handle: WorkerHandle) -> Result<()> {
        anyhow::ensure!(
            handle.id().0 != 0,
            "worker process identifier must be nonzero"
        );
        anyhow::ensure!(
            self.workers.len() < MAX_WORKER_POOL_SIZE,
            "worker pool capacity exceeded"
        );
        anyhow::ensure!(
            !self.workers.iter().any(|worker| worker.id() == handle.id()),
            "duplicate worker process identifier"
        );
        self.workers.push(handle);
        Ok(())
    }

    /// Number of workers in the pool.
    pub fn len(&self) -> usize {
        self.workers.len()
    }

    /// Whether the pool contains no workers.
    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    /// Browser requests cannot block; use [`Self::request_async`].
    pub fn request<Req, Resp>(&self, _request: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        Err(anyhow!(
            "browser worker pool requests are asynchronous; use request_async"
        ))
    }

    /// Send a request to the next Web Worker in round-robin order.
    pub async fn request_async<Req, Resp>(&self, request: Req) -> Result<Resp>
    where
        Req: Serialize + Send + 'static,
        Resp: for<'de> Deserialize<'de> + Send + 'static,
    {
        anyhow::ensure!(!self.workers.is_empty(), "worker pool is empty");
        let index = self.next_index.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        self.workers[index].request_async(request).await
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}
