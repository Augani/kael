#![allow(clippy::result_large_err)]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::io::Result<()> {
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use tungstenite::handshake::server::{Request, Response};
    use tungstenite::protocol::{Message, WebSocketConfig};

    const MAX_CONCURRENT_CONNECTIONS: usize = 16;

    struct ConnectionPermit(Arc<AtomicUsize>);

    impl Drop for ConnectionPermit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn acquire_connection(active: &Arc<AtomicUsize>) -> Option<ConnectionPermit> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_CONCURRENT_CONNECTIONS).then_some(count + 1)
            })
            .ok()?;
        Some(ConnectionPermit(Arc::clone(active)))
    }

    #[derive(Clone, Copy, Default)]
    enum Mode {
        #[default]
        Echo,
        Oversize,
        Reconnect,
    }

    fn serve(stream: TcpStream, reconnect_count: Arc<AtomicUsize>, _permit: ConnectionPermit) {
        let mode = Arc::new(Mutex::new(Mode::Echo));
        let selected_mode = Arc::clone(&mode);
        let config = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024))
            .max_frame_size(Some(4 * 1024));
        let Ok(mut socket) = tungstenite::accept_hdr_with_config(
            stream,
            move |request: &Request, mut response: Response| {
                if request.uri().path() == "/oversize" {
                    *selected_mode
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Mode::Oversize;
                } else if request.uri().path() == "/reconnect" {
                    *selected_mode
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Mode::Reconnect;
                }
                let offered = request
                    .headers()
                    .get("Sec-WebSocket-Protocol")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if offered
                    .split(',')
                    .any(|protocol| protocol.trim() == "kael.smoke.v1")
                {
                    response
                        .headers_mut()
                        .insert("Sec-WebSocket-Protocol", "kael.smoke.v1".parse().unwrap());
                }
                Ok(response)
            },
            Some(config),
        ) else {
            return;
        };

        let mode = *mode.lock().unwrap_or_else(|error| error.into_inner());
        match mode {
            Mode::Echo => {
                eprintln!("KAEL_WEBSOCKET_ECHO_CONNECTED mode=echo");
                loop {
                    match socket.read() {
                        Ok(message @ (Message::Text(_) | Message::Binary(_))) => {
                            eprintln!(
                                "KAEL_WEBSOCKET_ECHO_MESSAGE kind={} bytes={}",
                                if message.is_text() { "text" } else { "binary" },
                                message.len()
                            );
                            if socket.send(message).is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            eprintln!(
                                "KAEL_WEBSOCKET_ECHO_CLOSE code={}",
                                frame.map(|frame| u16::from(frame.code)).unwrap_or(1005)
                            );
                            let _ = socket.flush();
                            break;
                        }
                        Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                        Err(_) => break,
                    }
                }
            }
            Mode::Oversize => {
                eprintln!("KAEL_WEBSOCKET_ECHO_CONNECTED mode=oversize");
                let Ok(ready) = socket.read() else {
                    return;
                };
                eprintln!(
                    "KAEL_WEBSOCKET_ECHO_MESSAGE kind={} bytes={}",
                    if ready.is_text() { "text" } else { "other" },
                    ready.len()
                );
                let _ = socket.send(Message::Binary(vec![0x5a; 2 * 1024].into()));
                while let Ok(message) = socket.read() {
                    if let Message::Close(frame) = message {
                        eprintln!(
                            "KAEL_WEBSOCKET_ECHO_CLOSE code={}",
                            frame.map(|frame| u16::from(frame.code)).unwrap_or(1005)
                        );
                        let _ = socket.flush();
                        break;
                    }
                }
            }
            Mode::Reconnect => {
                let connection = reconnect_count.fetch_add(1, Ordering::AcqRel);
                eprintln!("KAEL_WEBSOCKET_ECHO_CONNECTED mode=reconnect attempt={connection}");
                let Ok(message) = socket.read() else {
                    return;
                };
                eprintln!(
                    "KAEL_WEBSOCKET_ECHO_MESSAGE kind={} bytes={}",
                    if message.is_text() { "text" } else { "other" },
                    message.len()
                );
                if connection == 0 {
                    drop(socket);
                    return;
                }
                if socket.send(message).is_err() {
                    return;
                }
                while let Ok(message) = socket.read() {
                    if let Message::Close(frame) = message {
                        eprintln!(
                            "KAEL_WEBSOCKET_ECHO_CLOSE code={}",
                            frame.map(|frame| u16::from(frame.code)).unwrap_or(1005)
                        );
                        let _ = socket.flush();
                        break;
                    }
                }
            }
        }
    }

    let port = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8_134);
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let reconnect_count = Arc::new(AtomicUsize::new(0));
    let active_connections = Arc::new(AtomicUsize::new(0));
    println!("KAEL_WEBSOCKET_ECHO_READY port={port}");
    for stream in listener.incoming().flatten() {
        let Some(permit) = acquire_connection(&active_connections) else {
            continue;
        };
        let reconnect_count = Arc::clone(&reconnect_count);
        let _ = std::thread::Builder::new()
            .name("kael-websocket-smoke-echo".to_string())
            .spawn(move || serve(stream, reconnect_count, permit));
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
