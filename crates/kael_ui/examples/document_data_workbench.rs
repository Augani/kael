//! Real document + async remote-record workflows on existing editor/grid controls.
//! Run `cargo run -p kael_ui --features markdown --example document_data_workbench`.
//! The remote fixture binds loopback, keeps sparse edits, and stops with the app.
use kael_ui::components::editor::{Redo, Undo};
use kael_ui::display::rich_text::render_blocks;
use kael_ui::prelude::*;
mod desktop_common;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const RECORDS: usize = 100_000;
const COLUMNS: usize = 8;
#[derive(Clone, Serialize, Deserialize)]
struct Query {
    descending: bool,
}
#[derive(Serialize, Deserialize)]
struct TileQuery {
    rows: std::ops::Range<usize>,
    columns: std::ops::Range<usize>,
    descending: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct RemoteEdit {
    record: usize,
    column: usize,
    value: String,
}
fn record_id(row: usize, query: &Query) -> usize {
    if query.descending {
        RECORDS - 1 - row
    } else {
        row
    }
}
struct Fixture {
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = stopping.clone();
        let worker = std::thread::spawn(move || {
            let mut edits = HashMap::<(usize, usize), String>::new();
            while let Ok((mut socket, _)) = listener.accept() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = socket.set_write_timeout(Some(Duration::from_secs(2)));
                let result = (|| -> Result<Vec<u8>, String> {
                    let request = read_http(&mut socket, || false)?;
                    if request.path == "/tile" {
                        let query: TileQuery =
                            serde_json::from_slice(&request.body).map_err(|e| e.to_string())?;
                        if query.rows.end > RECORDS
                            || query.columns.end > COLUMNS
                            || query.rows.start >= query.rows.end
                            || query.columns.start >= query.columns.end
                            || query
                                .rows
                                .len()
                                .checked_mul(query.columns.len())
                                .is_none_or(|cells| cells > VIRTUAL_SHEET_MAX_TILE_CELLS)
                        {
                            return Err("invalid tile".into());
                        }
                        std::thread::sleep(Duration::from_millis(40));
                        let mut values = Vec::with_capacity(query.rows.len() * query.columns.len());
                        for row in query.rows {
                            let id = if query.descending {
                                RECORDS - 1 - row
                            } else {
                                row
                            };
                            for column in query.columns.clone() {
                                values.push(edits.get(&(id, column)).cloned().unwrap_or_else(
                                    || match column {
                                        0 => format!("Record {id:06}"),
                                        1 => format!("Team {}", id % 50),
                                        2 => (id % 100).to_string(),
                                        3 => ["Ready", "Review", "Draft"][id % 3].into(),
                                        4 => format!("Sprint {}", id % 12 + 1),
                                        5 => ["Accra", "Tokyo", "Paris"][id % 3].into(),
                                        6 => format!("{}", id % 5000),
                                        _ => "Edit this note".into(),
                                    },
                                ));
                            }
                        }
                        serde_json::to_vec(&values).map_err(|e| e.to_string())
                    } else if request.path == "/edit" {
                        let edit: RemoteEdit =
                            serde_json::from_slice(&request.body).map_err(|e| e.to_string())?;
                        if edit.record >= RECORDS
                            || edit.column >= COLUMNS
                            || edit.value.len() > VIRTUAL_SHEET_MAX_CELL_BYTES
                            || (edits.len() >= 100_000
                                && !edits.contains_key(&(edit.record, edit.column)))
                        {
                            return Err("invalid or excessive edit".into());
                        }
                        edits.insert((edit.record, edit.column), edit.value);
                        Ok(b"true".to_vec())
                    } else {
                        Err("unknown route".into())
                    }
                })();
                let (status, body) = match result {
                    Ok(body) => ("200 OK", body),
                    Err(message) => ("400 Bad Request", message.into_bytes()),
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(&body);
            }
        });
        Ok(Self {
            address,
            stopping,
            worker: Some(worker),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct HttpMessage {
    path: String,
    body: Vec<u8>,
}
fn read_http(stream: &mut TcpStream, cancelled: impl Fn() -> bool) -> Result<HttpMessage, String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if cancelled() {
            return Err("request cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("HTTP fixture timed out".into());
        }
        match stream.read(&mut buffer) {
            Ok(0) => return Err("incomplete HTTP response".into()),
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        }
        if bytes.len() > VIRTUAL_SHEET_MAX_TILE_BYTES + 8192 {
            return Err("HTTP response exceeds byte limit".into());
        }
        let header_end = bytes.windows(4).position(|window| window == b"\r\n\r\n");
        if header_end.is_none() && bytes.len() > 8192 {
            return Err("oversized HTTP header".into());
        }
        if let Some(header_end) = header_end {
            if header_end > 8192 {
                return Err("oversized HTTP header".into());
            }
            let header = std::str::from_utf8(&bytes[..header_end]).map_err(|e| e.to_string())?;
            let first = header.lines().next().unwrap_or_default();
            let length = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or("missing HTTP length")?;
            if length > VIRTUAL_SHEET_MAX_TILE_BYTES {
                return Err("oversized HTTP body".into());
            }
            if bytes.len() >= header_end + 4 + length {
                if first.starts_with("HTTP/") && !first.contains(" 200 ") {
                    return Err(String::from_utf8_lossy(&bytes[header_end + 4..]).into_owned());
                }
                let path = first
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                return Ok(HttpMessage {
                    path,
                    body: bytes[header_end + 4..header_end + 4 + length].to_vec(),
                });
            }
        }
    }
}
fn post(
    address: SocketAddr,
    path: &str,
    body: &[u8],
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    if cancelled() {
        return Err("request cancelled".into());
    }
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(1)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(25)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(|e| e.to_string())?;
    write!(stream, "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    stream.write_all(body).map_err(|e| e.to_string())?;
    read_http(&mut stream, cancelled).map(|response| response.body)
}

struct Document {
    editor: Entity<EditorState>,
    split: Entity<SplitPaneState>,
    blocks: Arc<[BlockNode]>,
    version: u64,
    parse_task: Option<Task<()>>,
    _observer: Subscription,
}
impl Document {
    fn new(cx: &mut Context<Self>) -> Self {
        let editor = cx.new(EditorState::new);
        let source = include_str!("fixtures/project_launch.md");
        editor.update(cx, |editor, cx| {
            editor.set_language(EditorLanguage::Markdown);
            editor.set_content(source, cx);
        });
        let version = editor.read(cx).content_version();
        let observer = cx.observe(&editor, |view, _, cx| {
            let version = view.editor.read(cx).content_version();
            if version != view.version {
                view.version = version;
                let source = view.editor.read(cx).content();
                let background = cx.background_executor().clone();
                view.parse_task = Some(cx.spawn(async move |view, cx| {
                    let blocks: Arc<[BlockNode]> = background
                        .spawn(async move { parse_markdown(&source).into() })
                        .await;
                    let _ = view.update(cx, |view, cx| {
                        if view.version == version {
                            view.blocks = blocks;
                            cx.notify();
                        }
                    });
                }));
            }
            cx.notify();
        });
        Self {
            editor,
            split: cx.new(SplitPaneState::new),
            blocks: parse_markdown(source).into(),
            version,
            parse_task: None,
            _observer: observer,
        }
    }
}
impl Render for Document {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut toolbar = div().flex().gap_2().items_center().child(
            div()
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .child("Launch plan"),
        );
        for (id, label, before, after) in [
            ("bold", "Bold", "**", "**"),
            ("italic", "Italic", "*", "*"),
            ("code", "Code", "`", "`"),
        ] {
            let edit = self.editor.downgrade();
            toolbar = toolbar.child(
                Button::new(id, label)
                    .size(ButtonSize::Sm)
                    .variant(ButtonVariant::Ghost)
                    .on_click(move |_, window, cx| {
                        let _ = edit.update(cx, |editor, cx| {
                            let selected = editor.selection_text().unwrap_or_else(|| "text".into());
                            editor.replace_selection(&format!("{before}{selected}{after}"), cx);
                            window.focus(&editor.focus_handle(cx));
                        });
                    }),
            );
        }
        let undo = self.editor.downgrade();
        let redo = undo.clone();
        toolbar = toolbar
            .child(
                Button::new("doc-undo", "Undo")
                    .size(ButtonSize::Sm)
                    .disabled(self.editor.read(cx).undo_depth() == 0)
                    .on_click(move |_, window, cx| {
                        let _ = undo.update(cx, |editor, cx| editor.undo(&Undo, window, cx));
                    }),
            )
            .child(
                Button::new("doc-redo", "Redo")
                    .size(ButtonSize::Sm)
                    .disabled(self.editor.read(cx).redo_depth() == 0)
                    .on_click(move |_, window, cx| {
                        let _ = redo.update(cx, |editor, cx| editor.redo(&Redo, window, cx));
                    }),
            );
        let preview = div()
            .id("document-preview")
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .children(render_blocks(
                &self.blocks,
                px(14.0),
                &None,
                "launch-preview",
            ));
        div().size_full().flex().flex_col().gap_3().p_3().bg(theme.tokens.background).child(toolbar).child(div().text_xs().text_color(theme.tokens.muted_foreground).child("Select source text to format it. Preview parsing runs on workers. Try Japanese composition, emoji, selection and Undo."))
            .child(SplitPane::vertical(self.split.clone()).first(Editor::new(&self.editor).accessibility_label("Launch plan source").size_full()).second(preview).flex_1().min_h(px(0.0)))
    }
}

struct Workbench {
    document: Entity<Document>,
    remote: Entity<RemoteSheetState<Query>>,
    split: Entity<SplitPaneState>,
    address: SocketAddr,
    descending: bool,
    queue: VecDeque<RemoteEdit>,
    write_task: Option<Task<()>>,
    write_error: Option<String>,
    status: String,
    _events: Subscription,
    _observer: Subscription,
}
impl Workbench {
    fn new(address: SocketAddr, cx: &mut Context<Self>) -> Self {
        let remote = cx.new(|cx| {
            RemoteSheetState::new(
                RECORDS,
                COLUMNS,
                Query { descending: false },
                move |request| {
                    Box::pin(async move {
                        let body = serde_json::to_vec(&TileQuery {
                            rows: request.tile.rows.clone(),
                            columns: request.tile.columns.clone(),
                            descending: request.query.descending,
                        })
                        .map_err(|e| e.to_string())?;
                        let body = post(address, "/tile", &body, || request.is_cancelled())?;
                        let values: Vec<String> =
                            serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                        Ok(values.into_iter().map(SharedString::from).collect())
                    })
                },
                cx,
            )
            .unwrap()
        });
        remote.update(cx, |state, cx| {
            state.grid().update(cx, |grid, _| {
                grid.set_frozen_panes(1, 2).unwrap();
                for (column, label) in [
                    "Title", "Owner", "Score", "Status", "Sprint", "Region", "Budget", "Notes",
                ]
                .into_iter()
                .enumerate()
                {
                    grid.set_column_header(column, label).unwrap();
                }
            });
        });
        let events = cx.subscribe(&remote, |view, _, event: &RemoteSheetEvent<Query>, cx| {
            match event {
                RemoteSheetEvent::Edited { edit, query, .. } => {
                    if view.queue.len() >= 256 {
                        view.write_error =
                            Some("Write queue is full; further edits remain local".into());
                    } else {
                        view.queue.push_back(RemoteEdit {
                            record: record_id(edit.position.row, query),
                            column: edit.position.column,
                            value: edit.value.to_string(),
                        });
                        view.pump_write(cx);
                    }
                }
                RemoteSheetEvent::Loaded { .. } => {
                    if view.queue.is_empty() {
                        view.status = "Remote tiles loaded".into();
                    }
                }
                RemoteSheetEvent::Failed { message, .. } => {
                    view.status = format!("Remote load failed: {message}")
                }
            }
            cx.notify();
        });
        let observer = cx.observe(&remote, |_, _, cx| cx.notify());
        Self { document: cx.new(Document::new), remote, split: cx.new(SplitPaneState::new), address, descending: false, queue: VecDeque::new(), write_task: None, write_error: None, status: "Loopback HTTP fixture · Enter or double-click to edit · Copy/Paste and Undo/Redo supported".into(), _events: events, _observer: observer }
    }
    fn pump_write(&mut self, cx: &mut Context<Self>) {
        if self.write_task.is_some() || self.write_error.is_some() {
            return;
        }
        let Some(edit) = self.queue.front().cloned() else {
            return;
        };
        let address = self.address;
        let background = cx.background_executor().clone();
        self.status = "Saving edit to remote fixture…".into();
        self.write_task = Some(cx.spawn(async move |view, cx| {
            let result = background
                .spawn(async move {
                    let body = serde_json::to_vec(&edit).map_err(|e| e.to_string())?;
                    post(address, "/edit", &body, || false).map(|_| ())
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.write_task = None;
                match result {
                    Ok(()) => {
                        view.queue.pop_front();
                        view.status = "Edit saved remotely".into();
                        view.pump_write(cx);
                    }
                    Err(message) => view.write_error = Some(message),
                }
                cx.notify();
            });
        }));
    }
}
impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh = self.remote.downgrade();
        let sort = cx.entity().downgrade();
        let retry = sort.clone();
        let writes_pending = !self.queue.is_empty();
        let theme = Theme::of(cx);
        let error = self.write_error.clone().or_else(|| {
            self.remote
                .read(cx)
                .grid()
                .read(cx)
                .source_error()
                .map(str::to_string)
        });
        let records = div().size_full().flex().flex_col().gap_3().p_3()
            .child(div().flex().items_center().gap_2().child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child("Remote records · 100,000"))
                .child(Button::new("record-refresh", "Refresh").size(ButtonSize::Sm).disabled(writes_pending).on_click(move |_, _, cx| { let _ = refresh.update(cx, |state, cx| state.refresh(cx)); }))
                .child(Button::new("record-sort", if self.descending { "Sort ascending" } else { "Sort descending" }).size(ButtonSize::Sm).disabled(writes_pending).on_click(move |_, _, cx| { let _ = sort.update(cx, |view, cx| { view.descending = !view.descending; view.remote.update(cx, |state, cx| state.set_query(Query { descending: view.descending }, cx)); cx.notify(); }); })))
            .child(div().text_xs().text_color(theme.tokens.muted_foreground).child("The first row and two columns stay frozen. Edits, paste and undo are saved using the record identity from the active query."))
            .child(RemoteSheet::new("workbench-records", self.remote.clone()).flex_1().min_h(px(0.0)))
            .child(div().text_xs().text_color(theme.tokens.muted_foreground).child(format!("{} · {} loads · {} writes", self.status, self.remote.read(cx).pending_count(), self.queue.len())))
            .when_some(error, |element, message| element.child(div().flex().gap_2().items_center().text_color(theme.tokens.destructive).child(message).when(self.write_error.is_some(), |element| element.child(Button::new("retry-write", "Retry save").size(ButtonSize::Sm).on_click(move |_, _, cx| { let _ = retry.update(cx, |view, cx| { view.write_error = None; view.pump_write(cx); }); })))));
        div()
            .size_full()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                SplitPane::horizontal(self.split.clone())
                    .first(self.document.clone())
                    .second(records)
                    .size_full(),
            )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::start()?;
    let address = fixture.address;
    Application::try_new()?.run(move |cx| {
        kael_ui::init(cx);
        desktop_common::install(cx, "Kael document and data workbench");
        install_theme(cx, Theme::astryx_neutral());
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1480.0), px(900.0)),
                cx,
            ))),
            ..Default::default()
        };
        if let Err(error) = cx.open_window(options, move |window, cx| {
            window.set_window_title("Kael document and data workbench");
            cx.new(|cx| Workbench::new(address, cx))
        }) {
            eprintln!("failed to open workbench: {error}");
            cx.quit();
        }
    });
    drop(fixture);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn tile(fixture: &Fixture, row: usize, descending: bool) -> Vec<String> {
        let body = serde_json::to_vec(&TileQuery {
            rows: row..row + 1,
            columns: 0..COLUMNS,
            descending,
        })
        .unwrap();
        serde_json::from_slice(&post(fixture.address, "/tile", &body, || false).unwrap()).unwrap()
    }

    #[test]
    fn real_http_fixture_preserves_edits_by_record_identity_across_query_changes() {
        let fixture = Fixture::start().unwrap();
        assert_eq!(tile(&fixture, 17, false)[0], "Record 000017");
        let body = serde_json::to_vec(&RemoteEdit {
            record: 17,
            column: 7,
            value: "日本語 café 👩‍💻\tquoted note".into(),
        })
        .unwrap();
        post(fixture.address, "/edit", &body, || false).unwrap();
        assert_eq!(tile(&fixture, 17, false)[7], "日本語 café 👩‍💻\tquoted note");
        assert_eq!(
            tile(&fixture, RECORDS - 1 - 17, true)[7],
            "日本語 café 👩‍💻\tquoted note"
        );
        assert_eq!(tile(&fixture, 18, false)[7], "Edit this note");

        let invalid = serde_json::to_vec(&TileQuery {
            rows: RECORDS..RECORDS + 1,
            columns: 0..1,
            descending: false,
        })
        .unwrap();
        assert!(post(fixture.address, "/tile", &invalid, || false).is_err());
        assert_eq!(tile(&fixture, 17, false)[7], "日本語 café 👩‍💻\tquoted note");
    }

    #[test]
    fn http_request_cancels_while_waiting_and_fixture_stays_usable() {
        let fixture = Fixture::start().unwrap();
        let body = serde_json::to_vec(&TileQuery {
            rows: 0..64,
            columns: 0..COLUMNS,
            descending: false,
        })
        .unwrap();
        let polls = AtomicUsize::new(0);
        let result = post(fixture.address, "/tile", &body, || {
            polls.fetch_add(1, Ordering::Relaxed) >= 2
        });
        assert_eq!(result.unwrap_err(), "request cancelled");
        assert_eq!(tile(&fixture, 0, false)[0], "Record 000000");
    }
}
