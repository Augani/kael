use crate::{
    Action, AnyView, AnyWindowHandle, App, AppCell, AppContext, AsyncApp, AttentionType,
    AvailableSpace, BackgroundExecutor, BiometricStatus, BorrowAppContext, Bounds, Capslock,
    ClipboardItem, DrawPhase, Drawable, Element, Empty, EventEmitter, ForegroundExecutor, Global,
    InputEvent, Keystroke, MediaKeyEvent, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NetworkStatus, PathPromptOptions, Pixels,
    Platform, Point, PowerMode, PowerSaveBlockerKind, Render, Result, Size, SystemPowerEvent,
    SystemPowerSource, Task, TestDispatcher, TestPlatform, TestScreenCaptureSource, TestWindow,
    TextSystem, VisualContext, Window, WindowBounds, WindowHandle, WindowOptions,
};
use anyhow::{anyhow, bail};
use futures::{Stream, StreamExt, channel::oneshot};
use rand::{SeedableRng, rngs::StdRng};
use std::{
    cell::RefCell,
    future::Future,
    ops::{Deref, Range},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

/// A TestAppContext is provided to tests created with `#[kael::test]`, it provides
/// an implementation of `Context` with additional methods that are useful in tests.
#[derive(Clone)]
pub struct TestAppContext {
    #[doc(hidden)]
    pub app: Rc<AppCell>,
    #[doc(hidden)]
    pub background_executor: BackgroundExecutor,
    #[doc(hidden)]
    pub foreground_executor: ForegroundExecutor,
    #[doc(hidden)]
    pub dispatcher: TestDispatcher,
    test_platform: Rc<TestPlatform>,
    text_system: Arc<TextSystem>,
    fn_name: Option<&'static str>,
    on_quit: Rc<RefCell<Vec<Box<dyn FnOnce() + 'static>>>>,
}

impl AppContext for TestAppContext {
    type Result<T> = T;

    fn new<T: 'static>(
        &mut self,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        let mut app = self.app.borrow_mut();
        app.new(build_entity)
    }

    fn reserve_entity<T: 'static>(&mut self) -> Self::Result<crate::Reservation<T>> {
        let mut app = self.app.borrow_mut();
        app.reserve_entity()
    }

    fn insert_entity<T: 'static>(
        &mut self,
        reservation: crate::Reservation<T>,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        let mut app = self.app.borrow_mut();
        app.insert_entity(reservation, build_entity)
    }

    fn update_entity<T: 'static, R>(
        &mut self,
        handle: &Entity<T>,
        update: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> Self::Result<R> {
        let mut app = self.app.borrow_mut();
        app.update_entity(handle, update)
    }

    fn as_mut<'a, T>(&'a mut self, _: &Entity<T>) -> Self::Result<super::GpuiBorrow<'a, T>>
    where
        T: 'static,
    {
        panic!("Cannot use as_mut with a test app context. Try calling update() first")
    }

    fn read_entity<T, R>(
        &self,
        handle: &Entity<T>,
        read: impl FnOnce(&T, &App) -> R,
    ) -> Self::Result<R>
    where
        T: 'static,
    {
        let app = self.app.borrow();
        app.read_entity(handle, read)
    }

    fn update_window<T, F>(&mut self, window: AnyWindowHandle, f: F) -> Result<T>
    where
        F: FnOnce(AnyView, &mut Window, &mut App) -> T,
    {
        let mut lock = self.app.borrow_mut();
        lock.update_window(window, f)
    }

    fn read_window<T, R>(
        &self,
        window: &WindowHandle<T>,
        read: impl FnOnce(Entity<T>, &App) -> R,
    ) -> Result<R>
    where
        T: 'static,
    {
        let app = self.app.borrow();
        app.read_window(window, read)
    }

    fn background_spawn<R>(&self, future: impl Future<Output = R> + Send + 'static) -> Task<R>
    where
        R: Send + 'static,
    {
        self.background_executor.spawn(future)
    }

    fn read_global<G, R>(&self, callback: impl FnOnce(&G, &App) -> R) -> Self::Result<R>
    where
        G: Global,
    {
        let app = self.app.borrow();
        app.read_global(callback)
    }
}

impl TestAppContext {
    /// Creates a new `TestAppContext`. Usually you can rely on `#[kael::test]` to do this for you.
    pub fn build(dispatcher: TestDispatcher, fn_name: Option<&'static str>) -> Self {
        let arc_dispatcher = Arc::new(dispatcher.clone());
        let background_executor = BackgroundExecutor::new(arc_dispatcher.clone());
        let foreground_executor = ForegroundExecutor::new(arc_dispatcher);
        let platform = TestPlatform::new(background_executor.clone(), foreground_executor.clone());
        let asset_source = Arc::new(());
        let http_client = http_client::FakeHttpClient::with_404_response();
        let text_system = Arc::new(TextSystem::new(platform.text_system()));

        Self {
            app: App::new_app(platform.clone(), asset_source, http_client),
            background_executor,
            foreground_executor,
            dispatcher,
            test_platform: platform,
            text_system,
            fn_name,
            on_quit: Rc::new(RefCell::new(Vec::default())),
        }
    }

    /// Create a single TestAppContext, for non-multi-client tests
    pub fn single() -> Self {
        let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(0));
        Self::build(dispatcher, None)
    }

    /// The name of the test function that created this `TestAppContext`
    pub fn test_function_name(&self) -> Option<&'static str> {
        self.fn_name
    }

    /// Checks whether there have been any new path prompts received by the platform.
    pub fn did_prompt_for_new_path(&self) -> bool {
        self.test_platform.did_prompt_for_new_path()
    }

    /// Sets the simulated "reduce motion" accessibility preference for tests.
    pub fn set_reduce_motion(&self, reduce_motion: bool) {
        self.test_platform.set_reduce_motion(reduce_motion);
    }

    /// Sets the simulated external-power/battery source for tests.
    pub fn set_system_power_source(&self, source: SystemPowerSource) {
        self.test_platform.set_system_power_source(source);
    }

    /// Sets the simulated battery percentage for tests.
    pub fn set_battery_percentage(&self, percentage: Option<u8>) {
        self.test_platform.set_battery_percentage(percentage);
    }

    /// returns a new `TestAppContext` re-using the same executors to interleave tasks.
    pub fn new_app(&self) -> TestAppContext {
        Self::build(self.dispatcher.clone(), self.fn_name)
    }

    /// Called by the test helper to end the test.
    /// public so the macro can call it.
    pub fn quit(&self) {
        self.on_quit.borrow_mut().drain(..).for_each(|f| f());
        self.app.borrow_mut().shutdown();
    }

    /// Register cleanup to run when the test ends.
    pub fn on_quit(&mut self, f: impl FnOnce() + 'static) {
        self.on_quit.borrow_mut().push(Box::new(f));
    }

    /// Schedules all windows to be redrawn on the next effect cycle.
    pub fn refresh(&mut self) -> Result<()> {
        let mut app = self.app.borrow_mut();
        app.refresh_windows();
        Ok(())
    }

    /// Returns an executor (for running tasks in the background)
    pub fn executor(&self) -> BackgroundExecutor {
        self.background_executor.clone()
    }

    /// Returns an executor (for running tasks on the main thread)
    pub fn foreground_executor(&self) -> &ForegroundExecutor {
        &self.foreground_executor
    }

    #[expect(clippy::wrong_self_convention)]
    fn new<T: 'static>(&mut self, build_entity: impl FnOnce(&mut Context<T>) -> T) -> Entity<T> {
        let mut cx = self.app.borrow_mut();
        cx.new(build_entity)
    }

    /// Gives you an `&mut App` for the duration of the closure
    pub fn update<R>(&self, f: impl FnOnce(&mut App) -> R) -> R {
        let mut cx = self.app.borrow_mut();
        cx.update(f)
    }

    /// Gives you an `&App` for the duration of the closure
    pub fn read<R>(&self, f: impl FnOnce(&App) -> R) -> R {
        let cx = self.app.borrow();
        f(&cx)
    }

    /// Adds a new window. The Window will always be backed by a `TestWindow` which
    /// can be retrieved with `self.test_window(handle)`
    pub fn add_window<F, V>(&mut self, build_window: F) -> WindowHandle<V>
    where
        F: FnOnce(&mut Window, &mut Context<V>) -> V,
        V: 'static + Render,
    {
        let mut cx = self.app.borrow_mut();

        // Some tests rely on the window size matching the bounds of the test display
        let bounds = Bounds::maximized(None, &cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| build_window(window, cx)),
        )
        .unwrap()
    }

    /// Adds a new window with no content.
    pub fn add_empty_window(&mut self) -> &mut VisualTestContext {
        let mut cx = self.app.borrow_mut();
        let bounds = Bounds::maximized(None, &cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| Empty),
            )
            .unwrap();
        drop(cx);
        let cx = VisualTestContext::from_window(*window.deref(), self).into_mut();
        cx.run_until_parked();
        cx
    }

    /// Adds a new window, and returns its root view and a `VisualTestContext` which can be used
    /// as a `Window` and `App` for the rest of the test. Typically you would shadow this context with
    /// the returned one. `let (view, cx) = cx.add_window_view(...);`
    pub fn add_window_view<F, V>(
        &mut self,
        build_root_view: F,
    ) -> (Entity<V>, &mut VisualTestContext)
    where
        F: FnOnce(&mut Window, &mut Context<V>) -> V,
        V: 'static + Render,
    {
        let mut cx = self.app.borrow_mut();
        let bounds = Bounds::maximized(None, &cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| build_root_view(window, cx)),
            )
            .unwrap();
        drop(cx);
        let view = window.root(self).unwrap();
        let cx = VisualTestContext::from_window(*window.deref(), self).into_mut();
        cx.run_until_parked();

        // it might be nice to try and cleanup these at the end of each test.
        (view, cx)
    }

    /// returns the TextSystem
    pub fn text_system(&self) -> &Arc<TextSystem> {
        &self.text_system
    }

    /// Simulates writing to the platform clipboard
    pub fn write_to_clipboard(&self, item: ClipboardItem) {
        self.test_platform.write_to_clipboard(item)
    }

    /// Simulates writing plain text to the platform clipboard.
    pub fn write_clipboard_text(&self, text: impl Into<String>) {
        self.write_to_clipboard(ClipboardItem::new_string(text.into()));
    }

    /// Simulates reading from the platform clipboard.
    /// This will return the most recent value from `write_to_clipboard`.
    pub fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.test_platform.read_from_clipboard()
    }

    /// Simulates reading plain text from the platform clipboard.
    pub fn read_clipboard_text(&self) -> Option<String> {
        self.read_from_clipboard().and_then(|item| item.text())
    }

    /// Simulates choosing a File in the platform's "Open" dialog.
    pub fn simulate_new_path_selection(
        &self,
        select_path: impl FnOnce(&std::path::Path) -> Option<std::path::PathBuf>,
    ) {
        self.test_platform.simulate_new_path_selection(select_path);
    }

    /// Simulates choosing paths in the platform's "Open" dialog.
    pub fn simulate_path_selection(
        &self,
        select_paths: impl FnOnce(&PathPromptOptions) -> Option<Vec<std::path::PathBuf>>,
    ) {
        self.test_platform.simulate_path_selection(select_paths);
    }

    /// Returns true if there's a path selection dialog open.
    pub fn did_prompt_for_paths(&self) -> bool {
        self.test_platform.did_prompt_for_paths()
    }

    /// Simulates clicking a button in an platform-level alert dialog.
    #[track_caller]
    pub fn simulate_prompt_answer(&self, button: &str) {
        self.test_platform.simulate_prompt_answer(button);
    }

    /// Returns true if there's an alert dialog open.
    pub fn has_pending_prompt(&self) -> bool {
        self.test_platform.has_pending_prompt()
    }

    /// Returns true if there's an alert dialog open.
    pub fn pending_prompt(&self) -> Option<(String, String)> {
        self.test_platform.pending_prompt()
    }

    /// All the urls that have been opened with cx.open_url() during this test.
    pub fn opened_url(&self) -> Option<String> {
        self.test_platform.opened_url.borrow().clone()
    }

    /// Simulates the platform delivering open-URL events to the application.
    pub fn simulate_open_urls(&self, urls: &[&str]) {
        self.test_platform
            .simulate_open_urls(urls.iter().map(|url| (*url).to_string()).collect());
    }

    /// Returns URL schemes registered through the test platform.
    pub fn registered_url_schemes(&self) -> Vec<String> {
        self.test_platform.registered_url_schemes()
    }

    /// Paths requested for platform trash/recycle by this test.
    pub fn trashed_paths(&self) -> Vec<std::path::PathBuf> {
        self.test_platform.trashed_paths()
    }

    /// Return the current test platform tray menu.
    pub fn tray_menu(&self) -> Vec<crate::TrayMenuItem> {
        self.test_platform.tray_menu()
    }

    /// Return the current test platform tray tooltip.
    pub fn tray_tooltip(&self) -> String {
        self.test_platform.tray_tooltip()
    }

    /// Return whether the test platform tray is in panel mode.
    pub fn tray_panel_mode(&self) -> bool {
        self.test_platform.tray_panel_mode()
    }

    /// Return whether the test platform is keeping the app alive without windows.
    pub fn keep_alive_without_windows(&self) -> bool {
        self.test_platform.keep_alive_without_windows()
    }

    /// Returns document paths added to the OS recent-documents list through the
    /// test platform.
    pub fn recent_documents(&self) -> Vec<PathBuf> {
        self.test_platform.recent_documents()
    }

    /// Returns active power-save blockers started through the test platform.
    pub fn power_save_blockers(&self) -> Vec<(u32, PowerSaveBlockerKind)> {
        self.test_platform.power_save_blockers()
    }

    /// Returns the current test-platform user-attention request.
    pub fn user_attention(&self) -> Option<AttentionType> {
        self.test_platform.user_attention()
    }

    /// Returns how often user attention was cancelled through the test platform.
    pub fn user_attention_cancel_count(&self) -> usize {
        self.test_platform.user_attention_cancel_count()
    }

    /// Returns the current test-platform network status.
    pub fn network_status(&self) -> NetworkStatus {
        self.test_platform.network_status()
    }

    /// Simulates a network status change from the platform.
    pub fn simulate_network_status_change(&self, status: NetworkStatus) {
        self.test_platform.simulate_network_status_change(status);
    }

    /// Sets the biometric availability reported by the test platform.
    pub fn set_biometric_status(&self, status: BiometricStatus) {
        self.test_platform.set_biometric_status(status);
    }

    /// Sets the next biometric authentication result reported by the test platform.
    pub fn set_biometric_auth_success(&self, success: bool) {
        self.test_platform.set_biometric_auth_success(success);
    }

    /// Returns biometric prompt reasons sent to the test platform.
    pub fn biometric_auth_reasons(&self) -> Vec<String> {
        self.test_platform.biometric_auth_reasons()
    }

    /// Simulates the platform delivering a hardware media-key event.
    pub fn simulate_media_key_event(&self, event: MediaKeyEvent) {
        self.test_platform.simulate_media_key_event(event);
    }

    /// Simulates the user resizing the window to the new size.
    pub fn simulate_window_resize(&self, window_handle: AnyWindowHandle, size: Size<Pixels>) {
        self.test_window(window_handle).simulate_resize(size);
    }

    /// Causes the given sources to be returned if the application queries for screen
    /// capture sources.
    pub fn set_screen_capture_sources(&self, sources: Vec<TestScreenCaptureSource>) {
        self.test_platform.set_screen_capture_sources(sources);
    }

    /// Override the platform power mode for the current test app.
    pub fn set_power_mode(&self, power_mode: PowerMode) {
        self.test_platform.set_power_mode(power_mode);
    }

    /// Simulates a platform system power event.
    pub fn simulate_system_power_event(&self, event: SystemPowerEvent) {
        self.test_platform.simulate_system_power_event(event);
    }

    /// Returns all windows open in the test.
    pub fn windows(&self) -> Vec<AnyWindowHandle> {
        self.app.borrow().windows()
    }

    /// Run the given task on the main thread.
    #[track_caller]
    pub fn spawn<Fut, R>(&self, f: impl FnOnce(AsyncApp) -> Fut) -> Task<R>
    where
        Fut: Future<Output = R> + 'static,
        R: 'static,
    {
        self.foreground_executor.spawn(f(self.to_async()))
    }

    /// true if the given global is defined
    pub fn has_global<G: Global>(&self) -> bool {
        let app = self.app.borrow();
        app.has_global::<G>()
    }

    /// runs the given closure with a reference to the global
    /// panics if `has_global` would return false.
    pub fn read_global<G: Global, R>(&self, read: impl FnOnce(&G, &App) -> R) -> R {
        let app = self.app.borrow();
        read(app.global(), &app)
    }

    /// runs the given closure with a reference to the global (if set)
    pub fn try_read_global<G: Global, R>(&self, read: impl FnOnce(&G, &App) -> R) -> Option<R> {
        let lock = self.app.borrow();
        Some(read(lock.try_global()?, &lock))
    }

    /// sets the global in this context.
    pub fn set_global<G: Global>(&mut self, global: G) {
        let mut lock = self.app.borrow_mut();
        lock.update(|cx| cx.set_global(global))
    }

    /// updates the global in this context. (panics if `has_global` would return false)
    pub fn update_global<G: Global, R>(&mut self, update: impl FnOnce(&mut G, &mut App) -> R) -> R {
        let mut lock = self.app.borrow_mut();
        lock.update(|cx| cx.update_global(update))
    }

    /// Returns an `AsyncApp` which can be used to run tasks that expect to be on a background
    /// thread on the current thread in tests.
    pub fn to_async(&self) -> AsyncApp {
        AsyncApp {
            app: Rc::downgrade(&self.app),
            background_executor: self.background_executor.clone(),
            foreground_executor: self.foreground_executor.clone(),
        }
    }

    /// Wait until there are no more pending tasks.
    pub fn run_until_parked(&mut self) {
        self.background_executor.run_until_parked()
    }

    /// Simulate dispatching an action to the currently focused node in the window.
    pub fn dispatch_action<A>(&mut self, window: AnyWindowHandle, action: A)
    where
        A: Action,
    {
        window
            .update(self, |_, window, cx| {
                window.dispatch_action(action.boxed_clone(), cx)
            })
            .unwrap();

        self.background_executor.run_until_parked()
    }

    /// simulate_keystrokes takes a space-separated list of keys to type.
    /// cx.simulate_keystrokes("cmd-shift-p b k s p enter")
    /// in Kael, this will run backspace on the current editor through the command palette.
    /// This will also run the background executor until it's parked.
    pub fn simulate_keystrokes(&mut self, window: AnyWindowHandle, keystrokes: &str) {
        for keystroke in keystrokes
            .split(' ')
            .map(Keystroke::parse)
            .map(Result::unwrap)
        {
            self.dispatch_keystroke(window, keystroke);
        }

        self.background_executor.run_until_parked()
    }

    /// simulate_input takes a string of text to type.
    /// cx.simulate_input("abc")
    /// will type abc into your current editor
    /// This will also run the background executor until it's parked.
    pub fn simulate_input(&mut self, window: AnyWindowHandle, input: &str) {
        for keystroke in input.split("").map(Keystroke::parse).map(Result::unwrap) {
            self.dispatch_keystroke(window, keystroke);
        }

        self.background_executor.run_until_parked()
    }

    /// Simulate an in-progress input-method composition in the focused editor.
    pub fn simulate_marked_input(
        &mut self,
        window: AnyWindowHandle,
        input: &str,
        selected_range: Option<Range<usize>>,
    ) {
        let mut handler = self
            .update_window(window, |_, window, _| {
                window
                    .platform_window
                    .take_input_handler()
                    .expect("focused input handler")
            })
            .unwrap();
        handler.replace_and_mark_text_in_range(None, input, selected_range);
        self.update_window(window, |_, window, _| {
            window.platform_window.set_input_handler(handler);
        })
        .unwrap();
    }

    /// dispatches a single Keystroke (see also `simulate_keystrokes` and `simulate_input`)
    pub fn dispatch_keystroke(&mut self, window: AnyWindowHandle, keystroke: Keystroke) {
        self.update_window(window, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx)
        })
        .unwrap();
    }

    /// Returns the `TestWindow` backing the given handle.
    pub(crate) fn test_window(&self, window: AnyWindowHandle) -> TestWindow {
        self.app
            .borrow_mut()
            .windows
            .get_mut(window.id)
            .unwrap()
            .as_mut()
            .unwrap()
            .platform_window
            .as_test()
            .unwrap()
            .clone()
    }

    /// Returns a stream of notifications whenever the Entity is updated.
    pub fn notifications<T: 'static>(
        &mut self,
        entity: &Entity<T>,
    ) -> impl Stream<Item = ()> + use<T> {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        self.update(|cx| {
            cx.observe(entity, {
                let tx = tx.clone();
                move |_, _| {
                    let _ = tx.unbounded_send(());
                }
            })
            .detach();
            cx.observe_release(entity, move |_, _| tx.close_channel())
                .detach()
        });
        rx
    }

    /// Returns a stream of events emitted by the given Entity.
    pub fn events<Evt, T: 'static + EventEmitter<Evt>>(
        &mut self,
        entity: &Entity<T>,
    ) -> futures::channel::mpsc::UnboundedReceiver<Evt>
    where
        Evt: 'static + Clone,
    {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        entity
            .update(self, |_, cx: &mut Context<T>| {
                cx.subscribe(entity, move |_entity, _handle, event, _cx| {
                    let _ = tx.unbounded_send(event.clone());
                })
            })
            .detach();
        rx
    }

    /// Runs until the given condition becomes true. (Prefer `run_until_parked` if you
    /// don't need to jump in at a specific time).
    pub async fn condition<T: 'static>(
        &mut self,
        entity: &Entity<T>,
        mut predicate: impl FnMut(&mut T, &mut Context<T>) -> bool,
    ) {
        let timer = self.executor().timer(Duration::from_secs(3));
        let mut notifications = self.notifications(entity);

        use futures::FutureExt as _;
        use smol::future::FutureExt as _;

        async {
            loop {
                if entity.update(self, &mut predicate) {
                    return Ok(());
                }

                if notifications.next().await.is_none() {
                    bail!("entity dropped")
                }
            }
        }
        .race(timer.map(|_| Err(anyhow!("condition timed out"))))
        .await
        .unwrap();
    }

    /// Set a name for this App.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_name(&mut self, name: &'static str) {
        self.update(|cx| cx.name = Some(name))
    }
}

impl<T: 'static> Entity<T> {
    /// Block until the next event is emitted by the entity, then return it.
    pub fn next_event<Event>(&self, cx: &mut TestAppContext) -> impl Future<Output = Event>
    where
        Event: Send + Clone + 'static,
        T: EventEmitter<Event>,
    {
        let (tx, mut rx) = oneshot::channel();
        let mut tx = Some(tx);
        let subscription = self.update(cx, |_, cx| {
            cx.subscribe(self, move |_, _, event, _| {
                if let Some(tx) = tx.take() {
                    _ = tx.send(event.clone());
                }
            })
        });

        async move {
            let event = rx.await.expect("no event emitted");
            drop(subscription);
            event
        }
    }
}

impl<V: 'static> Entity<V> {
    /// Returns a future that resolves when the view is next updated.
    pub fn next_notification(
        &self,
        advance_clock_by: Duration,
        cx: &TestAppContext,
    ) -> impl Future<Output = ()> {
        use postage::prelude::{Sink as _, Stream as _};

        let (mut tx, mut rx) = postage::mpsc::channel(1);
        let subscription = cx.app.borrow_mut().observe(self, move |_, _| {
            tx.try_send(()).ok();
        });

        let duration = if std::env::var("CI").is_ok() {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(1)
        };

        cx.executor().advance_clock(advance_clock_by);

        async move {
            let notification = crate::util::smol_timeout(duration, rx.recv())
                .await
                .expect("next notification timed out");
            drop(subscription);
            notification.expect("entity dropped while test was waiting for its next notification")
        }
    }
}

impl<V> Entity<V> {
    /// Returns a future that resolves when the condition becomes true.
    pub fn condition<Evt>(
        &self,
        cx: &TestAppContext,
        mut predicate: impl FnMut(&V, &App) -> bool,
    ) -> impl Future<Output = ()>
    where
        Evt: 'static,
        V: EventEmitter<Evt>,
    {
        use postage::prelude::{Sink as _, Stream as _};

        let (tx, mut rx) = postage::mpsc::channel(1024);

        let mut cx = cx.app.borrow_mut();
        let subscriptions = (
            cx.observe(self, {
                let mut tx = tx.clone();
                move |_, _| {
                    tx.blocking_send(()).ok();
                }
            }),
            cx.subscribe(self, {
                let mut tx = tx;
                move |_, _: &Evt, _| {
                    tx.blocking_send(()).ok();
                }
            }),
        );

        let cx = cx.this.upgrade().unwrap();
        let handle = self.downgrade();

        async move {
            crate::util::smol_timeout(Duration::from_secs(1), async move {
                loop {
                    {
                        let cx = cx.borrow();
                        let cx = &*cx;
                        if predicate(
                            handle
                                .upgrade()
                                .expect("view dropped with pending condition")
                                .read(cx),
                            cx,
                        ) {
                            break;
                        }
                    }

                    cx.borrow().background_executor().start_waiting();
                    rx.recv()
                        .await
                        .expect("view dropped with pending condition");
                    cx.borrow().background_executor().finish_waiting();
                }
            })
            .await
            .expect("condition timed out");
            drop(subscriptions);
        }
    }
}

use derive_more::{Deref, DerefMut};

use super::{Context, Entity};
#[derive(Deref, DerefMut, Clone)]
/// A VisualTestContext is the test-equivalent of a `Window` and `App`. It allows you to
/// run window-specific test code. It can be dereferenced to a `TextAppContext`.
pub struct VisualTestContext {
    #[deref]
    #[deref_mut]
    /// cx is the original TestAppContext (you can more easily access this using Deref)
    pub cx: TestAppContext,
    window: AnyWindowHandle,
}

impl VisualTestContext {
    /// Provides a `Window` and `App` for the duration of the closure.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx
            .update_window(self.window, |_, window, cx| f(window, cx))
            .unwrap()
    }

    /// Creates a new VisualTestContext. You would typically shadow the passed in
    /// TestAppContext with this, as this is typically more useful.
    /// `let cx = VisualTestContext::from_window(window, cx);`
    pub fn from_window(window: AnyWindowHandle, cx: &TestAppContext) -> Self {
        Self {
            cx: cx.clone(),
            window,
        }
    }

    /// Wait until there are no more pending tasks.
    pub fn run_until_parked(&self) {
        self.cx.background_executor.run_until_parked();
    }

    /// Dispatch the action to the currently focused node.
    pub fn dispatch_action<A>(&mut self, action: A)
    where
        A: Action,
    {
        self.cx.dispatch_action(self.window, action)
    }

    /// Read the title off the window (set by `Window#set_window_title`)
    pub fn window_title(&mut self) -> Option<String> {
        self.cx.test_window(self.window).0.lock().title.clone()
    }

    /// Simulate a sequence of keystrokes `cx.simulate_keystrokes("cmd-p escape")`
    /// Automatically runs until parked.
    pub fn simulate_keystrokes(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(self.window, keystrokes)
    }

    /// Simulate typing text `cx.simulate_input("hello")`
    /// Automatically runs until parked.
    pub fn simulate_input(&mut self, input: &str) {
        self.cx.simulate_input(self.window, input)
    }

    /// Simulate an in-progress input-method composition in the focused editor.
    pub fn simulate_marked_input(&mut self, input: &str, selected_range: Option<Range<usize>>) {
        let mut handler = self.update(|window, _| {
            window
                .platform_window
                .take_input_handler()
                .expect("focused input handler")
        });
        handler.replace_and_mark_text_in_range(None, input, selected_range);
        self.update(|window, _| {
            window.platform_window.set_input_handler(handler);
        });
    }

    /// Simulate a mouse move event to the given point
    pub fn simulate_mouse_move(
        &mut self,
        position: Point<Pixels>,
        button: impl Into<Option<MouseButton>>,
        modifiers: Modifiers,
    ) {
        self.simulate_event(MouseMoveEvent {
            position,
            modifiers,
            pressed_button: button.into(),
        })
    }

    /// Simulate a mouse down event to the given point
    pub fn simulate_mouse_down(
        &mut self,
        position: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
    ) {
        self.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button,
            click_count: 1,
            first_mouse: false,
        })
    }

    /// Simulate a mouse up event to the given point
    pub fn simulate_mouse_up(
        &mut self,
        position: Point<Pixels>,
        button: MouseButton,
        modifiers: Modifiers,
    ) {
        self.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button,
            click_count: 1,
        })
    }

    /// Simulate a primary mouse click at the given point
    pub fn simulate_click(&mut self, position: Point<Pixels>, modifiers: Modifiers) {
        self.simulate_event(MouseDownEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.simulate_event(MouseUpEvent {
            position,
            modifiers,
            button: MouseButton::Left,
            click_count: 1,
        });
    }

    /// Simulate a modifiers changed event
    pub fn simulate_modifiers_change(&mut self, modifiers: Modifiers) {
        self.simulate_event(ModifiersChangedEvent {
            modifiers,
            capslock: Capslock { on: false },
        })
    }

    /// Simulate a capslock changed event
    pub fn simulate_capslock_change(&mut self, on: bool) {
        self.simulate_event(ModifiersChangedEvent {
            modifiers: Modifiers::none(),
            capslock: Capslock { on },
        })
    }

    /// Simulates the user resizing the window to the new size.
    pub fn simulate_resize(&self, size: Size<Pixels>) {
        self.simulate_window_resize(self.window, size)
    }

    /// debug_bounds returns the bounds of the element with the given selector.
    pub fn debug_bounds(&mut self, selector: &'static str) -> Option<Bounds<Pixels>> {
        self.update(|window, _| window.rendered_frame.debug_bounds.get(selector).copied())
    }

    /// Draw an element to the window. Useful for simulating events or actions
    pub fn draw<E>(
        &mut self,
        origin: Point<Pixels>,
        space: impl Into<Size<AvailableSpace>>,
        f: impl FnOnce(&mut Window, &mut App) -> E,
    ) -> (E::RequestLayoutState, E::PrepaintState)
    where
        E: Element,
    {
        self.update(|window, cx| {
            window.invalidator.set_phase(DrawPhase::Prepaint);
            let mut element = Drawable::new(f(window, cx));
            element.layout_as_root(space.into(), window, cx);
            window.with_absolute_element_offset(origin, |window| element.prepaint(window, cx));

            window.invalidator.set_phase(DrawPhase::Paint);
            let (request_layout_state, prepaint_state) = element.paint(window, cx);

            window.invalidator.set_phase(DrawPhase::None);
            window.refresh();

            (request_layout_state, prepaint_state)
        })
    }

    /// Simulate an event from the platform, e.g. a SrollWheelEvent
    /// Make sure you've called [VisualTestContext::draw] first!
    pub fn simulate_event<E: InputEvent>(&mut self, event: E) {
        self.test_window(self.window)
            .simulate_input(event.to_platform_input());
        self.background_executor.run_until_parked();
    }

    /// Simulates the user blurring the window.
    pub fn deactivate_window(&mut self) {
        if Some(self.window) == self.test_platform.active_window() {
            self.test_platform.set_active_window(None)
        }
        self.background_executor.run_until_parked();
    }

    /// Simulates the user closing the window.
    /// Returns true if the window was closed.
    pub fn simulate_close(&mut self) -> bool {
        let handler = self
            .cx
            .update_window(self.window, |_, window, _| {
                window
                    .platform_window
                    .as_test()
                    .unwrap()
                    .0
                    .lock()
                    .should_close_handler
                    .take()
            })
            .unwrap();
        if let Some(mut handler) = handler {
            let should_close = handler();
            self.cx
                .update_window(self.window, |_, window, _| {
                    window.platform_window.on_should_close(handler);
                })
                .unwrap();
            should_close
        } else {
            false
        }
    }

    /// Get an &mut VisualTestContext (which is mostly what you need to pass to other methods).
    /// This method internally retains the VisualTestContext until the end of the test.
    pub fn into_mut(self) -> &'static mut Self {
        let ptr = Box::into_raw(Box::new(self));
        // safety: on_quit will be called after the test has finished.
        // the executor will ensure that all tasks related to the test have stopped.
        // so there is no way for cx to be accessed after on_quit is called.
        let cx = unsafe { &mut *ptr };
        cx.on_quit(move || unsafe {
            drop(Box::from_raw(ptr));
        });
        cx
    }
}

impl AppContext for VisualTestContext {
    type Result<T> = <TestAppContext as AppContext>::Result<T>;

    fn new<T: 'static>(
        &mut self,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        self.cx.new(build_entity)
    }

    fn reserve_entity<T: 'static>(&mut self) -> Self::Result<crate::Reservation<T>> {
        self.cx.reserve_entity()
    }

    fn insert_entity<T: 'static>(
        &mut self,
        reservation: crate::Reservation<T>,
        build_entity: impl FnOnce(&mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        self.cx.insert_entity(reservation, build_entity)
    }

    fn update_entity<T, R>(
        &mut self,
        handle: &Entity<T>,
        update: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> Self::Result<R>
    where
        T: 'static,
    {
        self.cx.update_entity(handle, update)
    }

    fn as_mut<'a, T>(&'a mut self, handle: &Entity<T>) -> Self::Result<super::GpuiBorrow<'a, T>>
    where
        T: 'static,
    {
        self.cx.as_mut(handle)
    }

    fn read_entity<T, R>(
        &self,
        handle: &Entity<T>,
        read: impl FnOnce(&T, &App) -> R,
    ) -> Self::Result<R>
    where
        T: 'static,
    {
        self.cx.read_entity(handle, read)
    }

    fn update_window<T, F>(&mut self, window: AnyWindowHandle, f: F) -> Result<T>
    where
        F: FnOnce(AnyView, &mut Window, &mut App) -> T,
    {
        self.cx.update_window(window, f)
    }

    fn read_window<T, R>(
        &self,
        window: &WindowHandle<T>,
        read: impl FnOnce(Entity<T>, &App) -> R,
    ) -> Result<R>
    where
        T: 'static,
    {
        self.cx.read_window(window, read)
    }

    fn background_spawn<R>(&self, future: impl Future<Output = R> + Send + 'static) -> Task<R>
    where
        R: Send + 'static,
    {
        self.cx.background_spawn(future)
    }

    fn read_global<G, R>(&self, callback: impl FnOnce(&G, &App) -> R) -> Self::Result<R>
    where
        G: Global,
    {
        self.cx.read_global(callback)
    }
}

#[cfg(test)]
mod tests {
    use super::TestAppContext;
    use crate::{
        Capability, Empty, MessageDialogBuilder, OpenDialogBuilder, PathPromptOptions, PathScope,
        PowerMode, SaveDialogBuilder, SharedString, SystemPowerEvent,
    };
    use std::path::PathBuf;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn test_power_mode_can_be_overridden() {
        let cx = TestAppContext::single();
        cx.set_power_mode(PowerMode::LowPower);

        assert_eq!(cx.read(|app| app.power_mode()), PowerMode::LowPower);
    }

    #[test]
    fn system_power_events_refresh_windows_and_invoke_callback() {
        let mut cx = TestAppContext::single();
        let window = cx.add_window(|_, _| Empty);
        let called = Rc::new(Cell::new(false));
        let called_flag = called.clone();

        cx.update(|app| {
            app.on_system_power_event(move |event, _| {
                if event == SystemPowerEvent::PowerModeChanged {
                    called_flag.set(true);
                }
            });
        });

        let handle = window.into();
        assert!(!cx.test_window(handle).0.lock().frame_polling_active);

        cx.simulate_system_power_event(SystemPowerEvent::PowerModeChanged);

        assert!(called.get());
        assert!(cx.test_window(handle).0.lock().frame_polling_active);
    }

    #[test]
    fn prompt_for_paths_can_be_completed_in_tests() {
        let mut cx = TestAppContext::single();
        let expected = PathBuf::from("/selected/file.txt");
        cx.update(|app| {
            app.permission_broker.grant(
                app.current_process_id,
                Capability::FilesystemRead {
                    scope: PathScope::UserSelected,
                },
            );
        });
        let rx = cx.read(|app| {
            app.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(SharedString::from("Choose files")),
                filters: vec![],
            })
        });

        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_selection(|options| {
            assert!(options.files);
            assert!(!options.directories);
            assert!(options.multiple);
            Some(vec![expected.clone()])
        });

        let selected = cx.background_executor.block(rx).unwrap().unwrap().unwrap();
        assert_eq!(selected, vec![expected]);
        assert!(!cx.did_prompt_for_paths());
    }

    #[test]
    fn show_open_dialog_uses_builder_options() {
        let mut cx = TestAppContext::single();
        let expected = PathBuf::from("/selected/project");
        cx.update(|app| {
            app.permission_broker.grant(
                app.current_process_id,
                Capability::FilesystemRead {
                    scope: PathScope::UserSelected,
                },
            );
        });
        let rx = cx.read(|app| {
            app.show_open_dialog(
                OpenDialogBuilder::directory()
                    .multiple(true)
                    .prompt("Choose project"),
            )
        });

        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_selection(|options| {
            assert!(!options.files);
            assert!(options.directories);
            assert!(options.multiple);
            assert_eq!(
                options.prompt.as_ref().map(|prompt| prompt.as_ref()),
                Some("Choose project")
            );
            Some(vec![expected.clone()])
        });

        let selected = cx.background_executor.block(rx).unwrap().unwrap().unwrap();
        assert_eq!(selected, vec![expected]);
    }

    #[test]
    fn open_dialog_checked_previews_without_prompting() {
        let mut cx = TestAppContext::single();
        cx.update(|app| {
            app.permission_broker.grant(
                app.current_process_id,
                Capability::FilesystemRead {
                    scope: PathScope::UserSelected,
                },
            );
        });

        let plan = cx
            .read(|app| {
                app.open_dialog_checked(
                    OpenDialogBuilder::files()
                        .image_files()
                        .filter("Markdown", ["md", "markdown"])
                        .prompt("Open assets"),
                )
            })
            .unwrap();

        assert!(plan.allows_files());
        assert!(!plan.allows_directories());
        assert!(plan.allows_multiple());
        assert_eq!(
            plan.prompt().map(|prompt| prompt.as_ref()),
            Some("Open assets")
        );
        assert_eq!(plan.filter_count(), 2);
        assert_eq!(plan.filter_names(), vec!["Images", "Markdown"]);
        assert!(!cx.did_prompt_for_paths());
    }

    #[test]
    fn save_dialog_checked_previews_without_prompting() {
        let mut cx = TestAppContext::single();
        let directory = std::env::temp_dir();
        cx.update(|app| {
            app.permission_broker.grant(
                app.current_process_id,
                Capability::FilesystemWrite {
                    scope: PathScope::UserSelected,
                },
            );
        });

        let plan = cx
            .read(|app| {
                app.save_dialog_checked(
                    SaveDialogBuilder::new(&directory)
                        .suggested_name("report")
                        .pdf(),
                )
            })
            .unwrap();

        assert_eq!(plan.directory(), directory.as_path());
        assert_eq!(plan.suggested_name(), Some("report.pdf"));
        assert_eq!(plan.default_extension(), Some("pdf"));
        assert!(plan.appended_default_extension());
        assert!(!cx.did_prompt_for_new_path());
    }

    #[test]
    fn show_message_dialog_uses_builder_options() {
        let cx = TestAppContext::single();
        let rx = cx
            .read(|app| {
                app.show_message_dialog(
                    MessageDialogBuilder::confirm("Delete draft?", "This cannot be undone")
                        .detail("The draft will be removed from this device."),
                )
            })
            .unwrap();

        assert!(cx.has_pending_prompt());
        assert_eq!(
            cx.pending_prompt(),
            Some((
                "Delete draft?".to_string(),
                "The draft will be removed from this device.".to_string()
            ))
        );

        cx.simulate_prompt_answer("OK");

        let selected = cx.background_executor.block(rx).unwrap();
        assert_eq!(selected, 1);
        assert!(!cx.has_pending_prompt());
    }

    #[test]
    fn message_dialog_checked_previews_without_showing_prompt() {
        let cx = TestAppContext::single();
        let plan = cx
            .read(|app| {
                app.message_dialog_checked(
                    MessageDialogBuilder::save_discard_cancel(
                        "Save changes?",
                        "This document has unsaved changes.",
                    )
                    .detail("Unsaved changes will be lost."),
                )
            })
            .unwrap();

        assert_eq!(plan.button_count(), 3);
        assert_eq!(plan.button_index("Don't Save"), Some(1));
        assert_eq!(
            plan.default_button_label().map(|label| label.as_ref()),
            Some("Save")
        );
        assert_eq!(
            plan.cancel_button_label().map(|label| label.as_ref()),
            Some("Cancel")
        );
        assert!(plan.has_cancel_button());
        assert!(!cx.has_pending_prompt());
    }
}

impl VisualContext for VisualTestContext {
    /// Get the underlying window handle underlying this context.
    fn window_handle(&self) -> AnyWindowHandle {
        self.window
    }

    fn new_window_entity<T: 'static>(
        &mut self,
        build_entity: impl FnOnce(&mut Window, &mut Context<T>) -> T,
    ) -> Self::Result<Entity<T>> {
        self.window
            .update(&mut self.cx, |_, window, cx| {
                cx.new(|cx| build_entity(window, cx))
            })
            .unwrap()
    }

    fn update_window_entity<V: 'static, R>(
        &mut self,
        view: &Entity<V>,
        update: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Self::Result<R> {
        self.window
            .update(&mut self.cx, |_, window, cx| {
                view.update(cx, |v, cx| update(v, window, cx))
            })
            .unwrap()
    }

    fn replace_root_view<V>(
        &mut self,
        build_view: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Self::Result<Entity<V>>
    where
        V: 'static + Render,
    {
        self.window
            .update(&mut self.cx, |_, window, cx| {
                window.replace_root(cx, build_view)
            })
            .unwrap()
    }

    fn focus<V: crate::Focusable>(&mut self, view: &Entity<V>) -> Self::Result<()> {
        self.window
            .update(&mut self.cx, |_, window, cx| {
                view.read(cx).focus_handle(cx).focus(window)
            })
            .unwrap()
    }
}

impl AnyWindowHandle {
    /// Creates the given view in this window.
    pub fn build_entity<V: Render + 'static>(
        &self,
        cx: &mut TestAppContext,
        build_view: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) -> Entity<V> {
        self.update(cx, |_, window, cx| cx.new(|cx| build_view(window, cx)))
            .unwrap()
    }
}
