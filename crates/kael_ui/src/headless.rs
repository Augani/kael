//! Headless controllers: behavior and state with no opinion on rendering.
//!
//! These are the unstyled-but-correct primitives that let an application build any
//! visual it wants while reusing the fiddly interaction logic (open/close, toggle,
//! keyboard navigation, selection) that the styled `components` bake in. Hold a
//! controller in your view state, drive it from events, and render whatever you like
//! from the state it exposes. This is the escape hatch from same-looking apps.

/// Open/closed disclosure state for accordions, collapsibles, and details/summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DisclosureController {
    open: bool,
}

impl DisclosureController {
    /// Create a controller with the given initial open state.
    pub fn new(open: bool) -> Self {
        Self { open }
    }

    /// Whether the disclosure is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the disclosure.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Close the disclosure.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Toggle the open state and return the new value.
    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.open
    }

    /// Set the open state explicitly.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }
}

/// On/off toggle state for switches and checkboxes, with an optional mixed state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ToggleController {
    on: bool,
    mixed: bool,
}

impl ToggleController {
    /// Create a controller with the given initial on state (not mixed).
    pub fn new(on: bool) -> Self {
        Self { on, mixed: false }
    }

    /// Whether the toggle is on.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Whether the toggle is in the indeterminate (mixed) state.
    pub fn is_mixed(&self) -> bool {
        self.mixed
    }

    /// Set the on state, clearing the mixed flag.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        self.mixed = false;
    }

    /// Put the toggle into the indeterminate (mixed) state.
    pub fn set_mixed(&mut self) {
        self.mixed = true;
    }

    /// Toggle the on state (clearing mixed) and return the new value.
    pub fn toggle(&mut self) -> bool {
        self.on = !self.on;
        self.mixed = false;
        self.on
    }
}

/// Single-select list state with keyboard navigation, for dropdowns, menus,
/// comboboxes, and selects. Tracks the open state, the highlighted (keyboard-focused)
/// index, and the committed selection over a list of `len` items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectController {
    len: usize,
    open: bool,
    highlighted: Option<usize>,
    selected: Option<usize>,
}

impl SelectController {
    /// Create a controller over a list of `len` items with nothing selected.
    pub fn new(len: usize) -> Self {
        Self {
            len,
            open: false,
            highlighted: None,
            selected: None,
        }
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Update the item count, clamping the highlighted and selected indices.
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        self.highlighted = self.highlighted.filter(|&i| i < len);
        self.selected = self.selected.filter(|&i| i < len);
    }

    /// Whether the list is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open the list, highlighting the selected item (or the first item).
    pub fn open(&mut self) {
        if self.len == 0 {
            return;
        }
        self.open = true;
        if self.highlighted.is_none() {
            self.highlighted = Some(self.selected.unwrap_or(0));
        }
    }

    /// Close the list, clearing the transient highlight.
    pub fn close(&mut self) {
        self.open = false;
        self.highlighted = None;
    }

    /// Toggle the open state.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// The keyboard-highlighted index, if any.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// The committed selection, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Move the highlight to the next item, wrapping around.
    pub fn highlight_next(&mut self) {
        if self.len == 0 {
            return;
        }
        self.highlighted = Some(match self.highlighted {
            Some(index) => (index + 1) % self.len,
            None => 0,
        });
    }

    /// Move the highlight to the previous item, wrapping around.
    pub fn highlight_prev(&mut self) {
        if self.len == 0 {
            return;
        }
        self.highlighted = Some(match self.highlighted {
            Some(index) => (index + self.len - 1) % self.len,
            None => self.len - 1,
        });
    }

    /// Highlight the first item.
    pub fn highlight_first(&mut self) {
        if self.len > 0 {
            self.highlighted = Some(0);
        }
    }

    /// Highlight the last item.
    pub fn highlight_last(&mut self) {
        if self.len > 0 {
            self.highlighted = Some(self.len - 1);
        }
    }

    /// Commit the highlighted item as the selection, close the list, and return it.
    pub fn select_highlighted(&mut self) -> Option<usize> {
        if let Some(index) = self.highlighted {
            self.selected = Some(index);
            self.close();
        }
        self.selected
    }

    /// Select a specific index (ignored if out of range) and close the list.
    pub fn select(&mut self, index: usize) {
        if index < self.len {
            self.selected = Some(index);
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DisclosureController, SelectController, ToggleController};

    #[test]
    fn disclosure_toggles() {
        let mut disclosure = DisclosureController::default();
        assert!(!disclosure.is_open());
        assert!(disclosure.toggle());
        assert!(disclosure.is_open());
        disclosure.close();
        assert!(!disclosure.is_open());
        disclosure.open();
        assert!(disclosure.is_open());
    }

    #[test]
    fn toggle_clears_mixed_on_change() {
        let mut toggle = ToggleController::new(false);
        toggle.set_mixed();
        assert!(toggle.is_mixed());
        assert!(toggle.toggle());
        assert!(toggle.is_on());
        assert!(!toggle.is_mixed());
    }

    #[test]
    fn select_keyboard_navigation_wraps() {
        let mut select = SelectController::new(3);
        select.open();
        assert_eq!(select.highlighted(), Some(0));

        select.highlight_next();
        assert_eq!(select.highlighted(), Some(1));
        select.highlight_next();
        select.highlight_next();
        assert_eq!(select.highlighted(), Some(0)); // wrapped past the end

        select.highlight_prev();
        assert_eq!(select.highlighted(), Some(2)); // wrapped before the start

        select.highlight_first();
        assert_eq!(select.highlighted(), Some(0));
        select.highlight_last();
        assert_eq!(select.highlighted(), Some(2));
    }

    #[test]
    fn select_highlighted_commits_and_closes() {
        let mut select = SelectController::new(3);
        select.open();
        select.highlight_next();
        assert_eq!(select.select_highlighted(), Some(1));
        assert_eq!(select.selected(), Some(1));
        assert!(!select.is_open());
        assert_eq!(select.highlighted(), None);

        // Reopening highlights the current selection.
        select.open();
        assert_eq!(select.highlighted(), Some(1));
    }

    #[test]
    fn select_set_len_clamps_indices() {
        let mut select = SelectController::new(5);
        select.select(4);
        assert_eq!(select.selected(), Some(4));
        select.set_len(3);
        assert_eq!(select.selected(), None); // 4 no longer valid
    }

    #[test]
    fn empty_select_is_inert() {
        let mut select = SelectController::new(0);
        select.open();
        assert!(!select.is_open());
        select.highlight_next();
        assert_eq!(select.highlighted(), None);
        assert_eq!(select.select_highlighted(), None);
    }
}
