use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};

use crate::ipc_transport::{Transport, TypedTransport};
use crate::process_model::{
    BootstrapMessage, IpcMessage, WorkerError, WorkerProgress, WorkerRequest, WorkerResponse,
};

/// Client that runs inside a worker process and communicates with the host.
pub struct WorkerClient {
    transport:
        Arc<Mutex<TypedTransport<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>>>,
}

impl WorkerClient {
    /// Connect to the host using the socket path or pipe name from the environment.
    pub fn connect_from_env() -> Result<Self> {
        #[cfg(not(target_os = "windows"))]
        {
            let socket_path =
                std::env::var("KAEL_WORKER_SOCKET").context("KAEL_WORKER_SOCKET not set")?;
            Self::connect(&socket_path)
        }
        #[cfg(target_os = "windows")]
        {
            let pipe_name =
                std::env::var("KAEL_WORKER_PIPE").context("KAEL_WORKER_PIPE not set")?;
            Self::connect(&pipe_name)
        }
    }

    /// Connect to the host at the given path.
    pub fn connect(path: &str) -> Result<Self> {
        #[cfg(not(target_os = "windows"))]
        let raw_transport = {
            let t = crate::ipc_transport::UnixDomainSocketTransport::connect(path)
                .with_context(|| format!("failed to connect to unix socket: {}", path))?;
            Box::new(t) as Box<dyn Transport>
        };

        #[cfg(target_os = "windows")]
        let raw_transport = {
            let t = crate::ipc_transport::NamedPipeTransport::client(path)
                .with_context(|| format!("failed to connect to named pipe: {}", path))?;
            Box::new(t) as Box<dyn Transport>
        };

        let mut bootstrap =
            TypedTransport::<BootstrapMessage, BootstrapMessage, (), String>::new(raw_transport);

        let msg = bootstrap
            .recv_message()
            .context("failed to receive bootstrap handshake")?;
        let summary = msg.to_text();
        match msg {
            IpcMessage::Request {
                id: 1,
                body:
                    BootstrapMessage::Handshake {
                        version,
                        capabilities,
                    },
            } => {
                let handshake = BootstrapMessage::Handshake {
                    version,
                    capabilities: capabilities.clone(),
                };
                handshake.validate()?;
                anyhow::ensure!(
                    version == 1,
                    "unsupported worker bootstrap protocol version"
                );
                bootstrap.send_response(
                    1,
                    Ok(BootstrapMessage::HandshakeAck {
                        heartbeat_interval_secs: 5,
                        granted_capabilities: capabilities,
                    }),
                )?;
            }
            _ => return Err(anyhow!("unexpected bootstrap message: {summary}")),
        }

        let worker_transport =
            TypedTransport::<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>::new(
                bootstrap.into_inner(),
            );

        Ok(Self {
            transport: Arc::new(Mutex::new(worker_transport)),
        })
    }

    /// Run the worker event loop, invoking the provided handler for each request.
    pub fn run<F>(&self, mut handler: F) -> Result<()>
    where
        F: FnMut(
            WorkerRequest,
            Box<dyn Fn(WorkerProgress) + Send>,
        ) -> Result<WorkerResponse, WorkerError>,
    {
        loop {
            let msg = {
                let mut transport = self
                    .transport
                    .lock()
                    .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
                transport.recv_message()
            };

            match msg {
                Ok(IpcMessage::Request { id, body }) => {
                    let transport = Arc::clone(&self.transport);
                    let progress_fn: Box<dyn Fn(WorkerProgress) + Send> = Box::new(move |prog| {
                        if let Ok(mut t) = transport.lock() {
                            let _ = t.send_progress(id, prog);
                        }
                    });

                    let result = handler(body, progress_fn);

                    let mut transport = self
                        .transport
                        .lock()
                        .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
                    match result {
                        Ok(resp) => transport.send_response(id, Ok(resp))?,
                        Err(e) => transport.send_response(id, Err(e))?,
                    }
                }
                Ok(IpcMessage::Cancel { id }) => {
                    let mut transport = self
                        .transport
                        .lock()
                        .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
                    transport.send_response(id, Err(WorkerError::Cancelled))?;
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(anyhow!("worker transport error: {}", e));
                }
            }
        }
    }

    /// Send a progress update for the given request id.
    pub fn send_progress(&self, id: u64, progress: WorkerProgress) -> Result<()> {
        let mut transport = self
            .transport
            .lock()
            .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
        transport.send_progress(id, progress)?;
        Ok(())
    }

    /// Send a response for the given request id.
    pub fn send_response(
        &self,
        id: u64,
        response: Result<WorkerResponse, WorkerError>,
    ) -> Result<()> {
        let mut transport = self
            .transport
            .lock()
            .map_err(|_| anyhow!("worker transport lock is poisoned"))?;
        transport.send_response(id, response)?;
        Ok(())
    }
}
