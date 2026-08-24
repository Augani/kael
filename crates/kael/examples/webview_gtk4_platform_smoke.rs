// Production GTK4 PlatformWindow WebView smoke. The shared smoke body is also
// compiled against the legacy `webview` feature; this distinct target lets
// Cargo enforce the maintained Wayland/X11 feature contract without
// duplicating it.
include!("webview_smoke.rs");
