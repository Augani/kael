use std::sync::mpsc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::ipc_transport::TypedTransport;
use crate::process_model::{IpcMessage, ProcessId};
use crate::process_model::{WorkerError, WorkerProgress, WorkerRequest, WorkerResponse};

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
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let mut transport = self.transport.lock().unwrap();
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

    /// Send a request and block until a response is received.
    pub fn request<Req, Resp>(&self, request: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        let payload = serde_json::to_value(request).context("failed to serialize request")?;
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let mut transport = self.transport.lock().unwrap();
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
                            return serde_json::from_value(v)
                                .context("failed to deserialize response");
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

    /// Send a request without waiting for a response.
    pub fn fire_and_forget<Req>(&self, request: Req) -> Result<()>
    where
        Req: Serialize,
    {
        let payload = serde_json::to_value(request).context("failed to serialize request")?;
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let mut transport = self.transport.lock().unwrap();
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
        let payload = serde_json::to_value(request).context("failed to serialize request")?;
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let transport = Arc::clone(&self.transport);
        let (tx, rx) = mpsc::channel::<Result<Prog, WorkerError>>();

        std::thread::spawn(move || {
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
                        result: _,
                    }) if resp_id == id => break,
                    Ok(IpcMessage::Progress {
                        id: prog_id,
                        body: WorkerProgress::Update(value),
                    }) if prog_id == id => match serde_json::from_value::<Prog>(value) {
                        Ok(prog) => {
                            if tx.send(Ok(prog)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(WorkerError::Execution(e.to_string())));
                            break;
                        }
                    },
                    Ok(IpcMessage::Response {
                        id: resp_id,
                        result: Err(e),
                    }) if resp_id == id => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(Err(WorkerError::Execution(e.to_string())));
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
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
        self.workers.push(handle);
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
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}
