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

/// Selected-tab state with keyboard navigation, for tab strips and segmented controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabsController {
    len: usize,
    selected: usize,
}

impl TabsController {
    /// Create a controller over `len` tabs with the first tab selected.
    pub fn new(len: usize) -> Self {
        Self { len, selected: 0 }
    }

    /// The number of tabs.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are no tabs.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The selected tab index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Select a specific tab (ignored if out of range).
    pub fn select(&mut self, index: usize) {
        if index < self.len {
            self.selected = index;
        }
    }

    /// Select the next tab, wrapping around.
    pub fn next(&mut self) {
        if self.len > 0 {
            self.selected = (self.selected + 1) % self.len;
        }
    }

    /// Select the previous tab, wrapping around.
    pub fn prev(&mut self) {
        if self.len > 0 {
            self.selected = (self.selected + self.len - 1) % self.len;
        }
    }
}

/// Ranged numeric state for sliders, with clamping and step snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderController {
    value: f64,
    min: f64,
    max: f64,
    step: f64,
}

impl SliderController {
    /// Create a controller over `[min, max]` with the given step, clamping `value`.
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> Self {
        let mut controller = Self {
            value: min,
            min,
            max,
            step: step.max(0.0),
        };
        controller.set_value(value);
        controller
    }

    /// The current value (clamped and snapped).
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The minimum bound.
    pub fn min(&self) -> f64 {
        self.min
    }

    /// The maximum bound.
    pub fn max(&self) -> f64 {
        self.max
    }

    /// The step increment.
    pub fn step(&self) -> f64 {
        self.step
    }

    /// Set the value, clamping to `[min, max]` and snapping to the nearest step.
    pub fn set_value(&mut self, value: f64) {
        let clamped = value.clamp(self.min, self.max);
        self.value = if self.step > 0.0 {
            let steps = ((clamped - self.min) / self.step).round();
            (self.min + steps * self.step).clamp(self.min, self.max)
        } else {
            clamped
        };
    }

    /// Increase the value by one step.
    pub fn increment(&mut self) {
        self.set_value(self.value + self.step.max(f64::EPSILON));
    }

    /// Decrease the value by one step.
    pub fn decrement(&mut self) {
        self.set_value(self.value - self.step.max(f64::EPSILON));
    }

    /// The value as a fraction of the range, in `0.0..=1.0`.
    pub fn fraction(&self) -> f64 {
        if self.max <= self.min {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    /// Set the value from a fraction of the range (e.g. from a drag position).
    pub fn set_fraction(&mut self, fraction: f64) {
        self.set_value(self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min));
    }
}

/// Combobox/autocomplete state: a text query plus single-select navigation over the
/// app-filtered results. The app recomputes its filtered items when the query changes
/// and reports the new count via [`ComboboxController::set_result_count`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboboxController {
    query: String,
    select: SelectController,
}

impl ComboboxController {
    /// Create an empty combobox over the given number of currently-visible results.
    pub fn new(result_count: usize) -> Self {
        Self {
            query: String::new(),
            select: SelectController::new(result_count),
        }
    }

    /// The current query text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Set the query text, opening the popup and resetting the highlight to the top.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.select.open();
        self.select.highlight_first();
    }

    /// Update the number of visible (filtered) results.
    pub fn set_result_count(&mut self, count: usize) {
        self.select.set_len(count);
    }

    /// Whether the results popup is open.
    pub fn is_open(&self) -> bool {
        self.select.is_open()
    }

    /// Open the results popup.
    pub fn open(&mut self) {
        self.select.open();
    }

    /// Close the results popup.
    pub fn close(&mut self) {
        self.select.close();
    }

    /// The highlighted result index, if any.
    pub fn highlighted(&self) -> Option<usize> {
        self.select.highlighted()
    }

    /// Highlight the next result, wrapping.
    pub fn highlight_next(&mut self) {
        self.select.highlight_next();
    }

    /// Highlight the previous result, wrapping.
    pub fn highlight_prev(&mut self) {
        self.select.highlight_prev();
    }

    /// Commit the highlighted result and close; returns the chosen result index.
    pub fn select_highlighted(&mut self) -> Option<usize> {
        self.select.select_highlighted()
    }

    /// The committed result index, if any.
    pub fn selected(&self) -> Option<usize> {
        self.select.selected()
    }
}

/// Multi-section disclosure state for accordions: tracks which of `len` sections are
/// open, in either single-expand (one at a time) or multi-expand mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccordionController {
    open: Vec<bool>,
    allow_multiple: bool,
}

impl AccordionController {
    /// Create a controller over `len` sections, all closed.
    pub fn new(len: usize, allow_multiple: bool) -> Self {
        Self {
            open: vec![false; len],
            allow_multiple,
        }
    }

    /// The number of sections.
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// Whether there are no sections.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Whether section `index` is open.
    pub fn is_open(&self, index: usize) -> bool {
        self.open.get(index).copied().unwrap_or(false)
    }

    /// Open section `index`, closing the others in single-expand mode.
    pub fn open(&mut self, index: usize) {
        if index >= self.open.len() {
            return;
        }
        if !self.allow_multiple {
            self.open.iter_mut().for_each(|open| *open = false);
        }
        self.open[index] = true;
    }

    /// Close section `index`.
    pub fn close(&mut self, index: usize) {
        if let Some(open) = self.open.get_mut(index) {
            *open = false;
        }
    }

    /// Toggle section `index`.
    pub fn toggle(&mut self, index: usize) {
        if self.is_open(index) {
            self.close(index);
        } else {
            self.open(index);
        }
    }
}

/// Zero-based pagination state with bounds-checked navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaginationController {
    page: usize,
    page_count: usize,
}

impl PaginationController {
    /// Create a controller over `page_count` pages, positioned on the first page.
    pub fn new(page_count: usize) -> Self {
        Self {
            page: 0,
            page_count,
        }
    }

    /// The current (zero-based) page.
    pub fn page(&self) -> usize {
        self.page
    }

    /// The total number of pages.
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// Update the page count, clamping the current page into range.
    pub fn set_page_count(&mut self, page_count: usize) {
        self.page_count = page_count;
        self.page = self.page.min(page_count.saturating_sub(1));
    }

    /// Go to a specific page (clamped into range).
    pub fn set_page(&mut self, page: usize) {
        self.page = page.min(self.page_count.saturating_sub(1));
    }

    /// Whether there is a next page.
    pub fn has_next(&self) -> bool {
        self.page + 1 < self.page_count
    }

    /// Whether there is a previous page.
    pub fn has_prev(&self) -> bool {
        self.page > 0
    }

    /// Advance to the next page if there is one.
    pub fn next(&mut self) {
        if self.has_next() {
            self.page += 1;
        }
    }

    /// Go back to the previous page if there is one.
    pub fn prev(&mut self) {
        if self.has_prev() {
            self.page -= 1;
        }
    }

    /// Jump to the first page.
    pub fn first(&mut self) {
        self.page = 0;
    }

    /// Jump to the last page.
    pub fn last(&mut self) {
        self.page = self.page_count.saturating_sub(1);
    }
}

/// Multi-step flow state for wizards and onboarding: a current step over `step_count`
/// steps, with per-step completion tracking and forward/back navigation that cannot
/// skip past the next incomplete step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepperController {
    step: usize,
    completed: Vec<bool>,
}

impl StepperController {
    /// Create a stepper over `step_count` steps, positioned on the first step.
    pub fn new(step_count: usize) -> Self {
        Self {
            step: 0,
            completed: vec![false; step_count],
        }
    }

    /// The current (zero-based) step.
    pub fn step(&self) -> usize {
        self.step
    }

    /// The total number of steps.
    pub fn step_count(&self) -> usize {
        self.completed.len()
    }

    /// Whether step `index` has been marked complete.
    pub fn is_completed(&self, index: usize) -> bool {
        self.completed.get(index).copied().unwrap_or(false)
    }

    /// Whether the current step is the first.
    pub fn is_first(&self) -> bool {
        self.step == 0
    }

    /// Whether the current step is the last.
    pub fn is_last(&self) -> bool {
        self.step + 1 >= self.step_count()
    }

    /// Mark the current step complete and advance to the next, if any.
    pub fn next(&mut self) {
        if let Some(done) = self.completed.get_mut(self.step) {
            *done = true;
        }
        if !self.is_last() {
            self.step += 1;
        }
    }

    /// Go back to the previous step, if any.
    pub fn prev(&mut self) {
        if self.step > 0 {
            self.step -= 1;
        }
    }

    /// Jump to step `index` only if it is already reachable (at or before the current
    /// step, or the immediately next step). Returns whether the jump was allowed.
    pub fn go_to(&mut self, index: usize) -> bool {
        if index < self.step_count() && index <= self.step + 1 {
            self.step = index;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccordionController, ComboboxController, DisclosureController, PaginationController,
        SelectController, SliderController, StepperController, TabsController, ToggleController,
    };

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

    #[test]
    fn tabs_navigation_wraps_and_clamps() {
        let mut tabs = TabsController::new(3);
        assert_eq!(tabs.selected(), 0);
        tabs.next();
        assert_eq!(tabs.selected(), 1);
        tabs.prev();
        tabs.prev();
        assert_eq!(tabs.selected(), 2); // wrapped past the start
        tabs.select(5); // out of range, ignored
        assert_eq!(tabs.selected(), 2);
        tabs.select(0);
        assert_eq!(tabs.selected(), 0);
    }

    #[test]
    fn slider_clamps_snaps_and_steps() {
        let mut slider = SliderController::new(0.0, 0.0, 10.0, 2.0);
        slider.set_value(6.4);
        assert_eq!(slider.value(), 6.0); // snapped to nearest step
        slider.set_value(100.0);
        assert_eq!(slider.value(), 10.0); // clamped to max
        slider.increment();
        assert_eq!(slider.value(), 10.0); // already at max
        slider.decrement();
        assert_eq!(slider.value(), 8.0);
        assert_eq!(slider.fraction(), 0.8);
        slider.set_fraction(0.0);
        assert_eq!(slider.value(), 0.0);
    }

    #[test]
    fn combobox_query_drives_navigation() {
        let mut combobox = ComboboxController::new(5);
        combobox.set_query("ab");
        assert_eq!(combobox.query(), "ab");
        assert!(combobox.is_open());
        assert_eq!(combobox.highlighted(), Some(0));

        combobox.set_result_count(2);
        combobox.highlight_next();
        assert_eq!(combobox.highlighted(), Some(1));
        combobox.highlight_next();
        assert_eq!(combobox.highlighted(), Some(0)); // wrapped over 2 results
        combobox.highlight_next();
        assert_eq!(combobox.select_highlighted(), Some(1));
        assert!(!combobox.is_open());
    }

    #[test]
    fn accordion_single_vs_multi_expand() {
        let mut single = AccordionController::new(3, false);
        single.open(0);
        single.open(1);
        assert!(!single.is_open(0)); // single-expand closed section 0
        assert!(single.is_open(1));

        let mut multi = AccordionController::new(3, true);
        multi.open(0);
        multi.open(2);
        assert!(multi.is_open(0));
        assert!(multi.is_open(2));
        multi.toggle(0);
        assert!(!multi.is_open(0));
    }

    #[test]
    fn pagination_navigates_within_bounds() {
        let mut pagination = PaginationController::new(3);
        assert_eq!(pagination.page(), 0);
        assert!(!pagination.has_prev());
        assert!(pagination.has_next());

        pagination.next();
        pagination.next();
        assert_eq!(pagination.page(), 2);
        pagination.next(); // already last, no-op
        assert_eq!(pagination.page(), 2);
        assert!(!pagination.has_next());

        pagination.first();
        assert_eq!(pagination.page(), 0);
        pagination.last();
        assert_eq!(pagination.page(), 2);

        pagination.set_page_count(2); // clamps current page from 2 to 1
        assert_eq!(pagination.page(), 1);
        pagination.set_page(99);
        assert_eq!(pagination.page(), 1);
    }

    #[test]
    fn stepper_advances_and_gates_jumps() {
        let mut stepper = StepperController::new(3);
        assert!(stepper.is_first());
        assert!(!stepper.is_completed(0));

        stepper.next();
        assert_eq!(stepper.step(), 1);
        assert!(stepper.is_completed(0));

        assert!(!stepper.go_to(2 + 1)); // out of range
        assert!(stepper.go_to(0)); // back is allowed
        assert_eq!(stepper.step(), 0);

        stepper.next();
        stepper.next();
        assert!(stepper.is_last());
        stepper.next(); // last, stays put but marks complete
        assert_eq!(stepper.step(), 2);
        assert!(stepper.is_completed(2));
    }
}
