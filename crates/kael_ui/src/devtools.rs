//! Developer tools UI: a ready-made inspector renderer for Kael windows.
//!
//! Kael ships the picking machinery (hitboxes, [`kael::Inspector`], element-state
//! reflection) but leaves the inspector *panel* to the application. Without a
//! renderer, [`kael::Window::toggle_inspector`] opens a blank 30rem strip.
//!
//! [`install_inspector`] fills that gap with one call: it registers a styled
//! renderer that shows the picked element's id, bounds, and style summary, a
//! breadcrumb of the element path, and a live frame-stats strip fed by
//! [`kael::Window::frame_timeline`].
//!
//! ```rust,ignore
//! fn main() {
//!     Application::new().run(|cx| {
//!         kael_ui::init(cx);
//!         #[cfg(debug_assertions)]
//!         kael_ui::devtools::install_inspector(cx);
//!         // open windows, bind `cx.bind_keys` for toggle_inspector, ...
//!     });
//! }
//! ```
//!
//! The whole module is gated on `any(feature = "inspector", debug_assertions)`
//! so release builds pay nothing for it.

#[cfg(debug_assertions)]
mod inspector_panel {
    use crate::theme::Theme;
    use kael::prelude::FluentBuilder as _;
    use kael::{
        App, Context, DivInspectorState, Hsla, InspectorElementId, InteractiveElement as _,
        IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
        px,
    };

    const PANEL_PADDING: f32 = 16.0;
    const SECTION_GAP: f32 = 14.0;
    const ROW_GAP: f32 = 6.0;
    const MONO_SIZE: f32 = 12.0;
    const LABEL_SIZE: f32 = 11.0;
    const HEADING_SIZE: f32 = 13.0;

    /// Installs the default inspector renderer and element-state reflectors.
    ///
    /// After this call, toggling the inspector on any window renders a populated
    /// panel instead of an empty strip. Safe to call once during app startup,
    /// typically behind `#[cfg(debug_assertions)]`.
    pub fn install_inspector(cx: &mut App) {
        cx.register_inspector_element::<DivInspectorState, _>(render_div_state);
        cx.set_inspector_renderer(Box::new(render_inspector));
    }

    fn render_inspector(
        inspector: &mut kael::Inspector,
        window: &mut Window,
        cx: &mut Context<kael::Inspector>,
    ) -> kael::AnyElement {
        let tokens = Theme::try_get(cx)
            .map(|theme| theme.tokens.clone())
            .unwrap_or_else(|| Theme::dark().tokens);

        let picking = inspector.is_picking();
        let breadcrumb = inspector
            .active_element_id()
            .map(breadcrumb_segments)
            .unwrap_or_default();
        let frame_strip = frame_stats_rows(window, &tokens);
        let state_elements = inspector.render_inspector_states(window, cx);

        div()
            .flex()
            .flex_col()
            .h_full()
            .w_full()
            .bg(tokens.popover)
            .border_l_1()
            .border_color(tokens.border)
            .text_color(tokens.popover_foreground)
            .font_family(tokens.font_mono.clone())
            .child(panel_header(&tokens, picking))
            .child(
                div()
                    .id("kael-inspector-body")
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .gap(px(SECTION_GAP))
                    .p(px(PANEL_PADDING))
                    .overflow_y_scroll()
                    .child(breadcrumb_section(&tokens, &breadcrumb))
                    .children(state_elements)
                    .child(frame_strip),
            )
            .into_any_element()
    }

    fn render_div_state(
        id: InspectorElementId,
        state: &DivInspectorState,
        _window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement + use<> {
        let tokens = Theme::try_get(cx)
            .map(|theme| theme.tokens.clone())
            .unwrap_or_else(|| Theme::dark().tokens);

        let bounds = state.bounds;
        let content = state.content_size;
        let instance = id.instance_id;

        section(
            &tokens,
            "Element",
            div()
                .flex()
                .flex_col()
                .gap(px(ROW_GAP))
                .child(kv_row(&tokens, "instance", format!("#{instance}")))
                .child(kv_row(
                    &tokens,
                    "origin",
                    format!(
                        "{:.1}, {:.1}",
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y)
                    ),
                ))
                .child(kv_row(
                    &tokens,
                    "size",
                    format!(
                        "{:.1} x {:.1}",
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height)
                    ),
                ))
                .child(kv_row(
                    &tokens,
                    "content",
                    format!(
                        "{:.1} x {:.1}",
                        f32::from(content.width),
                        f32::from(content.height)
                    ),
                ))
                .child(style_summary(&tokens, state)),
        )
    }

    fn style_summary(
        tokens: &crate::theme::ThemeTokens,
        state: &DivInspectorState,
    ) -> impl IntoElement + use<> {
        let style = &state.base_style;
        let mut entries: Vec<(String, String)> = Vec::new();

        if let Some(display) = &style.display {
            entries.push(("display".into(), format!("{display:?}")));
        }
        if style.size.width.is_some() || style.size.height.is_some() {
            entries.push((
                "style.size".into(),
                format!("{:?} x {:?}", style.size.width, style.size.height),
            ));
        }
        if let Some(background) = &style.background {
            entries.push(("background".into(), format!("{background:?}")));
        }
        if let Some(flex_direction) = &style.flex_direction {
            entries.push(("flex".into(), format!("{flex_direction:?}")));
        }

        div()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .when(!entries.is_empty(), |this| {
                this.child(subheading(tokens, "style"))
            })
            .children(
                entries
                    .into_iter()
                    .map(|(key, value)| kv_row(tokens, &key, value)),
            )
    }

    fn breadcrumb_segments(id: &InspectorElementId) -> Vec<String> {
        let mut segments: Vec<String> = id
            .path
            .global_id
            .iter()
            .map(|seg| seg.to_string())
            .collect();
        if segments.is_empty() {
            segments.push("<root>".to_string());
        }
        segments.push(format!("@ {}", id.path.source_location));
        segments
    }

    fn breadcrumb_section(
        tokens: &crate::theme::ThemeTokens,
        segments: &[String],
    ) -> impl IntoElement + use<> {
        let body: kael::AnyElement = if segments.is_empty() {
            div()
                .text_size(px(MONO_SIZE))
                .text_color(tokens.muted_foreground)
                .child("Pick an element to inspect.")
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(ROW_GAP))
                .children(segments.iter().enumerate().map(|(depth, segment)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .pl(px(depth as f32 * 10.0))
                        .child(
                            div()
                                .text_size(px(LABEL_SIZE))
                                .text_color(tokens.muted_foreground)
                                .child(if depth == 0 { "•" } else { "↳" }),
                        )
                        .child(
                            div()
                                .text_size(px(MONO_SIZE))
                                .text_color(tokens.foreground)
                                .child(segment.clone()),
                        )
                }))
                .into_any_element()
        };

        section(tokens, "Path", body)
    }

    fn frame_stats_rows(window: &Window, tokens: &crate::theme::ThemeTokens) -> kael::AnyElement {
        let timeline = window.frame_timeline();
        let avg = timeline.average_duration_us();
        let p95 = timeline.p95_duration_us();
        let p99 = timeline.p99_duration_us();
        let frames = timeline.len();

        let avg_ms = avg.map(|us| us / 1000.0);
        let fps = avg.filter(|us| *us > 0.0).map(|us| 1_000_000.0 / us);

        let body = div()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .child(kv_row(tokens, "frames", frames.to_string()))
            .child(kv_row(
                tokens,
                "avg",
                avg_ms.map_or_else(|| "—".to_string(), |ms| format!("{ms:.2} ms")),
            ))
            .child(kv_row(
                tokens,
                "fps",
                fps.map_or_else(|| "—".to_string(), |value| format!("{value:.0}")),
            ))
            .child(kv_row(
                tokens,
                "p95",
                p95.map_or_else(
                    || "—".to_string(),
                    |us| format!("{:.2} ms", us as f64 / 1000.0),
                ),
            ))
            .child(kv_row(
                tokens,
                "p99",
                p99.map_or_else(
                    || "—".to_string(),
                    |us| format!("{:.2} ms", us as f64 / 1000.0),
                ),
            ));

        section(tokens, "Frames", body)
    }

    fn panel_header(tokens: &crate::theme::ThemeTokens, picking: bool) -> impl IntoElement + use<> {
        let (status_label, status_color) = if picking {
            ("PICKING", tokens.primary)
        } else {
            ("SELECTED", tokens.muted_foreground)
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(PANEL_PADDING))
            .py(px(12.0))
            .border_b_1()
            .border_color(tokens.border)
            .bg(tokens.card)
            .child(
                div()
                    .text_size(px(HEADING_SIZE))
                    .font_weight(kael::FontWeight::SEMIBOLD)
                    .text_color(tokens.foreground)
                    .child("Inspector"),
            )
            .child(
                div()
                    .text_size(px(LABEL_SIZE))
                    .font_weight(kael::FontWeight::MEDIUM)
                    .text_color(status_color)
                    .child(status_label),
            )
    }

    fn section(
        tokens: &crate::theme::ThemeTokens,
        title: &str,
        body: impl IntoElement,
    ) -> kael::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(heading(tokens, title))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .p(px(10.0))
                    .rounded(tokens.radius_md)
                    .bg(tokens.muted)
                    .border_1()
                    .border_color(tokens.border)
                    .child(body),
            )
            .into_any_element()
    }

    fn heading(tokens: &crate::theme::ThemeTokens, text: &str) -> impl IntoElement + use<> {
        div()
            .text_size(px(LABEL_SIZE))
            .font_weight(kael::FontWeight::BOLD)
            .text_color(tokens.muted_foreground)
            .child(text.to_uppercase())
    }

    fn subheading(tokens: &crate::theme::ThemeTokens, text: &str) -> impl IntoElement + use<> {
        div()
            .pt(px(4.0))
            .text_size(px(LABEL_SIZE))
            .text_color(tokens.muted_foreground)
            .child(text.to_uppercase())
    }

    fn kv_row<T: Into<String>>(
        tokens: &crate::theme::ThemeTokens,
        key: &str,
        value: T,
    ) -> impl IntoElement + use<T> {
        let value: String = value.into();
        row(
            tokens.muted_foreground,
            tokens.foreground,
            key.to_string(),
            value,
        )
    }

    fn row(key_color: Hsla, value_color: Hsla, key: String, value: String) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(12.0))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(LABEL_SIZE))
                    .text_color(key_color)
                    .child(key),
            )
            .child(
                div()
                    .flex_grow()
                    .text_size(px(MONO_SIZE))
                    .text_color(value_color)
                    .child(value),
            )
    }
}

#[cfg(debug_assertions)]
pub use inspector_panel::install_inspector;
