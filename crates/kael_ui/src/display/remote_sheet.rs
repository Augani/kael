//! Async source integration for the existing bounded virtual spreadsheet.
//! Sources receive generation-scoped tiles and cooperative cancellation. Queries
//! replace row identity; refresh preserves the current dataset's local overlay.
use super::virtual_sheet_grid::*;
use crate::components::model_observer::observe_model;
use kael::{prelude::*, *};
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Immutable query and rectangular tile requested from application storage.
pub struct RemoteSheetRequest<Q> {
    pub query: Arc<Q>,
    pub tile: SheetTileRequest,
    cancelled: Arc<AtomicBool>,
}
impl<Q> RemoteSheetRequest<Q> {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
impl<Q> Clone for RemoteSheetRequest<Q> {
    fn clone(&self) -> Self {
        Self {
            query: self.query.clone(),
            tile: self.tile.clone(),
            cancelled: self.cancelled.clone(),
        }
    }
}

/// Fetch bounds shared by the source controller and virtual grid.
#[derive(Clone, Copy, Debug)]
pub struct RemoteSheetOptions {
    pub cached_tiles: usize,
    pub pending_tiles: usize,
    pub tile_rows: usize,
    pub tile_columns: usize,
}
impl Default for RemoteSheetOptions {
    fn default() -> Self {
        Self {
            cached_tiles: 16,
            pending_tiles: 8,
            tile_rows: 64,
            tile_columns: 16,
        }
    }
}
/// Edits are application-owned writes. The captured query/generation identifies
/// the dataset at edit time, even if a later query replaces displayed rows.
pub enum RemoteSheetEvent<Q> {
    Loaded {
        key: SheetTileKey,
        generation: u64,
    },
    Failed {
        key: SheetTileKey,
        generation: u64,
        message: SharedString,
    },
    Edited {
        edit: SheetCellEdit,
        query: Arc<Q>,
        generation: u64,
    },
}
struct RemoteJob {
    _task: Task<()>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for RemoteJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
type Loader<Q> = Arc<
    dyn Fn(
            RemoteSheetRequest<Q>,
        ) -> futures::future::BoxFuture<'static, Result<Vec<SharedString>, String>>
        + Send
        + Sync,
>;

/// Reusable async controller over `VirtualSheetGrid`; it adds no eager rows or
/// second rendering implementation. A failed source pauses requests until an
/// explicit refresh, preventing a retry loop. Dropping/replacing the controller
/// cancels pending work. Sources should observe the cancellation token.
pub struct RemoteSheetState<Q: Send + Sync + 'static> {
    grid: Entity<VirtualSheetGrid>,
    query: Arc<Q>,
    generation: u64,
    loader: Loader<Q>,
    jobs: HashMap<SheetTileKey, RemoteJob>,
    pending_limit: usize,
}
impl<Q: Send + Sync + 'static> EventEmitter<RemoteSheetEvent<Q>> for RemoteSheetState<Q> {}
impl<Q: Send + Sync + 'static> RemoteSheetState<Q> {
    pub fn new(
        rows: usize,
        columns: usize,
        query: Q,
        loader: impl Fn(
            RemoteSheetRequest<Q>,
        )
            -> futures::future::BoxFuture<'static, Result<Vec<SharedString>, String>>
        + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Self, VirtualSheetGridError> {
        Self::with_options(
            rows,
            columns,
            query,
            RemoteSheetOptions::default(),
            loader,
            cx,
        )
    }
    pub fn with_options(
        rows: usize,
        columns: usize,
        query: Q,
        options: RemoteSheetOptions,
        loader: impl Fn(
            RemoteSheetRequest<Q>,
        )
            -> futures::future::BoxFuture<'static, Result<Vec<SharedString>, String>>
        + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Self, VirtualSheetGridError> {
        if rows == 0
            || columns == 0
            || rows > VIRTUAL_SHEET_MAX_ROWS
            || columns > VIRTUAL_SHEET_MAX_COLUMNS
        {
            return Err(VirtualSheetGridError::InvalidDimensions);
        }
        if options.tile_rows == 0
            || options.tile_columns == 0
            || options
                .tile_rows
                .checked_mul(options.tile_columns)
                .is_none_or(|count| count > VIRTUAL_SHEET_MAX_TILE_CELLS)
        {
            return Err(VirtualSheetGridError::InvalidTileShape);
        }
        let fetch = cx.entity().downgrade();
        let commit = fetch.clone();
        let pending_limit = options.pending_tiles.clamp(1, 64);
        let grid = cx.new(|cx| {
            VirtualSheetGrid::new(rows, columns, cx)
                .expect("validated remote dimensions")
                .with_tile_shape(options.tile_rows, options.tile_columns)
                .expect("validated remote tile shape")
                .with_cache_limits(options.cached_tiles.clamp(1, 128), pending_limit)
                .with_require_loaded_edits(true)
                .on_fetch_tile(move |tile, grid, _, cx| {
                    let _ = fetch.update(cx, |state, cx| state.fetch(tile, grid, cx));
                })
                .on_commit_edit(move |edit, _, cx| {
                    let _ = commit.update(cx, |state, cx| {
                        cx.emit(RemoteSheetEvent::Edited {
                            edit,
                            query: state.query.clone(),
                            generation: state.generation,
                        })
                    });
                })
        });
        let generation = grid.read(cx).generation();
        Ok(Self {
            grid,
            query: Arc::new(query),
            generation,
            loader: Arc::new(loader),
            jobs: HashMap::new(),
            pending_limit,
        })
    }
    pub fn grid(&self) -> &Entity<VirtualSheetGrid> {
        &self.grid
    }
    pub fn query(&self) -> &Arc<Q> {
        &self.query
    }
    pub fn pending_count(&self) -> usize {
        self.jobs.len()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Retry/reload the same records, retaining local edits and undo history.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.jobs.clear();
        self.generation = self.grid.update(cx, |grid, cx| {
            grid.reload();
            cx.notify();
            grid.generation()
        });
        cx.notify();
    }
    /// Replace row identity. Application-owned writes should be committed or
    /// explicitly discarded before changing the query; positional local history
    /// is retired so it cannot accidentally apply to different records.
    pub fn set_query(&mut self, query: Q, cx: &mut Context<Self>) {
        self.jobs.clear();
        self.query = Arc::new(query);
        self.generation = self.grid.update(cx, |grid, cx| {
            grid.reset_data(cx);
            grid.generation()
        });
        cx.notify();
    }
    fn fetch(
        &mut self,
        tile: SheetTileRequest,
        grid: Entity<VirtualSheetGrid>,
        cx: &mut Context<Self>,
    ) {
        if tile.generation != self.generation || self.jobs.contains_key(&tile.key) {
            return;
        }
        if self.jobs.len() >= self.pending_limit {
            cx.defer(move |cx| {
                grid.update(cx, |grid, cx| {
                    let _ = grid.fail_tile(&tile, "remote request limit reached");
                    cx.notify();
                });
            });
            return;
        }
        let key = tile.key;
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = RemoteSheetRequest {
            query: self.query.clone(),
            tile,
            cancelled: cancelled.clone(),
        };
        let loader = self.loader.clone();
        let background = cx.background_executor().clone();
        let task = cx.spawn(async move |state, cx| {
            let (tile, result) = background
                .spawn(async move {
                    let result = loader(request.clone()).await;
                    let result = if request.is_cancelled() {
                        Err("cancelled request".into())
                    } else {
                        validate_response(&request.tile, result)
                    };
                    (request.tile, result)
                })
                .await;
            let _ = state.update(cx, |state, cx| {
                if tile.generation != state.generation || !state.jobs.contains_key(&tile.key) {
                    return;
                }
                state.jobs.remove(&tile.key);
                let result = state.grid.update(cx, |grid, cx| {
                    let result = match result {
                        Ok(values) => grid
                            .provide_tile(tile.clone(), values)
                            .map_err(|error| error.to_string()),
                        Err(message) => Err(message),
                    };
                    if let Err(message) = &result {
                        let _ = grid.fail_tile(&tile, message);
                    }
                    cx.notify();
                    result
                });
                match result {
                    Ok(()) => cx.emit(RemoteSheetEvent::Loaded {
                        key: tile.key,
                        generation: tile.generation,
                    }),
                    Err(message) => cx.emit(RemoteSheetEvent::Failed {
                        key: tile.key,
                        generation: tile.generation,
                        message: message.chars().take(1024).collect::<String>().into(),
                    }),
                }
                cx.notify();
            });
        });
        self.jobs.insert(
            key,
            RemoteJob {
                _task: task,
                cancelled,
            },
        );
        cx.notify();
    }
}
fn validate_response(
    tile: &SheetTileRequest,
    result: Result<Vec<SharedString>, String>,
) -> Result<Vec<SharedString>, String> {
    let values = result?;
    if values.len() != tile.cell_count().unwrap_or(usize::MAX)
        || values.len() > VIRTUAL_SHEET_MAX_TILE_CELLS
    {
        return Err("remote tile has an invalid value count".into());
    }
    let mut bytes = 0usize;
    for value in &values {
        if value.len() > VIRTUAL_SHEET_MAX_CELL_BYTES {
            return Err("remote cell exceeds the byte limit".into());
        }
        bytes = bytes
            .checked_add(value.len())
            .ok_or("remote tile byte count overflows")?;
        if bytes > VIRTUAL_SHEET_MAX_TILE_BYTES {
            return Err("remote tile exceeds the byte limit".into());
        }
    }
    Ok(values)
}
/// Mount the existing virtual grid. Status/retry controls can use `source_error`
/// on `state.grid()`, and writes are handled through `RemoteSheetEvent::Edited`.
#[derive(IntoElement)]
pub struct RemoteSheet<Q: Send + Sync + 'static> {
    id: ElementId,
    state: Entity<RemoteSheetState<Q>>,
    style: StyleRefinement,
}
impl<Q: Send + Sync + 'static> RemoteSheet<Q> {
    pub fn new(id: impl Into<ElementId>, state: Entity<RemoteSheetState<Q>>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
        }
    }
}
impl<Q: Send + Sync + 'static> Styled for RemoteSheet<Q> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl<Q: Send + Sync + 'static> RenderOnce for RemoteSheet<Q> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        observe_model(self.id.clone(), &self.state, window, cx);
        let grid = self.state.read(cx).grid.clone();
        let mut element = div().id(self.id).size_full().child(grid);
        element.style().refine(&self.style);
        element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    struct Host {
        state: Entity<RemoteSheetState<usize>>,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            RemoteSheet::new("remote-test", self.state.clone())
                .w(px(420.0))
                .h(px(240.0))
        }
    }
    fn values(request: &RemoteSheetRequest<usize>) -> Vec<SharedString> {
        request
            .tile
            .rows
            .clone()
            .flat_map(|row| {
                request
                    .tile
                    .columns
                    .clone()
                    .map(move |column| format!("q{}-r{row}-c{column}", request.query).into())
            })
            .collect()
    }
    #[::core::prelude::v1::test]
    fn async_query_cancellation_frozen_edit_clipboard_and_history_use_real_grid() {
        type Reply = futures::channel::oneshot::Sender<Result<Vec<SharedString>, String>>;
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::dark());
        });
        let pending = Arc::new(Mutex::new(Vec::<(RemoteSheetRequest<usize>, Reply)>::new()));
        let state = cx.new({
            let pending = pending.clone();
            move |cx| {
                RemoteSheetState::with_options(
                    100_000,
                    12,
                    0,
                    RemoteSheetOptions {
                        cached_tiles: 2,
                        pending_tiles: 2,
                        tile_rows: 4,
                        tile_columns: 4,
                    },
                    move |request| {
                        let pending = pending.clone();
                        Box::pin(async move {
                            let (reply, value) = futures::channel::oneshot::channel();
                            pending.lock().unwrap().push((request, reply));
                            value.await.map_err(|_| "cancelled".to_string())?
                        })
                    },
                    cx,
                )
                .unwrap()
            }
        });
        state.update(&mut cx, |state, cx| {
            state
                .grid
                .update(cx, |grid, _| grid.set_frozen_panes(1, 1).unwrap());
        });
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        window.enable_styled_text_paint_trace();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert!(
            !window
                .painted_styled_text()
                .iter()
                .any(|(text, _, _)| text.starts_with("q0-r"))
        );
        window.run_until_parked();
        let original = std::mem::take(&mut *pending.lock().unwrap());
        assert_eq!(original.len(), 2);
        window.update(|_, cx| state.update(cx, |state, cx| state.set_query(1, cx)));
        window.run_until_parked();
        assert!(original.iter().all(|(request, _)| request.is_cancelled()));
        for (request, reply) in original {
            assert!(reply.send(Ok(values(&request))).is_err());
        }
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        let current = std::mem::take(&mut *pending.lock().unwrap());
        assert_eq!(current.len(), 2);
        for (request, reply) in current {
            assert_eq!(*request.query, 1);
            reply.send(Ok(values(&request))).unwrap();
        }
        window.run_until_parked();
        // No pointer, scroll, focus or editing interaction is needed to paint
        // the delayed baseline after the wrapper receives model notification.
        // Completion must dirty and repaint through the ordinary platform
        // callback; a forced draw could mask lost observer invalidation.
        window.simulate_platform_frame();
        assert!(
            window
                .painted_styled_text()
                .iter()
                .any(|(text, bounds, clip)| text.as_ref() == "q1-r0-c0"
                    && !bounds.intersect(clip).is_empty())
        );
        window.update(|window, cx| {
            let grid = state.read(cx).grid.clone();
            grid.update(cx, |grid, cx| {
                grid.select(SheetCellPosition::new(0, 0), false).unwrap();
                window.focus(&grid.focus_handle(cx));
            });
            window.draw(cx).clear();
        });
        window.simulate_keystrokes("enter");
        #[cfg(target_os = "macos")]
        window.simulate_keystrokes("cmd-a");
        #[cfg(not(target_os = "macos"))]
        window.simulate_keystrokes("ctrl-a");
        window.simulate_input("Edited by keyboard");
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            let grid = state.read(cx).grid.clone();
            grid.update(cx, |grid, cx| {
                assert_eq!(
                    grid.cell_value(SheetCellPosition::new(0, 0))
                        .as_ref()
                        .map(|value| value.as_ref()),
                    Some("Edited by keyboard")
                );
                assert!(grid.cached_tile_count() <= 2);
                grid.select(SheetCellPosition::new(0, 0), false).unwrap();
                grid.set_cell_value(SheetCellPosition::new(0, 0), "edited\tvalue", window, cx)
                    .unwrap();
                grid.copy_selection_to_clipboard(cx).unwrap();
                let copied = cx.read_from_clipboard().unwrap().unwrap().text().unwrap();
                assert_eq!(copied, "\"edited\tvalue\"");
                grid.select(SheetCellPosition::new(0, 1), false).unwrap();
                grid.paste_from_clipboard(window, cx).unwrap();
                assert_eq!(
                    grid.cell_value(SheetCellPosition::new(0, 1))
                        .as_ref()
                        .map(|value| value.as_ref()),
                    Some("edited\tvalue")
                );
                assert!(grid.undo(window, cx));
                assert_eq!(
                    grid.cell_value(SheetCellPosition::new(0, 1))
                        .as_ref()
                        .map(|value| value.as_ref()),
                    Some("q1-r0-c1")
                );
                assert!(grid.redo(window, cx));
            });
            window.draw(cx).clear();
            assert!(grid.read(cx).viewport_metrics().mounted_rows < 20);
            assert!(grid.read(cx).viewport_metrics().mounted_cells < 160);
            assert!(
                window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .any(|node| node.role == AccessibilityRole::Grid
                        && node.row_count == Some(100_001))
            );
            state.update(cx, |state, cx| state.set_query(2, cx));
            assert_eq!(
                grid.read(cx).sparse_edit_count(),
                0,
                "new row identity retires positional edits/history"
            );
            grid.update(cx, |grid, cx| {
                assert!(
                    !grid.undo(window, cx),
                    "an old query cannot write undo into a different record"
                );
                assert!(
                    !grid.redo(window, cx),
                    "an old query cannot write redo into a different record"
                );
            });
        });
    }
    #[::core::prelude::v1::test]
    fn invalid_source_response_pauses_fetch_without_a_retry_loop_until_refresh() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::dark());
        });
        let count = Arc::new(AtomicUsize::new(0));
        let healthy = Arc::new(AtomicBool::new(false));
        let state = cx.new({
            let count = count.clone();
            let healthy = healthy.clone();
            move |cx| {
                RemoteSheetState::new(
                    100_000,
                    12,
                    0,
                    move |request| {
                        count.fetch_add(1, Ordering::Relaxed);
                        let healthy = healthy.load(Ordering::Relaxed);
                        Box::pin(async move {
                            if healthy {
                                Ok(values(&request))
                            } else {
                                Ok(Vec::new())
                            }
                        })
                    },
                    cx,
                )
                .unwrap()
            }
        });
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        let failed_count = count.load(Ordering::Relaxed);
        for _ in 0..10 {
            window.update(|window, cx| {
                window.draw(cx).clear();
            });
            window.run_until_parked();
        }
        assert_eq!(count.load(Ordering::Relaxed), failed_count);
        window.update(|_, cx| {
            assert!(state.read(cx).grid.read(cx).source_error().is_some());
            state.update(cx, |state, cx| state.refresh(cx));
        });
        healthy.store(true, Ordering::Relaxed);
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.run_until_parked();
        window.update(|_, cx| {
            assert!(state.read(cx).grid.read(cx).source_error().is_none());
            assert!(state.read(cx).grid.read(cx).cached_tile_count() > 0);
        });
    }

    #[::core::prelude::v1::test]
    fn sustained_query_scroll_and_edit_workload_stays_bounded_and_captures_write_identity() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::dark());
        });
        let state = cx.new(|cx| {
            RemoteSheetState::with_options(
                100_000,
                12,
                0,
                RemoteSheetOptions {
                    cached_tiles: 8,
                    pending_tiles: 2,
                    tile_rows: 16,
                    tile_columns: 12,
                },
                |request| Box::pin(async move { Ok(values(&request)) }),
                cx,
            )
            .unwrap()
        });
        let writes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let _subscription = cx.update({
            let writes = writes.clone();
            let state = state.clone();
            move |cx| {
                cx.subscribe(&state, move |_, event, _| {
                    if let RemoteSheetEvent::Edited {
                        edit,
                        query,
                        generation,
                    } = event
                    {
                        writes.borrow_mut().push((
                            edit.position,
                            edit.value.clone(),
                            **query,
                            *generation,
                        ));
                    }
                })
            }
        });
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        for iteration in 0..100 {
            let row = iteration * 997;
            window.update(|window, cx| {
                if iteration % 10 == 0 {
                    state.update(cx, |state, cx| state.set_query(iteration, cx));
                }
                let grid = state.read(cx).grid.clone();
                grid.update(cx, |grid, cx| {
                    let position = SheetCellPosition::new(row, 2);
                    grid.select(position, false).unwrap();
                    grid.scroll_to_cell(position).unwrap();
                    cx.notify();
                });
                window.draw(cx).clear();
            });
            window.run_until_parked();
            window.update(|window, cx| {
                let grid = state.read(cx).grid.clone();
                grid.update(cx, |grid, cx| {
                    assert!(grid.cached_tile_count() <= 8);
                    assert!(grid.pending_tile_count() <= 2);
                    assert!(grid.viewport_metrics().mounted_rows < 20);
                    assert!(grid.viewport_metrics().mounted_cells < 160);
                    assert_eq!(
                        grid.cell_value(SheetCellPosition::new(row, 2))
                            .unwrap()
                            .as_ref(),
                        format!("q{}-r{row}-c2", iteration / 10 * 10)
                    );
                    grid.set_cell_value(
                        SheetCellPosition::new(row, 2),
                        format!("write-{iteration}"),
                        window,
                        cx,
                    )
                    .unwrap();
                });
                assert!(state.read(cx).pending_count() <= 2);
                if iteration == 99 {
                    // Subscription delivery is deferred, so a later query must
                    // not reinterpret this already emitted positional edit.
                    state.update(cx, |state, cx| state.set_query(100, cx));
                }
            });
            window.run_until_parked();
        }
        let writes = writes.borrow();
        assert_eq!(writes.len(), 100);
        assert_eq!(writes[99].0, SheetCellPosition::new(98_703, 2));
        assert_eq!(writes[99].1.as_ref(), "write-99");
        assert_eq!(writes[99].2, 90);
        assert_ne!(
            writes[99].3,
            window.update(|_, cx| state.read(cx).generation())
        );
    }
}
