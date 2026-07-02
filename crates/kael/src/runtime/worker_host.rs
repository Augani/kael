use anyhow::{Context as _, Result, anyhow};
use std::path::PathBuf;

use crate::{
    Tracer,
    ipc_transport::TypedTransport,
    process_model::{
        BootstrapMessage, ProcessClass, ProcessInfo, ProcessSpawnOptions,
        ProcessSpawnOptionsBuilder, SupervisorEvent, WorkerError, WorkerProgress, WorkerRequest,
        WorkerResponse,
    },
    supervisor::ProcessSupervisor,
    worker_api::WorkerHandle,
};

/// Manages worker processes with bootstrap handshake and health monitoring.
pub struct WorkerHost {
    supervisor: ProcessSupervisor,
    #[allow(dead_code)]
    socket_dir: PathBuf,
}

impl WorkerHost {
    /// Create a new worker host with the given socket directory.
    pub fn new(socket_dir: impl Into<PathBuf>) -> Self {
        Self {
            supervisor: ProcessSupervisor::new(),
            socket_dir: socket_dir.into(),
        }
    }

    /// Create a new worker host using the system temp directory.
    pub fn with_temp_dir() -> Self {
        Self::new(std::env::temp_dir())
    }

    /// Attach a tracer for observability.
    pub fn with_tracer(mut self, tracer: Tracer) -> Self {
        self.supervisor = self.supervisor.with_tracer(tracer);
        self
    }

    /// Register a callback to receive supervisor lifecycle events.
    pub fn on_event<F>(&mut self, callback: F)
    where
        F: Fn(SupervisorEvent) + Send + Sync + 'static,
    {
        self.supervisor.on_event(callback);
    }

    /// Spawn a worker process and return a handle after bootstrap.
    pub fn spawn_worker(&mut self, class: ProcessClass, info: ProcessInfo) -> Result<WorkerHandle> {
        self.spawn_worker_with_options(
            class,
            info,
            ProcessSpawnOptionsBuilder::new()
                .restart_on_failure(3, std::time::Duration::from_secs(1))
                .build_checked()?,
        )
    }

    /// Spawn a worker process with explicit, validated supervision options.
    pub fn spawn_worker_with_options(
        &mut self,
        class: ProcessClass,
        mut info: ProcessInfo,
        options: ProcessSpawnOptions,
    ) -> Result<WorkerHandle> {
        info.validate()?;
        options.validate()?;
        let socket_name = format!("gpui-worker-{}", uuid::Uuid::new_v4());

        #[cfg(not(target_os = "windows"))]
        let socket_path = self.socket_dir.join(format!("{}.sock", socket_name));
        #[cfg(target_os = "windows")]
        let pipe_name = socket_name;

        info.class = class;

        #[cfg(not(target_os = "windows"))]
        {
            info = info.env(
                "GPUI_WORKER_SOCKET",
                socket_path.to_string_lossy().to_string(),
            );
        }
        #[cfg(target_os = "windows")]
        {
            info = info.env("GPUI_WORKER_PIPE", format!("\\\\.\\pipe\\{}", pipe_name));
        }

        #[cfg(not(target_os = "windows"))]
        let listener = {
            let _ = std::fs::remove_file(&socket_path);
            std::os::unix::net::UnixListener::bind(&socket_path)
                .with_context(|| format!("failed to bind unix socket: {}", socket_path.display()))?
        };

        let id = self.supervisor.spawn_with_options(info, options)?;

        #[cfg(not(target_os = "windows"))]
        let transport = {
            let (stream, _) = listener
                .accept()
                .context("failed to accept worker connection")?;
            stream
                .set_nonblocking(false)
                .context("failed to set blocking mode")?;
            let unix_transport =
                crate::ipc_transport::UnixDomainSocketTransport::from_stream(stream)?;
            Box::new(unix_transport) as Box<dyn crate::ipc_transport::Transport>
        };

        #[cfg(target_os = "windows")]
        let transport = {
            let pipe_transport = crate::ipc_transport::NamedPipeTransport::server(&pipe_name)?;
            Box::new(pipe_transport) as Box<dyn crate::ipc_transport::Transport>
        };

        let mut bootstrap =
            TypedTransport::<BootstrapMessage, BootstrapMessage, (), String>::new(transport);

        bootstrap.send_request(
            1,
            BootstrapMessage::Handshake {
                version: 1,
                capabilities: vec!["file-io".to_string(), "network".to_string()],
            },
        )?;

        let ack = bootstrap
            .recv_message()
            .context("failed to receive bootstrap ack")?;
        match ack {
            crate::process_model::IpcMessage::Response {
                id: 1,
                result:
                    Ok(BootstrapMessage::HandshakeAck {
                        heartbeat_interval_secs,
                        granted_capabilities,
                    }),
            } => {
                let _ = heartbeat_interval_secs;
                let _ = granted_capabilities;
            }
            _ => return Err(anyhow!("unexpected bootstrap response: {:?}", ack)),
        }

        let worker_transport =
            TypedTransport::<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>::new(
                bootstrap.into_inner(),
            );

        Ok(WorkerHandle::new(id, worker_transport))
    }

    /// Access the underlying supervisor.
    pub fn supervisor(&self) -> &ProcessSupervisor {
        &self.supervisor
    }

    /// Access the underlying supervisor mutably.
    pub fn supervisor_mut(&mut self) -> &mut ProcessSupervisor {
        &mut self.supervisor
    }
}
