/// Status bar for displaying contextual information items in a bottom bar,
/// typically used by large applications such as IDEs and editors.
use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::collections::HashMap;

const MAX_STATUS_ITEMS: usize = 4_096;
const MAX_STATUS_ID_BYTES: usize = 128;
const MAX_STATUS_TEXT_BYTES: usize = 4_096;

/// Unique identifier for a status bar item.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StatusItemId(String);

impl StatusItemId {
    /// Creates a new status item identifier from the given string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Creates an identifier after validating its bounded single-line token.
    pub fn new_checked(id: impl Into<String>) -> Result<Self> {
        let id = Self(id.into());
        validate_status_id(&id)?;
        Ok(id)
    }
}

fn validate_status_id(id: &StatusItemId) -> Result<()> {
    anyhow::ensure!(
        !id.0.trim().is_empty(),
        "status item identifier cannot be empty"
    );
    anyhow::ensure!(
        id.0 == id.0.trim(),
        "status item identifier cannot have surrounding whitespace"
    );
    anyhow::ensure!(
        id.0.len() <= MAX_STATUS_ID_BYTES,
        "status item identifier cannot exceed {MAX_STATUS_ID_BYTES} bytes"
    );
    anyhow::ensure!(
        !id.0.chars().any(char::is_control),
        "status item identifier cannot contain control characters"
    );
    Ok(())
}

impl std::fmt::Display for StatusItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Determines which section of the status bar an item is placed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusItemAlignment {
    /// Align the item to the left section of the status bar.
    Left,
    /// Align the item to the center section of the status bar.
    Center,
    /// Align the item to the right section of the status bar.
    Right,
}

/// A single item displayed within the status bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusItem {
    /// Unique identifier for this item.
    pub id: StatusItemId,
    /// The text content displayed in the status bar.
    pub text: String,
    /// Optional tooltip shown on hover.
    pub tooltip: Option<String>,
    /// Which section of the bar this item belongs to.
    pub alignment: StatusItemAlignment,
    /// Sort priority within its alignment section. Higher values position
    /// the item closer to the outer edge (left edge for `Left`, right edge for `Right`).
    pub priority: i32,
    /// Whether this item is currently visible.
    pub visible: bool,
}

/// Manages a collection of [`StatusItem`]s for a status bar.
#[derive(Clone, Serialize)]
pub struct StatusBar {
    items: Vec<StatusItem>,
    #[serde(skip)]
    index: HashMap<StatusItemId, usize>,
}

impl std::fmt::Debug for StatusBar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatusBar")
            .field("item_count", &self.items.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for StatusBar {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct SerializedStatusBar {
            items: Vec<StatusItem>,
        }

        let serialized = SerializedStatusBar::deserialize(deserializer)?;
        let mut bar = Self {
            items: serialized.items,
            index: HashMap::new(),
        };
        bar.rebuild_index_checked().map_err(D::Error::custom)?;
        Ok(bar)
    }
}

impl StatusBar {
    /// Creates a new empty status bar.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Adds an item to the status bar. If an item with the same id already
    /// exists, it is replaced.
    pub fn add_item(&mut self, item: StatusItem) {
        let _ = self.add_item_checked(item);
    }

    /// Adds or replaces a validated item while enforcing bounded retention.
    pub fn add_item_checked(&mut self, item: StatusItem) -> Result<()> {
        validate_status_item(&item)?;
        if let Some(&idx) = self.index.get(&item.id) {
            self.items[idx] = item;
        } else {
            anyhow::ensure!(
                self.items.len() < MAX_STATUS_ITEMS,
                "status bar cannot exceed {MAX_STATUS_ITEMS} items"
            );
            let idx = self.items.len();
            self.index.insert(item.id.clone(), idx);
            self.items.push(item);
        }
        Ok(())
    }

    /// Removes an item from the status bar by its id. No-op if the id does
    /// not exist.
    pub fn remove_item(&mut self, id: &StatusItemId) {
        if let Some(idx) = self.index.remove(id) {
            self.items.remove(idx);
            for (next_idx, item) in self.items.iter().enumerate().skip(idx) {
                self.index.insert(item.id.clone(), next_idx);
            }
        }
    }

    /// Updates the display text of the item with the given id.
    pub fn update_text(&mut self, id: &StatusItemId, text: String) -> Result<()> {
        validate_status_text(&text, "status item text")?;
        let item = self.get_mut(id)?;
        item.text = text;
        Ok(())
    }

    /// Updates the tooltip of the item with the given id.
    pub fn update_tooltip(&mut self, id: &StatusItemId, tooltip: Option<String>) -> Result<()> {
        if let Some(tooltip) = &tooltip {
            validate_status_text(tooltip, "status item tooltip")?;
        }
        let item = self.get_mut(id)?;
        item.tooltip = tooltip;
        Ok(())
    }

    /// Sets the visibility of the item with the given id.
    pub fn set_visible(&mut self, id: &StatusItemId, visible: bool) -> Result<()> {
        let item = self.get_mut(id)?;
        item.visible = visible;
        Ok(())
    }

    /// Returns items matching the given alignment, sorted by priority descending
    /// (highest priority first).
    pub fn items(&self, alignment: StatusItemAlignment) -> Vec<&StatusItem> {
        let mut matched: Vec<&StatusItem> = self
            .items
            .iter()
            .filter(|item| item.alignment == alignment)
            .collect();
        matched.sort_by_key(|b| std::cmp::Reverse(b.priority));
        matched
    }

    /// Returns visible items for one alignment in deterministic priority order.
    pub fn visible_items(&self, alignment: StatusItemAlignment) -> Vec<&StatusItem> {
        let mut matched = self
            .items
            .iter()
            .filter(|item| item.visible && item.alignment == alignment)
            .collect::<Vec<_>>();
        matched.sort_by_key(|item| std::cmp::Reverse(item.priority));
        matched
    }

    /// Returns a slice of all items in insertion order.
    pub fn all_items(&self) -> &[StatusItem] {
        &self.items
    }

    /// Returns a reference to the item with the given id, if it exists.
    pub fn get(&self, id: &StatusItemId) -> Option<&StatusItem> {
        self.index.get(id).and_then(|&idx| self.items.get(idx))
    }

    /// Rebuilds the internal lookup index from the items list. Call this
    /// after deserializing a `StatusBar` to restore O(1) lookups by id.
    pub fn rebuild_index(&mut self) {
        let _ = self.rebuild_index_checked();
    }

    /// Rebuilds the index while validating persisted items and duplicates.
    pub fn rebuild_index_checked(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.items.len() <= MAX_STATUS_ITEMS,
            "status bar cannot exceed {MAX_STATUS_ITEMS} items"
        );
        let mut next_index = HashMap::with_capacity(self.items.len());
        for (idx, item) in self.items.iter().enumerate() {
            validate_status_item(item)?;
            anyhow::ensure!(
                next_index.insert(item.id.clone(), idx).is_none(),
                "status bar contains duplicate item identifiers"
            );
        }
        self.index = next_index;
        Ok(())
    }

    /// Returns the number of registered status items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether no status items are registered.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn get_mut(&mut self, id: &StatusItemId) -> Result<&mut StatusItem> {
        let idx = *self
            .index
            .get(id)
            .ok_or_else(|| anyhow!("status item not found"))?;
        self.items
            .get_mut(idx)
            .ok_or_else(|| anyhow!("status item index is stale"))
    }
}

fn validate_status_text(text: &str, field: &str) -> Result<()> {
    anyhow::ensure!(!text.trim().is_empty(), "{field} cannot be empty");
    anyhow::ensure!(
        text.len() <= MAX_STATUS_TEXT_BYTES,
        "{field} cannot exceed {MAX_STATUS_TEXT_BYTES} bytes"
    );
    anyhow::ensure!(
        !text.chars().any(char::is_control),
        "{field} cannot contain control characters"
    );
    Ok(())
}

fn validate_status_item(item: &StatusItem) -> Result<()> {
    validate_status_id(&item.id)?;
    validate_status_text(&item.text, "status item text")?;
    if let Some(tooltip) = &item.tooltip {
        validate_status_text(tooltip, "status item tooltip")?;
    }
    Ok(())
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(id: &str, alignment: StatusItemAlignment, priority: i32) -> StatusItem {
        StatusItem {
            id: StatusItemId::new(id),
            text: format!("{id} text"),
            tooltip: None,
            alignment,
            priority,
            visible: true,
        }
    }

    #[test]
    fn add_and_get_item() {
        let mut bar = StatusBar::new();
        let item = make_item("branch", StatusItemAlignment::Left, 10);
        bar.add_item(item);

        let retrieved = bar.get(&StatusItemId::new("branch"));
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().text, "branch text");
    }

    #[test]
    fn add_duplicate_replaces() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("branch", StatusItemAlignment::Left, 10));

        let mut replacement = make_item("branch", StatusItemAlignment::Right, 5);
        replacement.text = "updated".into();
        bar.add_item(replacement);

        assert_eq!(bar.all_items().len(), 1);
        let item = bar.get(&StatusItemId::new("branch")).unwrap();
        assert_eq!(item.text, "updated");
        assert_eq!(item.alignment, StatusItemAlignment::Right);
    }

    #[test]
    fn remove_item() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("a", StatusItemAlignment::Left, 1));
        bar.add_item(make_item("b", StatusItemAlignment::Left, 2));
        bar.add_item(make_item("c", StatusItemAlignment::Left, 3));

        bar.remove_item(&StatusItemId::new("a"));

        assert!(bar.get(&StatusItemId::new("a")).is_none());
        assert_eq!(bar.all_items().len(), 2);
        assert!(bar.get(&StatusItemId::new("b")).is_some());
        assert!(bar.get(&StatusItemId::new("c")).is_some());
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("a", StatusItemAlignment::Left, 1));
        bar.remove_item(&StatusItemId::new("missing"));
        assert_eq!(bar.all_items().len(), 1);
    }

    #[test]
    fn update_text_success() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("info", StatusItemAlignment::Center, 0));

        bar.update_text(&StatusItemId::new("info"), "new text".into())
            .unwrap();
        assert_eq!(
            bar.get(&StatusItemId::new("info")).unwrap().text,
            "new text"
        );
    }

    #[test]
    fn update_text_missing_item_errors() {
        let mut bar = StatusBar::new();
        let result = bar.update_text(&StatusItemId::new("missing"), "text".into());
        assert!(result.is_err());
    }

    #[test]
    fn update_tooltip() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("info", StatusItemAlignment::Left, 0));

        bar.update_tooltip(&StatusItemId::new("info"), Some("a tip".into()))
            .unwrap();
        assert_eq!(
            bar.get(&StatusItemId::new("info"))
                .unwrap()
                .tooltip
                .as_deref(),
            Some("a tip")
        );

        bar.update_tooltip(&StatusItemId::new("info"), None)
            .unwrap();
        assert!(
            bar.get(&StatusItemId::new("info"))
                .unwrap()
                .tooltip
                .is_none()
        );
    }

    #[test]
    fn update_tooltip_missing_item_errors() {
        let mut bar = StatusBar::new();
        let result = bar.update_tooltip(&StatusItemId::new("gone"), Some("tip".into()));
        assert!(result.is_err());
    }

    #[test]
    fn set_visible() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("x", StatusItemAlignment::Left, 0));

        bar.set_visible(&StatusItemId::new("x"), false).unwrap();
        assert!(!bar.get(&StatusItemId::new("x")).unwrap().visible);

        bar.set_visible(&StatusItemId::new("x"), true).unwrap();
        assert!(bar.get(&StatusItemId::new("x")).unwrap().visible);
    }

    #[test]
    fn set_visible_missing_item_errors() {
        let mut bar = StatusBar::new();
        let result = bar.set_visible(&StatusItemId::new("gone"), true);
        assert!(result.is_err());
    }

    #[test]
    fn items_filtered_and_sorted_by_priority() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("low", StatusItemAlignment::Left, 1));
        bar.add_item(make_item("high", StatusItemAlignment::Left, 100));
        bar.add_item(make_item("mid", StatusItemAlignment::Left, 50));
        bar.add_item(make_item("right_item", StatusItemAlignment::Right, 999));

        let left_items = bar.items(StatusItemAlignment::Left);
        assert_eq!(left_items.len(), 3);
        assert_eq!(left_items[0].id, StatusItemId::new("high"));
        assert_eq!(left_items[1].id, StatusItemId::new("mid"));
        assert_eq!(left_items[2].id, StatusItemId::new("low"));

        let right_items = bar.items(StatusItemAlignment::Right);
        assert_eq!(right_items.len(), 1);
        assert_eq!(right_items[0].id, StatusItemId::new("right_item"));

        let center_items = bar.items(StatusItemAlignment::Center);
        assert!(center_items.is_empty());
    }

    #[test]
    fn all_items_returns_full_collection() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("a", StatusItemAlignment::Left, 0));
        bar.add_item(make_item("b", StatusItemAlignment::Center, 0));
        bar.add_item(make_item("c", StatusItemAlignment::Right, 0));

        assert_eq!(bar.all_items().len(), 3);
    }

    #[test]
    fn get_returns_none_for_missing() {
        let bar = StatusBar::new();
        assert!(bar.get(&StatusItemId::new("nope")).is_none());
    }

    #[test]
    fn default_creates_empty_bar() {
        let bar = StatusBar::default();
        assert!(bar.all_items().is_empty());
    }

    #[test]
    fn remove_then_add_reuses_correctly() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("a", StatusItemAlignment::Left, 1));
        bar.add_item(make_item("b", StatusItemAlignment::Left, 2));

        bar.remove_item(&StatusItemId::new("a"));
        bar.add_item(make_item("c", StatusItemAlignment::Left, 3));

        assert_eq!(bar.all_items().len(), 2);
        assert!(bar.get(&StatusItemId::new("a")).is_none());
        assert!(bar.get(&StatusItemId::new("b")).is_some());
        assert!(bar.get(&StatusItemId::new("c")).is_some());
    }

    #[test]
    fn status_item_id_display() {
        let id = StatusItemId::new("git-branch");
        assert_eq!(format!("{id}"), "git-branch");
    }

    #[test]
    fn serialization_roundtrip() {
        let mut bar = StatusBar::new();
        bar.add_item(StatusItem {
            id: StatusItemId::new("line"),
            text: "Ln 42, Col 10".into(),
            tooltip: Some("Cursor position".into()),
            alignment: StatusItemAlignment::Right,
            priority: 50,
            visible: true,
        });

        let json = serde_json::to_string(&bar).unwrap();
        let mut restored: StatusBar = serde_json::from_str(&json).unwrap();
        restored.rebuild_index();

        assert_eq!(restored.all_items().len(), 1);
        let item = restored.get(&StatusItemId::new("line")).unwrap();
        assert_eq!(item.text, "Ln 42, Col 10");
        assert_eq!(item.tooltip.as_deref(), Some("Cursor position"));
        assert_eq!(item.alignment, StatusItemAlignment::Right);
        assert_eq!(item.priority, 50);
    }

    #[test]
    fn negative_priority_ordering() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("neg", StatusItemAlignment::Left, -10));
        bar.add_item(make_item("zero", StatusItemAlignment::Left, 0));
        bar.add_item(make_item("pos", StatusItemAlignment::Left, 10));

        let items = bar.items(StatusItemAlignment::Left);
        assert_eq!(items[0].id, StatusItemId::new("pos"));
        assert_eq!(items[1].id, StatusItemId::new("zero"));
        assert_eq!(items[2].id, StatusItemId::new("neg"));
    }

    #[test]
    fn checked_items_validate_identity_text_and_capacity_inputs() {
        assert!(StatusItemId::new_checked("").is_err());
        assert!(StatusItemId::new_checked(" bad").is_err());
        assert!(StatusItemId::new_checked("bad\nid").is_err());

        let mut bar = StatusBar::new();
        let mut invalid = make_item("valid", StatusItemAlignment::Left, 0);
        invalid.text = "bad\ntext".into();
        assert!(bar.add_item_checked(invalid).is_err());
        assert!(bar.is_empty());

        bar.add_item_checked(make_item("valid", StatusItemAlignment::Left, 0))
            .unwrap();
        assert!(
            bar.update_tooltip(
                &StatusItemId::new("valid"),
                Some("x".repeat(MAX_STATUS_TEXT_BYTES + 1)),
            )
            .is_err()
        );
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn visible_items_hide_invisible_entries_and_preserve_tie_order() {
        let mut bar = StatusBar::new();
        bar.add_item(make_item("first", StatusItemAlignment::Left, 5));
        bar.add_item(make_item("hidden", StatusItemAlignment::Left, 10));
        bar.add_item(make_item("second", StatusItemAlignment::Left, 5));
        bar.set_visible(&StatusItemId::new("hidden"), false)
            .unwrap();

        let visible = bar.visible_items(StatusItemAlignment::Left);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, StatusItemId::new("first"));
        assert_eq!(visible[1].id, StatusItemId::new("second"));

        bar.remove_item(&StatusItemId::new("first"));
        assert_eq!(bar.all_items()[0].id, StatusItemId::new("hidden"));
        assert_eq!(bar.all_items()[1].id, StatusItemId::new("second"));
    }

    #[test]
    fn deserialization_rebuilds_index_and_rejects_duplicates() {
        let json = r#"{
            "items": [{
                "id":"one",
                "text":"One",
                "tooltip":null,
                "alignment":"Left",
                "priority":0,
                "visible":true
            }]
        }"#;
        let restored: StatusBar = serde_json::from_str(json).unwrap();
        assert!(restored.get(&StatusItemId::new("one")).is_some());

        let duplicate = json.replace(
            "]",
            ",{
            \"id\":\"one\",\"text\":\"Duplicate\",\"tooltip\":null,
            \"alignment\":\"Right\",\"priority\":1,\"visible\":true
        }]",
        );
        assert!(serde_json::from_str::<StatusBar>(&duplicate).is_err());
    }

    #[test]
    fn status_bar_debug_redacts_item_content() {
        let mut bar = StatusBar::new();
        bar.add_item(StatusItem {
            id: StatusItemId::new("private-id"),
            text: "private branch and path".into(),
            tooltip: Some("private tooltip".into()),
            alignment: StatusItemAlignment::Left,
            priority: 0,
            visible: true,
        });
        let debug = format!("{bar:?}");
        assert_eq!(debug, "StatusBar { item_count: 1 }");
        assert!(!debug.contains("private"));
    }
}
