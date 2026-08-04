mod async_body;

pub use anyhow::{Result, anyhow};
pub use async_body::{AsyncBody, Inner};
use derive_more::Deref;
use http::HeaderValue;
pub use http::{self, Method, Request, Response, StatusCode, Uri};

use futures::{StreamExt as _, future::BoxFuture};
use http::request::Builder;
#[cfg(feature = "test-support")]
use parking_lot::Mutex;
use parking_lot::RwLock;
#[cfg(feature = "test-support")]
use std::fmt;
use std::{
    any::type_name,
    sync::{Arc, OnceLock},
};
pub use url::Url;

/// Maximum body size buffered by the reqwest adapter for a request or response.
pub const MAX_BUFFERED_HTTP_BODY_BYTES: usize = 256 * 1024 * 1024;

#[derive(Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RedirectPolicy {
    #[default]
    NoFollow,
    FollowLimit(u32),
    FollowAll,
}

pub trait HttpRequestExt {
    /// Conditionally modify self with the given closure.
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self
    where
        Self: Sized,
    {
        if condition { then(self) } else { self }
    }

    /// Conditionally unwrap and modify self with the given closure, if the given option is Some.
    fn when_some<T>(self, option: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self
    where
        Self: Sized,
    {
        match option {
            Some(value) => then(self, value),
            None => self,
        }
    }

    /// Whether or not to follow redirects
    fn follow_redirects(self, follow: RedirectPolicy) -> Self;
}

impl HttpRequestExt for http::request::Builder {
    fn follow_redirects(self, follow: RedirectPolicy) -> Self {
        self.extension(follow)
    }
}

pub trait HttpClient: 'static + Send + Sync {
    fn type_name(&self) -> &'static str;

    fn user_agent(&self) -> Option<&HeaderValue>;

    fn send(
        &self,
        req: http::Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>>;

    fn get(
        &self,
        uri: &str,
        body: AsyncBody,
        follow_redirects: bool,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let request = Builder::new()
            .uri(uri)
            .follow_redirects(if follow_redirects {
                RedirectPolicy::FollowAll
            } else {
                RedirectPolicy::NoFollow
            })
            .body(body);

        match request {
            Ok(request) => self.send(request),
            Err(e) => Box::pin(async move { Err(e.into()) }),
        }
    }

    fn post_json(
        &self,
        uri: &str,
        body: AsyncBody,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let request = Builder::new()
            .uri(uri)
            .method(Method::POST)
            .header("Content-Type", "application/json")
            .body(body);

        match request {
            Ok(request) => self.send(request),
            Err(e) => Box::pin(async move { Err(e.into()) }),
        }
    }

    fn proxy(&self) -> Option<&Url>;

    #[cfg(feature = "test-support")]
    fn as_fake(&self) -> &FakeHttpClient {
        panic!("called as_fake on {}", type_name::<Self>())
    }
}

/// An [`HttpClient`] that may have a proxy.
#[derive(Deref)]
pub struct HttpClientWithProxy {
    #[deref]
    client: Arc<dyn HttpClient>,
    proxy: Option<Url>,
}

impl HttpClientWithProxy {
    /// Returns a new [`HttpClientWithProxy`] with the given proxy URL.
    pub fn new(client: Arc<dyn HttpClient>, proxy_url: Option<String>) -> Self {
        let proxy_url = match proxy_url {
            Some(proxy) => proxy.parse().ok(),
            None => read_proxy_from_env(),
        };

        Self::new_url(client, proxy_url)
    }
    pub fn new_url(client: Arc<dyn HttpClient>, proxy_url: Option<Url>) -> Self {
        Self {
            client,
            proxy: proxy_url,
        }
    }
}

impl HttpClient for HttpClientWithProxy {
    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        self.client.send(req)
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        self.client.user_agent()
    }

    fn proxy(&self) -> Option<&Url> {
        self.proxy.as_ref()
    }

    fn type_name(&self) -> &'static str {
        self.client.type_name()
    }

    #[cfg(feature = "test-support")]
    fn as_fake(&self) -> &FakeHttpClient {
        self.client.as_fake()
    }
}

/// An [`HttpClient`] that has a base URL.
pub struct HttpClientWithUrl {
    base_url: RwLock<Arc<str>>,
    client: HttpClientWithProxy,
}

impl std::ops::Deref for HttpClientWithUrl {
    type Target = HttpClientWithProxy;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl HttpClientWithUrl {
    /// Returns a new [`HttpClientWithUrl`] with the given base URL.
    pub fn new(
        client: Arc<dyn HttpClient>,
        base_url: impl Into<String>,
        proxy_url: Option<String>,
    ) -> Self {
        let client = HttpClientWithProxy::new(client, proxy_url);

        Self {
            base_url: RwLock::new(Arc::<str>::from(base_url.into())),
            client,
        }
    }

    pub fn new_url(
        client: Arc<dyn HttpClient>,
        base_url: impl Into<String>,
        proxy_url: Option<Url>,
    ) -> Self {
        let client = HttpClientWithProxy::new_url(client, proxy_url);

        Self {
            base_url: RwLock::new(Arc::<str>::from(base_url.into())),
            client,
        }
    }

    /// Returns the base URL.
    pub fn base_url(&self) -> String {
        self.base_url.read().as_ref().to_string()
    }

    /// Sets the base URL.
    pub fn set_base_url(&self, base_url: impl Into<String>) {
        *self.base_url.write() = Arc::<str>::from(base_url.into());
    }

    /// Builds a URL using the given path.
    pub fn build_url(&self, path: &str) -> String {
        self.try_build_url(path)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| {
                let base_url = self.base_url.read();
                format!("{}{}", base_url.as_ref(), path)
            })
    }

    /// Builds and validates a URL from the configured base and `path`.
    pub fn try_build_url(&self, path: &str) -> Result<Url> {
        let base_url = self.base_url.read();
        let combined = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        Ok(Url::parse(&combined)?)
    }

    /// Builds a URL with query parameters using the base URL and the given path.
    pub fn build_url_with_params(&self, path: &str, query: &[(&str, &str)]) -> Result<Url> {
        let mut url = self.try_build_url(path)?;
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        Ok(url)
    }
}

impl HttpClient for HttpClientWithUrl {
    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        self.client.send(req)
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        self.client.user_agent()
    }

    fn proxy(&self) -> Option<&Url> {
        self.client.proxy.as_ref()
    }

    fn type_name(&self) -> &'static str {
        self.client.type_name()
    }

    #[cfg(feature = "test-support")]
    fn as_fake(&self) -> &FakeHttpClient {
        self.client.as_fake()
    }
}

pub fn read_proxy_from_env() -> Option<Url> {
    const ENV_VARS: &[&str] = &[
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];

    ENV_VARS
        .iter()
        .find_map(|var| std::env::var(var).ok())
        .and_then(|env| env.parse().ok())
}

pub fn read_no_proxy_from_env() -> Option<String> {
    const ENV_VARS: &[&str] = &["NO_PROXY", "no_proxy"];

    ENV_VARS.iter().find_map(|var| std::env::var(var).ok())
}

/// A concrete [`HttpClient`] backed by `reqwest`.
///
/// This adapter is useful for applications and examples that want a
/// batteries-included HTTP client without wiring a custom transport.
#[derive(Clone)]
pub struct ReqwestClient {
    user_agent: HeaderValue,
    proxy: Option<Url>,
}

impl ReqwestClient {
    /// Create a new reqwest-backed client with the given user agent.
    pub fn user_agent(user_agent: impl AsRef<str>) -> Result<Self> {
        Ok(Self {
            user_agent: HeaderValue::from_str(user_agent.as_ref())?,
            proxy: read_proxy_from_env(),
        })
    }

    fn redirect_policy(policy: &RedirectPolicy) -> reqwest::redirect::Policy {
        match policy {
            RedirectPolicy::NoFollow => reqwest::redirect::Policy::none(),
            RedirectPolicy::FollowLimit(limit) => {
                reqwest::redirect::Policy::limited(usize::try_from(*limit).unwrap_or(usize::MAX))
            }
            // Reqwest does not expose an unbounded follow policy, so we pick a
            // generous ceiling for "follow all" behavior.
            RedirectPolicy::FollowAll => reqwest::redirect::Policy::limited(32),
        }
    }

    fn build_client(&self, policy: &RedirectPolicy) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .user_agent(self.user_agent.to_str()?)
            .redirect(Self::redirect_policy(policy));

        if let Some(proxy) = &self.proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy.as_str())?);
        }

        Ok(builder.build()?)
    }

    async fn read_body(mut body: AsyncBody) -> Result<Vec<u8>> {
        Ok(body
            .read_to_end_limited(MAX_BUFFERED_HTTP_BODY_BYTES)
            .await?)
    }

    async fn into_response(response: reqwest::Response) -> Result<Response<AsyncBody>> {
        let status = response.status();
        let version = response.version();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BUFFERED_HTTP_BODY_BYTES as u64)
        {
            anyhow::bail!(
                "HTTP response exceeds {} byte limit",
                MAX_BUFFERED_HTTP_BODY_BYTES
            );
        }
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let next_len = body
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| anyhow!("HTTP response size overflow"))?;
            if next_len > MAX_BUFFERED_HTTP_BODY_BYTES {
                anyhow::bail!(
                    "HTTP response exceeds {} byte limit",
                    MAX_BUFFERED_HTTP_BODY_BYTES
                );
            }
            body.try_reserve(chunk.len())?;
            body.extend_from_slice(&chunk);
        }

        let mut response = Response::builder()
            .status(status)
            .version(version)
            .body(AsyncBody::from(body))?;
        *response.headers_mut() = headers;
        Ok(response)
    }

    fn runtime() -> Result<&'static tokio::runtime::Runtime> {
        static RUNTIME: OnceLock<std::result::Result<tokio::runtime::Runtime, String>> =
            OnceLock::new();

        RUNTIME
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("kael-http")
                    .build()
                    .map_err(|err| err.to_string())
            })
            .as_ref()
            .map_err(|err| anyhow!("failed to initialize HTTP runtime: {err}"))
    }

    async fn run_on_runtime<T>(
        future: impl std::future::Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T>
    where
        T: Send + 'static,
    {
        Self::runtime()?
            .spawn(future)
            .await
            .map_err(|err| anyhow!("HTTP runtime task failed: {err}"))?
    }
}

impl HttpClient for ReqwestClient {
    fn type_name(&self) -> &'static str {
        type_name::<Self>()
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let this = self.clone();

        Box::pin(async move {
            let (parts, body) = req.into_parts();
            let redirect_policy = parts
                .extensions
                .get::<RedirectPolicy>()
                .cloned()
                .unwrap_or_default();
            let url = parts.uri.to_string();
            let body = Self::read_body(body).await?;

            Self::run_on_runtime(async move {
                let client = this.build_client(&redirect_policy)?;
                let mut request = client.request(parts.method, url);
                for (name, value) in &parts.headers {
                    request = request.header(name, value);
                }
                if !body.is_empty() {
                    request = request.body(body);
                }

                let response = request.send().await?;
                Self::into_response(response).await
            })
            .await
        })
    }

    fn proxy(&self) -> Option<&Url> {
        self.proxy.as_ref()
    }
}

pub struct BlockedHttpClient;

impl BlockedHttpClient {
    pub fn new() -> Self {
        BlockedHttpClient
    }
}

impl HttpClient for BlockedHttpClient {
    fn send(
        &self,
        _req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        Box::pin(async {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "BlockedHttpClient disallowed request",
            )
            .into())
        })
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn type_name(&self) -> &'static str {
        type_name::<Self>()
    }

    #[cfg(feature = "test-support")]
    fn as_fake(&self) -> &FakeHttpClient {
        panic!("called as_fake on {}", type_name::<Self>())
    }
}

#[cfg(feature = "test-support")]
type FakeHttpHandler = Arc<
    dyn Fn(Request<AsyncBody>) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>>
        + Send
        + Sync
        + 'static,
>;

#[cfg(feature = "test-support")]
pub struct FakeHttpClient {
    handler: Mutex<Option<FakeHttpHandler>>,
    user_agent: HeaderValue,
}

#[cfg(feature = "test-support")]
impl FakeHttpClient {
    pub fn create<Fut, F>(handler: F) -> Arc<HttpClientWithUrl>
    where
        Fut: futures::Future<Output = anyhow::Result<Response<AsyncBody>>> + Send + 'static,
        F: Fn(Request<AsyncBody>) -> Fut + Send + Sync + 'static,
    {
        Arc::new(HttpClientWithUrl {
            base_url: RwLock::new(Arc::<str>::from("http://test.example")),
            client: HttpClientWithProxy {
                client: Arc::new(Self {
                    handler: Mutex::new(Some(Arc::new(move |req| Box::pin(handler(req))))),
                    user_agent: HeaderValue::from_static(type_name::<Self>()),
                }),
                proxy: None,
            },
        })
    }

    pub fn with_404_response() -> Arc<HttpClientWithUrl> {
        Self::create(|_| async move {
            Ok(Response::builder()
                .status(404)
                .body(Default::default())
                .unwrap())
        })
    }

    pub fn with_200_response() -> Arc<HttpClientWithUrl> {
        Self::create(|_| async move {
            Ok(Response::builder()
                .status(200)
                .body(Default::default())
                .unwrap())
        })
    }

    pub fn replace_handler<Fut, F>(&self, new_handler: F)
    where
        Fut: futures::Future<Output = anyhow::Result<Response<AsyncBody>>> + Send + 'static,
        F: Fn(FakeHttpHandler, Request<AsyncBody>) -> Fut + Send + Sync + 'static,
    {
        let mut handler = self.handler.lock();
        let old_handler = handler.take().unwrap();
        *handler = Some(Arc::new(move |req| {
            Box::pin(new_handler(old_handler.clone(), req))
        }));
    }
}

#[cfg(feature = "test-support")]
impl fmt::Debug for FakeHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeHttpClient").finish()
    }
}

#[cfg(feature = "test-support")]
impl HttpClient for FakeHttpClient {
    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        ((self.handler.lock().as_ref().unwrap())(req)) as _
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn type_name(&self) -> &'static str {
        type_name::<Self>()
    }

    fn as_fake(&self) -> &FakeHttpClient {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn explicit_invalid_proxy_does_not_fall_back_to_environment() {
        let client = HttpClientWithProxy::new(
            Arc::new(BlockedHttpClient::new()),
            Some("not a proxy URL".into()),
        );
        assert_eq!(client.proxy(), None);
    }

    #[test]
    fn base_url_joining_normalizes_slashes_and_encodes_query_values() {
        let client = HttpClientWithUrl::new(
            Arc::new(BlockedHttpClient::new()),
            "https://example.com/api/",
            Some("invalid".into()),
        );
        assert_eq!(client.build_url("/v1"), "https://example.com/api/v1");
        let url = client
            .build_url_with_params("search", &[("q", "a b&c")])
            .unwrap();
        assert_eq!(url.as_str(), "https://example.com/api/search?q=a+b%26c");
    }
}
