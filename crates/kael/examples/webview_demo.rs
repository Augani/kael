use kael::{
    App, Application, Bounds, Context, NavigationPolicy, SharedString, WebViewOptions, Window,
    WindowBounds, WindowOptions, black, div, prelude::*, px, rgb, size, text_input,
    webview_controller, webview_with_options, white,
};

const WEBVIEW_ID: &str = "demo-webview";

struct WebViewDemo {
    address_bar: SharedString,
    status: SharedString,
    last_message: SharedString,
}

fn button(text: &str, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(SharedString::from(text.to_string()))
        .implicit_transitions()
        .flex_none()
        .px_2()
        .bg(rgb(0xf7f7f7))
        .hover(|this| this.bg(rgb(0xefefef)).border_color(rgb(0xd8d8d8)))
        .active(|this| this.opacity(0.85).scale(0.985))
        .border_1()
        .border_color(rgb(0xe0e0e0))
        .rounded_sm()
        .cursor_pointer()
        .child(text.to_string())
        .on_click(move |_, window, cx| on_click(window, cx))
}

impl WebViewDemo {
    fn demo_url() -> SharedString {
        let html = r#"<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>GPUI WebView Demo</title>
    <style>
      :root { color-scheme: light dark; }
      body {
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        margin: 0;
        padding: 24px;
        line-height: 1.5;
        background: linear-gradient(160deg, #f4efe2 0%, #d8ebff 100%);
        color: #122033;
      }
      .card {
        max-width: 720px;
        border-radius: 18px;
        background: rgba(255, 255, 255, 0.88);
        padding: 24px;
        box-shadow: 0 24px 60px rgba(18, 32, 51, 0.18);
      }
      button {
        border: 0;
        border-radius: 999px;
        padding: 10px 16px;
        background: #0f766e;
        color: white;
        font: inherit;
        cursor: pointer;
      }
      code {
        background: rgba(15, 118, 110, 0.08);
        padding: 2px 6px;
        border-radius: 6px;
      }
    </style>
  </head>
  <body>
    <div class="card">
      <h1>GPUI WebView</h1>
      <p>This page is loaded from a <code>data:</code> URL so the example works without external network access.</p>
      <button id="ping">Post Message To GPUI</button>
      <p id="result">Waiting for JS → Rust bridge activity.</p>
    </div>
    <script>
      const result = document.getElementById('result');
      const sendPayload = (kind, detail) => {
        const payload = { kind, detail, href: window.location.href, title: document.title };
        if (window.gpui && typeof window.gpui.postMessage === 'function') {
          window.gpui.postMessage(payload);
        }
      };
      document.getElementById('ping').addEventListener('click', () => {
        result.textContent = 'Sent click payload to GPUI.';
        sendPayload('button-click', 'The in-page button was pressed');
      });
      window.addEventListener('message', event => {
        result.textContent = 'GPUI posted: ' + JSON.stringify(event.data);
      });
      sendPayload('page-ready', 'DOMContentLoaded');
    </script>
  </body>
</html>"#;

        format!("data:text/html;charset=utf-8,{}", percent_encode(html)).into()
    }
}

impl Render for WebViewDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let address_bar = self.address_bar.clone();
        let status = self.status.clone();
        let last_message = self.last_message.clone();
        let webview_controller = webview_controller(WEBVIEW_ID);

        div()
            .size_full()
            .bg(rgb(0xf4f1ea))
            .text_color(rgb(0x152238))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(rgb(0xd4c4ab))
                    .bg(rgb(0xfffbf2))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div().flex_1().child(
                                    text_input("address-bar", address_bar.clone())
                                        .placeholder("Enter a URL or data URI")
                                        .on_change({
                                            let entity = entity.clone();
                                            move |text, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.address_bar = text;
                                                    cx.notify();
                                                });
                                            }
                                        })
                                        .on_submit({
                                            let entity = entity.clone();
                                            let webview_controller = webview_controller.clone();
                                            move |text, window, cx| {
                                                let _ = webview_controller.navigate(window, text.clone());
                                                entity.update(cx, |this, cx| {
                                                    this.address_bar = text;
                                                    this.status = "Navigated from the address bar".into();
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                ),
                            )
                            .child(button("Load Demo", {
                                let entity = entity.clone();
                                let webview_controller = webview_controller.clone();
                                move |window, cx| {
                                    let url = WebViewDemo::demo_url();
                                    let _ = webview_controller.navigate(window, url.clone());
                                    entity.update(cx, |this, cx| {
                                        this.address_bar = url;
                                        this.status = "Loaded the local demo page".into();
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(button("Back", {
                                let webview_controller = webview_controller.clone();
                                move |window, _| {
                                    let _ = webview_controller.go_back(window);
                                }
                            }))
                            .child(button("Forward", {
                                let webview_controller = webview_controller.clone();
                                move |window, _| {
                                    let _ = webview_controller.go_forward(window);
                                }
                            }))
                            .child(button("Reload", {
                                let webview_controller = webview_controller.clone();
                                move |window, _| {
                                    let _ = webview_controller.reload(window);
                                }
                            }))
                            .child(button("Post Message", {
                                let webview_controller = webview_controller.clone();
                                move |window, _| {
                                    let _ = webview_controller.post_message(
                                        window,
                                        serde_json::json!({
                                            "kind": "host-message",
                                            "detail": "Hello from GPUI"
                                        }),
                                    );
                                }
                            }))
                            .child(button("Eval JS", {
                                let webview_controller = webview_controller.clone();
                                move |window, _| {
                                    let _ = webview_controller.evaluate_javascript(
                                        window,
                                        "document.body.style.filter = document.body.style.filter ? '' : 'hue-rotate(24deg) saturate(1.15)';",
                                    );
                                }
                            })),
                    )
                    .child(div().text_sm().child(format!("Status: {}", status)))
                    .child(div().text_sm().child(format!("Last message: {}", last_message))),
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .child(
                        webview_with_options(
                            WEBVIEW_ID,
                            address_bar,
                            WebViewOptions::auth_flow("gpui.webview.demo")
                                .user_agent("GPUI-WebView-Demo/1.0")
                                .on_message({
                                    let entity = entity.clone();
                                    move |message, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.last_message = message.to_string().into();
                                            this.status = "Received a JS bridge message".into();
                                            cx.notify();
                                        });
                                    }
                                })
                                .on_navigate({
                                    move |url, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.status =
                                                format!("Navigation requested: {}", url).into();
                                            cx.notify();
                                        });
                                        if url.starts_with("http://")
                                            || url.starts_with("https://")
                                            || url.starts_with("data:")
                                        {
                                            NavigationPolicy::Allow
                                        } else {
                                            NavigationPolicy::Deny
                                        }
                                    }
                                }),
                        )
                            .rounded_lg()
                            .border_1()
                            .border_color(black())
                            .bg(white())
                            .w_full()
                            .h_full(),
                    ),
            )
    }
}

fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| WebViewDemo {
                    address_bar: WebViewDemo::demo_url(),
                    status: "Ready".into(),
                    last_message: "No messages yet".into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
