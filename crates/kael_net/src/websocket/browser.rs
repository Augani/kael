use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use js_sys::{Array, ArrayBuffer, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, CloseEvent, Event, MessageEvent, WebSocket};

use super::{
    EventQueue, WebSocketClose, WebSocketCloseError, WebSocketCloseMetadata, WebSocketConfig,
    WebSocketConnectError, WebSocketErrorKind, WebSocketErrorMetadata, WebSocketEvent,
    WebSocketEventKind, WebSocketMessage, WebSocketOpenMetadata, WebSocketReconnectMetadata,
    WebSocketSendError, WebSocketState,
};

const OUTBOUND_PUMP_INTERVAL_MS: i32 = 10;

#[derive(Clone)]
pub(super) struct Client {
    inner: Rc<ClientInner>,
}

struct ClientInner {
    state: RefCell<State>,
}

struct State {
    config: WebSocketConfig,
    events: EventQueue,
    lifecycle: WebSocketState,
    outbound: VecDeque<WebSocketMessage>,
    outbound_bytes: usize,
    socket: Option<WebSocket>,
    handlers: Option<Handlers>,
    generation: u64,
    current_attempt: u16,
    opened_current: bool,
    error_seen_current: bool,
    reconnect_timer: Option<Timer>,
    pump_timer: Option<Timer>,
    explicit_close: Option<WebSocketClose>,
    terminal_failure: bool,
    terminal_close_code: Option<u16>,
    disposed: bool,
}

struct Handlers {
    _open: Closure<dyn FnMut(Event)>,
    _message: Closure<dyn FnMut(MessageEvent)>,
    _error: Closure<dyn FnMut(Event)>,
    _close: Closure<dyn FnMut(CloseEvent)>,
}

struct Timer {
    id: i32,
    callback: Closure<dyn FnMut()>,
}

impl Client {
    pub(super) fn connect(config: WebSocketConfig) -> Result<Self, WebSocketConnectError> {
        let inner = Rc::new(ClientInner {
            state: RefCell::new(State {
                events: EventQueue::new(&config),
                config,
                lifecycle: WebSocketState::Connecting,
                outbound: VecDeque::new(),
                outbound_bytes: 0,
                socket: None,
                handlers: None,
                generation: 0,
                current_attempt: 0,
                opened_current: false,
                error_seen_current: false,
                reconnect_timer: None,
                pump_timer: None,
                explicit_close: None,
                terminal_failure: false,
                terminal_close_code: None,
                disposed: false,
            }),
        });
        open_socket(&inner, 0)?;
        Ok(Self { inner })
    }

    pub(super) fn try_send(&self, message: WebSocketMessage) -> Result<(), WebSocketSendError> {
        let bytes = message.len();
        {
            let mut state = self.inner.state.borrow_mut();
            if bytes > state.config.max_message_bytes {
                return Err(WebSocketSendError::MessageTooLarge {
                    bytes,
                    max_bytes: state.config.max_message_bytes,
                });
            }
            if matches!(
                state.lifecycle,
                WebSocketState::Closing | WebSocketState::Closed
            ) {
                return Err(WebSocketSendError::Closed);
            }
            if state.outbound.len() >= state.config.outbound_capacity
                || state.outbound_bytes.saturating_add(bytes) > state.config.max_outbound_bytes
            {
                return Err(WebSocketSendError::Backpressure);
            }
            state.outbound.push_back(message);
            state.outbound_bytes += bytes;
        }
        pump_outbound(&self.inner);
        Ok(())
    }

    pub(super) fn poll_event(&self) -> Option<WebSocketEvent> {
        pump_outbound(&self.inner);
        self.inner.state.borrow_mut().events.pop()
    }

    pub(super) fn state(&self) -> WebSocketState {
        self.inner.state.borrow().lifecycle
    }

    pub(super) fn queued_inbound_events(&self) -> usize {
        self.inner.state.borrow().events.len()
    }

    pub(super) fn queued_outbound_messages(&self) -> usize {
        self.inner.state.borrow().outbound.len()
    }

    pub(super) fn queued_outbound_bytes(&self) -> usize {
        self.inner.state.borrow().outbound_bytes
    }

    pub(super) fn close(&self, close: WebSocketClose) -> Result<(), WebSocketCloseError> {
        close_explicit(&self.inner, close)
    }
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        state.disposed = true;
        state.lifecycle = WebSocketState::Closed;
        state.outbound.clear();
        state.outbound_bytes = 0;
        if let Some(timer) = state.reconnect_timer.take() {
            clear_timer(timer);
        }
        if let Some(timer) = state.pump_timer.take() {
            clear_timer(timer);
        }
        if let Some(socket) = state.socket.take() {
            detach_handlers(&socket);
            let _ = socket.close_with_code_and_reason(1000, "");
        }
        state.handlers.take();
    }
}

fn open_socket(inner: &Rc<ClientInner>, attempt: u16) -> Result<(), WebSocketConnectError> {
    let (url, protocols) = {
        let state = inner.state.borrow();
        (
            state.config.url().to_owned(),
            state.config.protocols.clone(),
        )
    };
    let protocol_values = Array::new();
    for protocol in protocols {
        protocol_values.push(&protocol.into());
    }
    let socket = WebSocket::new_with_str_sequence(&url, protocol_values.as_ref())
        .map_err(|_| WebSocketConnectError::BrowserRejected)?;
    socket.set_binary_type(BinaryType::Arraybuffer);

    let generation = {
        let mut state = inner.state.borrow_mut();
        state.generation = state.generation.wrapping_add(1);
        state.current_attempt = attempt;
        state.opened_current = false;
        state.error_seen_current = false;
        state.lifecycle = if attempt == 0 {
            WebSocketState::Connecting
        } else {
            WebSocketState::Reconnecting
        };
        state.generation
    };

    let weak = Rc::downgrade(inner);
    let open = Closure::wrap(Box::new(move |_event: Event| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.generation != generation {
            return;
        }
        state.lifecycle = WebSocketState::Open;
        state.opened_current = true;
        let protocol = state
            .socket
            .as_ref()
            .map(WebSocket::protocol)
            .filter(|protocol| !protocol.is_empty());
        let reconnect_attempt = state.current_attempt;
        let _ = state
            .events
            .push_control(WebSocketEventKind::Open(WebSocketOpenMetadata {
                protocol,
                reconnect_attempt,
            }));
        drop(state);
        pump_outbound(&inner);
    }) as Box<dyn FnMut(Event)>);

    let weak = Rc::downgrade(inner);
    let message = Closure::wrap(Box::new(move |event: MessageEvent| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        handle_message(&inner, generation, event);
    }) as Box<dyn FnMut(MessageEvent)>);

    let weak = Rc::downgrade(inner);
    let error = Closure::wrap(Box::new(move |_event: Event| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.generation != generation || state.error_seen_current {
            return;
        }
        state.error_seen_current = true;
        let recoverable = reconnect_available(&state);
        let kind = if state.opened_current {
            WebSocketErrorKind::Transport
        } else {
            WebSocketErrorKind::Connection
        };
        let _ = state
            .events
            .push_control(WebSocketEventKind::Error(WebSocketErrorMetadata {
                kind,
                recoverable,
            }));
    }) as Box<dyn FnMut(Event)>);

    let weak = Rc::downgrade(inner);
    let close = Closure::wrap(Box::new(move |event: CloseEvent| {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        handle_close(&inner, generation, event);
    }) as Box<dyn FnMut(CloseEvent)>);

    socket.set_onopen(Some(open.as_ref().unchecked_ref()));
    socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
    socket.set_onerror(Some(error.as_ref().unchecked_ref()));
    socket.set_onclose(Some(close.as_ref().unchecked_ref()));

    let mut state = inner.state.borrow_mut();
    if state.disposed || state.generation != generation {
        detach_handlers(&socket);
        let _ = socket.close_with_code_and_reason(1000, "");
        return Err(WebSocketConnectError::BrowserRejected);
    }
    state.socket = Some(socket);
    state.handlers = Some(Handlers {
        _open: open,
        _message: message,
        _error: error,
        _close: close,
    });
    Ok(())
}

fn handle_message(inner: &Rc<ClientInner>, generation: u64, event: MessageEvent) {
    let message = if let Some(text) = event.data().as_string() {
        Some(WebSocketMessage::Text(text))
    } else if event.data().is_instance_of::<ArrayBuffer>() {
        let buffer = event.data().unchecked_into::<ArrayBuffer>();
        Some(WebSocketMessage::Binary(Uint8Array::new(&buffer).to_vec()))
    } else {
        None
    };

    let mut force_close = None;
    {
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.generation != generation || !state.opened_current {
            return;
        }
        match message {
            Some(message) if message.len() <= state.config.max_message_bytes => {
                if state.events.push_message(message).is_err() {
                    state.terminal_failure = true;
                    state.terminal_close_code = Some(1013);
                    state.error_seen_current = true;
                    let _ = state.events.push_control(WebSocketEventKind::Error(
                        WebSocketErrorMetadata {
                            kind: WebSocketErrorKind::InboundBackpressure,
                            recoverable: false,
                        },
                    ));
                    force_close = Some((4013, "inbound queue full"));
                }
            }
            Some(_) => {
                state.terminal_failure = true;
                state.terminal_close_code = Some(1009);
                state.error_seen_current = true;
                let _ =
                    state
                        .events
                        .push_control(WebSocketEventKind::Error(WebSocketErrorMetadata {
                            kind: WebSocketErrorKind::MessageTooLarge,
                            recoverable: false,
                        }));
                force_close = Some((4009, "message exceeds configured limit"));
            }
            None => {
                state.terminal_failure = true;
                state.terminal_close_code = Some(1003);
                state.error_seen_current = true;
                let _ =
                    state
                        .events
                        .push_control(WebSocketEventKind::Error(WebSocketErrorMetadata {
                            kind: WebSocketErrorKind::UnsupportedMessage,
                            recoverable: false,
                        }));
                force_close = Some((4003, "unsupported browser message"));
            }
        }
    }
    if let Some((code, reason)) = force_close {
        if let Some(socket) = inner.state.borrow().socket.clone() {
            let _ = socket.close_with_code_and_reason(code, reason);
        }
    }
}

fn handle_close(inner: &Rc<ClientInner>, generation: u64, event: CloseEvent) {
    let (socket, handlers, reconnect) = {
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.generation != generation {
            return;
        }
        let socket = state.socket.take();
        let handlers = state.handlers.take();
        if let Some(timer) = state.pump_timer.take() {
            clear_timer(timer);
        }

        let explicit = state.explicit_close.take();
        let terminal_close_code = state.terminal_close_code.take();
        let opened = state.opened_current;
        let previous_attempt = if opened { 0 } else { state.current_attempt };
        let abnormal = !matches!(event.code(), 1000 | 1001);
        let reconnect = if explicit.is_none() && !state.terminal_failure && abnormal {
            state.config.reconnect_policy.and_then(|policy| {
                let attempt = previous_attempt.saturating_add(1);
                policy
                    .delay_for_attempt(attempt)
                    .map(|delay| (attempt, delay))
            })
        } else {
            None
        };

        if abnormal && explicit.is_none() && !state.error_seen_current {
            let _ = state
                .events
                .push_control(WebSocketEventKind::Error(WebSocketErrorMetadata {
                    kind: if opened {
                        WebSocketErrorKind::Transport
                    } else {
                        WebSocketErrorKind::Connection
                    },
                    recoverable: reconnect.is_some(),
                }));
        }
        let (code, reason) = explicit
            .as_ref()
            .map(|close| (close.code(), close.reason().to_owned()))
            .unwrap_or_else(|| {
                (
                    terminal_close_code.unwrap_or_else(|| event.code()),
                    event.reason(),
                )
            });
        let _ = state
            .events
            .push_control(WebSocketEventKind::Closed(WebSocketCloseMetadata {
                code,
                reason,
                was_clean: event.was_clean(),
                will_reconnect: reconnect.is_some(),
            }));
        state.lifecycle = if reconnect.is_some() {
            WebSocketState::Reconnecting
        } else {
            WebSocketState::Closed
        };
        state.opened_current = false;
        state.error_seen_current = false;
        state.terminal_failure = false;
        state.terminal_close_code = None;
        (socket, handlers, reconnect)
    };

    if let Some(socket) = socket {
        detach_handlers(&socket);
    }
    drop(handlers);
    if let Some((attempt, delay)) = reconnect {
        schedule_reconnect(inner, attempt, delay);
    } else {
        let mut state = inner.state.borrow_mut();
        state.outbound.clear();
        state.outbound_bytes = 0;
    }
}

fn schedule_reconnect(inner: &Rc<ClientInner>, attempt: u16, delay: Duration) {
    {
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.explicit_close.is_some() {
            return;
        }
        let _ = state.events.push_control(WebSocketEventKind::Reconnecting(
            WebSocketReconnectMetadata { attempt, delay },
        ));
        state.lifecycle = WebSocketState::Reconnecting;
    }

    let weak = Rc::downgrade(inner);
    let callback = Closure::wrap(Box::new(move || {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        {
            let mut state = inner.state.borrow_mut();
            state.reconnect_timer.take();
            if state.disposed || state.explicit_close.is_some() {
                return;
            }
        }
        if open_socket(&inner, attempt).is_err() {
            handle_construction_failure(&inner, attempt);
        }
    }) as Box<dyn FnMut()>);
    let milliseconds = delay.as_millis().min(i32::MAX as u128) as i32;
    let Some(window) = web_sys::window() else {
        handle_construction_failure(inner, attempt);
        return;
    };
    let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        milliseconds,
    ) else {
        handle_construction_failure(inner, attempt);
        return;
    };
    inner.state.borrow_mut().reconnect_timer = Some(Timer { id, callback });
}

fn handle_construction_failure(inner: &Rc<ClientInner>, failed_attempt: u16) {
    let reconnect = {
        let mut state = inner.state.borrow_mut();
        if state.disposed || state.explicit_close.is_some() {
            return;
        }
        let reconnect = state.config.reconnect_policy.and_then(|policy| {
            let attempt = failed_attempt.saturating_add(1);
            policy
                .delay_for_attempt(attempt)
                .map(|delay| (attempt, delay))
        });
        let _ = state
            .events
            .push_control(WebSocketEventKind::Error(WebSocketErrorMetadata {
                kind: WebSocketErrorKind::Connection,
                recoverable: reconnect.is_some(),
            }));
        let _ = state
            .events
            .push_control(WebSocketEventKind::Closed(WebSocketCloseMetadata {
                code: 1006,
                reason: String::new(),
                was_clean: false,
                will_reconnect: reconnect.is_some(),
            }));
        state.lifecycle = if reconnect.is_some() {
            WebSocketState::Reconnecting
        } else {
            WebSocketState::Closed
        };
        reconnect
    };
    if let Some((attempt, delay)) = reconnect {
        schedule_reconnect(inner, attempt, delay);
    }
}

fn pump_outbound(inner: &Rc<ClientInner>) {
    loop {
        let next = {
            let mut state = inner.state.borrow_mut();
            if state.disposed || state.lifecycle != WebSocketState::Open {
                return;
            }
            let Some(socket) = state.socket.clone() else {
                return;
            };
            if socket.buffered_amount() as usize > state.config.max_browser_buffered_bytes {
                drop(state);
                schedule_pump(inner);
                return;
            }
            let Some(message) = state.outbound.pop_front() else {
                if let Some(timer) = state.pump_timer.take() {
                    clear_timer(timer);
                }
                return;
            };
            state.outbound_bytes = state.outbound_bytes.saturating_sub(message.len());
            (socket, message)
        };

        let result = match &next.1 {
            WebSocketMessage::Text(text) => next.0.send_with_str(text),
            WebSocketMessage::Binary(bytes) => next.0.send_with_u8_array(bytes),
        };
        if result.is_err() {
            let mut state = inner.state.borrow_mut();
            state.outbound_bytes = state.outbound_bytes.saturating_add(next.1.len());
            state.outbound.push_front(next.1);
            drop(state);
            schedule_pump(inner);
            return;
        }
    }
}

fn schedule_pump(inner: &Rc<ClientInner>) {
    if inner.state.borrow().pump_timer.is_some() {
        return;
    }
    let weak = Rc::downgrade(inner);
    let callback = Closure::wrap(Box::new(move || {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        inner.state.borrow_mut().pump_timer.take();
        pump_outbound(&inner);
    }) as Box<dyn FnMut()>);
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        OUTBOUND_PUMP_INTERVAL_MS,
    ) else {
        return;
    };
    inner.state.borrow_mut().pump_timer = Some(Timer { id, callback });
}

fn close_explicit(
    inner: &Rc<ClientInner>,
    close: WebSocketClose,
) -> Result<(), WebSocketCloseError> {
    let socket = {
        let mut state = inner.state.borrow_mut();
        if matches!(
            state.lifecycle,
            WebSocketState::Closing | WebSocketState::Closed
        ) {
            return Err(WebSocketCloseError::AlreadyClosed);
        }
        state.lifecycle = WebSocketState::Closing;
        state.explicit_close = Some(close.clone());
        state.outbound.clear();
        state.outbound_bytes = 0;
        if let Some(timer) = state.reconnect_timer.take() {
            clear_timer(timer);
        }
        if let Some(timer) = state.pump_timer.take() {
            clear_timer(timer);
        }
        state.socket.clone()
    };
    if let Some(socket) = socket {
        socket
            .close_with_code_and_reason(close.code(), close.reason())
            .map_err(|_| WebSocketCloseError::BrowserRejected)
    } else {
        let mut state = inner.state.borrow_mut();
        state.lifecycle = WebSocketState::Closed;
        state.explicit_close = None;
        let _ = state
            .events
            .push_control(WebSocketEventKind::Closed(WebSocketCloseMetadata {
                code: close.code(),
                reason: close.reason().to_owned(),
                was_clean: false,
                will_reconnect: false,
            }));
        Ok(())
    }
}

fn reconnect_available(state: &State) -> bool {
    if state.explicit_close.is_some() || state.terminal_failure {
        return false;
    }
    let previous = if state.opened_current {
        0
    } else {
        state.current_attempt
    };
    state
        .config
        .reconnect_policy
        .and_then(|policy| policy.delay_for_attempt(previous.saturating_add(1)))
        .is_some()
}

fn detach_handlers(socket: &WebSocket) {
    socket.set_onopen(None);
    socket.set_onmessage(None);
    socket.set_onerror(None);
    socket.set_onclose(None);
}

fn clear_timer(timer: Timer) {
    if let Some(window) = web_sys::window() {
        window.clear_timeout_with_handle(timer.id);
    }
    drop(timer.callback);
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use crate::websocket::{DenyAllWebSocketHosts, WebSocketHostPolicy};

    #[wasm_bindgen_test]
    fn browser_policy_gate_runs_before_websocket_construction() {
        let config = WebSocketConfig::builder("ws://127.0.0.1:1/private")
            .build()
            .unwrap();
        let policy = DenyAllWebSocketHosts;
        assert!(policy.is_valid());
        assert!(!policy.allows_host(config.host()));
        assert_eq!(
            crate::WebSocketClient::connect(config, &policy).unwrap_err(),
            WebSocketConnectError::HostDenied
        );
    }
}
