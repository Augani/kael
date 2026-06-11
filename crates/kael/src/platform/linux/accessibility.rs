//! AT-SPI2 accessibility support for the Linux backend via AccessKit.
//!
//! Each window owns an [`AtSpiAccessibleRoot`] that wraps an
//! [`accesskit_unix::Adapter`]. The adapter speaks the AT-SPI2 D-Bus protocol
//! (`org.a11y.atspi.*`) to expose GPUI elements to screen readers such as Orca.
//!
//! ## Async runtime
//!
//! `accesskit_unix` runs its own async executor. With the default `async-io`
//! feature (which this crate relies on), the adapter spawns and owns a
//! background thread for its zbus connection the first time an adapter is
//! created, so it does not need to be driven by kael's foreground/background
//! executors. The activation, action, and deactivation handlers are therefore
//! invoked from that adapter-owned thread, which is why the shared state they
//! touch is held behind `Arc<Mutex<_>>`.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_unix::Adapter;

use crate::PermissionStatus;

const TOOLKIT_NAME: &str = "Kael";
const TOOLKIT_VERSION: &str = env!("CARGO_PKG_VERSION");

type SharedUpdate = Arc<Mutex<Option<TreeUpdate>>>;
type PendingActions = Arc<Mutex<Vec<ActionRequest>>>;

struct InitialTreeHandler {
    latest: SharedUpdate,
}

impl ActivationHandler for InitialTreeHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.latest.lock().ok().and_then(|guard| guard.clone())
    }
}

struct CollectingActionHandler {
    pending: PendingActions,
}

impl ActionHandler for CollectingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(request);
        }
    }
}

struct NoopDeactivationHandler;

impl DeactivationHandler for NoopDeactivationHandler {
    fn deactivate_accessibility(&mut self) {}
}

/// The root AT-SPI2 accessible object for a GPUI window, backed by AccessKit.
///
/// Wraps an [`accesskit_unix::Adapter`] and feeds it [`TreeUpdate`]s built from
/// the shared [`crate::AccessibilityTree`].
pub struct AtSpiAccessibleRoot {
    app_name: String,
    adapter: RefCell<Adapter>,
    latest: SharedUpdate,
    pending_actions: PendingActions,
}

impl AtSpiAccessibleRoot {
    pub fn new(app_name: &str) -> Self {
        let latest: SharedUpdate = Arc::new(Mutex::new(None));
        let pending_actions: PendingActions = Arc::new(Mutex::new(Vec::new()));
        let adapter = Adapter::new(
            InitialTreeHandler {
                latest: latest.clone(),
            },
            CollectingActionHandler {
                pending: pending_actions.clone(),
            },
            NoopDeactivationHandler,
        );
        Self {
            app_name: app_name.to_string(),
            adapter: RefCell::new(adapter),
            latest,
            pending_actions,
        }
    }

    /// Feed the latest accessibility tree to the AT-SPI2 adapter.
    pub fn update_tree(&self, tree: &crate::AccessibilityTree) {
        let update = tree.to_accesskit_tree_update(
            Some(self.app_name.as_str()),
            Some(TOOLKIT_NAME),
            Some(TOOLKIT_VERSION),
        );
        if let Ok(mut guard) = self.latest.lock() {
            *guard = Some(update.clone());
        }
        self.adapter.borrow_mut().update_if_active(|| update);
    }

    /// Drain action requests received from assistive technology, translated
    /// into kael's [`crate::AccessibilityAction`] plus the target node id.
    pub fn drain_actions(&self) -> Vec<(crate::AccessibilityId, crate::AccessibilityAction)> {
        let mut out = Vec::new();
        if let Ok(mut pending) = self.pending_actions.lock() {
            for request in pending.drain(..) {
                if let Some(action) = crate::AccessibilityAction::from_accesskit(request.action) {
                    out.push((crate::AccessibilityId(request.target.0), action));
                }
            }
        }
        out
    }
}

/// Check whether the AT-SPI2 accessibility bus is available on this system.
///
/// On Linux, AT-SPI2 does not require special permissions; any application can
/// register with the accessibility bus. AccessKit handles the actual bus
/// handshake internally, so this always reports `Granted`.
pub fn accessibility_status() -> PermissionStatus {
    PermissionStatus::Granted
}
