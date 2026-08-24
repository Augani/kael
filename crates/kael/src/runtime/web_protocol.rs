#![cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{
    ipc_transport::{MAX_IPC_FRAME_BYTES, decode_exact_frame, encode_frame},
    process_model::{
        BootstrapMessage, IpcMessage, WORKER_PROTOCOL_VERSION, WorkerError, WorkerProgress,
        WorkerRequest, WorkerResponse,
    },
};

/// Browser workers use the exact protocol version negotiated by native workers.
pub const BROWSER_WORKER_PROTOCOL_VERSION: u32 = WORKER_PROTOCOL_VERSION;

/// Maximum number of requests awaiting a browser-worker response at once.
///
/// This matches the existing worker-pool limit and bounds callback/channel
/// retention even when an unresponsive worker never replies.
pub const MAX_BROWSER_WORKER_PENDING_REQUESTS: usize = 1_024;

pub(crate) const MAX_BROWSER_WORKER_FRAME_BYTES: usize = MAX_IPC_FRAME_BYTES + 4;

pub(crate) type BootstrapIpcMessage = IpcMessage<BootstrapMessage, BootstrapMessage, (), String>;
pub(crate) type WorkerIpcMessage =
    IpcMessage<WorkerRequest, WorkerResponse, WorkerProgress, WorkerError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum BrowserWorkerWireBody {
    Bootstrap(BootstrapIpcMessage),
    Worker(WorkerIpcMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct BrowserWorkerEnvelope {
    version: u32,
    body: BrowserWorkerWireBody,
}

pub(crate) fn encode_browser_worker_message(body: BrowserWorkerWireBody) -> Result<Vec<u8>> {
    let envelope = BrowserWorkerEnvelope {
        version: BROWSER_WORKER_PROTOCOL_VERSION,
        body,
    };
    let payload =
        serde_json::to_vec(&envelope).context("failed to serialize browser worker message")?;
    encode_frame(&payload).context("failed to frame browser worker message")
}

pub(crate) fn decode_browser_worker_message(frame: &[u8]) -> Result<BrowserWorkerWireBody> {
    anyhow::ensure!(
        frame.len() <= MAX_BROWSER_WORKER_FRAME_BYTES,
        "browser worker frame exceeds transport limit"
    );
    let payload = decode_exact_frame(frame).context("invalid browser worker frame")?;
    let envelope: BrowserWorkerEnvelope =
        serde_json::from_slice(&payload).context("failed to deserialize browser worker message")?;
    anyhow::ensure!(
        envelope.version == BROWSER_WORKER_PROTOCOL_VERSION,
        "unsupported browser worker protocol version {}",
        envelope.version
    );
    Ok(envelope.body)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn browser_worker_message_from_js(
    value: &wasm_bindgen::JsValue,
) -> Result<BrowserWorkerWireBody> {
    use wasm_bindgen::JsCast as _;

    anyhow::ensure!(
        value.is_instance_of::<js_sys::Uint8Array>(),
        "browser worker message must be a Uint8Array"
    );
    let bytes = js_sys::Uint8Array::new(value);
    let byte_length = usize::try_from(bytes.byte_length())
        .context("browser worker frame length does not fit usize")?;
    anyhow::ensure!(
        byte_length <= MAX_BROWSER_WORKER_FRAME_BYTES,
        "browser worker frame exceeds transport limit"
    );
    decode_browser_worker_message(&bytes.to_vec())
}

#[cfg(target_arch = "wasm32")]
fn transferable_browser_worker_frame(
    body: BrowserWorkerWireBody,
) -> Result<(js_sys::Uint8Array, js_sys::Array)> {
    let frame = encode_browser_worker_message(body)?;
    let bytes = js_sys::Uint8Array::from(frame.as_slice());
    let transfer = js_sys::Array::new();
    transfer.push(&bytes.buffer());
    Ok((bytes, transfer))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn post_browser_worker_message(
    worker: &web_sys::Worker,
    body: BrowserWorkerWireBody,
) -> Result<()> {
    let (bytes, transfer) = transferable_browser_worker_frame(body)?;
    worker
        .post_message_with_transfer(bytes.as_ref(), transfer.as_ref())
        .map_err(|error| anyhow::anyhow!("failed to post browser worker message: {error:?}"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn post_browser_worker_scope_message(
    scope: &web_sys::DedicatedWorkerGlobalScope,
    body: BrowserWorkerWireBody,
) -> Result<()> {
    let (bytes, transfer) = transferable_browser_worker_frame(body)?;
    scope
        .post_message_with_transfer(bytes.as_ref(), transfer.as_ref())
        .map_err(|error| anyhow::anyhow!("failed to post browser worker message: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_transport::encode_frame;
    use serde_json::json;

    #[test]
    fn browser_worker_protocol_round_trips_typed_messages() {
        let body = BrowserWorkerWireBody::Worker(IpcMessage::Request {
            id: 7,
            body: WorkerRequest::Execute {
                payload: json!({ "operation": "sum", "items": 1_000_000 }),
            },
        });
        let frame = encode_browser_worker_message(body.clone()).unwrap();
        assert_eq!(decode_browser_worker_message(&frame).unwrap(), body);
    }

    #[test]
    fn browser_worker_protocol_rejects_version_drift() {
        let envelope = BrowserWorkerEnvelope {
            version: BROWSER_WORKER_PROTOCOL_VERSION + 1,
            body: BrowserWorkerWireBody::Bootstrap(IpcMessage::Request {
                id: 1,
                body: BootstrapMessage::Handshake {
                    version: BROWSER_WORKER_PROTOCOL_VERSION + 1,
                    capabilities: vec!["worker:execute".to_string()],
                },
            }),
        };
        let payload = serde_json::to_vec(&envelope).unwrap();
        let frame = encode_frame(&payload).unwrap();
        assert!(
            decode_browser_worker_message(&frame)
                .unwrap_err()
                .to_string()
                .contains("unsupported browser worker protocol version")
        );
    }

    #[test]
    fn browser_worker_protocol_rejects_trailing_and_oversized_frames() {
        let mut frame =
            encode_browser_worker_message(BrowserWorkerWireBody::Worker(IpcMessage::Cancel {
                id: 9,
            }))
            .unwrap();
        frame.push(0);
        assert!(decode_browser_worker_message(&frame).is_err());

        assert!(
            decode_browser_worker_message(&vec![0; MAX_BROWSER_WORKER_FRAME_BYTES + 1]).is_err()
        );
    }

    #[test]
    fn browser_worker_protocol_applies_the_shared_ipc_payload_bound() {
        let body = BrowserWorkerWireBody::Worker(IpcMessage::Request {
            id: 1,
            body: WorkerRequest::Execute {
                payload: json!("x".repeat(MAX_IPC_FRAME_BYTES)),
            },
        });
        assert!(encode_browser_worker_message(body).is_err());
    }
}
