#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    eprintln!("WEBVIEW_WAYLAND_GTK4_SKIP: Linux/FreeBSD only");
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux {
    use gtk4::{
        Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Overlay, Picture,
        Separator, glib, prelude::*,
    };
    use std::{cell::RefCell, rc::Rc, time::Duration};
    use webkit6::{LoadEvent, UserContentManager, WebView, prelude::*};

    const SCENE_READY: u8 = 1 << 0;
    const SAME_SURFACE: u8 = 1 << 1;
    const PAGE_LOADED: u8 = 1 << 2;
    const PAGE_TO_HOST: u8 = 1 << 3;
    const JAVASCRIPT_RESULT: u8 = 1 << 4;
    const VISIBLE_ALLOCATION: u8 = 1 << 5;
    const RETAINED_ART_BOTTOM: i32 = 470;
    const RETAINED_DETAIL_TOP: i32 = 510;
    const _: () = assert!(RETAINED_ART_BOTTOM < RETAINED_DETAIL_TOP);
    const ALL_READY: u8 = SCENE_READY
        | SAME_SURFACE
        | PAGE_LOADED
        | PAGE_TO_HOST
        | JAVASCRIPT_RESULT
        | VISIBLE_ALLOCATION;

    #[derive(Default)]
    struct ProbeState {
        stages: u8,
        failed: Option<String>,
        finished: bool,
    }

    fn stage(state: &Rc<RefCell<ProbeState>>, application: &Application, bit: u8, name: &str) {
        let mut state = state.borrow_mut();
        if state.finished || state.failed.is_some() {
            return;
        }
        if state.stages & bit == 0 {
            println!("WEBVIEW_WAYLAND_GTK4_STAGE: {name}");
            state.stages |= bit;
        }
        if state.stages == ALL_READY {
            state.finished = true;
            println!(
                "WEBVIEW_WAYLAND_GTK4_OK: same-surface Kael GSK scene + WebKitGTK 6; IPC + JavaScript"
            );
            let evidence_hold = std::env::var("KAEL_WEBVIEW_WAYLAND_EVIDENCE_HOLD_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_default();
            if evidence_hold == 0 {
                application.quit();
            } else {
                println!("WEBVIEW_WAYLAND_GTK4_EVIDENCE_HOLD_MS: {evidence_hold}");
                let application = application.clone();
                glib::timeout_add_local_once(Duration::from_millis(evidence_hold), move || {
                    application.quit();
                });
            }
        }
    }

    fn fail(
        state: &Rc<RefCell<ProbeState>>,
        application: &Application,
        message: impl Into<String>,
    ) {
        let mut state = state.borrow_mut();
        if state.finished || state.failed.is_some() {
            return;
        }
        let message = message.into();
        eprintln!("WEBVIEW_WAYLAND_GTK4_FAIL: {message}");
        state.failed = Some(message);
        application.quit();
    }

    fn build_window(application: &Application, state: Rc<RefCell<ProbeState>>) {
        let display = gtk4::gdk::Display::default().expect("GTK did not provide a GDK display");
        let display_type = display.type_().name();
        println!("WEBVIEW_WAYLAND_GTK4_BACKEND: {display_type}");
        if !display_type.contains("Wayland") {
            fail(
                &state,
                application,
                format!("expected a native Wayland GDK display, got {display_type}"),
            );
            return;
        }

        let window = ApplicationWindow::builder()
            .application(application)
            .title("Kael GTK4/WebKitGTK 6 Wayland composition proof")
            .default_width(1000)
            .default_height(640)
            .build();

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.set_widget_name("kael-composition-root");
        let content = GtkBox::new(Orientation::Horizontal, 0);
        content.set_hexpand(true);
        content.set_vexpand(true);

        let retained_panel = Overlay::new();
        retained_panel.set_size_request(340, -1);
        let paintable = match kael::gtk4_wayland_scene_proof_paintable() {
            Ok(paintable) => paintable,
            Err(error) => {
                fail(
                    &state,
                    application,
                    format!("could not build the retained Kael GSK scene: {error:#}"),
                );
                return;
            }
        };
        let retained_surface = Picture::for_paintable(&paintable);
        retained_surface.set_widget_name("kael-retained-surface");
        retained_surface.set_content_fit(gtk4::ContentFit::Fill);
        retained_surface.set_can_shrink(true);
        retained_surface.set_hexpand(true);
        retained_surface.set_vexpand(true);
        retained_panel.set_child(Some(&retained_surface));

        let manager = UserContentManager::new();
        if !manager.register_script_message_handler("kael", None) {
            fail(
                &state,
                application,
                "could not register the WebKit script-message bridge",
            );
            return;
        }

        let webview = WebView::builder()
            .user_content_manager(&manager)
            .hexpand(true)
            .vexpand(true)
            .build();
        webview.set_size_request(620, 510);

        let badge = Label::new(Some("KAEL RETAINED GPU SURFACE  •  NATIVE WAYLAND"));
        badge.set_widget_name("kael-composition-badge");
        badge.set_halign(gtk4::Align::Start);
        badge.set_valign(gtk4::Align::Start);
        badge.set_margin_start(22);
        badge.set_margin_top(24);
        badge.set_wrap(true);
        badge.set_max_width_chars(27);
        retained_panel.add_overlay(&badge);

        let detail = Label::new(Some(
            "Kael Scene → cached GPU-backed GSK nodes.\nWebKitGTK 6 stays a native sibling in the same render graph.",
        ));
        detail.set_widget_name("kael-composition-detail");
        detail.set_halign(gtk4::Align::Start);
        detail.set_valign(gtk4::Align::Start);
        detail.set_margin_start(24);
        detail.set_margin_end(24);
        detail.set_margin_top(RETAINED_DETAIL_TOP);
        detail.set_wrap(true);
        detail.set_max_width_chars(30);
        retained_panel.add_overlay(&detail);

        let separator = Separator::new(Orientation::Vertical);
        content.append(&retained_panel);
        content.append(&separator);
        content.append(&webview);
        root.append(&content);

        let css = gtk4::CssProvider::new();
        css.load_from_string(
            r#"
#kael-retained-surface { background: #0b1020; }
#kael-composition-root { background: #0b1020; }
#kael-composition-badge {
  color: #9ee7ff;
  background: rgba(10, 21, 43, 0.94);
  border: 1px solid rgba(92, 203, 255, 0.45);
  border-radius: 9px;
  font-family: system-ui, sans-serif;
  font-weight: 700;
  font-size: 13px;
  letter-spacing: 0.08em;
  padding: 10px 14px;
}
#kael-composition-detail {
  color: #98abc2;
  font-family: system-ui, sans-serif;
  font-size: 14px;
  line-height: 1.45;
}
"#,
        );
        gtk4::style_context_add_provider_for_display(
            &display,
            &css,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let scene_state = state.clone();
        let scene_application = application.clone();
        retained_surface.connect_realize(move |_| {
            stage(
                &scene_state,
                &scene_application,
                SCENE_READY,
                "kael-gsk-scene-realized",
            );
        });

        let message_state = state.clone();
        let message_application = application.clone();
        manager.connect_script_message_received(Some("kael"), move |_, value| {
            if value.to_str().as_str() == "page-ready" {
                stage(
                    &message_state,
                    &message_application,
                    PAGE_TO_HOST,
                    "page-to-host-ipc",
                );
            } else {
                fail(
                    &message_state,
                    &message_application,
                    format!("unexpected page message: {}", value.to_str()),
                );
            }
        });

        let load_state = state.clone();
        let load_application = application.clone();
        webview.connect_load_changed(move |webview, event| {
            if event != LoadEvent::Finished {
                return;
            }
            stage(
                &load_state,
                &load_application,
                PAGE_LOADED,
                "page-load-finished",
            );
            let result_state = load_state.clone();
            let result_application = load_application.clone();
            webview.evaluate_javascript(
                "document.title + ':' + String(6 * 7)",
                None,
                None,
                None::<&gtk4::gio::Cancellable>,
                move |result| match result {
                    Ok(value) if value.to_str().as_str() == "Kael Wayland proof:42" => stage(
                        &result_state,
                        &result_application,
                        JAVASCRIPT_RESULT,
                        "javascript-result",
                    ),
                    Ok(value) => fail(
                        &result_state,
                        &result_application,
                        format!("unexpected JavaScript result: {}", value.to_str()),
                    ),
                    Err(error) => fail(
                        &result_state,
                        &result_application,
                        format!("JavaScript evaluation failed: {error}"),
                    ),
                },
            );
        });

        let surface_state = state.clone();
        let surface_application = application.clone();
        let retained_for_map = retained_surface.clone();
        webview.connect_map(move |webview| {
            let retained_native = retained_for_map.native();
            let webview_native = webview.native();
            let retained_gdk_surface = retained_native.as_ref().and_then(NativeExt::surface);
            let webview_gdk_surface = webview_native.as_ref().and_then(NativeExt::surface);
            match (retained_gdk_surface, webview_gdk_surface) {
                (Some(retained), Some(browser)) if retained == browser => stage(
                    &surface_state,
                    &surface_application,
                    SAME_SURFACE,
                    "same-gdk-surface",
                ),
                (Some(_), Some(_)) => fail(
                    &surface_state,
                    &surface_application,
                    "Kael's GSK scene and WebKit were mapped to different GDK surfaces",
                ),
                _ => fail(
                    &surface_state,
                    &surface_application,
                    "GTK did not expose the composed native GDK surface",
                ),
            }
        });

        let allocation_state = state.clone();
        let allocation_application = application.clone();
        webview.add_tick_callback(move |webview, _| {
            if webview.width() < 400 || webview.height() < 300 || !webview.is_visible() {
                return glib::ControlFlow::Continue;
            }
            println!(
                "WEBVIEW_WAYLAND_GTK4_ALLOCATION: {}x{}",
                webview.width(),
                webview.height()
            );
            stage(
                &allocation_state,
                &allocation_application,
                VISIBLE_ALLOCATION,
                "visible-webview-allocation",
            );
            glib::ControlFlow::Break
        });

        webview.load_html(
            r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Kael Wayland proof</title>
<style>
:root { color-scheme: dark; font-family: system-ui, sans-serif; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; color: #e9f5ff;
  background: radial-gradient(circle at 10% 0%, #264f82 0, #132644 42%, #091221 100%); }
main { height: 100vh; display: grid; place-content: center; gap: 18px; padding: 42px; }
.card { width: min(460px, 80vw); padding: 30px; border: 1px solid #4a7cab;
  border-radius: 18px; background: rgba(8, 19, 36, .82); box-shadow: 0 24px 70px #0008; }
h1 { margin: 0 0 10px; font-size: 30px; letter-spacing: -.03em; }
p { margin: 0; color: #b8d4e9; line-height: 1.55; }
.ok { display: inline-block; margin-top: 20px; padding: 8px 12px; border-radius: 999px;
  color: #8dffd0; background: #0d4a3a; font-weight: 700; }
</style></head><body><main><section class="card"><h1>Native Wayland composition</h1>
<p>WebKitGTK 6 is clipped, focused and scaled inside the same GTK4 surface as Kael's GPU host.</p>
<span class="ok">IPC connected • JS running</span></section></main>
<script>window.webkit.messageHandlers.kael.postMessage('page-ready')</script></body></html>"#,
            Some("kael://wayland-smoke/"),
        );

        window.set_child(Some(&root));
        window.present();
    }

    pub fn run() -> anyhow::Result<()> {
        let application = Application::builder()
            .application_id("dev.kael.WebViewWaylandGtk4Smoke")
            .build();
        let state = Rc::new(RefCell::new(ProbeState::default()));
        let activate_state = state.clone();
        application.connect_activate(move |application| {
            build_window(application, activate_state.clone());
        });

        let timeout_state = state.clone();
        let timeout_application = application.clone();
        glib::timeout_add_local_once(Duration::from_secs(20), move || {
            if !timeout_state.borrow().finished {
                fail(
                    &timeout_state,
                    &timeout_application,
                    "timed out waiting for GL/WebKit/IPC composition proof",
                );
            }
        });

        let _ = application.run();
        let state = state.borrow();
        anyhow::ensure!(
            state.finished && state.failed.is_none(),
            "GTK4/WebKitGTK 6 Wayland smoke failed: {}",
            state.failed.as_deref().unwrap_or("incomplete proof")
        );
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() -> anyhow::Result<()> {
    linux::run()
}
