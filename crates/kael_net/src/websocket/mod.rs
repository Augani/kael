//! A bounded WebSocket client with the same application-facing API on native
//! targets and in browsers.
//!
//! Opening a socket always requires an explicit [`WebSocketHostPolicy`]. This
//! keeps the actual side effect behind the same host authorization boundary as
//! Kael's checked network descriptors. URLs, payloads, and close reasons are
//! intentionally omitted from every `Debug` and `Display` implementation in
//! this module.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::time::Duration;

use url::Url;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(target_arch = "wasm32")]
use browser as platform;
#[cfg(not(target_arch = "wasm32"))]
use native as platform;

const DEFAULT_INBOUND_MESSAGES: usize = 1_024;
const DEFAULT_OUTBOUND_MESSAGES: usize = 256;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_INBOUND_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_OUTBOUND_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_BROWSER_BUFFERED_BYTES: usize = 4 * 1024 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_QUEUE_ITEMS: usize = 65_536;
const MAX_MESSAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_QUEUE_BYTES: usize = 512 * 1024 * 1024;
const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_PROTOCOLS: usize = 16;
const MAX_PROTOCOL_BYTES: usize = 128;
const MAX_CLOSE_REASON_BYTES: usize = 123;

/// A policy checked synchronously before a WebSocket can be opened.
///
/// Implementations should validate their own host lists in [`Self::is_valid`]
/// and compare the already-normalized host passed to [`Self::allows_host`].
/// Kael's `NetworkPolicy` implements this trait when the framework crate is
/// used, so checked app descriptors and the transport share one policy gate.
pub trait WebSocketHostPolicy {
    /// Whether the policy itself is internally valid.
    fn is_valid(&self) -> bool;

    /// Whether a normalized DNS name or IP address may be contacted.
    fn allows_host(&self, host: &str) -> bool;
}

/// Explicit policy that permits every valid WebSocket host.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllWebSocketHosts;

impl WebSocketHostPolicy for AllowAllWebSocketHosts {
    fn is_valid(&self) -> bool {
        true
    }

    fn allows_host(&self, _host: &str) -> bool {
        true
    }
}

/// Explicit deny-by-default WebSocket host policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllWebSocketHosts;

impl WebSocketHostPolicy for DenyAllWebSocketHosts {
    fn is_valid(&self) -> bool {
        true
    }

    fn allows_host(&self, _host: &str) -> bool {
        false
    }
}

/// Reconnection bounds for abnormal transport loss.
///
/// A successfully opened connection resets the attempt counter. Clean server
/// closes (`1000` and `1001`) and application-requested closes do not reconnect.
#[derive(Clone, Copy, PartialEq)]
pub struct WebSocketReconnectPolicy {
    max_attempts: u16,
    initial_delay: Duration,
    max_delay: Duration,
    jitter_ratio: f64,
}

impl fmt::Debug for WebSocketReconnectPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketReconnectPolicy")
            .field("max_attempts", &self.max_attempts)
            .field("initial_delay", &self.initial_delay)
            .field("max_delay", &self.max_delay)
            .field("jitter_ratio", &self.jitter_ratio)
            .finish()
    }
}

impl WebSocketReconnectPolicy {
    /// Create checked exponential reconnection bounds without jitter.
    pub fn new(
        max_attempts: u16,
        initial_delay: Duration,
        max_delay: Duration,
    ) -> Result<Self, WebSocketConnectError> {
        Self::with_jitter(max_attempts, initial_delay, max_delay, 0.0)
    }

    /// Create checked exponential reconnection bounds with symmetric jitter.
    pub fn with_jitter(
        max_attempts: u16,
        initial_delay: Duration,
        max_delay: Duration,
        jitter_ratio: f64,
    ) -> Result<Self, WebSocketConnectError> {
        let policy = Self {
            max_attempts,
            initial_delay,
            max_delay,
            jitter_ratio,
        };
        policy.validate()?;
        Ok(policy)
    }

    /// A conservative policy for collaborative application state.
    pub fn conservative() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter_ratio: 0.1,
        }
    }

    /// Maximum attempts after an abnormal disconnect.
    pub fn max_attempts(self) -> u16 {
        self.max_attempts
    }

    /// Initial retry delay.
    pub fn initial_delay(self) -> Duration {
        self.initial_delay
    }

    /// Maximum retry delay.
    pub fn max_delay(self) -> Duration {
        self.max_delay
    }

    /// Symmetric delay jitter ratio.
    pub fn jitter_ratio(self) -> f64 {
        self.jitter_ratio
    }

    pub(crate) fn delay_for_attempt(self, attempt: u16) -> Option<Duration> {
        if attempt == 0 || attempt > self.max_attempts || self.validate().is_err() {
            return None;
        }
        let exponent = u32::from(attempt.saturating_sub(1));
        let factor = 2_u32.checked_pow(exponent).unwrap_or(u32::MAX);
        let delay = self
            .initial_delay
            .checked_mul(factor)
            .unwrap_or(self.max_delay)
            .min(self.max_delay);
        if self.jitter_ratio == 0.0 {
            return Some(delay);
        }
        let millis = delay.as_secs_f64() * 1_000.0;
        let jitter = (fastrand::f64() * 2.0 - 1.0) * self.jitter_ratio;
        let jittered = (millis * (1.0 + jitter)).clamp(0.0, self.max_delay.as_millis() as f64);
        Some(Duration::from_millis(jittered.round() as u64))
    }

    fn validate(self) -> Result<(), WebSocketConnectError> {
        if self.max_attempts == 0 || self.max_attempts > 100 {
            return Err(WebSocketConnectError::InvalidReconnectPolicy);
        }
        if self.initial_delay < Duration::from_millis(100)
            || self.max_delay < self.initial_delay
            || self.max_delay > Duration::from_secs(60 * 60)
            || !self.jitter_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.jitter_ratio)
        {
            return Err(WebSocketConnectError::InvalidReconnectPolicy);
        }
        Ok(())
    }
}

/// Checked, immutable WebSocket connection configuration.
#[derive(Clone)]
pub struct WebSocketConfig {
    url: Url,
    protocols: Vec<String>,
    inbound_capacity: usize,
    outbound_capacity: usize,
    max_message_bytes: usize,
    max_inbound_bytes: usize,
    max_outbound_bytes: usize,
    max_browser_buffered_bytes: usize,
    connect_timeout: Duration,
    reconnect_policy: Option<WebSocketReconnectPolicy>,
}

impl fmt::Debug for WebSocketConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketConfig")
            .field("url", &"<redacted>")
            .field("protocol_count", &self.protocols.len())
            .field("inbound_capacity", &self.inbound_capacity)
            .field("outbound_capacity", &self.outbound_capacity)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("max_inbound_bytes", &self.max_inbound_bytes)
            .field("max_outbound_bytes", &self.max_outbound_bytes)
            .field(
                "max_browser_buffered_bytes",
                &self.max_browser_buffered_bytes,
            )
            .field("connect_timeout", &self.connect_timeout)
            .field("reconnect_policy", &self.reconnect_policy)
            .finish()
    }
}

impl WebSocketConfig {
    /// Start building a checked configuration.
    pub fn builder(url: impl Into<String>) -> WebSocketConfigBuilder {
        WebSocketConfigBuilder::new(url)
    }

    /// The checked connection URL. Avoid including it in logs because query
    /// parameters can carry credentials.
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Normalized host checked by the supplied network policy.
    pub fn host(&self) -> &str {
        self.url
            .host_str()
            .expect("checked WebSocket URLs always have a host")
    }

    /// Requested subprotocols in preference order.
    pub fn protocols(&self) -> &[String] {
        &self.protocols
    }

    /// Maximum queued inbound message count.
    pub fn inbound_capacity(&self) -> usize {
        self.inbound_capacity
    }

    /// Maximum queued outbound message count.
    pub fn outbound_capacity(&self) -> usize {
        self.outbound_capacity
    }

    /// Maximum size of one text or binary message.
    pub fn max_message_bytes(&self) -> usize {
        self.max_message_bytes
    }

    /// Maximum aggregate bytes held by queued inbound messages.
    pub fn max_inbound_bytes(&self) -> usize {
        self.max_inbound_bytes
    }

    /// Maximum aggregate bytes held by queued outbound messages.
    pub fn max_outbound_bytes(&self) -> usize {
        self.max_outbound_bytes
    }

    /// Browser `bufferedAmount` threshold used to defer additional sends.
    pub fn max_browser_buffered_bytes(&self) -> usize {
        self.max_browser_buffered_bytes
    }

    /// Native TCP/TLS connection timeout.
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Optional abnormal-disconnect reconnection policy.
    pub fn reconnect_policy(&self) -> Option<WebSocketReconnectPolicy> {
        self.reconnect_policy
    }
}

/// Builder for [`WebSocketConfig`].
#[derive(Clone)]
pub struct WebSocketConfigBuilder {
    url: String,
    protocols: Vec<String>,
    inbound_capacity: usize,
    outbound_capacity: usize,
    max_message_bytes: usize,
    max_inbound_bytes: usize,
    max_outbound_bytes: usize,
    max_browser_buffered_bytes: usize,
    connect_timeout: Duration,
    reconnect_policy: Option<WebSocketReconnectPolicy>,
}

impl fmt::Debug for WebSocketConfigBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketConfigBuilder")
            .field("url", &"<redacted>")
            .field("protocol_count", &self.protocols.len())
            .field("inbound_capacity", &self.inbound_capacity)
            .field("outbound_capacity", &self.outbound_capacity)
            .field("max_message_bytes", &self.max_message_bytes)
            .field("max_inbound_bytes", &self.max_inbound_bytes)
            .field("max_outbound_bytes", &self.max_outbound_bytes)
            .field(
                "max_browser_buffered_bytes",
                &self.max_browser_buffered_bytes,
            )
            .field("connect_timeout", &self.connect_timeout)
            .field("reconnect_policy", &self.reconnect_policy)
            .finish()
    }
}

impl WebSocketConfigBuilder {
    /// Create a builder with bounded production defaults.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            protocols: Vec::new(),
            inbound_capacity: DEFAULT_INBOUND_MESSAGES,
            outbound_capacity: DEFAULT_OUTBOUND_MESSAGES,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_inbound_bytes: DEFAULT_MAX_INBOUND_BYTES,
            max_outbound_bytes: DEFAULT_MAX_OUTBOUND_BYTES,
            max_browser_buffered_bytes: DEFAULT_MAX_BROWSER_BUFFERED_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            reconnect_policy: None,
        }
    }

    /// Add a requested WebSocket subprotocol.
    pub fn protocol(mut self, protocol: impl Into<String>) -> Self {
        self.protocols.push(protocol.into());
        self
    }

    /// Add requested WebSocket subprotocols in preference order.
    pub fn protocols(mut self, protocols: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.protocols.extend(protocols.into_iter().map(Into::into));
        self
    }

    /// Set the maximum queued inbound message count.
    pub fn inbound_capacity(mut self, capacity: usize) -> Self {
        self.inbound_capacity = capacity;
        self
    }

    /// Set the maximum queued outbound message count.
    pub fn outbound_capacity(mut self, capacity: usize) -> Self {
        self.outbound_capacity = capacity;
        self
    }

    /// Set the per-message byte limit for both directions.
    pub fn max_message_bytes(mut self, bytes: usize) -> Self {
        self.max_message_bytes = bytes;
        self
    }

    /// Set the aggregate queued inbound byte limit.
    pub fn max_inbound_bytes(mut self, bytes: usize) -> Self {
        self.max_inbound_bytes = bytes;
        self
    }

    /// Set the aggregate queued outbound byte limit.
    pub fn max_outbound_bytes(mut self, bytes: usize) -> Self {
        self.max_outbound_bytes = bytes;
        self
    }

    /// Set the browser `bufferedAmount` threshold.
    pub fn max_browser_buffered_bytes(mut self, bytes: usize) -> Self {
        self.max_browser_buffered_bytes = bytes;
        self
    }

    /// Set the native TCP/TLS connection timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Reconnect after abnormal transport loss within explicit bounds.
    pub fn reconnect_policy(mut self, policy: WebSocketReconnectPolicy) -> Self {
        self.reconnect_policy = Some(policy);
        self
    }

    /// Disable automatic reconnection.
    pub fn without_reconnect(mut self) -> Self {
        self.reconnect_policy = None;
        self
    }

    /// Validate and build the immutable configuration.
    pub fn build(self) -> Result<WebSocketConfig, WebSocketConnectError> {
        let url = validate_url(&self.url)?;
        validate_protocols(&self.protocols)?;
        validate_count_limit(self.inbound_capacity)?;
        validate_count_limit(self.outbound_capacity)?;
        validate_byte_limit(self.max_message_bytes, MAX_MESSAGE_BYTES)?;
        validate_byte_limit(self.max_inbound_bytes, MAX_QUEUE_BYTES)?;
        validate_byte_limit(self.max_outbound_bytes, MAX_QUEUE_BYTES)?;
        validate_byte_limit(self.max_browser_buffered_bytes, MAX_QUEUE_BYTES)?;
        if self.max_inbound_bytes < self.max_message_bytes
            || self.max_outbound_bytes < self.max_message_bytes
        {
            return Err(WebSocketConnectError::InvalidQueueLimits);
        }
        if self.connect_timeout < Duration::from_millis(100)
            || self.connect_timeout > Duration::from_secs(5 * 60)
        {
            return Err(WebSocketConnectError::InvalidConnectTimeout);
        }
        if let Some(policy) = self.reconnect_policy {
            policy.validate()?;
        }
        Ok(WebSocketConfig {
            url,
            protocols: self.protocols,
            inbound_capacity: self.inbound_capacity,
            outbound_capacity: self.outbound_capacity,
            max_message_bytes: self.max_message_bytes,
            max_inbound_bytes: self.max_inbound_bytes,
            max_outbound_bytes: self.max_outbound_bytes,
            max_browser_buffered_bytes: self.max_browser_buffered_bytes,
            connect_timeout: self.connect_timeout,
            reconnect_policy: self.reconnect_policy,
        })
    }
}

/// A text or binary application message.
#[derive(Clone, PartialEq, Eq)]
pub enum WebSocketMessage {
    /// UTF-8 text message.
    Text(String),
    /// Binary message.
    Binary(Vec<u8>),
}

impl WebSocketMessage {
    /// Message payload length in bytes.
    pub fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Binary(bytes) => bytes.len(),
        }
    }

    /// Whether the payload has zero bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrow this message as text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Binary(_) => None,
        }
    }

    /// Borrow this message as binary data.
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Self::Text(_) => None,
            Self::Binary(bytes) => Some(bytes),
        }
    }
}

impl fmt::Debug for WebSocketMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketMessage")
            .field(
                "kind",
                &match self {
                    Self::Text(_) => "text",
                    Self::Binary(_) => "binary",
                },
            )
            .field("bytes", &self.len())
            .finish()
    }
}

impl From<String> for WebSocketMessage {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<u8>> for WebSocketMessage {
    fn from(value: Vec<u8>) -> Self {
        Self::Binary(value)
    }
}

/// Lifecycle state observable without consuming the event queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketState {
    /// Initial connection is in progress.
    Connecting,
    /// The socket is open.
    Open,
    /// The transport is waiting before another connection attempt.
    Reconnecting,
    /// An application close has been requested.
    Closing,
    /// The transport is terminally closed.
    Closed,
}

/// Metadata emitted when a connection becomes open.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSocketOpenMetadata {
    /// Server-selected subprotocol, when one was negotiated.
    pub protocol: Option<String>,
    /// Zero for the initial connection, otherwise the reconnect attempt that opened.
    pub reconnect_attempt: u16,
}

impl fmt::Debug for WebSocketOpenMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketOpenMetadata")
            .field("protocol_negotiated", &self.protocol.is_some())
            .field("reconnect_attempt", &self.reconnect_attempt)
            .finish()
    }
}

/// Sanitized asynchronous transport error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketErrorKind {
    /// DNS, TCP, TLS, proxy, or handshake failure.
    Connection,
    /// An established transport failed.
    Transport,
    /// A received message exceeded configured limits.
    MessageTooLarge,
    /// The application did not drain inbound messages within queue bounds.
    InboundBackpressure,
    /// The peer or browser produced an unsupported message representation.
    UnsupportedMessage,
    /// The peer violated the WebSocket protocol.
    Protocol,
}

/// Sanitized error metadata that never contains a URL or payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSocketErrorMetadata {
    /// Stable failure category.
    pub kind: WebSocketErrorKind,
    /// Whether configured reconnection may recover this failure.
    pub recoverable: bool,
}

/// Metadata emitted before an automatic reconnect attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSocketReconnectMetadata {
    /// One-based reconnect attempt.
    pub attempt: u16,
    /// Delay before the attempt begins.
    pub delay: Duration,
}

/// Close-frame metadata.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSocketCloseMetadata {
    /// RFC 6455 close code, or `1006` for an abnormal loss without a frame.
    pub code: u16,
    /// Peer-provided UTF-8 reason. Avoid logging this application-controlled text.
    pub reason: String,
    /// Whether the browser/transport observed a clean close handshake.
    pub was_clean: bool,
    /// Whether another connection attempt has been scheduled.
    pub will_reconnect: bool,
}

impl fmt::Debug for WebSocketCloseMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketCloseMetadata")
            .field("code", &self.code)
            .field("reason", &"<redacted>")
            .field("reason_bytes", &self.reason.len())
            .field("was_clean", &self.was_clean)
            .field("will_reconnect", &self.will_reconnect)
            .finish()
    }
}

/// Ordered event payload.
#[derive(Clone, PartialEq, Eq)]
pub enum WebSocketEventKind {
    /// The initial or reconnected socket is open.
    Open(WebSocketOpenMetadata),
    /// One complete text or binary message.
    Message(WebSocketMessage),
    /// A sanitized transport failure.
    Error(WebSocketErrorMetadata),
    /// An automatic reconnect attempt was scheduled.
    Reconnecting(WebSocketReconnectMetadata),
    /// One underlying connection closed.
    Closed(WebSocketCloseMetadata),
}

impl fmt::Debug for WebSocketEventKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(metadata) => formatter.debug_tuple("Open").field(metadata).finish(),
            Self::Message(message) => formatter.debug_tuple("Message").field(message).finish(),
            Self::Error(metadata) => formatter.debug_tuple("Error").field(metadata).finish(),
            Self::Reconnecting(metadata) => formatter
                .debug_tuple("Reconnecting")
                .field(metadata)
                .finish(),
            Self::Closed(metadata) => formatter.debug_tuple("Closed").field(metadata).finish(),
        }
    }
}

/// An event with a monotonically increasing per-client sequence number.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSocketEvent {
    sequence: u64,
    kind: WebSocketEventKind,
}

impl WebSocketEvent {
    /// Per-client sequence number, beginning at zero.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Borrow the event payload.
    pub fn kind(&self) -> &WebSocketEventKind {
        &self.kind
    }

    /// Consume the envelope and return its payload.
    pub fn into_kind(self) -> WebSocketEventKind {
        self.kind
    }
}

impl fmt::Debug for WebSocketEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketEvent")
            .field("sequence", &self.sequence)
            .field("kind", &self.kind)
            .finish()
    }
}

/// An explicit close request portable to browser and native WebSocket APIs.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSocketClose {
    code: u16,
    reason: String,
}

impl WebSocketClose {
    /// A normal application close (`1000`) without a reason.
    pub fn normal() -> Self {
        Self {
            code: 1000,
            reason: String::new(),
        }
    }

    /// Create a checked close request.
    ///
    /// Browsers permit code `1000` or application codes in `3000..=4999`.
    pub fn new(code: u16, reason: impl Into<String>) -> Result<Self, WebSocketCloseError> {
        let reason = reason.into();
        if code != 1000 && !(3000..=4999).contains(&code) {
            return Err(WebSocketCloseError::InvalidCode);
        }
        if reason.len() > MAX_CLOSE_REASON_BYTES {
            return Err(WebSocketCloseError::ReasonTooLong);
        }
        Ok(Self { code, reason })
    }

    /// Close code.
    pub fn code(&self) -> u16 {
        self.code
    }

    /// Application-controlled close reason. Avoid logging it.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Debug for WebSocketClose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketClose")
            .field("code", &self.code)
            .field("reason", &"<redacted>")
            .field("reason_bytes", &self.reason.len())
            .finish()
    }
}

/// Synchronous configuration or startup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketConnectError {
    /// URL is malformed, too long, or contains control/whitespace bytes.
    InvalidUrl,
    /// URL does not use `ws` or `wss`.
    InvalidScheme,
    /// URL has no host.
    MissingHost,
    /// URL contains credentials, which are deliberately unsupported.
    CredentialsNotAllowed,
    /// URL contains a fragment, which is not sent in a WebSocket handshake.
    FragmentNotAllowed,
    /// A subprotocol is empty, duplicated, too long, or not an HTTP token.
    InvalidSubprotocol,
    /// Queue item or byte bounds are invalid.
    InvalidQueueLimits,
    /// Native connection timeout is outside supported bounds.
    InvalidConnectTimeout,
    /// Reconnection bounds are invalid.
    InvalidReconnectPolicy,
    /// The supplied host policy is invalid.
    InvalidHostPolicy,
    /// The supplied host policy denied this destination.
    HostDenied,
    /// The browser rejected socket construction before events could be installed.
    BrowserRejected,
    /// The native background worker could not be started.
    WorkerUnavailable,
    /// This build target has no transport implementation.
    UnsupportedTarget,
}

impl fmt::Display for WebSocketConnectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidUrl => "invalid WebSocket URL",
            Self::InvalidScheme => "WebSocket URL must use ws or wss",
            Self::MissingHost => "WebSocket URL must include a host",
            Self::CredentialsNotAllowed => "WebSocket URL credentials are not allowed",
            Self::FragmentNotAllowed => "WebSocket URL fragments are not allowed",
            Self::InvalidSubprotocol => "invalid WebSocket subprotocol configuration",
            Self::InvalidQueueLimits => "invalid WebSocket queue limits",
            Self::InvalidConnectTimeout => "invalid WebSocket connection timeout",
            Self::InvalidReconnectPolicy => "invalid WebSocket reconnect policy",
            Self::InvalidHostPolicy => "invalid WebSocket host policy",
            Self::HostDenied => "WebSocket host denied by network policy",
            Self::BrowserRejected => "browser rejected WebSocket construction",
            Self::WorkerUnavailable => "WebSocket background worker is unavailable",
            Self::UnsupportedTarget => "WebSocket transport is unsupported on this target",
        })
    }
}

impl Error for WebSocketConnectError {}

/// Non-blocking send failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketSendError {
    /// The payload exceeds the per-message byte limit.
    MessageTooLarge {
        /// Payload bytes.
        bytes: usize,
        /// Configured maximum.
        max_bytes: usize,
    },
    /// The bounded outbound count or byte budget is full.
    Backpressure,
    /// The client is closing or terminally closed.
    Closed,
}

impl fmt::Display for WebSocketSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageTooLarge { bytes, max_bytes } => {
                write!(
                    formatter,
                    "WebSocket message has {bytes} bytes; limit is {max_bytes}"
                )
            }
            Self::Backpressure => formatter.write_str("WebSocket outbound queue is full"),
            Self::Closed => formatter.write_str("WebSocket client is closing or closed"),
        }
    }
}

impl Error for WebSocketSendError {}

/// Explicit close failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketCloseError {
    /// Only code `1000` and application codes `3000..=4999` are portable.
    InvalidCode,
    /// UTF-8 close reasons cannot exceed 123 bytes.
    ReasonTooLong,
    /// The transport is already closing or closed.
    AlreadyClosed,
    /// The browser rejected the close call.
    BrowserRejected,
}

impl fmt::Display for WebSocketCloseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCode => "invalid portable WebSocket close code",
            Self::ReasonTooLong => "WebSocket close reason exceeds 123 UTF-8 bytes",
            Self::AlreadyClosed => "WebSocket client is already closing or closed",
            Self::BrowserRejected => "browser rejected WebSocket close request",
        })
    }
}

impl Error for WebSocketCloseError {}

/// A cloneable WebSocket client. Clones share queues and lifecycle state.
///
/// `try_send` never waits. Applications should retry after draining/polling
/// when it reports [`WebSocketSendError::Backpressure`]. Dropping the final
/// clone initiates a normal close and releases all browser callbacks/timers or
/// asks the native worker to stop.
#[derive(Clone)]
pub struct WebSocketClient {
    inner: platform::Client,
}

impl fmt::Debug for WebSocketClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebSocketClient")
            .field("state", &self.state())
            .field("queued_inbound_events", &self.queued_inbound_events())
            .field("queued_outbound_messages", &self.queued_outbound_messages())
            .field("queued_outbound_bytes", &self.queued_outbound_bytes())
            .finish()
    }
}

impl WebSocketClient {
    /// Validate the host policy and begin connecting without blocking the UI.
    pub fn connect(
        config: WebSocketConfig,
        policy: &impl WebSocketHostPolicy,
    ) -> Result<Self, WebSocketConnectError> {
        if !policy.is_valid() {
            return Err(WebSocketConnectError::InvalidHostPolicy);
        }
        if !policy.allows_host(config.host()) {
            return Err(WebSocketConnectError::HostDenied);
        }
        Ok(Self {
            inner: platform::Client::connect(config)?,
        })
    }

    /// Queue a text or binary message without blocking.
    pub fn try_send(&self, message: impl Into<WebSocketMessage>) -> Result<(), WebSocketSendError> {
        self.inner.try_send(message.into())
    }

    /// Poll the next event in transport order.
    pub fn poll_event(&self) -> Option<WebSocketEvent> {
        self.inner.poll_event()
    }

    /// Current lifecycle state.
    pub fn state(&self) -> WebSocketState {
        self.inner.state()
    }

    /// Number of queued inbound events, including lifecycle events.
    pub fn queued_inbound_events(&self) -> usize {
        self.inner.queued_inbound_events()
    }

    /// Number of queued outbound application messages.
    pub fn queued_outbound_messages(&self) -> usize {
        self.inner.queued_outbound_messages()
    }

    /// Aggregate bytes in queued outbound messages.
    pub fn queued_outbound_bytes(&self) -> usize {
        self.inner.queued_outbound_bytes()
    }

    /// Request an explicit close and disable reconnection.
    pub fn close(&self, close: WebSocketClose) -> Result<(), WebSocketCloseError> {
        self.inner.close(close)
    }

    /// Request a normal explicit close.
    pub fn close_normal(&self) -> Result<(), WebSocketCloseError> {
        self.close(WebSocketClose::normal())
    }
}

pub(crate) struct EventQueue {
    events: VecDeque<WebSocketEvent>,
    next_sequence: u64,
    message_count: usize,
    message_bytes: usize,
    max_message_count: usize,
    max_message_bytes: usize,
    max_total_events: usize,
}

impl EventQueue {
    pub(crate) fn new(config: &WebSocketConfig) -> Self {
        let reconnect_reserve = config
            .reconnect_policy
            .map(|policy| usize::from(policy.max_attempts()) * 3)
            .unwrap_or(0);
        Self {
            events: VecDeque::new(),
            next_sequence: 0,
            message_count: 0,
            message_bytes: 0,
            max_message_count: config.inbound_capacity,
            max_message_bytes: config.max_inbound_bytes,
            max_total_events: config
                .inbound_capacity
                .saturating_add(reconnect_reserve)
                .saturating_add(16),
        }
    }

    pub(crate) fn push_message(&mut self, message: WebSocketMessage) -> Result<(), ()> {
        let bytes = message.len();
        if self.message_count >= self.max_message_count
            || self.message_bytes.saturating_add(bytes) > self.max_message_bytes
            || self.events.len() >= self.max_total_events
        {
            return Err(());
        }
        self.message_count += 1;
        self.message_bytes += bytes;
        self.push_unchecked(WebSocketEventKind::Message(message));
        Ok(())
    }

    pub(crate) fn push_control(&mut self, kind: WebSocketEventKind) -> bool {
        if self.events.len() >= self.max_total_events {
            return false;
        }
        self.push_unchecked(kind);
        true
    }

    pub(crate) fn pop(&mut self) -> Option<WebSocketEvent> {
        let event = self.events.pop_front()?;
        if let WebSocketEventKind::Message(message) = &event.kind {
            self.message_count = self.message_count.saturating_sub(1);
            self.message_bytes = self.message_bytes.saturating_sub(message.len());
        }
        Some(event)
    }

    pub(crate) fn len(&self) -> usize {
        self.events.len()
    }

    fn push_unchecked(&mut self, kind: WebSocketEventKind) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.events.push_back(WebSocketEvent { sequence, kind });
    }
}

fn validate_url(raw: &str) -> Result<Url, WebSocketConnectError> {
    if raw.is_empty()
        || raw.len() > MAX_URL_BYTES
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(WebSocketConnectError::InvalidUrl);
    }
    let url = Url::parse(raw).map_err(|_| WebSocketConnectError::InvalidUrl)?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(WebSocketConnectError::InvalidScheme);
    }
    if url.host_str().is_none() {
        return Err(WebSocketConnectError::MissingHost);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(WebSocketConnectError::CredentialsNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(WebSocketConnectError::FragmentNotAllowed);
    }
    Ok(url)
}

fn validate_protocols(protocols: &[String]) -> Result<(), WebSocketConnectError> {
    if protocols.len() > MAX_PROTOCOLS {
        return Err(WebSocketConnectError::InvalidSubprotocol);
    }
    for (index, protocol) in protocols.iter().enumerate() {
        if protocol.is_empty()
            || protocol.len() > MAX_PROTOCOL_BYTES
            || !protocol.bytes().all(is_http_token_byte)
            || protocols[..index]
                .iter()
                .any(|earlier| earlier.eq_ignore_ascii_case(protocol))
        {
            return Err(WebSocketConnectError::InvalidSubprotocol);
        }
    }
    Ok(())
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn validate_count_limit(value: usize) -> Result<(), WebSocketConnectError> {
    if value == 0 || value > MAX_QUEUE_ITEMS {
        Err(WebSocketConnectError::InvalidQueueLimits)
    } else {
        Ok(())
    }
}

fn validate_byte_limit(value: usize, maximum: usize) -> Result<(), WebSocketConnectError> {
    if value == 0 || value > maximum {
        Err(WebSocketConnectError::InvalidQueueLimits)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validates_urls_protocols_and_bounds_without_leaking_url() {
        let secret_url = "wss://example.test/socket?access_token=super-secret";
        let config = WebSocketConfig::builder(secret_url)
            .protocol("kael.collab.v1")
            .build()
            .unwrap();
        assert_eq!(config.host(), "example.test");
        assert_eq!(config.protocols(), &["kael.collab.v1"]);
        let debug = format!("{config:?}");
        assert!(!debug.contains("example.test"));
        assert!(!debug.contains("super-secret"));

        assert_eq!(
            WebSocketConfig::builder("https://example.test")
                .build()
                .unwrap_err(),
            WebSocketConnectError::InvalidScheme
        );
        assert_eq!(
            WebSocketConfig::builder("wss://user:secret@example.test")
                .build()
                .unwrap_err(),
            WebSocketConnectError::CredentialsNotAllowed
        );
        assert_eq!(
            WebSocketConfig::builder("wss://example.test/#hidden")
                .build()
                .unwrap_err(),
            WebSocketConnectError::FragmentNotAllowed
        );
        assert_eq!(
            WebSocketConfig::builder("wss://example.test")
                .protocol("has a space")
                .build()
                .unwrap_err(),
            WebSocketConnectError::InvalidSubprotocol
        );
        assert_eq!(
            WebSocketConfig::builder("wss://example.test")
                .protocol("chat")
                .protocol("CHAT")
                .build()
                .unwrap_err(),
            WebSocketConnectError::InvalidSubprotocol
        );
    }

    #[test]
    fn message_and_close_debug_output_redacts_application_content() {
        let message = WebSocketMessage::Text("payload-secret".to_string());
        let close = WebSocketClose::new(3001, "reason-secret").unwrap();
        assert!(!format!("{message:?}").contains("payload-secret"));
        assert!(!format!("{close:?}").contains("reason-secret"));

        let metadata = WebSocketCloseMetadata {
            code: 3001,
            reason: "peer-secret".to_string(),
            was_clean: true,
            will_reconnect: false,
        };
        assert!(!format!("{metadata:?}").contains("peer-secret"));
    }

    #[test]
    fn event_queue_is_count_and_byte_bounded_and_sequence_ordered() {
        let config = WebSocketConfig::builder("ws://127.0.0.1:9000")
            .inbound_capacity(2)
            .max_message_bytes(8)
            .max_inbound_bytes(8)
            .max_outbound_bytes(8)
            .build()
            .unwrap();
        let mut queue = EventQueue::new(&config);
        queue
            .push_message(WebSocketMessage::Text("1234".to_string()))
            .unwrap();
        queue
            .push_message(WebSocketMessage::Binary(vec![5, 6, 7, 8]))
            .unwrap();
        assert!(
            queue
                .push_message(WebSocketMessage::Text("x".to_string()))
                .is_err()
        );
        let first = queue.pop().unwrap();
        let second = queue.pop().unwrap();
        assert_eq!(first.sequence(), 0);
        assert_eq!(second.sequence(), 1);
        assert!(queue.pop().is_none());
    }

    #[test]
    fn reconnect_policy_is_capped_and_checked() {
        let policy = WebSocketReconnectPolicy::new(
            5,
            Duration::from_millis(100),
            Duration::from_millis(350),
        )
        .unwrap();
        assert_eq!(
            policy.delay_for_attempt(1),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            policy.delay_for_attempt(2),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            policy.delay_for_attempt(3),
            Some(Duration::from_millis(350))
        );
        assert_eq!(policy.delay_for_attempt(6), None);
        assert_eq!(
            WebSocketReconnectPolicy::new(0, Duration::from_millis(100), Duration::from_secs(1)),
            Err(WebSocketConnectError::InvalidReconnectPolicy)
        );
    }

    #[test]
    fn explicit_policy_is_required_and_deny_is_synchronous() {
        let config = WebSocketConfig::builder("wss://example.test")
            .build()
            .unwrap();
        let error = WebSocketClient::connect(config, &DenyAllWebSocketHosts).unwrap_err();
        assert_eq!(error, WebSocketConnectError::HostDenied);
    }
}
