// Feature: platform-parity-browser-runtime-features, Property 2: Tab management invariants

use proptest::prelude::*;

use crate::platform::tab_manager::WindowTabManager;
use crate::{AnyWindowHandle, EmptyView, WindowHandle, WindowId};
use std::collections::HashSet;

/// Creates an `AnyWindowHandle` from a raw u32 value.
///
/// Uses the same `slotmap::KeyData::from_ffi` pattern as the existing
/// `WindowTabManager` unit tests.
fn make_handle(raw: u32) -> AnyWindowHandle {
    let id: WindowId = slotmap::KeyData::from_ffi(raw as u64).into();
    WindowHandle::<EmptyView>::new(id).into()
}

/// Generates a non-empty tabbing identifier string.
///
/// Identifiers are 1-20 character alphanumeric strings with hyphens,
/// covering typical group names like "editor", "terminal-1", etc.
fn identifier_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9\\-]{0,19}"
}

/// Generates a list of (window_raw_id, identifier) pairs representing
/// windows assigned to tab groups.
///
/// Raw IDs start from an offset to avoid slotmap zero-key issues and are
/// unique within each generated set. Each window is assigned one of a
/// small set of identifiers to ensure grouping collisions.
fn windows_with_identifiers_strategy() -> impl Strategy<Value = Vec<(u32, String)>> {
    let identifiers = prop::collection::vec(identifier_strategy(), 1..=4);
    let count = 2u32..=10;

    (identifiers, count).prop_flat_map(|(ids, n)| {
        let id_count = ids.len();
        prop::collection::vec((0..id_count).prop_map(move |i| ids[i].clone()), n as usize).prop_map(
            |chosen_ids| {
                chosen_ids
                    .into_iter()
                    .enumerate()
                    .map(|(i, id)| {
                        // Offset by 1 to avoid slotmap null key at 0
                        let raw = (i as u32) + 1;
                        (raw, id)
                    })
                    .collect::<Vec<_>>()
            },
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 2.1, 2.2, 2.3**
    ///
    /// Grouping correctness: windows sharing the same tabbing identifier
    /// appear in the same tab group, and windows with different identifiers
    /// do not appear in each other's groups.
    #[test]
    fn grouping_correctness(entries in windows_with_identifiers_strategy()) {
        let state = WindowTabManager::shared_state();

        // Create managers and assign identifiers.
        let managers: Vec<(AnyWindowHandle, WindowTabManager, String)> = entries
            .iter()
            .map(|(raw, id)| {
                let handle = make_handle(*raw);
                let mgr = WindowTabManager::new(handle, state.clone());
                mgr.set_tabbing_identifier(Some(id.clone()));
                (handle, mgr, id.clone())
            })
            .collect();

        // Build expected groups: identifier -> set of window IDs.
        let mut expected_groups: std::collections::HashMap<String, HashSet<WindowId>> =
            std::collections::HashMap::new();
        for (handle, _, id) in &managers {
            expected_groups
                .entry(id.clone())
                .or_default()
                .insert(handle.window_id());
        }

        // Verify each window sees exactly the windows in its group.
        for (handle, mgr, id) in &managers {
            let tabs = mgr.tabbed_windows();
            prop_assert!(
                tabs.is_some(),
                "window {:?} with identifier '{}' must have tabbed_windows",
                handle.window_id(),
                id
            );
            let tab_ids: HashSet<WindowId> =
                tabs.unwrap().iter().map(|t| t.handle.window_id()).collect();
            let expected = &expected_groups[id];
            prop_assert_eq!(
                &tab_ids,
                expected,
                "tab group for identifier '{}' must contain exactly the windows with that identifier",
                id
            );
        }
    }

    /// **Validates: Requirements 2.1, 2.2, 2.3**
    ///
    /// After `merge_all_windows`, `tabbed_windows` returns all windows with
    /// the same identifier. Since the data structure already groups by
    /// identifier, merge should preserve the invariant.
    #[test]
    fn merge_all_windows_preserves_groups(entries in windows_with_identifiers_strategy()) {
        let state = WindowTabManager::shared_state();

        let managers: Vec<(AnyWindowHandle, WindowTabManager, String)> = entries
            .iter()
            .map(|(raw, id)| {
                let handle = make_handle(*raw);
                let mgr = WindowTabManager::new(handle, state.clone());
                mgr.set_tabbing_identifier(Some(id.clone()));
                (handle, mgr, id.clone())
            })
            .collect();

        // Build expected groups before merge.
        let mut expected_groups: std::collections::HashMap<String, HashSet<WindowId>> =
            std::collections::HashMap::new();
        for (handle, _, id) in &managers {
            expected_groups
                .entry(id.clone())
                .or_default()
                .insert(handle.window_id());
        }

        // Call merge_all_windows on every manager.
        for (_, mgr, _) in &managers {
            mgr.merge_all_windows();
        }

        // Verify groups are unchanged after merge.
        for (handle, mgr, id) in &managers {
            let tabs = mgr.tabbed_windows();
            prop_assert!(
                tabs.is_some(),
                "window {:?} must still have tabbed_windows after merge",
                handle.window_id()
            );
            let tab_ids: HashSet<WindowId> =
                tabs.unwrap().iter().map(|t| t.handle.window_id()).collect();
            let expected = &expected_groups[id];
            prop_assert_eq!(
                &tab_ids,
                expected,
                "after merge_all_windows, group for '{}' must contain all windows with that identifier",
                id
            );
        }
    }

    /// **Validates: Requirements 2.1, 2.2, 2.3**
    ///
    /// After `move_tab_to_new_window` on a group of N tabs, the original
    /// group has N-1 tabs and the moved window is standalone (no tabbing
    /// identifier, tabbed_windows returns None).
    #[test]
    fn move_tab_to_new_window_invariant(entries in windows_with_identifiers_strategy()) {
        let state = WindowTabManager::shared_state();

        let managers: Vec<(AnyWindowHandle, WindowTabManager, String)> = entries
            .iter()
            .map(|(raw, id)| {
                let handle = make_handle(*raw);
                let mgr = WindowTabManager::new(handle, state.clone());
                mgr.set_tabbing_identifier(Some(id.clone()));
                (handle, mgr, id.clone())
            })
            .collect();

        // Pick the first window to move out.
        let (moved_handle, moved_mgr, moved_id) = &managers[0];

        // Count how many windows share the same identifier before the move.
        let group_size_before = managers
            .iter()
            .filter(|(_, _, id)| id == moved_id)
            .count();

        moved_mgr.move_tab_to_new_window();

        // The moved window should be standalone.
        prop_assert!(
            moved_mgr.tabbed_windows().is_none(),
            "moved window {:?} must be standalone (tabbed_windows returns None)",
            moved_handle.window_id()
        );
        prop_assert_eq!(
            moved_mgr.tab_count(),
            0,
            "moved window must have tab_count 0"
        );

        // Remaining windows in the same group should have N-1 tabs.
        let remaining: Vec<&(AnyWindowHandle, WindowTabManager, String)> = managers
            .iter()
            .filter(|(h, _, id)| id == moved_id && h.window_id() != moved_handle.window_id())
            .collect();

        if group_size_before > 1 {
            for (_, mgr, _) in &remaining {
                prop_assert_eq!(
                    mgr.tab_count(),
                    group_size_before - 1,
                    "remaining group members must have N-1 tabs after move"
                );
            }
        }
    }
}
