#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{WorkerHandle, WorkerPool, WorkerProgressStream};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::mpsc;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    use std::{panic::AssertUnwindSafe, panic::catch_unwind};

    use anyhow::{Context as _, Result, anyhow};
    use serde::{Deserialize, Serialize};

    use crate::ipc_transport::TypedTransport;
    use crate::process_model::{IpcMessage, ProcessId};
    use crate::process_model::{WorkerError, WorkerProgress, WorkerRequest, WorkerResponse};

    const MAX_WORKER_POOL_SIZE: usize = 1_024;

    /// Asynchronous progress channel used by the shared desktop/browser worker API.
    pub type WorkerProgressStream<T> =
        futures::channel::mpsc::UnboundedReceiver<Result<T, WorkerError>>;

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

    /// A handle to a running worker process.
    #[derive(Clone)]
    pub struct WorkerHandle {
        id: ProcessId,
        transport:
            Arc<Mutex<TypedTransport<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>>>,
        next_request_id: Arc<AtomicU64>,
    }

    impl WorkerHandle {
        /// Create a new worker handle wrapping the given transport.
        pub fn new(
            id: ProcessId,
            transport: TypedTransport<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>,
        ) -> Self {
            Self {
                id,
                transport: Arc::new(Mutex::new(transport)),
                next_request_id: Arc::new(AtomicU64::new(1)),
            }
        }

        /// The process identifier of this worker.
        pub fn id(&self) -> ProcessId {
            self.id
        }

        /// Verify the worker is responsive by sending a ping request.
        /// Returns `Ok(())` if the worker replies within a reasonable time,
        /// or an error if the transport is broken.
        pub fn health_check(&self) -> Result<()> {
            let id = next_request_id(&self.next_request_id)?;
            let mut transport = self
                .transport
                .lock()
                .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
            transport.send_request(id, crate::process_model::WorkerRequest::Ping)?;
            loop {
                match transport.recv_message()? {
                    IpcMessage::Response {
                        id: resp_id,
                        result,
                    } if resp_id == id => {
                        return result
                            .map(|_| ())
                            .map_err(|e| anyhow!("worker health check failed: {:?}", e));
                    }
                    _ => {}
                }
            }
        }

        /// Verify worker liveness without blocking the caller's executor thread.
        pub async fn health_check_async(&self) -> Result<()> {
            let handle = self.clone();
            smol::unblock(move || handle.health_check()).await
        }

        /// Send a request and block until a response is received.
        pub fn request<Req, Resp>(&self, request: Req) -> Result<Resp>
        where
            Req: Serialize,
            Resp: for<'de> Deserialize<'de>,
        {
            let payload = serialize_payload(request)?;
            let id = next_request_id(&self.next_request_id)?;
            let mut transport = self
                .transport
                .lock()
                .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
            transport.send_request(id, WorkerRequest::Execute { payload })?;

            loop {
                match transport.recv_message()? {
                    IpcMessage::Response {
                        id: resp_id,
                        result,
                    } if resp_id == id => {
                        let value = result.map_err(|e| anyhow!("worker error: {:?}", e))?;
                        match value {
                            WorkerResponse::Result(v) => {
                                return deserialize_payload(v);
                            }
                            WorkerResponse::Pong => {
                                return Err(anyhow!("unexpected pong response"));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        /// Send a typed request without blocking the caller's executor thread.
        ///
        /// This is the portable request API: native builds perform the socket wait
        /// on the blocking pool and browser builds await a Web Worker message.
        pub async fn request_async<Req, Resp>(&self, request: Req) -> Result<Resp>
        where
            Req: Serialize + Send + 'static,
            Resp: for<'de> Deserialize<'de> + Send + 'static,
        {
            let handle = self.clone();
            smol::unblock(move || handle.request(request)).await
        }

        /// Send a request without waiting for a response.
        pub fn fire_and_forget<Req>(&self, request: Req) -> Result<()>
        where
            Req: Serialize,
        {
            let payload = serialize_payload(request)?;
            let id = next_request_id(&self.next_request_id)?;
            let mut transport = self
                .transport
                .lock()
                .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
            transport.send_request(id, WorkerRequest::Execute { payload })
        }

        /// Send a request and return a receiver for progress updates.
        pub fn stream_progress<Req, Prog>(
            &self,
            request: Req,
        ) -> Result<mpsc::Receiver<Result<Prog, WorkerError>>>
        where
            Req: Serialize + Send + 'static,
            Prog: for<'de> Deserialize<'de> + Send + 'static,
        {
            let payload = serialize_payload(request)?;
            let id = next_request_id(&self.next_request_id)?;
            let transport = Arc::clone(&self.transport);
            let (tx, rx) = mpsc::channel::<Result<Prog, WorkerError>>();

            std::thread::Builder::new()
                .name(format!("kael-worker-progress-{id}"))
                .spawn(move || {
                    let mut transport = match transport.lock() {
                        Ok(t) => t,
                        Err(_) => {
                            let _ = tx.send(Err(WorkerError::Execution(
                                "failed to lock transport".to_string(),
                            )));
                            return;
                        }
                    };

                    if let Err(e) = transport.send_request(id, WorkerRequest::Execute { payload }) {
                        let _ = tx.send(Err(WorkerError::Execution(e.to_string())));
                        return;
                    }

                    loop {
                        match transport.recv_message() {
                            Ok(IpcMessage::Response {
                                id: resp_id,
                                result,
                            }) if resp_id == id => {
                                if let Err(error) = result {
                                    let _ = tx.send(Err(error));
                                }
                                break;
                            }
                            Ok(IpcMessage::Progress {
                                id: prog_id,
                                body: WorkerProgress::Update(value),
                            }) if prog_id == id => match deserialize_payload::<Prog>(value) {
                                Ok(prog) => {
                                    if tx.send(Ok(prog)).is_err() {
                                        let _ = transport.send_cancel(id);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(Err(WorkerError::Execution(e.to_string())));
                                    let _ = transport.send_cancel(id);
                                    break;
                                }
                            },
                            Ok(_) => {}
                            Err(e) => {
                                let _ = tx.send(Err(WorkerError::Execution(e.to_string())));
                                break;
                            }
                        }
                    }
                })
                .context("failed to spawn worker progress reader")?;

            Ok(rx)
        }

        /// Send a typed request and receive progress through an async channel.
        pub fn stream_progress_async<Req, Prog>(
            &self,
            request: Req,
        ) -> Result<WorkerProgressStream<Prog>>
        where
            Req: Serialize + Send + 'static,
            Prog: for<'de> Deserialize<'de> + Send + 'static,
        {
            let progress = self.stream_progress(request)?;
            let (tx, rx) = futures::channel::mpsc::unbounded();
            std::thread::Builder::new()
                .name("kael-worker-async-progress".to_string())
                .spawn(move || {
                    while let Ok(update) = progress.recv() {
                        if tx.unbounded_send(update).is_err() {
                            break;
                        }
                    }
                })
                .context("failed to spawn async worker progress forwarder")?;
            Ok(rx)
        }

        /// Native worker handles are ready after their blocking bootstrap completes.
        pub fn is_ready(&self) -> bool {
            true
        }

        /// Native transport failures are returned by individual operations.
        pub fn last_error(&self) -> Option<String> {
            None
        }
    }

    fn next_request_id(counter: &AtomicU64) -> Result<u64> {
        counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow!("worker request identifier space exhausted"))
    }

    /// A pool of worker processes for load distribution.
    pub struct WorkerPool {
        workers: Vec<WorkerHandle>,
        next_index: std::sync::atomic::AtomicUsize,
    }

    impl WorkerPool {
        /// Create an empty worker pool.
        pub fn new() -> Self {
            Self {
                workers: Vec::new(),
                next_index: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        /// Add a worker handle to the pool.
        pub fn add(&mut self, handle: WorkerHandle) {
            let _ = self.try_add(handle);
        }

        /// Add a unique, nonzero worker handle within the pool capacity.
        pub fn try_add(&mut self, handle: WorkerHandle) -> Result<()> {
            if handle.id().0 == 0 {
                return Err(anyhow!("worker process identifier must be nonzero"));
            }
            if self.workers.len() >= MAX_WORKER_POOL_SIZE {
                return Err(anyhow!("worker pool capacity exceeded"));
            }
            if self.workers.iter().any(|worker| worker.id() == handle.id()) {
                return Err(anyhow!("duplicate worker process identifier"));
            }
            self.workers.push(handle);
            Ok(())
        }

        /// Return the number of workers in the pool.
        pub fn len(&self) -> usize {
            self.workers.len()
        }

        /// Return whether the pool has no workers.
        pub fn is_empty(&self) -> bool {
            self.workers.is_empty()
        }

        /// Send a request to the next worker in round-robin order.
        pub fn request<Req, Resp>(&self, request: Req) -> Result<Resp>
        where
            Req: Serialize,
            Resp: for<'de> Deserialize<'de>,
        {
            if self.workers.is_empty() {
                return Err(anyhow!("worker pool is empty"));
            }
            let idx = self.next_index.fetch_add(1, Ordering::Relaxed) % self.workers.len();
            self.workers[idx].request(request)
        }

        /// Send a request to the next worker through the portable async API.
        pub async fn request_async<Req, Resp>(&self, request: Req) -> Result<Resp>
        where
            Req: Serialize + Send + 'static,
            Resp: for<'de> Deserialize<'de> + Send + 'static,
        {
            if self.workers.is_empty() {
                return Err(anyhow!("worker pool is empty"));
            }
            let idx = self.next_index.fetch_add(1, Ordering::Relaxed) % self.workers.len();
            self.workers[idx].request_async(request).await
        }
    }

    impl Default for WorkerPool {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::ipc_transport::InMemoryTransport;
        use serde_json::json;

        type TestTransport =
            TypedTransport<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>;

        fn worker_pair() -> (WorkerHandle, TestTransport) {
            let (host, worker) = InMemoryTransport::pair();
            (
                WorkerHandle::new(ProcessId(7), TypedTransport::new(Box::new(host))),
                TypedTransport::new(Box::new(worker)),
            )
        }

        struct PanickingSerialize;

        impl Serialize for PanickingSerialize {
            fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                panic!("serializer panic")
            }
        }

        struct PanickingDeserialize;

        impl<'de> Deserialize<'de> for PanickingDeserialize {
            fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                panic!("deserializer panic")
            }
        }

        #[test]
        fn worker_request_ids_fail_closed_at_exhaustion() {
            let counter = AtomicU64::new(u64::MAX - 1);
            assert_eq!(next_request_id(&counter).unwrap(), u64::MAX - 1);
            assert!(next_request_id(&counter).is_err());
            assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        }

        #[test]
        fn progress_stream_forwards_updates_and_worker_errors() {
            let (handle, mut worker) = worker_pair();
            let worker_thread = std::thread::spawn(move || {
                let IpcMessage::Request { id, .. } = worker.recv_message().unwrap() else {
                    panic!("expected request");
                };
                worker
                    .send_progress(id, WorkerProgress::Update(json!({ "step": 1 })))
                    .unwrap();
                worker
                    .send_response(id, Err(WorkerError::Execution("failed".into())))
                    .unwrap();
            });

            let progress = handle
                .stream_progress::<_, serde_json::Value>(json!({ "op": "progress" }))
                .unwrap();
            assert_eq!(progress.recv().unwrap().unwrap(), json!({ "step": 1 }));
            assert_eq!(
                progress.recv().unwrap(),
                Err(WorkerError::Execution("failed".into()))
            );
            assert!(progress.recv().is_err());
            worker_thread.join().unwrap();
        }

        #[test]
        fn dropping_progress_receiver_cancels_request_after_next_update() {
            let (handle, mut worker) = worker_pair();
            let progress = handle
                .stream_progress::<_, serde_json::Value>(json!({ "op": "progress" }))
                .unwrap();
            drop(progress);

            let IpcMessage::Request { id, .. } = worker.recv_message().unwrap() else {
                panic!("expected request");
            };
            worker
                .send_progress(id, WorkerProgress::Update(json!({ "step": 1 })))
                .unwrap();
            assert_eq!(worker.recv_message().unwrap(), IpcMessage::Cancel { id });
        }

        #[test]
        fn custom_serde_panics_are_contained() {
            assert!(serialize_payload(PanickingSerialize).is_err());
            assert!(deserialize_payload::<PanickingDeserialize>(json!(null)).is_err());
        }

        #[test]
        fn async_worker_request_uses_the_native_transport_off_thread() {
            let (handle, mut worker) = worker_pair();
            let worker_thread = std::thread::spawn(move || {
                let IpcMessage::Request { id, body } = worker.recv_message().unwrap() else {
                    panic!("expected request");
                };
                assert!(matches!(body, WorkerRequest::Execute { .. }));
                worker
                    .send_response(id, Ok(WorkerResponse::Result(json!({ "value": 42 }))))
                    .unwrap();
            });

            let response: serde_json::Value =
                smol::block_on(handle.request_async(json!({ "operation": "answer" }))).unwrap();
            assert_eq!(response, json!({ "value": 42 }));
            worker_thread.join().unwrap();
        }

        #[test]
        fn worker_pool_rejects_zero_duplicate_and_excess_workers() {
            let (handle, _) = worker_pair();
            let mut pool = WorkerPool::new();
            pool.try_add(handle.clone()).unwrap();
            assert!(pool.try_add(handle.clone()).is_err());

            let (host, _worker) = InMemoryTransport::pair();
            let zero = WorkerHandle::new(ProcessId(0), TypedTransport::new(Box::new(host)));
            assert!(pool.try_add(zero).is_err());

            pool.workers = vec![handle.clone(); MAX_WORKER_POOL_SIZE];
            let (host, _worker) = InMemoryTransport::pair();
            let extra = WorkerHandle::new(ProcessId(8), TypedTransport::new(Box::new(host)));
            assert!(pool.try_add(extra).is_err());
            assert_eq!(pool.len(), MAX_WORKER_POOL_SIZE);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{WorkerHandle, WorkerPool, WorkerProgressStream};
