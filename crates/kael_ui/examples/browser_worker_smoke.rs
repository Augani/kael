#[cfg(target_arch = "wasm32")]
mod browser {
    use std::time::Duration;

    use anyhow::{Context as _, Result, anyhow};
    use futures::{StreamExt as _, future::Either};
    use kael::{
        ProcessClass, ProcessId, ProcessInfo, Timer, WorkerClient, WorkerError, WorkerHost,
        WorkerProgress, WorkerRequest, WorkerResponse,
    };
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::JsCast as _;

    const PROBE_ITEMS: u64 = 1_000_000;
    const UI_OFFLOAD_CPU_FLOOR_MILLIS: u64 = 25;

    #[derive(Debug, Serialize, Deserialize)]
    struct ProbeRequest {
        items: u64,
        emit_progress: bool,
        minimum_cpu_millis: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ProbeResponse {
        items: u64,
        checksum: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct ProbeProgress {
        completed: u64,
    }

    pub fn start() {
        if js_sys::global().is_instance_of::<web_sys::DedicatedWorkerGlobalScope>() {
            start_worker();
        } else {
            wasm_bindgen_futures::spawn_local(async {
                if let Err(error) = run_host_probe().await {
                    publish_failure(&error.to_string());
                }
            });
        }
    }

    fn start_worker() {
        let result = WorkerClient::connect_from_env().and_then(|client| {
            client.run(|request, on_progress| match request {
                WorkerRequest::Ping => Ok(WorkerResponse::Pong),
                WorkerRequest::Execute { payload } => {
                    let request: ProbeRequest = serde_json::from_value(payload)
                        .map_err(|error| WorkerError::Execution(error.to_string()))?;
                    if request.emit_progress {
                        on_progress(WorkerProgress::Update(
                            serde_json::to_value(ProbeProgress {
                                completed: request.items / 2,
                            })
                            .map_err(|error| WorkerError::Execution(error.to_string()))?,
                        ));
                    }
                    let mut checksum = 0x9e37_79b9_7f4a_7c15_u64;
                    for index in 0..request.items {
                        checksum = checksum
                            .rotate_left(7)
                            .wrapping_add(index ^ 0xa5a5_a5a5_a5a5_a5a5);
                    }
                    let cpu_deadline = web_time::Instant::now()
                        + Duration::from_millis(request.minimum_cpu_millis);
                    while web_time::Instant::now() < cpu_deadline {
                        checksum = checksum.rotate_left(7).wrapping_add(1);
                    }
                    Ok(WorkerResponse::Result(
                        serde_json::to_value(ProbeResponse {
                            items: request.items,
                            checksum,
                        })
                        .map_err(|error| WorkerError::Execution(error.to_string()))?,
                    ))
                }
            })
        });
        if let Err(error) = result {
            web_sys::console::error_1(&format!("Kael worker startup failed: {error}").into());
        }
    }

    async fn run_host_probe() -> Result<()> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .context("browser worker smoke requires a Document")?;
        let root = document
            .document_element()
            .context("browser worker smoke requires a document element")?;
        set_marker(&root, "data-kael-worker-probe", "running")?;

        let mut host = WorkerHost::with_temp_dir();
        let info = ProcessInfo::worker(ProcessId(0), "kael-browser-worker-smoke")
            .executable("./browser_worker_bootstrap.js");
        let handle = host.spawn_worker(ProcessClass::Worker, info)?;
        handle.health_check_async().await?;

        let synchronous_error = handle
            .request::<_, ProbeResponse>(ProbeRequest {
                items: 1,
                emit_progress: false,
                minimum_cpu_millis: 0,
            })
            .expect_err("browser synchronous worker request must fail explicitly");
        anyhow::ensure!(
            synchronous_error.to_string().contains("asynchronous"),
            "browser synchronous worker error did not direct callers to request_async"
        );

        let mut progress = handle.stream_progress_async::<_, ProbeProgress>(ProbeRequest {
            items: PROBE_ITEMS,
            emit_progress: true,
            minimum_cpu_millis: 0,
        })?;
        let progress_update = next_progress_with_timeout(&mut progress).await?;
        anyhow::ensure!(
            progress_update.completed == PROBE_ITEMS / 2,
            "browser worker progress payload differed"
        );

        let response = handle.request_async::<_, ProbeResponse>(ProbeRequest {
            items: PROBE_ITEMS,
            emit_progress: false,
            minimum_cpu_millis: UI_OFFLOAD_CPU_FLOOR_MILLIS,
        });
        let ui_tick = Timer::after(Duration::from_millis(1));
        futures::pin_mut!(response, ui_tick);
        let response = match futures::future::select(ui_tick, response).await {
            Either::Left((_ui_tick, response)) => response.await?,
            Either::Right((response, _ui_tick)) => {
                response?;
                return Err(anyhow!(
                    "worker CPU response completed before the UI event-loop timer"
                ));
            }
        };
        anyhow::ensure!(response.items == PROBE_ITEMS, "worker item count differed");
        anyhow::ensure!(
            response.checksum != 0,
            "worker checksum was unexpectedly zero"
        );

        let worker_id = handle.id();
        host.terminate_worker(worker_id)?;
        anyhow::ensure!(
            host.worker_count() == 0,
            "worker host retained a terminated worker"
        );
        anyhow::ensure!(
            handle
                .request_async::<_, ProbeResponse>(ProbeRequest {
                    items: 1,
                    emit_progress: false,
                    minimum_cpu_millis: 0,
                })
                .await
                .is_err(),
            "terminated browser worker accepted a request"
        );

        set_marker(&root, "data-kael-worker-protocol", "1")?;
        set_marker(&root, "data-kael-worker-items", &PROBE_ITEMS.to_string())?;
        set_marker(&root, "data-kael-worker-progress", "passed")?;
        set_marker(&root, "data-kael-worker-ui-thread", "responsive")?;
        set_marker(&root, "data-kael-worker-terminated", "passed")?;
        set_marker(&root, "data-kael-worker-probe", "passed")?;
        Ok(())
    }

    fn set_marker(root: &web_sys::Element, name: &str, value: &str) -> Result<()> {
        root.set_attribute(name, value)
            .map_err(|error| anyhow!("failed to publish {name}: {error:?}"))
    }

    async fn next_progress_with_timeout(
        progress: &mut kael::WorkerProgressStream<ProbeProgress>,
    ) -> Result<ProbeProgress> {
        let next = progress.next();
        let timeout = Timer::after(Duration::from_secs(10));
        futures::pin_mut!(next, timeout);
        match futures::future::select(next, timeout).await {
            Either::Left((Some(Ok(progress)), _)) => Ok(progress),
            Either::Left((Some(Err(error)), _)) => Err(anyhow!(
                "browser worker progress failed: {}",
                error.to_text()
            )),
            Either::Left((None, _)) => Err(anyhow!("browser worker progress ended early")),
            Either::Right((_deadline, _)) => Err(anyhow!("browser worker progress timed out")),
        }
    }

    fn publish_failure(message: &str) {
        web_sys::console::error_1(&message.into());
        if let Some(root) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
        {
            let _ = root.set_attribute("data-kael-worker-probe", "failed");
            let _ = root.set_attribute("data-kael-worker-error", message);
        }
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    browser::start();

    #[cfg(not(target_arch = "wasm32"))]
    println!("browser_worker_smoke is a WebAssembly-only maintained probe");
}
