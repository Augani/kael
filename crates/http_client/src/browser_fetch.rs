use std::io;

use super::{
    AsyncBody, MAX_BUFFERED_HTTP_BODY_BYTES, Method, RedirectPolicy, Request, Response, StatusCode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserRedirectMode {
    Manual,
    Follow,
}

#[derive(Debug, PartialEq, Eq)]
struct PreparedBrowserRequest {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    redirect: BrowserRedirectMode,
}

fn checked_buffered_len(current: usize, additional: usize) -> io::Result<usize> {
    let next = current
        .checked_add(additional)
        .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "HTTP body size overflow"))?;
    if next > MAX_BUFFERED_HTTP_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "HTTP body exceeds {} byte limit",
                MAX_BUFFERED_HTTP_BODY_BYTES
            ),
        ));
    }
    Ok(next)
}

fn browser_redirect_mode(policy: RedirectPolicy) -> io::Result<BrowserRedirectMode> {
    match policy {
        RedirectPolicy::NoFollow | RedirectPolicy::FollowLimit(0) => {
            Ok(BrowserRedirectMode::Manual)
        }
        RedirectPolicy::FollowAll => Ok(BrowserRedirectMode::Follow),
        RedirectPolicy::FollowLimit(limit) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "browser Fetch cannot enforce a {limit}-redirect limit; use NoFollow or FollowAll"
            ),
        )),
    }
}

fn prepare_browser_request(
    mut parts: http::request::Parts,
    body: Vec<u8>,
) -> io::Result<PreparedBrowserRequest> {
    checked_buffered_len(0, body.len())?;

    if !body.is_empty() && matches!(parts.method, Method::GET | Method::HEAD) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "browser Fetch does not permit a body on GET or HEAD requests",
        ));
    }
    if matches!(parts.method.as_str(), "CONNECT" | "TRACE" | "TRACK") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("browser Fetch does not permit the {} method", parts.method),
        ));
    }

    let redirect = browser_redirect_mode(
        parts
            .extensions
            .remove::<RedirectPolicy>()
            .unwrap_or_default(),
    )?;
    let mut headers = Vec::with_capacity(parts.headers.len());
    for (name, value) in &parts.headers {
        let value = value.to_str().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("request header {name} cannot be represented by browser Fetch: {error}"),
            )
        })?;
        headers.push((name.as_str().to_owned(), value.to_owned()));
    }

    Ok(PreparedBrowserRequest {
        url: parts.uri.to_string(),
        method: parts.method.as_str().to_owned(),
        headers,
        body,
        redirect,
    })
}

fn validate_content_length(headers: &[(String, String)]) -> io::Result<()> {
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        for value in value.split(',') {
            let length = value.trim().parse::<u64>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid browser response Content-Length: {error}"),
                )
            })?;
            if length > MAX_BUFFERED_HTTP_BODY_BYTES as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "HTTP response exceeds {} byte limit",
                        MAX_BUFFERED_HTTP_BODY_BYTES
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn fetch_error_kind(name: &str) -> Option<io::ErrorKind> {
    match name {
        "AbortError" => Some(io::ErrorKind::Interrupted),
        "TimeoutError" => Some(io::ErrorKind::TimedOut),
        "SecurityError" | "NotAllowedError" => Some(io::ErrorKind::PermissionDenied),
        "NetworkError" => Some(io::ErrorKind::ConnectionAborted),
        _ => None,
    }
}

fn build_http_response(
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> io::Result<Response<AsyncBody>> {
    if status == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "browser Fetch returned an opaque response; check CORS and manual redirect policy",
        ));
    }
    let status = StatusCode::from_u16(status).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid browser response status: {error}"),
        )
    })?;
    validate_content_length(&headers)?;
    checked_buffered_len(0, body.len())?;

    let mut response = Response::builder().status(status);
    for (name, value) in headers {
        response = response.header(name, value);
    }
    response.body(AsyncBody::from(body)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid browser response headers: {error}"),
        )
    })
}

#[cfg(target_arch = "wasm32")]
mod web {
    use futures::{channel::oneshot, future::BoxFuture};
    use js_sys::{Array, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast as _, JsValue};
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{
        AbortController, Headers, ReadableStreamDefaultReader, RequestInit, RequestRedirect,
    };

    use super::*;

    struct AbortOnDrop(Option<AbortController>);

    impl AbortOnDrop {
        fn disarm(&mut self) {
            self.0 = None;
        }
    }

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            if let Some(controller) = self.0.take() {
                controller.abort();
            }
        }
    }

    fn js_error(error: JsValue, context: &str, fallback: io::ErrorKind) -> io::Error {
        let (name, message) = if let Some(exception) = error.dyn_ref::<web_sys::DomException>() {
            (Some(exception.name()), exception.message())
        } else if let Some(error) = error.dyn_ref::<js_sys::Error>() {
            (Some(error.name().into()), error.message().into())
        } else if let Some(message) = error.as_string() {
            (None, message)
        } else {
            (None, "JavaScript exception".to_owned())
        };
        let kind = name
            .as_deref()
            .and_then(fetch_error_kind)
            .unwrap_or(fallback);
        let message = match name {
            Some(name) if !name.is_empty() => format!("{context}: {name}: {message}"),
            _ => format!("{context}: {message}"),
        };
        io::Error::new(kind, message)
    }

    fn collect_response_headers(headers: Headers) -> io::Result<Vec<(String, String)>> {
        let mut result = Vec::new();
        for entry in headers.entries() {
            let entry = entry.map_err(|error| {
                js_error(
                    error,
                    "could not iterate browser response headers",
                    io::ErrorKind::InvalidData,
                )
            })?;
            let pair = Array::from(&entry);
            let name = pair.get(0).as_string().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "browser response header name was not a string",
                )
            })?;
            let value = pair.get(1).as_string().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "browser response header value was not a string",
                )
            })?;
            result.push((name, value));
        }
        Ok(result)
    }

    async fn read_response_body(response: &web_sys::Response) -> io::Result<Vec<u8>> {
        let Some(stream) = response.body() else {
            return Ok(Vec::new());
        };
        let reader = ReadableStreamDefaultReader::new(&stream).map_err(|error| {
            js_error(
                error,
                "could not open browser response stream",
                io::ErrorKind::InvalidData,
            )
        })?;
        let mut body = Vec::new();

        loop {
            let read = JsFuture::from(reader.read()).await.map_err(|error| {
                js_error(
                    error,
                    "browser response stream failed",
                    io::ErrorKind::ConnectionAborted,
                )
            })?;
            let done = Reflect::get(&read, &JsValue::from_str("done"))
                .map_err(|error| {
                    js_error(
                        error,
                        "could not inspect browser response stream",
                        io::ErrorKind::InvalidData,
                    )
                })?
                .as_bool()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "browser response stream returned a non-boolean done flag",
                    )
                })?;
            if done {
                reader.release_lock();
                return Ok(body);
            }

            let value = Reflect::get(&read, &JsValue::from_str("value")).map_err(|error| {
                js_error(
                    error,
                    "could not read browser response stream chunk",
                    io::ErrorKind::InvalidData,
                )
            })?;
            let chunk = value.dyn_into::<Uint8Array>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "browser response stream returned a non-byte chunk",
                )
            })?;
            let chunk_len = usize::try_from(chunk.length()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    format!("browser response chunk length overflow: {error}"),
                )
            })?;
            let next_len = checked_buffered_len(body.len(), chunk_len)?;
            body.try_reserve(chunk_len).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    format!("could not reserve browser response buffer: {error}"),
                )
            })?;
            let start = body.len();
            body.resize(next_len, 0);
            chunk.copy_to(&mut body[start..]);
        }
    }

    fn build_fetch_request(
        request: PreparedBrowserRequest,
        signal: &web_sys::AbortSignal,
    ) -> io::Result<web_sys::Request> {
        let init = RequestInit::new();
        init.set_method(&request.method);
        init.set_signal(Some(signal));
        init.set_redirect(match request.redirect {
            BrowserRedirectMode::Manual => RequestRedirect::Manual,
            BrowserRedirectMode::Follow => RequestRedirect::Follow,
        });

        let headers = Headers::new().map_err(|error| {
            js_error(
                error,
                "could not create browser request headers",
                io::ErrorKind::InvalidInput,
            )
        })?;
        for (name, value) in request.headers {
            headers.append(&name, &value).map_err(|error| {
                js_error(
                    error,
                    "browser rejected a request header",
                    io::ErrorKind::InvalidInput,
                )
            })?;
        }
        init.set_headers_headers(&headers);

        let body = (!request.body.is_empty()).then(|| Uint8Array::from(request.body.as_slice()));
        if let Some(body) = &body {
            init.set_body_opt_u8_array(Some(body));
        }

        web_sys::Request::new_with_str_and_init(&request.url, &init).map_err(|error| {
            js_error(
                error,
                "could not construct browser request",
                io::ErrorKind::InvalidInput,
            )
        })
    }

    async fn send_local(
        request: Request<AsyncBody>,
        controller: AbortController,
    ) -> anyhow::Result<Response<AsyncBody>> {
        let signal = controller.signal();
        let mut abort_on_drop = AbortOnDrop(Some(controller));
        let (parts, mut body) = request.into_parts();
        let body = body
            .read_to_end_limited(MAX_BUFFERED_HTTP_BODY_BYTES)
            .await?;
        if signal.aborted() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "browser Fetch request was cancelled",
            )
            .into());
        }
        let request = prepare_browser_request(parts, body)?;
        let request = build_fetch_request(request, &signal)?;
        let window = web_sys::window().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "browser Fetch requires a Window global",
            )
        })?;
        let response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|error| {
                js_error(
                    error,
                    "browser Fetch request failed",
                    io::ErrorKind::ConnectionAborted,
                )
            })?
            .dyn_into::<web_sys::Response>()
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "browser Fetch returned a non-Response value",
                )
            })?;
        let status = response.status();
        let headers = collect_response_headers(response.headers())?;
        validate_content_length(&headers)?;
        let body = read_response_body(&response).await?;
        let response = build_http_response(status, headers, body)?;
        abort_on_drop.disarm();
        Ok(response)
    }

    pub(super) fn send(
        request: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let controller = match AbortController::new() {
            Ok(controller) => controller,
            Err(error) => {
                let error = js_error(
                    error,
                    "could not create browser request cancellation signal",
                    io::ErrorKind::Other,
                );
                return Box::pin(async move { Err(error.into()) });
            }
        };
        let worker_controller = controller.clone();
        let (sender, receiver) = oneshot::channel();
        spawn_local(async move {
            let result = send_local(request, worker_controller).await;
            let _ = sender.send(result);
        });

        // Construct the guard before the future so dropping an unpolled future
        // still cancels the already-spawned browser task.
        let mut abort_on_drop = AbortOnDrop(Some(controller));
        Box::pin(async move {
            let result = receiver.await.map_err(|_| {
                io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "browser Fetch task ended before returning a response",
                )
            })?;
            abort_on_drop.disarm();
            result
        })
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn send(
    request: Request<AsyncBody>,
) -> futures::future::BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
    web::send(request)
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn request_parts_preserve_method_headers_body_and_redirect_mode() {
        let request = Request::builder()
            .method(Method::POST)
            .uri("https://example.com/items")
            .header("content-type", "application/octet-stream")
            .header("x-kael-test", "one")
            .header("x-kael-test", "two")
            .extension(RedirectPolicy::FollowAll)
            .body(())
            .unwrap();
        let (parts, ()) = request.into_parts();
        let request = prepare_browser_request(parts, vec![1, 2, 3]).unwrap();

        assert_eq!(request.url, "https://example.com/items");
        assert_eq!(request.method, "POST");
        assert_eq!(request.body, [1, 2, 3]);
        assert_eq!(request.redirect, BrowserRedirectMode::Follow);
        assert_eq!(
            request
                .headers
                .iter()
                .filter(|(name, _)| name == "x-kael-test")
                .count(),
            2
        );
    }

    #[test]
    fn request_helpers_reject_fetch_incompatible_semantics() {
        let get = Request::builder()
            .method(Method::GET)
            .uri("https://example.com")
            .body(())
            .unwrap();
        let (parts, ()) = get.into_parts();
        let error = prepare_browser_request(parts, vec![1]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

        assert_eq!(
            browser_redirect_mode(RedirectPolicy::FollowLimit(0)).unwrap(),
            BrowserRedirectMode::Manual
        );
        let error = browser_redirect_mode(RedirectPolicy::FollowLimit(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn response_helpers_preserve_status_headers_and_bytes() {
        let mut response = build_http_response(
            206,
            vec![
                (
                    "content-type".to_owned(),
                    "application/octet-stream".to_owned(),
                ),
                ("content-length".to_owned(), "3".to_owned()),
            ],
            vec![7, 8, 9],
        )
        .unwrap();

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            block_on(
                response
                    .body_mut()
                    .read_to_end_limited(MAX_BUFFERED_HTTP_BODY_BYTES)
            )
            .unwrap(),
            [7, 8, 9]
        );
    }

    #[test]
    fn response_helpers_enforce_limits_and_reject_opaque_status() {
        assert_eq!(
            checked_buffered_len(MAX_BUFFERED_HTTP_BODY_BYTES, 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            checked_buffered_len(usize::MAX, 1).unwrap_err().kind(),
            io::ErrorKind::OutOfMemory
        );
        assert!(
            validate_content_length(&[(
                "content-length".to_owned(),
                (MAX_BUFFERED_HTTP_BODY_BYTES as u64 + 1).to_string(),
            )])
            .is_err()
        );
        let error = match build_http_response(0, Vec::new(), Vec::new()) {
            Ok(_) => panic!("accepted an opaque browser response"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn fetch_error_names_map_cancellation_and_browser_failures() {
        assert_eq!(
            fetch_error_kind("AbortError"),
            Some(io::ErrorKind::Interrupted)
        );
        assert_eq!(
            fetch_error_kind("TimeoutError"),
            Some(io::ErrorKind::TimedOut)
        );
        assert_eq!(
            fetch_error_kind("SecurityError"),
            Some(io::ErrorKind::PermissionDenied)
        );
        assert_eq!(
            fetch_error_kind("NetworkError"),
            Some(io::ErrorKind::ConnectionAborted)
        );
        assert_eq!(fetch_error_kind("TypeError"), None);
    }
}
