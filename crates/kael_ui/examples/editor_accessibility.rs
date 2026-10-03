//! Real editor fixture for native text protocol clients and manual interaction.
//! Run `cargo run -p kael_ui --example editor_accessibility`.
use kael_ui::components::editor::{Redo, Undo};
use kael_ui::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

mod desktop_common;
#[cfg(target_os = "macos")]
mod editor_accessibility_macos;

const ORIGINAL: &str = include_str!("fixtures/native_unicode_document.txt");
const REPLACEMENT: &str = include_str!("fixtures/native_unicode_replacement.txt");
const TITLE: &str = "Kael native text accessibility";

type Stamp = (u64, usize, (usize, usize), usize, usize);

struct TextFixture {
    editor: Entity<EditorState>,
    read_only: Entity<EditorState>,
    last_stamp: Option<Stamp>,
    original: bool,
    replacement: bool,
    disabled: bool,
    complete: Arc<AtomicBool>,
    _observer: Subscription,
}

impl TextFixture {
    fn new(complete: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(EditorState::new);
        let read_only = cx.new(EditorState::new);
        for state in [&editor, &read_only] {
            state.update(cx, |state, cx| {
                state.set_language(EditorLanguage::Plain);
                state.set_font_size(14.0, cx);
                state.set_font_family("Menlo", cx);
                state.set_content(ORIGINAL, cx);
            });
        }
        read_only.update(cx, |state, cx| state.set_read_only(true, cx));
        let observer = cx.observe(&editor, |view, _, cx| {
            view.report_state(cx);
            cx.notify();
        });
        let mut fixture = Self {
            editor,
            read_only,
            last_stamp: None,
            original: true,
            replacement: false,
            disabled: false,
            complete,
            _observer: observer,
        };
        fixture.report_state(cx);
        fixture
    }

    fn report_state(&mut self, cx: &App) {
        let state = self.editor.read(cx);
        let stamp = (
            state.content_version(),
            state.content_len_bytes(),
            state.selection_bytes(),
            state.undo_depth(),
            state.redo_depth(),
        );
        if self.last_stamp == Some(stamp) {
            return;
        }
        // Flatten only on a content revision, never on caret/geometry repaint.
        if self.last_stamp.is_none_or(|last| last.0 != stamp.0) {
            let content = state.content();
            self.original = content == ORIGINAL;
            self.replacement = content == REPLACEMENT;
        }
        self.last_stamp = Some(stamp);
        println!(
            "NATIVE_TEXT_STATE {}",
            serde_json::json!({
                "version":stamp.0, "bytes":stamp.1, "anchor":stamp.2.0,
                "focus":stamp.2.1, "undo_depth":stamp.3, "redo_depth":stamp.4,
                "original":self.original, "replacement":self.replacement,
                "readonly_bytes":self.read_only.read(cx).content_len_bytes(),
            })
        );
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_content(ORIGINAL, cx));
    }

    fn replace(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_content(REPLACEMENT, cx));
    }
}

impl Render for TextFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .gap_2()
            .p_3()
            .child(
                div().flex().flex_col().gap_2()
                    .child(div().text_lg().child("Native Unicode editor protocol fixture"))
                    .child(div().flex().items_center().gap_2()
                    .child(Button::new("reset-document", "Reset document")
                        .on_click(cx.listener(|view, _, _, cx| view.reset(cx))))
                    .child(Button::new("replace-document", "Replace document")
                        .on_click(cx.listener(|view, _, _, cx| view.replace(cx))))
                    .child(Button::new("undo-document", "Undo document")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.editor.update(cx, |state, cx| state.undo(&Undo, window, cx));
                        })))
                    .child(Button::new("redo-document", "Redo document")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.editor.update(cx, |state, cx| state.redo(&Redo, window, cx));
                        })))
                    .child(Button::new("toggle-disabled-document", "Toggle disabled document")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.disabled = !view.disabled;
                            println!("NATIVE_TEXT_DISABLED: disabled={}", view.disabled);
                            cx.notify();
                        })))
                    .child(Button::new("finish-native-text", "Native text checks complete")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.complete.store(true, Ordering::Release);
                            println!("NATIVE_TEXT_FIXTURE_COMPLETE");
                            cx.quit();
                        })))),
            )
            .child(div().text_sm().child(
                "Explore, select and reveal Unicode text. The right editor allows reading and selection. Reset/Replace creates a new text revision.",
            ))
            .child(
                div().flex().flex_1().min_h(px(0.0)).gap_3()
                    .child(Editor::new(&self.editor)
                        .accessibility_label("Native Unicode document")
                        .disabled(self.disabled)
                        .flex_1().h_full().min_w(px(0.0)))
                    .child(Editor::new(&self.read_only)
                        .accessibility_label("Read-only Unicode document")
                        .w(px(350.0)).h_full()),
            )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    let self_test = std::env::var_os("KAEL_NATIVE_TEXT_SELF_TEST").is_some();
    let smoke = std::env::var_os("KAEL_NATIVE_TEXT_SMOKE").is_some()
        || cfg!(target_os = "macos") && std::env::var_os("KAEL_NATIVE_TEXT_SELF_TEST").is_some();
    Application::try_new()?.run(move |cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::astryx_neutral_dark());
        desktop_common::install(cx, TITLE);
        let complete = Arc::new(AtomicBool::new(false));
        if smoke {
            cx.on_app_quit({
                let complete = complete.clone();
                move |_| {
                    let complete = complete.clone();
                    async move {
                        if !complete.load(Ordering::Acquire) {
                            std::process::exit(1);
                        }
                    }
                }
            }).detach();
            cx.spawn(async move |cx| {
                kael::Timer::after(Duration::from_secs(120)).await;
                eprintln!("NATIVE_TEXT_TIMEOUT: protocol client did not finish");
                let _ = cx.update(|cx| cx.quit());
            }).detach();
        }
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        match cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        }, move |window, cx| {
            window.set_window_title(TITLE);
            cx.new(|cx| TextFixture::new(complete, cx))
        }) {
            Ok(window) => {
                println!("NATIVE_TEXT_FIXTURE_READY: bytes={} content_lines=1000 logical_lines=1001 utf16_units=66983", ORIGINAL.len());
                cx.activate(true);
                #[cfg(target_os = "macos")]
                if self_test {
                    editor_accessibility_macos::start(window, cx);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = window;
            }
            Err(error) => {
                eprintln!("NATIVE_TEXT_WINDOW_ERROR: {error}");
                cx.quit();
            }
        }
    });
    Ok(())
}
