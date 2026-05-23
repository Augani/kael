use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;

/// HTTP method for API requests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
    /// PUT request.
    Put,
    /// PATCH request.
    Patch,
    /// DELETE request.
    Delete,
}

/// A typed, transport-agnostic API request.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiRequest {
    /// The HTTP method.
    pub method: HttpMethod,
    /// The request path (e.g. "/api/v1/users").
    pub path: String,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Optional request body as raw bytes.
    pub body: Option<Vec<u8>>,
    /// Optional timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Maximum number of bytes to read from the response body.
    pub max_response_bytes: Option<u64>,
}

impl fmt::Debug for ApiRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut headers = self.headers.clone();
        for (key, value) in &mut headers {
            if is_sensitive_header(key) {
                *value = "<redacted>".to_string();
            }
        }
        let body = self
            .body
            .as_ref()
            .map(|body| format!("<{} bytes>", body.len()));

        formatter
            .debug_struct("ApiRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("headers", &headers)
            .field("body", &body)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

fn is_sensitive_header(key: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
    ]
    .iter()
    .any(|header| key.eq_ignore_ascii_case(header))
}

/// A typed API response.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body as raw bytes.
    pub body: Vec<u8>,
}

impl ApiRequest {
    /// Create a GET request for the given path.
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            path: path.into(),
            headers: HashMap::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: Some(10 * 1024 * 1024),
        }
    }

    /// Create a POST request for the given path.
    pub fn post(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            path: path.into(),
            headers: HashMap::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: Some(10 * 1024 * 1024),
        }
    }

    /// Create a PUT request for the given path.
    pub fn put(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Put,
            path: path.into(),
            headers: HashMap::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: Some(10 * 1024 * 1024),
        }
    }

    /// Create a PATCH request for the given path.
    pub fn patch(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Patch,
            path: path.into(),
            headers: HashMap::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: Some(10 * 1024 * 1024),
        }
    }

    /// Create a DELETE request for the given path.
    pub fn delete(path: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Delete,
            path: path.into(),
            headers: HashMap::new(),
            body: None,
            timeout_ms: None,
            max_response_bytes: Some(10 * 1024 * 1024),
        }
    }

    /// Add a header to the request.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set a JSON-serializable body, also setting the Content-Type header.
    pub fn with_json_body<T: Serialize>(mut self, body: &T) -> Result<Self> {
        let json_bytes = serde_json::to_vec(body)
            .map_err(|err| anyhow!("failed to serialize request body: {err}"))?;
        self.body = Some(json_bytes);
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        Ok(self)
    }

    /// Set the request timeout in milliseconds.
    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Set the maximum number of bytes to read from the response body.
    pub fn with_max_response_bytes(mut self, limit: u64) -> Self {
        self.max_response_bytes = Some(limit);
        self
    }

    /// Remove the response body size limit, allowing unlimited bytes.
    pub fn without_response_limit(mut self) -> Self {
        self.max_response_bytes = None;
        self
    }
}

impl ApiResponse {
    /// Returns true if the status code indicates success (2xx).
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Returns the raw response body bytes without cloning.
    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    /// Deserialize the response body as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body)
            .map_err(|err| anyhow!("failed to deserialize response body: {err}"))
    }

    /// Interpret the response body as a borrowed UTF-8 string slice.
    pub fn text_ref(&self) -> Result<&str> {
        std::str::from_utf8(&self.body)
            .map_err(|err| anyhow!("response body is not valid UTF-8: {err}"))
    }

    /// Interpret the response body as UTF-8 text.
    pub fn text(&self) -> Result<String> {
        self.text_ref().map(ToOwned::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_request() {
        let req = ApiRequest::get("/api/users");
        assert_eq!(req.method, HttpMethod::Get);
        assert_eq!(req.path, "/api/users");
        assert!(req.headers.is_empty());
        assert!(req.body.is_none());
        assert!(req.timeout_ms.is_none());
    }

    #[test]
    fn test_post_request() {
        let req = ApiRequest::post("/api/users");
        assert_eq!(req.method, HttpMethod::Post);
    }

    #[test]
    fn test_put_request() {
        let req = ApiRequest::put("/api/users/1");
        assert_eq!(req.method, HttpMethod::Put);
    }

    #[test]
    fn test_patch_request() {
        let req = ApiRequest::patch("/api/users/1");
        assert_eq!(req.method, HttpMethod::Patch);
    }

    #[test]
    fn test_delete_request() {
        let req = ApiRequest::delete("/api/users/1");
        assert_eq!(req.method, HttpMethod::Delete);
    }

    #[test]
    fn test_with_header() {
        let req = ApiRequest::get("/test")
            .with_header("Authorization", "Bearer token123")
            .with_header("Accept", "application/json");
        assert_eq!(req.headers.get("Authorization").unwrap(), "Bearer token123");
        assert_eq!(req.headers.get("Accept").unwrap(), "application/json");
    }

    #[test]
    fn debug_redacts_authorization_and_body() {
        let req = ApiRequest::post("/login")
            .with_header("Authorization", "Bearer token123")
            .with_json_body(&serde_json::json!({ "password": "secret" }))
            .unwrap();
        let debug = format!("{req:?}");

        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("bytes"));
        assert!(!debug.contains("token123"));
        assert!(!debug.contains("password"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn test_with_json_body() {
        #[derive(Serialize)]
        struct Payload {
            name: String,
        }
        let payload = Payload {
            name: "test".to_string(),
        };
        let req = ApiRequest::post("/test").with_json_body(&payload).unwrap();
        assert!(req.body.is_some());
        assert_eq!(req.headers.get("Content-Type").unwrap(), "application/json");
        let body_str = String::from_utf8(req.body.unwrap()).unwrap();
        assert!(body_str.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_with_timeout() {
        let req = ApiRequest::get("/test").with_timeout(5000);
        assert_eq!(req.timeout_ms, Some(5000));
    }

    #[test]
    fn test_response_is_success() {
        for status in [200, 201, 204, 299] {
            let resp = ApiResponse {
                status,
                headers: HashMap::new(),
                body: vec![],
            };
            assert!(resp.is_success(), "status {status} should be success");
        }
        for status in [100, 301, 400, 404, 500] {
            let resp = ApiResponse {
                status,
                headers: HashMap::new(),
                body: vec![],
            };
            assert!(!resp.is_success(), "status {status} should not be success");
        }
    }

    #[test]
    fn test_response_json() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct User {
            id: u32,
            name: String,
        }
        let resp = ApiResponse {
            status: 200,
            headers: HashMap::new(),
            body: br#"{"id":1,"name":"Alice"}"#.to_vec(),
        };
        let user: User = resp.json().unwrap();
        assert_eq!(
            user,
            User {
                id: 1,
                name: "Alice".to_string()
            }
        );
    }

    #[test]
    fn test_response_json_invalid() {
        let resp = ApiResponse {
            status: 200,
            headers: HashMap::new(),
            body: b"not json".to_vec(),
        };
        let result: Result<serde_json::Value> = resp.json();
        assert!(result.is_err());
    }

    #[test]
    fn test_response_text() {
        let resp = ApiResponse {
            status: 200,
            headers: HashMap::new(),
            body: b"hello world".to_vec(),
        };
        assert_eq!(resp.body_bytes(), b"hello world");
        assert_eq!(resp.text_ref().unwrap(), "hello world");
        assert_eq!(resp.text().unwrap(), "hello world");
    }

    #[test]
    fn test_response_text_invalid_utf8() {
        let resp = ApiResponse {
            status: 200,
            headers: HashMap::new(),
            body: vec![0xff, 0xfe],
        };
        assert!(resp.text().is_err());
    }

    #[test]
    fn request_has_default_body_limit() {
        let req = ApiRequest::get("/api/data");
        assert_eq!(req.max_response_bytes, Some(10 * 1024 * 1024));
    }

    #[test]
    fn request_body_limit_is_configurable() {
        let req = ApiRequest::get("/api/data").with_max_response_bytes(1024);
        assert_eq!(req.max_response_bytes, Some(1024));
    }

    #[test]
    fn request_can_disable_body_limit() {
        let req = ApiRequest::get("/api/data").without_response_limit();
        assert_eq!(req.max_response_bytes, None);
    }

    #[test]
    fn test_builder_chaining() {
        let req = ApiRequest::post("/api/data")
            .with_header("X-Custom", "value")
            .with_timeout(3000);
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.path, "/api/data");
        assert_eq!(req.headers.get("X-Custom").unwrap(), "value");
        assert_eq!(req.timeout_ms, Some(3000));
    }
}
