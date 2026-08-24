#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use kael_net::{
        AllowAllWebSocketHosts, DenyAllWebSocketHosts, WebSocketClient, WebSocketClose,
        WebSocketCloseMetadata, WebSocketConfig, WebSocketConnectError, WebSocketErrorKind,
        WebSocketEvent, WebSocketEventKind, WebSocketMessage, WebSocketReconnectPolicy,
        WebSocketSendError,
    };
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::closure::Closure;

    const ECHO_URL: &str = "ws://127.0.0.1:8134/echo";
    const OVERSIZE_URL: &str = "ws://127.0.0.1:8134/oversize";
    const RECONNECT_URL: &str = "ws://127.0.0.1:8134/reconnect";

    struct Probe {
        primary: WebSocketClient,
        overflow: Option<WebSocketClient>,
        reconnect: Option<WebSocketClient>,
        timer_id: i32,
        last_primary_sequence: Option<u64>,
        last_overflow_sequence: Option<u64>,
        last_reconnect_sequence: Option<u64>,
        protocol: bool,
        queued_echo: bool,
        text_echo: bool,
        binary_echo: bool,
        ordered: bool,
        explicit_close_requested: bool,
        close: bool,
        cancellation: bool,
        overflow_error: bool,
        overflow_close: bool,
        reconnect_error: bool,
        reconnect_closed: bool,
        reconnect_scheduled: bool,
        reconnect_open: bool,
        reconnect_echo: bool,
        reconnect_close: bool,
        backpressure: bool,
        policy: bool,
        size: bool,
        finished: bool,
    }

    pub fn start() {
        if let Err(error) = start_checked() {
            publish_failure(error);
        }
    }

    fn start_checked() -> Result<(), &'static str> {
        set_marker("data-kael-websocket-probe", "running")?;
        let config = smoke_config(ECHO_URL)?;
        let policy = matches!(
            WebSocketClient::connect(config.clone(), &DenyAllWebSocketHosts),
            Err(WebSocketConnectError::HostDenied)
        );
        let primary = WebSocketClient::connect(config, &AllowAllWebSocketHosts)
            .map_err(|_| "browser rejected primary WebSocket")?;
        primary
            .try_send(WebSocketMessage::Text("queued-before-open".to_string()))
            .map_err(|_| "failed to queue pre-open message")?;
        let backpressure = matches!(
            primary.try_send(WebSocketMessage::Text("must-backpressure".to_string())),
            Err(WebSocketSendError::Backpressure)
        );
        let size = matches!(
            primary.try_send(WebSocketMessage::Binary(vec![0; 1_025])),
            Err(WebSocketSendError::MessageTooLarge { .. })
        );

        let probe = Rc::new(RefCell::new(Probe {
            primary,
            overflow: None,
            reconnect: None,
            timer_id: 0,
            last_primary_sequence: None,
            last_overflow_sequence: None,
            last_reconnect_sequence: None,
            protocol: false,
            queued_echo: false,
            text_echo: false,
            binary_echo: false,
            ordered: true,
            explicit_close_requested: false,
            close: false,
            cancellation: false,
            overflow_error: false,
            overflow_close: false,
            reconnect_error: false,
            reconnect_closed: false,
            reconnect_scheduled: false,
            reconnect_open: false,
            reconnect_echo: false,
            reconnect_close: false,
            backpressure,
            policy,
            size,
            finished: false,
        }));
        let timer_probe = Rc::clone(&probe);
        let callback = Closure::wrap(Box::new(move || {
            tick(&timer_probe);
        }) as Box<dyn FnMut()>);
        let window = web_sys::window().ok_or("missing browser Window")?;
        let timer_id = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                5,
            )
            .map_err(|_| "browser rejected probe timer")?;
        probe.borrow_mut().timer_id = timer_id;
        callback.forget();
        Ok(())
    }

    fn smoke_config(url: &str) -> Result<WebSocketConfig, &'static str> {
        WebSocketConfig::builder(url)
            .protocol("kael.smoke.v1")
            .inbound_capacity(8)
            .outbound_capacity(1)
            .max_message_bytes(1_024)
            .max_inbound_bytes(8 * 1_024)
            .max_outbound_bytes(2 * 1_024)
            .max_browser_buffered_bytes(1_024)
            .build()
            .map_err(|_| "invalid smoke WebSocket configuration")
    }

    fn tick(probe: &Rc<RefCell<Probe>>) {
        if probe.borrow().finished {
            return;
        }
        let primary = probe.borrow().primary.clone();
        while let Some(event) = primary.poll_event() {
            if let Err(error) = handle_primary(probe, event) {
                fail_probe(probe, error);
                return;
            }
        }
        let overflow = probe.borrow().overflow.clone();
        if let Some(overflow) = overflow {
            while let Some(event) = overflow.poll_event() {
                if let Err(error) = handle_overflow(probe, event) {
                    fail_probe(probe, error);
                    return;
                }
            }
        }
        let reconnect = probe.borrow().reconnect.clone();
        if let Some(reconnect) = reconnect {
            while let Some(event) = reconnect.poll_event() {
                if let Err(error) = handle_reconnect(probe, event) {
                    fail_probe(probe, error);
                    return;
                }
            }
        }

        let passed = {
            let probe = probe.borrow();
            probe.protocol
                && probe.queued_echo
                && probe.text_echo
                && probe.binary_echo
                && probe.ordered
                && probe.close
                && probe.cancellation
                && probe.overflow_error
                && probe.overflow_close
                && probe.reconnect_error
                && probe.reconnect_closed
                && probe.reconnect_scheduled
                && probe.reconnect_open
                && probe.reconnect_echo
                && probe.reconnect_close
                && probe.backpressure
                && probe.policy
                && probe.size
        };
        if passed {
            finish_probe(probe);
        }
    }

    fn handle_primary(
        probe: &Rc<RefCell<Probe>>,
        event: WebSocketEvent,
    ) -> Result<(), &'static str> {
        {
            let mut probe = probe.borrow_mut();
            let ordered = check_sequence(&mut probe.last_primary_sequence, &event);
            probe.ordered &= ordered;
        }
        match event.into_kind() {
            WebSocketEventKind::Open(metadata) => {
                if metadata.protocol.as_deref() != Some("kael.smoke.v1") {
                    return Err("server did not negotiate the expected subprotocol");
                }
                probe.borrow_mut().protocol = true;
                let primary = probe.borrow().primary.clone();
                primary
                    .try_send(WebSocketMessage::Text("text-echo".to_string()))
                    .map_err(|_| "failed to send text message")?;
                primary
                    .try_send(WebSocketMessage::Binary(vec![1, 3, 5, 7]))
                    .map_err(|_| "failed to send binary message")?;
            }
            WebSocketEventKind::Message(WebSocketMessage::Text(text)) => {
                let mut probe = probe.borrow_mut();
                if text == "queued-before-open" {
                    probe.queued_echo = true;
                } else if text == "text-echo" {
                    probe.text_echo = true;
                } else {
                    return Err("unexpected text echo");
                }
                maybe_close_primary(&mut probe)?;
            }
            WebSocketEventKind::Message(WebSocketMessage::Binary(bytes)) => {
                if bytes != [1, 3, 5, 7] {
                    return Err("unexpected binary echo");
                }
                let mut probe = probe.borrow_mut();
                probe.binary_echo = true;
                maybe_close_primary(&mut probe)?;
            }
            WebSocketEventKind::Closed(WebSocketCloseMetadata { code: 3000, .. }) => {
                {
                    let mut probe = probe.borrow_mut();
                    probe.close = true;
                    probe.cancellation = true;
                }
                let overflow =
                    WebSocketClient::connect(smoke_config(OVERSIZE_URL)?, &AllowAllWebSocketHosts)
                        .map_err(|_| "browser rejected overflow WebSocket")?;
                probe.borrow_mut().overflow = Some(overflow);
            }
            WebSocketEventKind::Closed(_) => return Err("primary WebSocket closed unexpectedly"),
            WebSocketEventKind::Error(_) => return Err("primary WebSocket reported an error"),
            WebSocketEventKind::Reconnecting(_) => {
                return Err("primary WebSocket reconnected unexpectedly");
            }
        }
        Ok(())
    }

    fn handle_overflow(
        probe: &Rc<RefCell<Probe>>,
        event: WebSocketEvent,
    ) -> Result<(), &'static str> {
        {
            let mut probe = probe.borrow_mut();
            let ordered = check_sequence(&mut probe.last_overflow_sequence, &event);
            probe.ordered &= ordered;
        }
        match event.into_kind() {
            WebSocketEventKind::Open(metadata) => {
                if metadata.protocol.as_deref() != Some("kael.smoke.v1") {
                    return Err("overflow socket protocol differed");
                }
                probe
                    .borrow()
                    .overflow
                    .as_ref()
                    .ok_or("overflow socket handle disappeared")?
                    .try_send(WebSocketMessage::Text("ready".to_string()))
                    .map_err(|_| "overflow readiness send failed")?;
            }
            WebSocketEventKind::Error(metadata)
                if metadata.kind == WebSocketErrorKind::MessageTooLarge =>
            {
                probe.borrow_mut().overflow_error = true;
            }
            WebSocketEventKind::Closed(WebSocketCloseMetadata { code: 1009, .. }) => {
                probe.borrow_mut().overflow_close = true;
                let reconnect_policy = WebSocketReconnectPolicy::new(
                    1,
                    Duration::from_millis(100),
                    Duration::from_millis(100),
                )
                .map_err(|_| "invalid reconnect policy")?;
                let reconnect_config = WebSocketConfig::builder(RECONNECT_URL)
                    .protocol("kael.smoke.v1")
                    .inbound_capacity(16)
                    .outbound_capacity(4)
                    .max_message_bytes(1_024)
                    .max_inbound_bytes(16 * 1_024)
                    .max_outbound_bytes(4 * 1_024)
                    .reconnect_policy(reconnect_policy)
                    .build()
                    .map_err(|_| "invalid reconnect socket configuration")?;
                let reconnect = WebSocketClient::connect(reconnect_config, &AllowAllWebSocketHosts)
                    .map_err(|_| "browser rejected reconnect WebSocket")?;
                probe.borrow_mut().reconnect = Some(reconnect);
            }
            WebSocketEventKind::Message(_) => {
                return Err("oversized message escaped the configured limit");
            }
            WebSocketEventKind::Error(_) => return Err("overflow socket error kind differed"),
            WebSocketEventKind::Closed(_) => return Err("overflow socket close code differed"),
            WebSocketEventKind::Reconnecting(_) => {
                return Err("overflow socket reconnected unexpectedly");
            }
        }
        Ok(())
    }

    fn handle_reconnect(
        probe: &Rc<RefCell<Probe>>,
        event: WebSocketEvent,
    ) -> Result<(), &'static str> {
        {
            let mut probe = probe.borrow_mut();
            let ordered = check_sequence(&mut probe.last_reconnect_sequence, &event);
            probe.ordered &= ordered;
        }
        match event.into_kind() {
            WebSocketEventKind::Open(metadata) => {
                let reconnect = probe
                    .borrow()
                    .reconnect
                    .clone()
                    .ok_or("reconnect socket handle disappeared")?;
                match metadata.reconnect_attempt {
                    0 => reconnect
                        .try_send(WebSocketMessage::Text("first attempt".to_string()))
                        .map_err(|_| "first reconnect readiness send failed")?,
                    1 => {
                        probe.borrow_mut().reconnect_open = true;
                        reconnect
                            .try_send(WebSocketMessage::Text("after reconnect".to_string()))
                            .map_err(|_| "reconnected echo send failed")?;
                    }
                    _ => return Err("unexpected reconnect attempt opened"),
                }
            }
            WebSocketEventKind::Error(metadata)
                if metadata.kind == WebSocketErrorKind::Transport && metadata.recoverable =>
            {
                probe.borrow_mut().reconnect_error = true;
            }
            WebSocketEventKind::Closed(WebSocketCloseMetadata {
                code: 1006,
                will_reconnect: true,
                ..
            }) => probe.borrow_mut().reconnect_closed = true,
            WebSocketEventKind::Reconnecting(metadata) if metadata.attempt == 1 => {
                probe.borrow_mut().reconnect_scheduled = true;
            }
            WebSocketEventKind::Message(WebSocketMessage::Text(text))
                if text == "after reconnect" =>
            {
                let reconnect = {
                    let mut probe = probe.borrow_mut();
                    probe.reconnect_echo = true;
                    probe
                        .reconnect
                        .clone()
                        .ok_or("reconnect socket handle disappeared")?
                };
                reconnect
                    .close(
                        WebSocketClose::new(3002, "reconnect complete")
                            .map_err(|_| "invalid reconnect close")?,
                    )
                    .map_err(|_| "reconnect close was rejected")?;
            }
            WebSocketEventKind::Closed(WebSocketCloseMetadata {
                code: 3002,
                will_reconnect: false,
                ..
            }) => probe.borrow_mut().reconnect_close = true,
            WebSocketEventKind::Error(_) => return Err("reconnect socket error differed"),
            WebSocketEventKind::Closed(_) => return Err("reconnect socket close differed"),
            WebSocketEventKind::Message(_) => return Err("reconnect echo differed"),
            WebSocketEventKind::Reconnecting(_) => {
                return Err("reconnect schedule metadata differed");
            }
        }
        Ok(())
    }

    fn maybe_close_primary(probe: &mut Probe) -> Result<(), &'static str> {
        if probe.queued_echo
            && probe.text_echo
            && probe.binary_echo
            && !probe.explicit_close_requested
        {
            probe.explicit_close_requested = true;
            probe
                .primary
                .close(
                    WebSocketClose::new(3000, "smoke complete")
                        .map_err(|_| "invalid explicit close")?,
                )
                .map_err(|_| "explicit close was rejected")?;
        }
        Ok(())
    }

    fn check_sequence(last: &mut Option<u64>, event: &WebSocketEvent) -> bool {
        let ordered = last.is_none_or(|previous| event.sequence() > previous);
        *last = Some(event.sequence());
        ordered
    }

    fn finish_probe(probe: &Rc<RefCell<Probe>>) {
        let timer_id = {
            let mut probe = probe.borrow_mut();
            probe.finished = true;
            probe.timer_id
        };
        if let Some(window) = web_sys::window() {
            window.clear_interval_with_handle(timer_id);
        }
        for (name, value) in [
            ("data-kael-websocket-probe", "passed"),
            ("data-kael-websocket-protocol", "passed"),
            ("data-kael-websocket-text", "passed"),
            ("data-kael-websocket-binary", "passed"),
            ("data-kael-websocket-ordered", "passed"),
            ("data-kael-websocket-close", "passed"),
            ("data-kael-websocket-error", "passed"),
            ("data-kael-websocket-cancellation", "passed"),
            ("data-kael-websocket-backpressure", "passed"),
            ("data-kael-websocket-policy", "passed"),
            ("data-kael-websocket-size", "passed"),
            ("data-kael-websocket-reconnect", "passed"),
        ] {
            let _ = set_marker(name, value);
        }
    }

    fn fail_probe(probe: &Rc<RefCell<Probe>>, error: &'static str) {
        let timer_id = {
            let mut probe = probe.borrow_mut();
            probe.finished = true;
            probe.timer_id
        };
        if let Some(window) = web_sys::window() {
            window.clear_interval_with_handle(timer_id);
        }
        publish_failure(error);
    }

    fn publish_failure(error: &str) {
        let _ = set_marker("data-kael-websocket-probe", "failed");
        let _ = set_marker("data-kael-websocket-error-detail", error);
    }

    fn set_marker(name: &str, value: &str) -> Result<(), &'static str> {
        let root = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
            .ok_or("missing document root")?;
        root.set_attribute(name, value)
            .map_err(|_| "failed to publish browser marker")
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    browser::start();

    #[cfg(not(target_arch = "wasm32"))]
    println!("browser_websocket_smoke is a WebAssembly-only maintained probe");
}
