//! Native hierarchical UIA through AccessKit, with thread-safe idle wakeups.

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

use crate::accessibility::PendingActionQueue;

const TOOLKIT: &str = "Kael";
const VERSION: &str = env!("CARGO_PKG_VERSION");
type Pending = Arc<Mutex<PendingActionQueue>>;
type Latest = Arc<Mutex<Option<crate::AccessibilityTree>>>;

struct InitialTree {
    latest: Latest,
}

impl ActivationHandler for InitialTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        let tree = self.latest.lock().ok()?.clone()?;
        Some(tree.to_accesskit_tree_update(Some(TOOLKIT), Some(VERSION)))
    }
}

fn enqueue(
    pending: &Pending,
    alive: &AtomicBool,
    request: ActionRequest,
    wake: impl FnOnce(),
) -> bool {
    let (accepted, wake_needed, first_overflow) = {
        let Ok(mut queue) = pending.lock() else {
            return false;
        };
        if !alive.load(Ordering::Acquire) {
            return false;
        }
        debug_assert!(queue.len() <= PendingActionQueue::MAX_REQUESTS);
        let empty = queue.is_empty();
        let accepted = queue.push(request);
        (
            accepted,
            accepted && empty,
            !accepted && queue.dropped() == 1,
        )
    };
    if first_overflow {
        PendingActionQueue::report_overflow();
    }
    if wake_needed {
        wake();
    }
    accepted
}

struct Actions {
    pending: Pending,
    alive: Arc<AtomicBool>,
    // AccessKit may call from a UIA worker. Only PostMessage crosses back to the
    // owner thread; App, window borrows and callbacks remain on that thread.
    hwnd: usize,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        if !self.alive.load(Ordering::Acquire) {
            return;
        }
        enqueue(&self.pending, &self.alive, request, || {
            if self.alive.load(Ordering::Acquire) {
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(self.hwnd as *mut _)),
                        super::events::WM_GPUI_FORCE_UPDATE_WINDOW,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        });
    }
}

/// Window-owned native adapter. AccessKit retains the logical tree and creates
/// UIA providers on demand, preserving hierarchy, 64-bit semantic IDs, actions,
/// focus and offscreen navigation without 100,000 eager COM objects per frame.
pub(crate) struct WindowsAccessibilityProvider {
    adapter: RefCell<accesskit_windows::Adapter>,
    latest: Latest,
    previous: RefCell<Option<crate::AccessibilityTree>>,
    pending: Pending,
    alive: Arc<AtomicBool>,
    hwnd: HWND,
    pending_host_focus: Cell<Option<crate::AccessibilityActionRequest>>,
}

impl WindowsAccessibilityProvider {
    pub(crate) fn new(hwnd: HWND) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let action = Actions {
            pending: pending.clone(),
            alive: alive.clone(),
            hwnd: hwnd.0 as usize,
        };
        // Initialize UIA at window creation, before WM_GETOBJECT; initialization
        // during WM_GETOBJECT can trigger a nested query and miss native UIA.
        let adapter =
            accesskit_windows::Adapter::new(accesskit_windows::HWND(hwnd.0), false, action);
        Self {
            adapter: RefCell::new(adapter),
            latest,
            previous: RefCell::new(None),
            pending,
            alive,
            hwnd,
            pending_host_focus: Cell::new(None),
        }
    }

    pub(crate) fn handle_wm_getobject(&self, wparam: WPARAM, lparam: LPARAM) -> Option<isize> {
        let mut activation = InitialTree {
            latest: self.latest.clone(),
        };
        let mut adapter = self.adapter.borrow_mut();
        let result = adapter.handle_wm_getobject(
            accesskit_windows::WPARAM(wparam.0),
            accesskit_windows::LPARAM(lparam.0),
            &mut activation,
        );
        drop(adapter);
        // UIA conversion may synchronously re-enter WM_GETOBJECT. The adapter
        // and window-state borrows must be released before this conversion.
        result.map(|result| {
            let result: accesskit_windows::LRESULT = result.into();
            result.0
        })
    }

    pub(crate) fn update_focus(&self, focused: bool) {
        let events = self.adapter.borrow_mut().update_window_focus_state(focused);
        if let Some(events) = events {
            events.raise();
        }
    }

    pub(crate) fn update_tree(
        &self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(tree.clone());
        }
        let events = {
            let previous = self.previous.borrow();
            self.adapter.borrow_mut().update_if_active(|| {
                tree.to_accesskit_tree_update_after(previous.as_ref(), Some(TOOLKIT), Some(VERSION))
            })
        };
        *self.previous.borrow_mut() = Some(tree.clone());
        // UIA event emission can re-enter; never hold adapter/semantic borrows.
        if let Some(events) = events {
            events.raise();
        }
        let raw = self
            .pending
            .lock()
            .map(|mut queue| queue.take())
            .unwrap_or_default();
        let requests: Vec<crate::AccessibilityActionRequest> = raw
            .into_iter()
            .filter_map(|request| request.normalize(tree))
            .collect();
        if let Some(request) = requests
            .iter()
            .rev()
            .find(|request| request.action == crate::AccessibilityAction::Focus)
        {
            // SetFocus synchronously sends activation/focus messages. Defer it
            // until the current App/window draw borrow has ended, coalescing to
            // one owner-thread message and retaining the exact accepted target.
            let already_pending = self
                .pending_host_focus
                .replace(Some(request.clone()))
                .is_some();
            if !already_pending {
                if let Err(error) = unsafe {
                    PostMessageW(
                        Some(self.hwnd),
                        super::events::WM_GPUI_ACCESSIBILITY_FOCUS,
                        WPARAM(0),
                        LPARAM(0),
                    )
                } {
                    self.pending_host_focus.take();
                    log::warn!("failed to queue native accessibility host focus: {error}");
                }
            }
        }
        requests
    }

    pub(crate) fn take_valid_host_focus_request(&self) -> bool {
        let Some(request) = self.pending_host_focus.take() else {
            return false;
        };
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        self.latest
            .lock()
            .ok()
            .and_then(|latest| {
                latest
                    .as_ref()
                    .and_then(|tree| tree.validate_action_request(request))
            })
            .is_some()
    }
}

impl Drop for WindowsAccessibilityProvider {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.pending_host_focus.take();
        if let Ok(mut queue) = self.pending.lock() {
            queue.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(id: u64) -> ActionRequest {
        ActionRequest {
            action: accesskit::Action::Click,
            target_node: accesskit::NodeId(id),
            target_tree: accesskit::TreeId::ROOT,
            data: None,
        }
    }
    #[test]
    fn background_actions_coalesce_and_wake_outside_the_lock() {
        let pending: Pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let worker = pending.clone();
        std::thread::spawn(move || {
            let mut wakes = 0;
            for id in [0x1_0000_0001, 0x2_0000_0001] {
                assert!(enqueue(
                    &worker,
                    &AtomicBool::new(true),
                    request(id),
                    || {
                        assert!(worker.try_lock().is_ok());
                        wakes += 1;
                    }
                ));
            }
            assert_eq!(wakes, 1);
        })
        .join()
        .unwrap();
        let drained = pending.lock().unwrap().take();
        assert_eq!(
            drained[0],
            crate::PendingAccessibilityAction::Raw(request(0x1_0000_0001))
        );
        assert_eq!(
            drained[1],
            crate::PendingAccessibilityAction::Raw(request(0x2_0000_0001))
        );
        let mut woke = false;
        assert!(enqueue(
            &pending,
            &AtomicBool::new(true),
            request(3),
            || woke = true
        ));
        assert!(woke);
    }
    #[test]
    fn action_flood_is_bounded_before_another_window_frame() {
        let pending: Pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        for id in 0..PendingActionQueue::MAX_REQUESTS as u64 {
            assert!(enqueue(
                &pending,
                &AtomicBool::new(true),
                request(id),
                || {}
            ));
        }
        assert!(!enqueue(
            &pending,
            &AtomicBool::new(true),
            request(u64::MAX),
            || panic!("rejected action must not wake")
        ));
        assert_eq!(
            pending.lock().unwrap().len(),
            PendingActionQueue::MAX_REQUESTS
        );
    }
    #[test]
    fn retained_ui_automation_handler_cannot_queue_after_window_drop() {
        let pending: Pending = Arc::new(Mutex::new(PendingActionQueue::default()));
        let mut handler = Actions {
            pending: pending.clone(),
            alive: Arc::new(AtomicBool::new(false)),
            hwnd: 0,
        };
        handler.do_action(request(1));
        assert!(pending.lock().unwrap().is_empty());
    }
}
