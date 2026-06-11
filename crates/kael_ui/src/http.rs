const DEFAULT_USER_AGENT: &str = "kael_ui/0.3.0";

#[cfg(feature = "http")]
fn install_client(cx: &mut kael::App, user_agent: &str) {
    use kael::http_client::ReqwestClient;
    use std::sync::Arc;

    if let Ok(client) = ReqwestClient::user_agent(user_agent) {
        cx.set_http_client(Arc::new(client));
    }
}

pub fn init_http(cx: &mut kael::App) {
    init_http_with_user_agent(cx, DEFAULT_USER_AGENT);
}

pub fn init_http_with_user_agent(cx: &mut kael::App, user_agent: &str) {
    #[cfg(feature = "http")]
    {
        install_client(cx, user_agent);
    }
    #[cfg(not(feature = "http"))]
    {
        let _ = (cx, user_agent);
    }
}
