//! Cross-platform IPC transport for the GPUI process-isolation model.
//!
//! Provides typed message exchange between processes using Unix domain sockets
//! on macOS/Linux and named pipes on Windows. An in-memory transport is
//! available for testing.

use std::path::PathBuf;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::process_model::IpcMessage;

/// Maximum serialized payload accepted by Kael IPC transports.
pub const MAX_IPC_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRANSPORT_FRAME_BYTES: usize = MAX_IPC_FRAME_BYTES + 4;

// ---------------------------------------------------------------------------
// Transport trait
// ---------------------------------------------------------------------------

/// A bidirectional transport for exchanging serialized messages between
/// processes.
pub trait Transport: Send {
    /// Send one complete length-prefixed serialized message frame.
    fn send_frame(&mut self, data: &[u8]) -> Result<()>;
    /// Receive the next serialized message frame.
    fn recv_frame(&mut self) -> Result<Vec<u8>>;
    /// Close the transport.
    fn close(&mut self) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Frame encoding
// ---------------------------------------------------------------------------

/// Encode a payload as a length-prefixed frame.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>> {
    anyhow::ensure!(
        payload.len() <= MAX_IPC_FRAME_BYTES,
        "IPC payload exceeds {MAX_IPC_FRAME_BYTES} byte limit"
    );
    let len = u32::try_from(payload.len()).context("IPC payload length exceeds wire format")?;
    let mut frame = Vec::with_capacity(4usize.saturating_add(payload.len()));
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decode a length-prefixed frame from the given buffer.
/// Returns the payload and the number of bytes consumed.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    anyhow::ensure!(
        len <= MAX_IPC_FRAME_BYTES,
        "declared IPC payload exceeds {MAX_IPC_FRAME_BYTES} byte limit"
    );
    let consumed = 4usize
        .checked_add(len)
        .ok_or_else(|| anyhow!("IPC frame length overflow"))?;
    if buf.len() < consumed {
        return Ok(None);
    }
    let payload = buf[4..consumed].to_vec();
    Ok(Some((payload, consumed)))
}

/// Decode exactly one complete frame, rejecting incomplete or trailing bytes.
pub fn decode_exact_frame(buf: &[u8]) -> Result<Vec<u8>> {
    let (payload, consumed) = decode_frame(buf)?.ok_or_else(|| anyhow!("incomplete IPC frame"))?;
    anyhow::ensure!(consumed == buf.len(), "IPC frame contains trailing bytes");
    Ok(payload)
}

/// Content-safe summary of a length-prefixed IPC frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcFrameSummary {
    /// Total bytes present in the frame buffer.
    pub buffer_len_bytes: usize,
    /// Declared payload length, when the length prefix is present.
    pub declared_payload_len_bytes: Option<usize>,
    /// Number of bytes consumed by a complete frame.
    pub consumed_len_bytes: Option<usize>,
    /// Whether the frame has enough bytes for the declared payload.
    pub complete: bool,
}

impl IpcFrameSummary {
    /// Returns true when the frame includes a length prefix.
    pub fn has_length_prefix(&self) -> bool {
        self.declared_payload_len_bytes.is_some()
    }

    /// Returns true when extra bytes are present after the first complete frame.
    pub fn has_trailing_bytes(&self) -> bool {
        self.consumed_len_bytes
            .is_some_and(|consumed| self.buffer_len_bytes > consumed)
    }

    /// Content-safe frame summary that does not expose payload bytes.
    pub fn to_text(&self) -> String {
        format!(
            "ipc_frame(buffer_len_bytes={}, has_length_prefix={}, declared_payload_len_bytes={}, complete={}, consumed_len_bytes={}, has_trailing_bytes={})",
            self.buffer_len_bytes,
            self.has_length_prefix(),
            self.declared_payload_len_bytes.unwrap_or(0),
            self.complete,
            self.consumed_len_bytes.unwrap_or(0),
            self.has_trailing_bytes()
        )
    }
}

/// Inspect a length-prefixed IPC frame without decoding or logging payload bytes.
pub fn frame_summary(buf: &[u8]) -> IpcFrameSummary {
    if buf.len() < 4 {
        return IpcFrameSummary {
            buffer_len_bytes: buf.len(),
            declared_payload_len_bytes: None,
            consumed_len_bytes: None,
            complete: false,
        };
    }

    let payload_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let consumed = 4usize.checked_add(payload_len);
    let complete = payload_len <= MAX_IPC_FRAME_BYTES
        && consumed.is_some_and(|consumed| buf.len() >= consumed);
    IpcFrameSummary {
        buffer_len_bytes: buf.len(),
        declared_payload_len_bytes: Some(payload_len),
        consumed_len_bytes: complete.then_some(consumed).flatten(),
        complete,
    }
}

// ---------------------------------------------------------------------------
// Typed transport wrapper
// ---------------------------------------------------------------------------

/// A wrapper around a [`Transport`] that handles serialization.
pub struct TypedTransport<Request, Response, Progress, Error> {
    inner: Box<dyn Transport>,
    _phantom: std::marker::PhantomData<(Request, Response, Progress, Error)>,
}

impl<Request, Response, Progress, Error> TypedTransport<Request, Response, Progress, Error>
where
    Request: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    Response: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    Progress: Serialize + for<'de> Deserialize<'de> + Send + 'static,
    Error: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    /// Wrap an existing transport.
    pub fn new(inner: Box<dyn Transport>) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Send a request.
    pub fn send_request(&mut self, id: u64, body: Request) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Request { id, body };
        let payload = serde_json::to_vec(&msg).context("failed to serialize request")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send a response.
    pub fn send_response(&mut self, id: u64, result: Result<Response, Error>) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Response { id, result };
        let payload = serde_json::to_vec(&msg).context("failed to serialize response")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send a progress update.
    pub fn send_progress(&mut self, id: u64, body: Progress) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Progress { id, body };
        let payload = serde_json::to_vec(&msg).context("failed to serialize progress")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Send a cancellation signal.
    pub fn send_cancel(&mut self, id: u64) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Cancel { id };
        let payload = serde_json::to_vec(&msg).context("failed to serialize cancel")?;
        self.inner.send_frame(&encode_frame(&payload)?)
    }

    /// Receive the next message.
    pub fn recv_message(&mut self) -> Result<IpcMessage<Request, Response, Progress, Error>> {
        let frame = self.inner.recv_frame()?;
        let payload = decode_exact_frame(&frame)?;
        serde_json::from_slice(&payload).context("failed to deserialize message")
    }

    /// Unwrap the underlying transport.
    pub fn into_inner(self) -> Box<dyn Transport> {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// In-memory transport (for testing)
// ---------------------------------------------------------------------------

use std::sync::mpsc::{Receiver, Sender, channel};

/// An in-memory transport pair for testing.
pub struct InMemoryTransport {
    tx: Option<Sender<Vec<u8>>>,
    rx: Option<Receiver<Vec<u8>>>,
}

impl InMemoryTransport {
    /// Create a connected pair of in-memory transports.
    pub fn pair() -> (InMemoryTransport, InMemoryTransport) {
        let (a_tx, a_rx) = channel::<Vec<u8>>();
        let (b_tx, b_rx) = channel::<Vec<u8>>();
        (
            InMemoryTransport {
                tx: Some(a_tx),
                rx: Some(b_rx),
            },
            InMemoryTransport {
                tx: Some(b_tx),
                rx: Some(a_rx),
            },
        )
    }
}

impl Transport for InMemoryTransport {
    fn send_frame(&mut self, data: &[u8]) -> Result<()> {
        anyhow::ensure!(
            data.len() <= MAX_TRANSPORT_FRAME_BYTES,
            "IPC frame exceeds transport limit"
        );
        self.tx
            .as_ref()
            .ok_or_else(|| anyhow!("in-memory transport is closed"))?
            .send(data.to_vec())
            .map_err(|_| anyhow!("in-memory transport disconnected"))
    }

    fn recv_frame(&mut self) -> Result<Vec<u8>> {
        self.rx
            .as_ref()
            .ok_or_else(|| anyhow!("in-memory transport is closed"))?
            .recv()
            .map_err(|_| anyhow!("in-memory transport disconnected"))
    }

    fn close(&mut self) -> Result<()> {
        self.tx = None;
        self.rx = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Socket path resolution
// ---------------------------------------------------------------------------

fn validate_ipc_path_component(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!value.is_empty(), "{label} cannot be empty");
    anyhow::ensure!(value.len() <= 255, "{label} cannot exceed 255 bytes");
    anyhow::ensure!(
        value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')),
        "{label} contains unsupported characters"
    );
    Ok(())
}

fn resolve_ipc_socket_path(app_id: &str, process_name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(format!(
            "\\\\.\\pipe\\{}",
            crate::platform::pipe_name(app_id, process_name)
        ))
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::resolve_socket_path(app_id, process_name)
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::resolve_socket_path(app_id, process_name)
    }
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
    {
        let base = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(base).join(format!("{}-{}.sock", app_id, process_name))
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        PathBuf::from(format!("{}-{}", app_id, process_name))
    }
}

/// Resolve a checked platform-appropriate IPC endpoint.
pub fn try_ipc_socket_path(app_id: &str, process_name: &str) -> Result<PathBuf> {
    validate_ipc_path_component(app_id, "IPC app id")?;
    validate_ipc_path_component(process_name, "IPC process name")?;
    let path = resolve_ipc_socket_path(app_id, process_name);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        anyhow::ensure!(
            path.as_os_str().as_bytes().len() <= 100,
            "IPC socket path exceeds platform limit"
        );
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt as _;
        anyhow::ensure!(
            path.as_os_str().encode_wide().count() <= 256,
            "IPC named-pipe path exceeds platform limit"
        );
    }
    Ok(path)
}

/// Resolve a platform-appropriate IPC endpoint.
///
/// Invalid path-like identifiers are mapped to a deterministic safe endpoint.
/// Use [`try_ipc_socket_path`] when invalid identifiers should be reported.
pub fn ipc_socket_path(app_id: &str, process_name: &str) -> PathBuf {
    try_ipc_socket_path(app_id, process_name).unwrap_or_else(|_| {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in app_id
            .bytes()
            .chain(std::iter::once(0xff))
            .chain(process_name.bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        resolve_ipc_socket_path("kael-invalid", &format!("endpoint-{hash:016x}"))
    })
}

// ---------------------------------------------------------------------------
// Unix domain socket transport
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;

    use anyhow::{Context as _, Result};

    use super::{MAX_TRANSPORT_FRAME_BYTES, Transport};

    /// Transport over a Unix domain socket.
    pub struct UnixDomainSocketTransport {
        stream: UnixStream,
        closed: bool,
    }

    impl UnixDomainSocketTransport {
        /// Connect to a Unix domain socket at the given path.
        pub fn connect(path: impl AsRef<Path>) -> Result<Self> {
            let stream = UnixStream::connect(path.as_ref()).with_context(|| {
                format!(
                    "failed to connect to unix socket {}",
                    path.as_ref().display()
                )
            })?;
            stream.set_nonblocking(false)?;
            Ok(Self {
                stream,
                closed: false,
            })
        }

        /// Wrap an existing Unix stream.
        pub fn from_stream(stream: UnixStream) -> Result<Self> {
            stream.set_nonblocking(false)?;
            Ok(Self {
                stream,
                closed: false,
            })
        }

        /// Create a connected pair of Unix domain sockets.
        pub fn pair() -> Result<(Self, Self)> {
            let (a, b) = UnixStream::pair().context("failed to create unix socket pair")?;
            a.set_nonblocking(false)?;
            b.set_nonblocking(false)?;
            Ok((
                Self {
                    stream: a,
                    closed: false,
                },
                Self {
                    stream: b,
                    closed: false,
                },
            ))
        }

        /// Bind a Unix domain socket and wait for a client connection.
        ///
        /// The caller owns removal of the socket path after this returns.
        pub fn listen(path: impl AsRef<Path>) -> Result<Self> {
            let path = path.as_ref();
            let listener = std::os::unix::net::UnixListener::bind(path)
                .with_context(|| format!("failed to bind unix socket {}", path.display()))?;
            let (stream, _) = listener
                .accept()
                .with_context(|| format!("failed to accept connection on {}", path.display()))?;
            stream.set_nonblocking(false)?;
            Ok(Self {
                stream,
                closed: false,
            })
        }
    }

    impl Transport for UnixDomainSocketTransport {
        fn send_frame(&mut self, data: &[u8]) -> Result<()> {
            anyhow::ensure!(!self.closed, "unix socket transport is closed");
            anyhow::ensure!(
                data.len() <= MAX_TRANSPORT_FRAME_BYTES,
                "IPC frame exceeds transport limit"
            );
            let len = u32::try_from(data.len()).context("IPC transport frame is too large")?;
            self.stream
                .write_all(&len.to_be_bytes())
                .context("failed to write frame length to unix socket")?;
            self.stream
                .write_all(data)
                .context("failed to write frame payload to unix socket")?;
            self.stream.flush().context("failed to flush unix socket")?;
            Ok(())
        }

        fn recv_frame(&mut self) -> Result<Vec<u8>> {
            anyhow::ensure!(!self.closed, "unix socket transport is closed");
            let mut len_buf = [0u8; 4];
            self.stream
                .read_exact(&mut len_buf)
                .context("failed to read frame length from unix socket")?;
            let len = u32::from_be_bytes(len_buf) as usize;
            anyhow::ensure!(
                len <= MAX_TRANSPORT_FRAME_BYTES,
                "declared IPC payload exceeds transport limit"
            );
            let mut payload = vec![0u8; len];
            self.stream
                .read_exact(&mut payload)
                .context("failed to read frame payload from unix socket")?;
            Ok(payload)
        }

        fn close(&mut self) -> Result<()> {
            if self.closed {
                return Ok(());
            }
            let result = self
                .stream
                .shutdown(std::net::Shutdown::Both)
                .context("failed to shutdown unix socket");
            self.closed = true;
            result
        }
    }
}

#[cfg(unix)]
pub use unix::UnixDomainSocketTransport;

// ---------------------------------------------------------------------------
// Windows named pipe transport
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows_pipe {
    use std::io::{Read, Write};
    use std::os::windows::io::FromRawHandle;

    use anyhow::{Context as _, Result, anyhow};
    use windows::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE,
            OPEN_EXISTING,
        },
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
    };
    use windows_core::PCWSTR;

    use super::{MAX_TRANSPORT_FRAME_BYTES, Transport};

    const PIPE_ACCESS_DUPLEX_VALUE: FILE_FLAGS_AND_ATTRIBUTES = FILE_FLAGS_AND_ATTRIBUTES(3u32);

    /// Transport over a Windows named pipe.
    pub struct NamedPipeTransport {
        file: Option<std::fs::File>,
    }

    impl NamedPipeTransport {
        /// Create a named pipe server and wait for a client connection.
        pub fn server(pipe_name: &str) -> Result<Self> {
            let name: Vec<u16> = format!("\\\\.\\pipe\\{}", pipe_name)
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let handle = unsafe {
                CreateNamedPipeW(
                    PCWSTR(name.as_ptr()),
                    PIPE_ACCESS_DUPLEX_VALUE,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    1,
                    65536,
                    65536,
                    0,
                    None,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(anyhow!("failed to create named pipe server"));
            }
            let result = unsafe { ConnectNamedPipe(handle, None) };
            if result.is_err() {
                let _ = unsafe { CloseHandle(handle) };
                return Err(anyhow!("failed to connect named pipe"));
            }
            let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
            Ok(Self { file: Some(file) })
        }

        /// Connect to an existing named pipe server with retries.
        pub fn client(pipe_name: &str) -> Result<Self> {
            let name: Vec<u16> = format!("\\\\.\\pipe\\{}", pipe_name)
                .encode_utf16()
                .chain(Some(0))
                .collect();

            let mut attempts = 0;
            let handle = loop {
                let h = unsafe {
                    CreateFileW(
                        PCWSTR(name.as_ptr()),
                        0x80000000u32 | 0x40000000u32,
                        FILE_SHARE_NONE,
                        None,
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                };
                if let Ok(h) = h {
                    break h;
                }
                attempts += 1;
                if attempts >= 10 {
                    return Err(anyhow!(
                        "failed to open named pipe client after {} attempts",
                        attempts
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };

            let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
            Ok(Self { file: Some(file) })
        }
    }

    impl Transport for NamedPipeTransport {
        fn send_frame(&mut self, data: &[u8]) -> Result<()> {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| anyhow!("named pipe transport is closed"))?;
            anyhow::ensure!(
                data.len() <= MAX_TRANSPORT_FRAME_BYTES,
                "IPC frame exceeds transport limit"
            );
            let len = u32::try_from(data.len()).context("IPC transport frame is too large")?;
            file.write_all(&len.to_be_bytes())
                .context("failed to write frame length to named pipe")?;
            file.write_all(data)
                .context("failed to write frame payload to named pipe")?;
            file.flush().context("failed to flush named pipe")?;
            Ok(())
        }

        fn recv_frame(&mut self) -> Result<Vec<u8>> {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| anyhow!("named pipe transport is closed"))?;
            let mut len_buf = [0u8; 4];
            file.read_exact(&mut len_buf)
                .context("failed to read frame length from named pipe")?;
            let len = u32::from_be_bytes(len_buf) as usize;
            anyhow::ensure!(
                len <= MAX_TRANSPORT_FRAME_BYTES,
                "declared IPC payload exceeds transport limit"
            );
            let mut payload = vec![0u8; len];
            file.read_exact(&mut payload)
                .context("failed to read frame payload from named pipe")?;
            Ok(payload)
        }

        fn close(&mut self) -> Result<()> {
            self.file = None;
            Ok(())
        }
    }
}

#[cfg(windows)]
pub use windows_pipe::NamedPipeTransport;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_frame() {
        let payload = b"hello world";
        let frame = encode_frame(payload).unwrap();
        assert_eq!(&frame[..4], &(payload.len() as u32).to_be_bytes());
        assert_eq!(&frame[4..], payload);

        let (decoded, consumed) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(consumed, frame.len());
    }

    #[test]
    fn test_decode_frame_incomplete() {
        let buf = vec![0u8; 2];
        assert!(decode_frame(&buf).unwrap().is_none());
    }

    #[test]
    fn exact_frame_decode_rejects_incomplete_and_trailing_data() {
        assert!(decode_exact_frame(&[0, 0, 0]).is_err());

        let mut trailing = encode_frame(b"message").unwrap();
        trailing.extend_from_slice(b"smuggled");
        assert!(decode_exact_frame(&trailing).is_err());
        assert_eq!(
            decode_exact_frame(&encode_frame(b"message").unwrap()).unwrap(),
            b"message"
        );
    }

    #[test]
    fn rejects_oversized_frames_before_allocation() {
        let declared = (MAX_IPC_FRAME_BYTES as u32 + 1).to_be_bytes();
        assert!(decode_frame(&declared).is_err());
        assert!(encode_frame(&vec![0; MAX_IPC_FRAME_BYTES + 1]).is_err());
    }

    #[test]
    fn ipc_frame_summary_is_content_safe() {
        let payload = br#"{"secret":"customer-token","items":[1,2,3]}"#;
        let mut frame = encode_frame(payload).unwrap();
        frame.extend_from_slice(b"trailing-secret");

        let summary = frame_summary(&frame);
        assert!(summary.has_length_prefix());
        assert!(summary.complete);
        assert!(summary.has_trailing_bytes());
        assert_eq!(summary.declared_payload_len_bytes, Some(payload.len()));
        assert_eq!(summary.consumed_len_bytes, Some(4 + payload.len()));

        let text = summary.to_text();
        assert!(text.contains("complete=true"));
        assert!(text.contains("has_trailing_bytes=true"));
        assert!(!text.contains("customer-token"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("trailing-secret"));

        let incomplete = frame_summary(&[0, 0, 0, 8, 1, 2]);
        assert!(incomplete.has_length_prefix());
        assert!(!incomplete.complete);
        assert_eq!(incomplete.consumed_len_bytes, None);
        assert!(incomplete.to_text().contains("complete=false"));
    }

    #[test]
    fn test_in_memory_transport_roundtrip() {
        let (mut a, mut b) = InMemoryTransport::pair();
        a.send_frame(b"ping").unwrap();
        let received = b.recv_frame().unwrap();
        assert_eq!(received, b"ping");
    }

    #[test]
    fn closing_in_memory_transport_disconnects_both_directions() {
        let (mut a, mut b) = InMemoryTransport::pair();
        a.close().unwrap();
        assert!(a.send_frame(b"closed").is_err());
        assert!(a.recv_frame().is_err());
        assert!(b.send_frame(b"peer-closed").is_err());
        assert!(b.recv_frame().is_err());
        assert!(a.close().is_ok());
    }

    #[test]
    fn typed_transport_rejects_trailing_inner_frame_bytes() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct Unit;

        let (mut sender, receiver) = InMemoryTransport::pair();
        let message = IpcMessage::<Unit, Unit, Unit, Unit>::Request { id: 1, body: Unit };
        let mut frame = encode_frame(&serde_json::to_vec(&message).unwrap()).unwrap();
        frame.extend_from_slice(b"smuggled");
        sender.send_frame(&frame).unwrap();

        let mut typed = TypedTransport::<Unit, Unit, Unit, Unit>::new(Box::new(receiver));
        assert!(typed.recv_message().is_err());
    }

    #[test]
    fn test_typed_transport_request_response() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Req {
            Add(i32, i32),
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Resp {
            Sum(i32),
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Prog {
            Halfway,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Err {
            Bad,
        }

        let (ta, tb) = InMemoryTransport::pair();
        let mut client = TypedTransport::<Req, Resp, Prog, Err>::new(Box::new(ta));
        let mut server = TypedTransport::<Req, Resp, Prog, Err>::new(Box::new(tb));

        client.send_request(1, Req::Add(2, 3)).unwrap();
        let msg = server.recv_message().unwrap();
        assert_eq!(
            msg,
            IpcMessage::Request {
                id: 1,
                body: Req::Add(2, 3)
            }
        );

        server.send_response(1, Ok(Resp::Sum(5))).unwrap();
        let msg = client.recv_message().unwrap();
        assert_eq!(
            msg,
            IpcMessage::Response {
                id: 1,
                result: Ok(Resp::Sum(5))
            }
        );
    }

    #[test]
    fn test_ipc_socket_path() {
        let path = ipc_socket_path("com.example.app", "worker-1");
        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert!(file_name.contains("com.example.app"));
        assert!(file_name.contains("worker-1"));

        assert!(try_ipc_socket_path("../../escape", "worker").is_err());
        assert!(try_ipc_socket_path("app", "../escape").is_err());
        #[cfg(unix)]
        assert!(try_ipc_socket_path(&"a".repeat(90), "worker").is_err());
        let fallback = ipc_socket_path("../../escape", "../worker");
        assert!(
            !fallback
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        );
        assert!(!fallback.to_string_lossy().contains("../worker"));
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_domain_socket_transport_roundtrip() {
        let (mut a, mut b) = UnixDomainSocketTransport::pair().unwrap();
        a.send_frame(b"ping").unwrap();
        let received = b.recv_frame().unwrap();
        assert_eq!(received, b"ping");
        a.close().unwrap();
        a.close().unwrap();
        assert!(a.send_frame(b"closed").is_err());
        assert!(b.recv_frame().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_domain_socket_typed_transport() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Req {
            Echo(String),
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Resp {
            Echo(String),
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Prog {
            Started,
        }
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        enum Err {
            Fail,
        }

        let (ta, tb) = UnixDomainSocketTransport::pair().unwrap();
        let mut client = TypedTransport::<Req, Resp, Prog, Err>::new(Box::new(ta));
        let mut server = TypedTransport::<Req, Resp, Prog, Err>::new(Box::new(tb));

        client
            .send_request(1, Req::Echo("hello".to_string()))
            .unwrap();
        let msg = server.recv_message().unwrap();
        assert_eq!(
            msg,
            IpcMessage::Request {
                id: 1,
                body: Req::Echo("hello".to_string())
            }
        );

        server
            .send_response(1, Ok(Resp::Echo("hello".to_string())))
            .unwrap();
        let msg = client.recv_message().unwrap();
        assert_eq!(
            msg,
            IpcMessage::Response {
                id: 1,
                result: Ok(Resp::Echo("hello".to_string()))
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_named_pipe_transport_roundtrip() {
        let pipe_name = format!("kael-test-{}", std::process::id());

        let server_name = pipe_name.clone();
        let server_handle = std::thread::spawn(move || {
            let mut server = NamedPipeTransport::server(&server_name).unwrap();
            server.send_frame(b"pong").unwrap();
            server.recv_frame().unwrap()
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut client = NamedPipeTransport::client(&pipe_name).unwrap();
        let received = client.recv_frame().unwrap();
        assert_eq!(received, b"pong");
        client.send_frame(b"ping").unwrap();

        let server_received = server_handle.join().unwrap();
        assert_eq!(server_received, b"ping");
    }
}
