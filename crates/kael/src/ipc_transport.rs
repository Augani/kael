//! Cross-platform IPC transport for the GPUI process-isolation model.
//!
//! Provides typed message exchange between processes using Unix domain sockets
//! on macOS/Linux and named pipes on Windows. An in-memory transport is
//! available for testing.

use std::path::PathBuf;

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::process_model::IpcMessage;

// ---------------------------------------------------------------------------
// Transport trait
// ---------------------------------------------------------------------------

/// A bidirectional transport for exchanging serialized messages between
/// processes.
pub trait Transport: Send {
    /// Send a serialized message frame.
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
pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Decode a length-prefixed frame from the given buffer.
/// Returns the payload and the number of bytes consumed.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let payload = buf[4..4 + len].to_vec();
    Ok(Some((payload, 4 + len)))
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
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send a response.
    pub fn send_response(&mut self, id: u64, result: Result<Response, Error>) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Response { id, result };
        let payload = serde_json::to_vec(&msg).context("failed to serialize response")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send a progress update.
    pub fn send_progress(&mut self, id: u64, body: Progress) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Progress { id, body };
        let payload = serde_json::to_vec(&msg).context("failed to serialize progress")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Send a cancellation signal.
    pub fn send_cancel(&mut self, id: u64) -> Result<()> {
        let msg = IpcMessage::<Request, Response, Progress, Error>::Cancel { id };
        let payload = serde_json::to_vec(&msg).context("failed to serialize cancel")?;
        self.inner.send_frame(&encode_frame(&payload))
    }

    /// Receive the next message.
    pub fn recv_message(&mut self) -> Result<IpcMessage<Request, Response, Progress, Error>> {
        let frame = self.inner.recv_frame()?;
        let (payload, _consumed) =
            decode_frame(&frame)?.ok_or_else(|| anyhow!("incomplete frame"))?;
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
    tx: Sender<Vec<u8>>,
    rx: Receiver<Vec<u8>>,
}

impl InMemoryTransport {
    /// Create a connected pair of in-memory transports.
    pub fn pair() -> (InMemoryTransport, InMemoryTransport) {
        let (a_tx, a_rx) = channel::<Vec<u8>>();
        let (b_tx, b_rx) = channel::<Vec<u8>>();
        (
            InMemoryTransport { tx: a_tx, rx: b_rx },
            InMemoryTransport { tx: b_tx, rx: a_rx },
        )
    }
}

impl Transport for InMemoryTransport {
    fn send_frame(&mut self, data: &[u8]) -> Result<()> {
        self.tx
            .send(data.to_vec())
            .map_err(|_| anyhow!("in-memory transport disconnected"))
    }

    fn recv_frame(&mut self) -> Result<Vec<u8>> {
        self.rx
            .recv()
            .map_err(|_| anyhow!("in-memory transport disconnected"))
    }

    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Socket path resolution
// ---------------------------------------------------------------------------

/// Resolve a platform-appropriate IPC socket path for the given app and
/// process identifiers.
pub fn ipc_socket_path(app_id: &str, process_name: &str) -> PathBuf {
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

// ---------------------------------------------------------------------------
// Unix domain socket transport
// ---------------------------------------------------------------------------

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;

    use anyhow::{Context as _, Result};

    use super::{Transport, encode_frame};

    /// Transport over a Unix domain socket.
    pub struct UnixDomainSocketTransport {
        stream: UnixStream,
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
            Ok(Self { stream })
        }

        /// Wrap an existing Unix stream.
        pub fn from_stream(stream: UnixStream) -> Result<Self> {
            stream.set_nonblocking(false)?;
            Ok(Self { stream })
        }

        /// Create a connected pair of Unix domain sockets.
        pub fn pair() -> Result<(Self, Self)> {
            let (a, b) = UnixStream::pair().context("failed to create unix socket pair")?;
            a.set_nonblocking(false)?;
            b.set_nonblocking(false)?;
            Ok((Self { stream: a }, Self { stream: b }))
        }

        /// Bind a Unix domain socket and wait for a client connection.
        pub fn listen(path: impl AsRef<Path>) -> Result<Self> {
            let path = path.as_ref();
            let _ = std::fs::remove_file(path);
            let listener = std::os::unix::net::UnixListener::bind(path)
                .with_context(|| format!("failed to bind unix socket {}", path.display()))?;
            let (stream, _) = listener
                .accept()
                .with_context(|| format!("failed to accept connection on {}", path.display()))?;
            stream.set_nonblocking(false)?;
            Ok(Self { stream })
        }
    }

    impl Transport for UnixDomainSocketTransport {
        fn send_frame(&mut self, data: &[u8]) -> Result<()> {
            let frame = encode_frame(data);
            self.stream
                .write_all(&frame)
                .context("failed to write frame to unix socket")?;
            self.stream.flush().context("failed to flush unix socket")?;
            Ok(())
        }

        fn recv_frame(&mut self) -> Result<Vec<u8>> {
            let mut len_buf = [0u8; 4];
            self.stream
                .read_exact(&mut len_buf)
                .context("failed to read frame length from unix socket")?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut payload = vec![0u8; len];
            self.stream
                .read_exact(&mut payload)
                .context("failed to read frame payload from unix socket")?;
            Ok(payload)
        }

        fn close(&mut self) -> Result<()> {
            self.stream
                .shutdown(std::net::Shutdown::Both)
                .context("failed to shutdown unix socket")
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

    use super::{Transport, encode_frame};

    const PIPE_ACCESS_DUPLEX_VALUE: FILE_FLAGS_AND_ATTRIBUTES = FILE_FLAGS_AND_ATTRIBUTES(3u32);

    /// Transport over a Windows named pipe.
    pub struct NamedPipeTransport {
        file: std::fs::File,
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
            Ok(Self { file })
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
            Ok(Self { file })
        }
    }

    impl Transport for NamedPipeTransport {
        fn send_frame(&mut self, data: &[u8]) -> Result<()> {
            let frame = encode_frame(data);
            self.file
                .write_all(&frame)
                .context("failed to write to named pipe")?;
            self.file.flush().context("failed to flush named pipe")?;
            Ok(())
        }

        fn recv_frame(&mut self) -> Result<Vec<u8>> {
            let mut len_buf = [0u8; 4];
            self.file
                .read_exact(&mut len_buf)
                .context("failed to read frame length from named pipe")?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut payload = vec![0u8; len];
            self.file
                .read_exact(&mut payload)
                .context("failed to read frame payload from named pipe")?;
            Ok(payload)
        }

        fn close(&mut self) -> Result<()> {
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
        let frame = encode_frame(payload);
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
    fn test_in_memory_transport_roundtrip() {
        let (mut a, mut b) = InMemoryTransport::pair();
        a.send_frame(b"ping").unwrap();
        let received = b.recv_frame().unwrap();
        assert_eq!(received, b"ping");
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
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_domain_socket_transport_roundtrip() {
        let (mut a, mut b) = UnixDomainSocketTransport::pair().unwrap();
        a.send_frame(b"ping").unwrap();
        let received = b.recv_frame().unwrap();
        assert_eq!(received, b"ping");
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
