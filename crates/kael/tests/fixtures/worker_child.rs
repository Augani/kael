use anyhow::Result;
use kael::{WorkerClient, WorkerError, WorkerProgress, WorkerRequest, WorkerResponse};

fn main() -> Result<()> {
    let client = WorkerClient::connect_from_env()?;
    client.run(|request, progress| match request {
        WorkerRequest::Ping => Ok(WorkerResponse::Pong),
        WorkerRequest::Execute { payload } => {
            let op = payload
                .get("op")
                .and_then(|value| value.as_str())
                .unwrap_or_default();

            match op {
                "echo" => Ok(WorkerResponse::Result(payload)),
                "progress" => {
                    progress(WorkerProgress::Update(serde_json::json!({ "step": 1 })));
                    progress(WorkerProgress::Update(serde_json::json!({ "step": 2 })));
                    Ok(WorkerResponse::Result(serde_json::json!({ "done": true })))
                }
                "exit" => std::process::exit(13),
                other => Err(WorkerError::Execution(format!("unknown op: {other}"))),
            }
        }
    })
}
