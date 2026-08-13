use kael::{
    App, AppContext, Application, Bounds, Context, ParentElement, Render, SharedString, Styled,
    Window, WindowBounds, WindowOptions, div, px, size, webview_html,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

const WEBVIEW_ID: &str = "webview-smoke";

struct WebViewSmoke {
    finished: Arc<Mutex<Option<Result<SharedString, SharedString>>>>,
}

impl Render for WebViewSmoke {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl kael::IntoElement {
        let finished = self.finished.clone();
        div().size_full().child(
            webview_html(
                WEBVIEW_ID,
                r#"<!doctype html><meta charset="utf-8"><title>Kael WebView smoke</title><script>window.gpui.postMessage({ ready: true, value: 42 });</script>"#,
            )
            .on_message(move |message, window, _| {
                if message.get("ready").and_then(|ready| ready.as_bool()) == Some(true) {
                    let slot = finished.clone();
                    if let Err(error) = window.evaluate_webview_javascript_with_result(
                        WEBVIEW_ID,
                        "({ value: document.title + ':' + String(6 * 7) })",
                        move |result| *slot.lock().expect("smoke result mutex") = Some(result),
                    ) {
                        *finished.lock().expect("smoke result mutex") =
                            Some(Err(error.to_string().into()));
                    }
                }
            })
            .native_permission_policy(|_| kael::WebViewPermissionDecision::Deny)
            .size_full(),
        )
    }
}

fn main() -> anyhow::Result<()> {
    let finished = Arc::new(Mutex::new(None));
    let outcome = Arc::new(AtomicU8::new(0));
    let outcome_for_app = outcome.clone();
    Application::try_new()?.run(move |cx: &mut App| {
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(480.0), px(320.0)),
                    cx,
                ))),
                titlebar: None,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| WebViewSmoke {
                    finished: finished.clone(),
                })
            },
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("WEBVIEW_SMOKE_FAIL: could not open window: {error:#}");
                outcome_for_app.store(2, Ordering::Release);
                cx.quit();
                return;
            }
        };

        let outcome = outcome_for_app.clone();
        cx.spawn(async move |cx| {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                if let Ok(Some(result)) = window.update(cx, |view, _, _| {
                    view.finished.lock().expect("smoke result mutex").take()
                }) {
                    match result {
                        Ok(value) if value.as_ref().contains("Kael WebView smoke:42") => {
                            println!("WEBVIEW_SMOKE_OK: {value}");
                            outcome.store(1, Ordering::Release);
                        }
                        Ok(value) => {
                            eprintln!("WEBVIEW_SMOKE_FAIL: unexpected JavaScript result: {value}");
                            outcome.store(2, Ordering::Release);
                        }
                        Err(error) => {
                            eprintln!("WEBVIEW_SMOKE_FAIL: JavaScript evaluation failed: {error}");
                            outcome.store(2, Ordering::Release);
                        }
                    }
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!(
                        "WEBVIEW_SMOKE_FAIL: timed out waiting for IPC/JavaScript round trip"
                    );
                    outcome.store(2, Ordering::Release);
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(25))
                    .await;
            }
        })
        .detach();
    });
    anyhow::ensure!(
        outcome.load(Ordering::Acquire) == 1,
        "WebView smoke test failed"
    );
    Ok(())
}
