use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{AnyWindowHandle, SharedString, SystemWindowTab, WindowId};

/// Shared state for all `WindowTabManager` instances.
/// Tracks which windows belong to which tabbing identifier group.
#[derive(Default)]
pub struct TabManagerState {
    /// Maps tabbing identifier -> ordered list of window handles in that group.
    groups: HashMap<String, Vec<AnyWindowHandle>>,
    /// Maps window ID -> its tabbing identifier.
    window_identifiers: HashMap<WindowId, String>,
    /// Maps window ID -> window title (for building `SystemWindowTab` results).
    window_titles: HashMap<WindowId, SharedString>,
}

/// A cross-platform window tab manager shared between Windows and Linux backends.
///
/// Each platform window holds a `WindowTabManager` that delegates tab operations
/// to shared global state. Windows sharing the same tabbing identifier are grouped
/// together and can be merged, split, or queried as a tab group.
#[derive(Clone)]
pub struct WindowTabManager {
    state: Arc<Mutex<TabManagerState>>,
    window_handle: AnyWindowHandle,
}

impl WindowTabManager {
    /// Create a new `WindowTabManager` for the given window, using shared state.
    pub fn new(window_handle: AnyWindowHandle, state: Arc<Mutex<TabManagerState>>) -> Self {
        Self {
            state,
            window_handle,
        }
    }

    /// Create a new shared state instance. Platform backends should create one
    /// of these and pass it to all `WindowTabManager` instances.
    pub fn shared_state() -> Arc<Mutex<TabManagerState>> {
        Arc::new(Mutex::new(TabManagerState::default()))
    }

    /// Set or clear the tabbing identifier for this window.
    ///
    /// When set to a non-empty identifier, this window joins the tab group
    /// for that identifier. When set to `None`, the window leaves its current
    /// tab group.
    pub fn set_tabbing_identifier(&self, identifier: Option<String>) {
        let mut state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        // Remove from current group if any.
        if let Some(old_id) = state.window_identifiers.remove(&window_id) {
            if let Some(group) = state.groups.get_mut(&old_id) {
                group.retain(|h| h.window_id() != window_id);
                if group.is_empty() {
                    state.groups.remove(&old_id);
                }
            }
        }

        // Add to new group if identifier is provided.
        if let Some(id) = identifier {
            if !id.is_empty() {
                state
                    .groups
                    .entry(id.clone())
                    .or_default()
                    .push(self.window_handle);
                state.window_identifiers.insert(window_id, id);
            }
        }
    }

    /// Merge all windows that share the same tabbing identifier as this window
    /// into a single tab group.
    ///
    /// After this call, `tabbed_windows()` returns all windows with the same
    /// identifier.
    pub fn merge_all_windows(&self) {
        let state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        // The current implementation already groups by identifier, so
        // merge_all_windows is effectively a no-op on the data structure
        // since all windows with the same identifier are already in the
        // same group. The real work happens at the platform level where
        // the tab bar UI needs to be refreshed.
        //
        // If we later support sub-groups within an identifier, this would
        // flatten them.
        let _ = (state, window_id);
    }

    /// Move this window's tab out of its current group into a new standalone window.
    ///
    /// After this call, the original group has N-1 tabs and this window
    /// exists as a standalone (single-tab or no-tab) window.
    pub fn move_tab_to_new_window(&self) {
        let mut state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        if let Some(identifier) = state.window_identifiers.remove(&window_id) {
            if let Some(group) = state.groups.get_mut(&identifier) {
                group.retain(|h| h.window_id() != window_id);
                if group.is_empty() {
                    state.groups.remove(&identifier);
                }
            }
        }
    }

    /// Return the list of tabs in this window's current tab group.
    ///
    /// Returns `None` if this window has no tabbing identifier set.
    /// Returns `Some(vec)` with all windows sharing the same identifier.
    pub fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>> {
        let state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        let identifier = state.window_identifiers.get(&window_id)?;
        let group = state.groups.get(identifier)?;

        let tabs: Vec<SystemWindowTab> = group
            .iter()
            .map(|handle| {
                let title = state
                    .window_titles
                    .get(&handle.window_id())
                    .cloned()
                    .unwrap_or_default();
                SystemWindowTab::new(title, *handle)
            })
            .collect();

        Some(tabs)
    }

    /// Update the stored title for this window. Called by the platform backend
    /// when `set_title` is invoked so that `tabbed_windows` returns current titles.
    pub fn set_title(&self, title: SharedString) {
        let mut state = self.state.lock().unwrap();
        state
            .window_titles
            .insert(self.window_handle.window_id(), title);
    }

    /// Remove this window from all tracking. Should be called when the window closes.
    pub fn remove_window(&self) {
        let mut state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        state.window_titles.remove(&window_id);

        if let Some(identifier) = state.window_identifiers.remove(&window_id) {
            if let Some(group) = state.groups.get_mut(&identifier) {
                group.retain(|h| h.window_id() != window_id);
                if group.is_empty() {
                    state.groups.remove(&identifier);
                }
            }
        }
    }

    /// Get the tabbing identifier for this window, if any.
    pub fn tabbing_identifier(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state
            .window_identifiers
            .get(&self.window_handle.window_id())
            .cloned()
    }

    /// Get the number of tabs in this window's current group.
    pub fn tab_count(&self) -> usize {
        let state = self.state.lock().unwrap();
        let window_id = self.window_handle.window_id();

        state
            .window_identifiers
            .get(&window_id)
            .and_then(|id| state.groups.get(id))
            .map(|g| g.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EmptyView, WindowHandle, WindowId};

    fn make_handle(raw: u32) -> AnyWindowHandle {
        let id: WindowId = slotmap::KeyData::from_ffi(raw as u64).into();
        WindowHandle::<EmptyView>::new(id).into()
    }

    #[test]
    fn test_set_tabbing_identifier_groups_windows() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(1);
        let h2 = make_handle(2);
        let h3 = make_handle(3);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());
        let m3 = WindowTabManager::new(h3, state.clone());

        // No identifier set -> no tabs.
        assert!(m1.tabbed_windows().is_none());

        // Set same identifier on m1 and m2.
        m1.set_tabbing_identifier(Some("group-a".into()));
        m2.set_tabbing_identifier(Some("group-a".into()));
        m3.set_tabbing_identifier(Some("group-b".into()));

        let tabs = m1.tabbed_windows().unwrap();
        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().any(|t| t.handle == h1));
        assert!(tabs.iter().any(|t| t.handle == h2));

        // m3 is in a different group.
        let tabs_b = m3.tabbed_windows().unwrap();
        assert_eq!(tabs_b.len(), 1);
        assert!(tabs_b.iter().any(|t| t.handle == h3));
    }

    #[test]
    fn test_move_tab_to_new_window() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(10);
        let h2 = make_handle(11);
        let h3 = make_handle(12);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());
        let m3 = WindowTabManager::new(h3, state.clone());

        m1.set_tabbing_identifier(Some("grp".into()));
        m2.set_tabbing_identifier(Some("grp".into()));
        m3.set_tabbing_identifier(Some("grp".into()));

        assert_eq!(m1.tab_count(), 3);

        // Move m2 out.
        m2.move_tab_to_new_window();

        // Original group now has 2 tabs.
        assert_eq!(m1.tab_count(), 2);
        // m2 is standalone (no identifier).
        assert!(m2.tabbed_windows().is_none());
        assert_eq!(m2.tab_count(), 0);
    }

    #[test]
    fn test_tabbed_windows_returns_titles() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(20);
        let h2 = make_handle(21);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());

        m1.set_title("Window 1".into());
        m2.set_title("Window 2".into());

        m1.set_tabbing_identifier(Some("titled".into()));
        m2.set_tabbing_identifier(Some("titled".into()));

        let tabs = m1.tabbed_windows().unwrap();
        assert_eq!(tabs.len(), 2);

        let titles: Vec<&str> = tabs.iter().map(|t| t.title.as_ref()).collect();
        assert!(titles.contains(&"Window 1"));
        assert!(titles.contains(&"Window 2"));
    }

    #[test]
    fn test_remove_window_cleans_up() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(30);
        let h2 = make_handle(31);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());

        m1.set_tabbing_identifier(Some("cleanup".into()));
        m2.set_tabbing_identifier(Some("cleanup".into()));

        assert_eq!(m1.tab_count(), 2);

        m1.remove_window();

        // m2 still in group with 1 tab.
        assert_eq!(m2.tab_count(), 1);
        // m1 is gone.
        assert!(m1.tabbed_windows().is_none());
    }

    #[test]
    fn test_set_tabbing_identifier_none_removes_from_group() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(40);
        let h2 = make_handle(41);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());

        m1.set_tabbing_identifier(Some("temp".into()));
        m2.set_tabbing_identifier(Some("temp".into()));
        assert_eq!(m1.tab_count(), 2);

        // Clear identifier.
        m1.set_tabbing_identifier(None);
        assert!(m1.tabbed_windows().is_none());
        assert_eq!(m2.tab_count(), 1);
    }

    #[test]
    fn test_set_tabbing_identifier_switches_group() {
        let state = WindowTabManager::shared_state();
        let h1 = make_handle(50);
        let h2 = make_handle(51);
        let h3 = make_handle(52);

        let m1 = WindowTabManager::new(h1, state.clone());
        let m2 = WindowTabManager::new(h2, state.clone());
        let m3 = WindowTabManager::new(h3, state.clone());

        m1.set_tabbing_identifier(Some("alpha".into()));
        m2.set_tabbing_identifier(Some("alpha".into()));
        m3.set_tabbing_identifier(Some("beta".into()));

        assert_eq!(m1.tab_count(), 2);
        assert_eq!(m3.tab_count(), 1);

        // Move m1 from alpha to beta.
        m1.set_tabbing_identifier(Some("beta".into()));

        assert_eq!(m2.tab_count(), 1); // alpha now has only m2
        assert_eq!(m3.tab_count(), 2); // beta now has m3 + m1
        assert_eq!(m1.tabbing_identifier(), Some("beta".into()));
    }
}
