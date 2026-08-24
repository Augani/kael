use std::io::ErrorKind as IoErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::client::ClientRequestBuilder;
use tungstenite::error::ProtocolError;
use tungstenite::http::Uri;
use tungstenite::protocol::frame::{CloseFrame, coding::CloseCode};
use tungstenite::protocol::{Message, WebSocketConfig as TungsteniteConfig};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as TungsteniteError, WebSocket, client_tls_with_config};

use super::{
    EventQueue, WebSocketClose, WebSocketCloseError, WebSocketCloseMetadata, WebSocketConfig,
    WebSocketConnectError, WebSocketErrorKind, WebSocketErrorMetadata, WebSocketEvent,
    WebSocketEventKind, WebSocketMessage, WebSocketOpenMetadata, WebSocketReconnectMetadata,
    WebSocketSendError, WebSocketState,
};

const READ_POLL_INTERVAL: Duration = Duration::from_millis(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const CLOSE_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub(super) struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    shared: Arc<Shared>,
    outbound: SyncSender<WebSocketMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    config: WebSocketConfig,
    events: Mutex<EventQueue>,
    state: AtomicU8,
    shutdown: AtomicBool,
    close_request: Mutex<Option<WebSocketClose>>,
    outbound_messages: AtomicUsize,
    outbound_bytes: AtomicUsize,
}

impl Client {
    pub(super) fn connect(config: WebSocketConfig) -> Result<Self, WebSocketConnectError> {
        let (sender, receiver) = mpsc::sync_channel(config.outbound_capacity);
        let shared = Arc::new(Shared {
            events: Mutex::new(EventQueue::new(&config)),
            config,
            state: AtomicU8::new(encode_state(WebSocketState::Connecting)),
            shutdown: AtomicBool::new(false),
            close_request: Mutex::new(None),
            outbound_messages: AtomicUsize::new(0),
            outbound_bytes: AtomicUsize::new(0),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("kael-websocket".to_string())
            .spawn(move || run_worker(worker_shared, receiver))
            .map_err(|_| WebSocketConnectError::WorkerUnavailable)?;
        Ok(Self {
            inner: Arc::new(ClientInner {
                shared,
                outbound: sender,
                worker: Mutex::new(Some(worker)),
            }),
        })
    }

    pub(super) fn try_send(&self, message: WebSocketMessage) -> Result<(), WebSocketSendError> {
        let bytes = message.len();
        if bytes > self.inner.shared.config.max_message_bytes {
            return Err(WebSocketSendError::MessageTooLarge {
                bytes,
                max_bytes: self.inner.shared.config.max_message_bytes,
            });
        }
        if matches!(
            self.state(),
            WebSocketState::Closing | WebSocketState::Closed
        ) {
            return Err(WebSocketSendError::Closed);
        }
        reserve_bytes(
            &self.inner.shared.outbound_bytes,
            bytes,
            self.inner.shared.config.max_outbound_bytes,
        )?;
        self.inner
            .shared
            .outbound_messages
            .fetch_add(1, Ordering::AcqRel);
        match self.inner.outbound.try_send(message) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.inner
                    .shared
                    .outbound_messages
                    .fetch_sub(1, Ordering::AcqRel);
                self.inner
                    .shared
                    .outbound_bytes
                    .fetch_sub(bytes, Ordering::AcqRel);
                Err(WebSocketSendError::Backpressure)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.inner
                    .shared
                    .outbound_messages
                    .fetch_sub(1, Ordering::AcqRel);
                self.inner
                    .shared
                    .outbound_bytes
                    .fetch_sub(bytes, Ordering::AcqRel);
                Err(WebSocketSendError::Closed)
            }
        }
    }

    pub(super) fn poll_event(&self) -> Option<WebSocketEvent> {
        lock(&self.inner.shared.events).pop()
    }

    pub(super) fn state(&self) -> WebSocketState {
        decode_state(self.inner.shared.state.load(Ordering::Acquire))
    }

    pub(super) fn queued_inbound_events(&self) -> usize {
        lock(&self.inner.shared.events).len()
    }

    pub(super) fn queued_outbound_messages(&self) -> usize {
        self.inner.shared.outbound_messages.load(Ordering::Acquire)
    }

    pub(super) fn queued_outbound_bytes(&self) -> usize {
        self.inner.shared.outbound_bytes.load(Ordering::Acquire)
    }

    pub(super) fn close(&self, close: WebSocketClose) -> Result<(), WebSocketCloseError> {
        request_close(&self.inner.shared, close)
    }
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Release);
        if !matches!(
            decode_state(self.shared.state.load(Ordering::Acquire)),
            WebSocketState::Closing | WebSocketState::Closed
        ) {
            self.shared
                .state
                .store(encode_state(WebSocketState::Closing), Ordering::Release);
            *lock(&self.shared.close_request) = Some(WebSocketClose::normal());
        }

        // Joining here could block a UI thread on DNS or an operating-system TLS
        // handshake. The worker owns no client handle and terminates after the
        // bounded socket timeout, so detach it on final-handle drop.
        let _detached = lock(&self.worker).take();
    }
}

fn run_worker(shared: Arc<Shared>, receiver: mpsc::Receiver<WebSocketMessage>) {
    let mut reconnect_attempt = 0_u16;
    loop {
        if shared.shutdown.load(Ordering::Acquire) {
            break;
        }
        if lock(&shared.close_request).is_some() {
            terminal_local_close(&shared);
            break;
        }

        set_state(
            &shared,
            if reconnect_attempt == 0 {
                WebSocketState::Connecting
            } else {
                WebSocketState::Reconnecting
            },
        );

        match open_socket(&shared.config) {
            Ok((mut socket, protocol)) => {
                set_state(&shared, WebSocketState::Open);
                if !push_control(
                    &shared,
                    WebSocketEventKind::Open(WebSocketOpenMetadata {
                        protocol,
                        reconnect_attempt,
                    }),
                ) {
                    break;
                }
                reconnect_attempt = 0;
                match pump_open_socket(&shared, &receiver, &mut socket) {
                    SocketExit::Explicit => break,
                    SocketExit::PeerClosed(close) => {
                        let _ = push_control(&shared, WebSocketEventKind::Closed(close));
                        break;
                    }
                    SocketExit::Terminal { error, close } => {
                        push_failure_and_close(&shared, error, close, false);
                        break;
                    }
                    SocketExit::Reconnectable { error, close } => {
                        let Some((attempt, delay)) = next_reconnect(&shared, reconnect_attempt)
                        else {
                            push_failure_and_close(&shared, error, close, false);
                            break;
                        };
                        reconnect_attempt = attempt;
                        push_failure_and_close(&shared, error, close, true);
                        if !schedule_reconnect(&shared, attempt, delay) {
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                if lock(&shared.close_request).is_some() {
                    terminal_local_close(&shared);
                    break;
                }
                let close = WebSocketCloseMetadata {
                    code: 1006,
                    reason: String::new(),
                    was_clean: false,
                    will_reconnect: false,
                };
                let Some((attempt, delay)) = next_reconnect(&shared, reconnect_attempt) else {
                    push_failure_and_close(&shared, error, close, false);
                    break;
                };
                reconnect_attempt = attempt;
                push_failure_and_close(&shared, error, close, true);
                if !schedule_reconnect(&shared, attempt, delay) {
                    break;
                }
            }
        }
    }

    set_state(&shared, WebSocketState::Closed);
    drain_outbound(&shared, &receiver);
}

fn open_socket(
    config: &WebSocketConfig,
) -> Result<(WebSocket<MaybeTlsStream<TcpStream>>, Option<String>), WebSocketErrorMetadata> {
    let port = config
        .url
        .port_or_known_default()
        .ok_or(WebSocketErrorMetadata {
            kind: WebSocketErrorKind::Connection,
            recoverable: true,
        })?;
    let addresses = (config.host(), port)
        .to_socket_addrs()
        .map_err(|_| connection_error())?;
    let started = Instant::now();
    let mut tcp = None;
    for address in addresses {
        let remaining = config
            .connect_timeout
            .checked_sub(started.elapsed())
            .ok_or_else(connection_error)?;
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(stream) => {
                tcp = Some(stream);
                break;
            }
            Err(_) => continue,
        }
    }
    let tcp = tcp.ok_or_else(connection_error)?;
    tcp.set_nodelay(true).map_err(|_| connection_error())?;
    tcp.set_read_timeout(Some(config.connect_timeout))
        .map_err(|_| connection_error())?;
    tcp.set_write_timeout(Some(config.connect_timeout))
        .map_err(|_| connection_error())?;

    let uri = config
        .url
        .as_str()
        .parse::<Uri>()
        .map_err(|_| connection_error())?;
    let mut request = ClientRequestBuilder::new(uri);
    for protocol in &config.protocols {
        request = request.with_sub_protocol(protocol);
    }
    let tungstenite_config = TungsteniteConfig::default()
        .read_buffer_size(64 * 1024)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(
            config
                .max_outbound_bytes
                .saturating_add(config.max_message_bytes)
                .saturating_add(64 * 1024),
        )
        .max_message_size(Some(config.max_message_bytes))
        .max_frame_size(Some(config.max_message_bytes));
    let (mut socket, response) =
        client_tls_with_config(request, tcp, Some(tungstenite_config), None)
            .map_err(|_| connection_error())?;
    set_established_timeouts(socket.get_mut()).map_err(|_| connection_error())?;

    let protocol = response
        .headers()
        .get("Sec-WebSocket-Protocol")
        .map(|value| value.to_str().map(str::to_owned))
        .transpose()
        .map_err(|_| protocol_error(false))?;
    if protocol
        .as_ref()
        .is_some_and(|selected| !config.protocols.iter().any(|item| item == selected))
    {
        return Err(protocol_error(false));
    }
    Ok((socket, protocol))
}

fn set_established_timeouts(stream: &mut MaybeTlsStream<TcpStream>) -> std::io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(tcp) => {
            tcp.set_read_timeout(Some(READ_POLL_INTERVAL))?;
            tcp.set_write_timeout(Some(WRITE_TIMEOUT))
        }
        MaybeTlsStream::Rustls(tls) => {
            tls.sock.set_read_timeout(Some(READ_POLL_INTERVAL))?;
            tls.sock.set_write_timeout(Some(WRITE_TIMEOUT))
        }
        _ => Ok(()),
    }
}

enum SocketExit {
    Explicit,
    PeerClosed(WebSocketCloseMetadata),
    Reconnectable {
        error: WebSocketErrorMetadata,
        close: WebSocketCloseMetadata,
    },
    Terminal {
        error: WebSocketErrorMetadata,
        close: WebSocketCloseMetadata,
    },
}

fn pump_open_socket(
    shared: &Shared,
    receiver: &mpsc::Receiver<WebSocketMessage>,
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> SocketExit {
    let mut closing: Option<(WebSocketClose, Instant)> = None;
    loop {
        if shared.shutdown.load(Ordering::Acquire) && closing.is_none() {
            *lock(&shared.close_request) = Some(WebSocketClose::normal());
        }
        if closing.is_none() {
            if let Some(close) = lock(&shared.close_request).take() {
                set_state(shared, WebSocketState::Closing);
                let frame = CloseFrame {
                    code: CloseCode::from(close.code()),
                    reason: close.reason().to_owned().into(),
                };
                let _ = socket.close(Some(frame));
                closing = Some((close, Instant::now()));
            }
        }

        if let Some((close, started)) = &closing {
            if started.elapsed() >= CLOSE_HANDSHAKE_TIMEOUT {
                push_control(
                    shared,
                    WebSocketEventKind::Closed(WebSocketCloseMetadata {
                        code: close.code(),
                        reason: close.reason().to_owned(),
                        was_clean: false,
                        will_reconnect: false,
                    }),
                );
                return SocketExit::Explicit;
            }
        } else {
            loop {
                match receiver.try_recv() {
                    Ok(message) => {
                        release_outbound(shared, &message);
                        let native = match message {
                            WebSocketMessage::Text(text) => Message::Text(text.into()),
                            WebSocketMessage::Binary(bytes) => Message::Binary(bytes.into()),
                        };
                        if let Err(error) = socket.send(native) {
                            return classify_socket_error(error, true);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        return SocketExit::Terminal {
                            error: WebSocketErrorMetadata {
                                kind: WebSocketErrorKind::Transport,
                                recoverable: false,
                            },
                            close: abnormal_close(),
                        };
                    }
                }
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if closing.is_none()
                    && push_inbound(shared, WebSocketMessage::Text(text.to_string())).is_err()
                {
                    let _ = socket.close(Some(CloseFrame {
                        code: CloseCode::Again,
                        reason: "inbound queue full".into(),
                    }));
                    return SocketExit::Terminal {
                        error: WebSocketErrorMetadata {
                            kind: WebSocketErrorKind::InboundBackpressure,
                            recoverable: false,
                        },
                        close: WebSocketCloseMetadata {
                            code: 1013,
                            reason: String::new(),
                            was_clean: false,
                            will_reconnect: false,
                        },
                    };
                }
            }
            Ok(Message::Binary(bytes)) => {
                if closing.is_none()
                    && push_inbound(shared, WebSocketMessage::Binary(bytes.to_vec())).is_err()
                {
                    let _ = socket.close(Some(CloseFrame {
                        code: CloseCode::Again,
                        reason: "inbound queue full".into(),
                    }));
                    return SocketExit::Terminal {
                        error: WebSocketErrorMetadata {
                            kind: WebSocketErrorKind::InboundBackpressure,
                            recoverable: false,
                        },
                        close: WebSocketCloseMetadata {
                            code: 1013,
                            reason: String::new(),
                            was_clean: false,
                            will_reconnect: false,
                        },
                    };
                }
            }
            Ok(Message::Close(frame)) => {
                let metadata = frame
                    .map(|frame| WebSocketCloseMetadata {
                        code: u16::from(frame.code),
                        reason: frame.reason.to_string(),
                        was_clean: socket.flush().is_ok(),
                        will_reconnect: false,
                    })
                    .unwrap_or(WebSocketCloseMetadata {
                        code: 1005,
                        reason: String::new(),
                        was_clean: socket.flush().is_ok(),
                        will_reconnect: false,
                    });
                if closing.is_some() {
                    push_control(shared, WebSocketEventKind::Closed(metadata));
                    return SocketExit::Explicit;
                }
                if matches!(metadata.code, 1000 | 1001) {
                    return SocketExit::PeerClosed(metadata);
                }
                return SocketExit::Reconnectable {
                    error: WebSocketErrorMetadata {
                        kind: WebSocketErrorKind::Transport,
                        recoverable: true,
                    },
                    close: metadata,
                };
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                let _ = socket.flush();
            }
            Ok(Message::Frame(_)) => {}
            Err(TungsteniteError::Io(error))
                if matches!(
                    error.kind(),
                    IoErrorKind::WouldBlock | IoErrorKind::TimedOut
                ) => {}
            Err(error) => {
                if let Some((close, _)) = closing {
                    push_control(
                        shared,
                        WebSocketEventKind::Closed(WebSocketCloseMetadata {
                            code: close.code(),
                            reason: close.reason().to_owned(),
                            was_clean: matches!(error, TungsteniteError::ConnectionClosed),
                            will_reconnect: false,
                        }),
                    );
                    return SocketExit::Explicit;
                }
                return classify_socket_error(error, true);
            }
        }
    }
}

fn classify_socket_error(error: TungsteniteError, recoverable: bool) -> SocketExit {
    let (kind, terminal) = match error {
        TungsteniteError::Capacity(_) => (WebSocketErrorKind::MessageTooLarge, true),
        TungsteniteError::Protocol(ProtocolError::ResetWithoutClosingHandshake) => {
            (WebSocketErrorKind::Transport, false)
        }
        TungsteniteError::Protocol(_) | TungsteniteError::Utf8(_) => {
            (WebSocketErrorKind::Protocol, true)
        }
        TungsteniteError::ConnectionClosed | TungsteniteError::AlreadyClosed => {
            (WebSocketErrorKind::Transport, false)
        }
        _ => (WebSocketErrorKind::Transport, false),
    };
    let metadata = WebSocketErrorMetadata {
        kind,
        recoverable: recoverable && !terminal,
    };
    let close = if kind == WebSocketErrorKind::MessageTooLarge {
        WebSocketCloseMetadata {
            code: 1009,
            reason: String::new(),
            was_clean: false,
            will_reconnect: false,
        }
    } else {
        abnormal_close()
    };
    if terminal {
        SocketExit::Terminal {
            error: metadata,
            close,
        }
    } else {
        SocketExit::Reconnectable {
            error: metadata,
            close,
        }
    }
}

fn next_reconnect(shared: &Shared, previous_attempt: u16) -> Option<(u16, Duration)> {
    if shared.shutdown.load(Ordering::Acquire) || lock(&shared.close_request).is_some() {
        return None;
    }
    let policy = shared.config.reconnect_policy?;
    let attempt = previous_attempt.saturating_add(1);
    policy
        .delay_for_attempt(attempt)
        .map(|delay| (attempt, delay))
}

fn schedule_reconnect(shared: &Shared, attempt: u16, delay: Duration) -> bool {
    set_state(shared, WebSocketState::Reconnecting);
    if !push_control(
        shared,
        WebSocketEventKind::Reconnecting(WebSocketReconnectMetadata { attempt, delay }),
    ) {
        return false;
    }
    let deadline = Instant::now() + delay;
    while Instant::now() < deadline {
        if shared.shutdown.load(Ordering::Acquire) || lock(&shared.close_request).is_some() {
            terminal_local_close(shared);
            return false;
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25)),
        );
    }
    true
}

fn terminal_local_close(shared: &Shared) {
    let close = lock(&shared.close_request)
        .take()
        .unwrap_or_else(WebSocketClose::normal);
    set_state(shared, WebSocketState::Closed);
    push_control(
        shared,
        WebSocketEventKind::Closed(WebSocketCloseMetadata {
            code: close.code(),
            reason: close.reason().to_owned(),
            was_clean: false,
            will_reconnect: false,
        }),
    );
}

fn push_failure_and_close(
    shared: &Shared,
    mut error: WebSocketErrorMetadata,
    mut close: WebSocketCloseMetadata,
    will_reconnect: bool,
) {
    error.recoverable = error.recoverable && will_reconnect;
    close.will_reconnect = will_reconnect;
    let _ = push_control(shared, WebSocketEventKind::Error(error));
    let _ = push_control(shared, WebSocketEventKind::Closed(close));
}

fn push_inbound(shared: &Shared, message: WebSocketMessage) -> Result<(), ()> {
    lock(&shared.events).push_message(message)
}

fn push_control(shared: &Shared, event: WebSocketEventKind) -> bool {
    lock(&shared.events).push_control(event)
}

fn request_close(shared: &Shared, close: WebSocketClose) -> Result<(), WebSocketCloseError> {
    loop {
        let current = shared.state.load(Ordering::Acquire);
        let state = decode_state(current);
        if matches!(state, WebSocketState::Closing | WebSocketState::Closed) {
            return Err(WebSocketCloseError::AlreadyClosed);
        }
        if shared
            .state
            .compare_exchange(
                current,
                encode_state(WebSocketState::Closing),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            *lock(&shared.close_request) = Some(close);
            return Ok(());
        }
    }
}

fn reserve_bytes(
    bytes: &AtomicUsize,
    additional: usize,
    maximum: usize,
) -> Result<(), WebSocketSendError> {
    let mut current = bytes.load(Ordering::Acquire);
    loop {
        let Some(next) = current.checked_add(additional) else {
            return Err(WebSocketSendError::Backpressure);
        };
        if next > maximum {
            return Err(WebSocketSendError::Backpressure);
        }
        match bytes.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}

fn release_outbound(shared: &Shared, message: &WebSocketMessage) {
    shared.outbound_messages.fetch_sub(1, Ordering::AcqRel);
    shared
        .outbound_bytes
        .fetch_sub(message.len(), Ordering::AcqRel);
}

fn drain_outbound(shared: &Shared, receiver: &mpsc::Receiver<WebSocketMessage>) {
    while let Ok(message) = receiver.try_recv() {
        release_outbound(shared, &message);
    }
    shared.outbound_messages.store(0, Ordering::Release);
    shared.outbound_bytes.store(0, Ordering::Release);
}

fn set_state(shared: &Shared, state: WebSocketState) {
    shared.state.store(encode_state(state), Ordering::Release);
}

fn encode_state(state: WebSocketState) -> u8 {
    match state {
        WebSocketState::Connecting => 0,
        WebSocketState::Open => 1,
        WebSocketState::Reconnecting => 2,
        WebSocketState::Closing => 3,
        WebSocketState::Closed => 4,
    }
}

fn decode_state(value: u8) -> WebSocketState {
    match value {
        0 => WebSocketState::Connecting,
        1 => WebSocketState::Open,
        2 => WebSocketState::Reconnecting,
        3 => WebSocketState::Closing,
        _ => WebSocketState::Closed,
    }
}

fn connection_error() -> WebSocketErrorMetadata {
    WebSocketErrorMetadata {
        kind: WebSocketErrorKind::Connection,
        recoverable: true,
    }
}

fn protocol_error(recoverable: bool) -> WebSocketErrorMetadata {
    WebSocketErrorMetadata {
        kind: WebSocketErrorKind::Protocol,
        recoverable,
    }
}

fn abnormal_close() -> WebSocketCloseMetadata {
    WebSocketCloseMetadata {
        code: 1006,
        reason: String::new(),
        was_clean: false,
        will_reconnect: false,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::result_large_err)]

    use std::net::TcpListener;
    use std::thread;

    use tungstenite::handshake::server::{Request, Response};

    use super::*;
    use crate::websocket::{AllowAllWebSocketHosts, WebSocketEventKind};

    #[test]
    fn local_echo_preserves_protocol_message_order_and_close_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket =
                tungstenite::accept_hdr(stream, |request: &Request, mut response: Response| {
                    let offered = request
                        .headers()
                        .get("Sec-WebSocket-Protocol")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    assert!(offered.split(',').any(|item| item.trim() == "kael.echo.v1"));
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", "kael.echo.v1".parse().unwrap());
                    Ok(response)
                })
                .unwrap();
            for _ in 0..2 {
                let message = socket.read().unwrap();
                socket.send(message).unwrap();
            }
            let _ = socket.close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "finished".into(),
            }));
        });

        let config = WebSocketConfig::builder(format!("ws://{address}/echo?token=hidden"))
            .protocol("kael.echo.v1")
            .inbound_capacity(8)
            .outbound_capacity(8)
            .max_message_bytes(1_024)
            .max_inbound_bytes(8 * 1_024)
            .max_outbound_bytes(8 * 1_024)
            .build()
            .unwrap();
        let client = Client::connect(config).unwrap();
        wait_until(Duration::from_secs(5), || {
            client.state() == WebSocketState::Open
        });
        client
            .try_send(WebSocketMessage::Text("secret text".to_string()))
            .unwrap();
        client
            .try_send(WebSocketMessage::Binary(vec![1, 2, 3, 4]))
            .unwrap();

        let mut events = Vec::new();
        wait_until(Duration::from_secs(5), || {
            while let Some(event) = client.poll_event() {
                events.push(event);
            }
            events.iter().any(|event| {
                matches!(
                    event.kind(),
                    WebSocketEventKind::Closed(WebSocketCloseMetadata { code: 1000, .. })
                )
            })
        });
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence() < pair[1].sequence())
        );
        assert!(matches!(
            events.first().map(WebSocketEvent::kind),
            Some(WebSocketEventKind::Open(WebSocketOpenMetadata {
                protocol: Some(protocol),
                reconnect_attempt: 0,
            })) if protocol == "kael.echo.v1"
        ));
        let messages = events
            .iter()
            .filter_map(|event| match event.kind() {
                WebSocketEventKind::Message(message) => Some(message.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            vec![
                WebSocketMessage::Text("secret text".to_string()),
                WebSocketMessage::Binary(vec![1, 2, 3, 4]),
            ]
        );
        server.join().unwrap();
    }

    #[test]
    fn public_connect_applies_policy_before_starting_worker() {
        let config = WebSocketConfig::builder("ws://127.0.0.1:9")
            .build()
            .unwrap();
        let client = crate::WebSocketClient::connect(config, &AllowAllWebSocketHosts).unwrap();
        assert!(matches!(
            client.state(),
            WebSocketState::Connecting | WebSocketState::Closed
        ));
        let _ = client.close_normal();
    }

    #[test]
    fn abnormal_loss_reconnects_with_ordered_lifecycle_events() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for connection_index in 0..2 {
                let (stream, _) = listener.accept().unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                if connection_index == 0 {
                    drop(socket);
                    continue;
                }
                let message = socket.read().unwrap();
                socket.send(message).unwrap();
                let _ = socket.close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: "done".into(),
                }));
            }
        });

        let reconnect = super::super::WebSocketReconnectPolicy::new(
            2,
            Duration::from_millis(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let config = WebSocketConfig::builder(format!("ws://{address}/reconnect"))
            .reconnect_policy(reconnect)
            .build()
            .unwrap();
        let client = Client::connect(config).unwrap();
        let mut events = Vec::new();
        let mut sent_after_reconnect = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        let received_echo = loop {
            while let Some(event) = client.poll_event() {
                if matches!(
                    event.kind(),
                    WebSocketEventKind::Open(WebSocketOpenMetadata {
                        reconnect_attempt: 1,
                        ..
                    })
                ) && !sent_after_reconnect
                {
                    client
                        .try_send(WebSocketMessage::Text("after reconnect".to_string()))
                        .unwrap();
                    sent_after_reconnect = true;
                }
                events.push(event);
            }
            if events.iter().any(|event| {
                matches!(
                    event.kind(),
                    WebSocketEventKind::Message(WebSocketMessage::Text(text))
                        if text == "after reconnect"
                )
            }) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(received_echo, "events before timeout: {events:?}");

        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].sequence() < pair[1].sequence())
        );
        assert!(events.iter().any(|event| {
            matches!(
                event.kind(),
                WebSocketEventKind::Closed(WebSocketCloseMetadata {
                    code: 1006,
                    will_reconnect: true,
                    ..
                })
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event.kind(),
                WebSocketEventKind::Reconnecting(WebSocketReconnectMetadata { attempt: 1, .. })
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event.kind(),
                WebSocketEventKind::Open(WebSocketOpenMetadata {
                    reconnect_attempt: 1,
                    ..
                })
            )
        }));
        server.join().unwrap();
    }

    fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("condition was not met before timeout");
    }
}
